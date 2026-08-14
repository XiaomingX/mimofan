//! `mimofan review` — AI-assisted code review and review-receipt validation.

use super::*;

use crate::client::ApiClient;
use crate::config::Config;
use crate::dependencies::ExternalTool;
use crate::models::{ContentBlock, Message, MessageRequest, SystemPrompt};
use crate::utils::truncate_with_ellipsis;

pub(crate) async fn run_review(config: &Config, args: ReviewArgs) -> Result<()> {
    let diff = collect_diff(&args)?;
    if diff.trim().is_empty() {
        bail!("No diff to review.");
    }
    validate_review_receipt_args(&args)?;
    if args.check_receipt {
        return run_review_receipt_check(&diff, &args);
    }

    let model = args
        .model
        .clone()
        .or_else(|| config.default_text_model.clone())
        .unwrap_or_else(|| config.default_model());
    let route = resolve_cli_auto_route(config, &model, &diff).await?;
    let execution_config = config_for_cli_route(config, &route);
    let model = route.model.clone();
    let reasoning_effort = route
        .reasoning_effort
        .and_then(|effort| cli_reasoning_effort_value(&execution_config, effort));

    let system = SystemPrompt::Text(
        "You are a senior code reviewer. Focus on bugs, risks, behavioral regressions, and missing tests. \
Provide findings ordered by severity with file references, then open questions, then a brief summary."
            .to_string(),
    );
    let user_prompt =
        format!("Review the following diff and provide feedback:\n\n{diff}\n\nEnd of diff.");

    let client = ApiClient::new_detached(&execution_config)?;
    let request = MessageRequest {
        model: model.clone(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: user_prompt,
                cache_control: None,
            }],
        }],
        max_tokens: 4096,
        system: Some(system),
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort,
        stream: Some(false),
        temperature: Some(0.2),
        top_p: Some(0.9),
        response_format: None,
    };

    let response = client.create_message(request).await?;
    let mut output = String::new();
    for block in response.content {
        if let ContentBlock::Text { text, .. } = block {
            output.push_str(&text);
        }
    }
    let receipt = if args.write_receipt {
        let parsed_output = crate::tools::review::ReviewOutput::from_str(&output);
        let receipt = crate::tools::review::build_review_receipt(
            review_target_label(&args),
            &diff,
            route.provider.as_str(),
            &model,
            &parsed_output,
            &output,
            Vec::new(),
        );
        let path =
            crate::tools::review::write_review_receipt(&receipt, args.receipt_path.as_deref())?;
        Some((path, receipt))
    } else {
        None
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "review",
                "model": model,
                "success": true,
                "content": output,
                "receipt_path": receipt
                    .as_ref()
                    .map(|(path, _)| path.display().to_string()),
                "receipt": receipt.as_ref().map(|(_, receipt)| receipt),
            }))?
        );
    } else {
        println!("{output}");
        if let Some((path, _)) = receipt {
            eprintln!("Review receipt written: {}", path.display());
        }
    }
    Ok(())
}

pub(crate) fn validate_review_receipt_args(args: &ReviewArgs) -> Result<()> {
    if args.receipt_path.is_some() && !args.write_receipt && !args.check_receipt {
        bail!("--receipt-path requires --write-receipt or --check-receipt");
    }
    if args.write_receipt && args.check_receipt {
        bail!("--write-receipt and --check-receipt are mutually exclusive");
    }
    Ok(())
}

pub(crate) fn run_review_receipt_check(diff: &str, args: &ReviewArgs) -> Result<()> {
    let (path, receipt) = if let Some(path) = args.receipt_path.as_ref() {
        (
            path.clone(),
            crate::tools::review::read_review_receipt(path)
                .with_context(|| format!("failed to read review receipt {}", path.display()))?,
        )
    } else {
        crate::tools::review::latest_review_receipt_for_diff(diff)?.ok_or_else(|| {
            anyhow!(
                "No review receipt found for the current diff. Run `mimofan review --write-receipt` first, or pass --receipt-path."
            )
        })?
    };
    let validation =
        crate::tools::review::validate_review_receipt_for_diff(diff, &receipt, Some(path.clone()));

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "review_receipt_check",
                "success": validation.passed,
                "validation": review_receipt_validation_public_json(&validation),
            }))?
        );
    } else if validation.passed {
        println!("Review receipt valid: {}", path.display());
    }

    if !validation.passed {
        bail!("Review receipt check failed: {}", validation.reason);
    }
    Ok(())
}

pub(crate) fn review_receipt_validation_public_json(
    validation: &crate::tools::review::ReviewReceiptValidation,
) -> serde_json::Value {
    let unresolved_risk = validation.unresolved_risk.as_ref();
    serde_json::json!({
        "passed": validation.passed,
        "status": review_receipt_validation_status(validation),
        "diff_fingerprint": validation.diff_fingerprint.as_str(),
        "receipt_fingerprint": validation.receipt_fingerprint.as_deref(),
        "unresolved": unresolved_risk.is_some_and(|risk| risk.unresolved),
        "risk_level": unresolved_risk.map(|risk| risk.level.as_str()),
    })
}

pub(crate) fn review_receipt_validation_status(
    validation: &crate::tools::review::ReviewReceiptValidation,
) -> &'static str {
    if validation.passed {
        "valid"
    } else if validation
        .receipt_fingerprint
        .as_deref()
        .is_some_and(|fingerprint| fingerprint != validation.diff_fingerprint.as_str())
    {
        "diff_mismatch"
    } else if validation
        .unresolved_risk
        .as_ref()
        .is_some_and(|risk| risk.unresolved)
    {
        "unresolved_risk"
    } else if validation
        .reason
        .starts_with("unsupported review receipt schema version")
    {
        "unsupported_schema"
    } else if validation.reason.starts_with("review receipt check ") {
        "check_failed"
    } else {
        "invalid"
    }
}

pub(crate) fn collect_diff(args: &ReviewArgs) -> Result<String> {
    let mut cmd = crate::dependencies::Git::command()
        .ok_or_else(|| anyhow::anyhow!("git not found on PATH"))?;
    cmd.arg("diff");
    if args.staged {
        cmd.arg("--cached");
    }
    if let Some(base) = &args.base {
        cmd.arg(format!("{base}...HEAD"));
    }
    if let Some(path) = &args.path {
        cmd.arg("--").arg(path);
    }

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run git diff. Is git installed? ({e})"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git diff failed: {}", stderr.trim());
    }
    let mut diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.len() > args.max_chars {
        diff = truncate_with_ellipsis(&diff, args.max_chars, "\n...[truncated]\n");
    }
    Ok(diff)
}

pub(crate) fn review_target_label(args: &ReviewArgs) -> String {
    let mut label = if args.staged {
        "staged".to_string()
    } else if let Some(base) = args
        .base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
    {
        format!("base:{base}")
    } else {
        "working-tree".to_string()
    };
    if let Some(path) = &args.path {
        label.push(' ');
        label.push_str(path.to_string_lossy().as_ref());
    }
    label
}
