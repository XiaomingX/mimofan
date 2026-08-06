//! Model listing, speech synthesis, and API connectivity test commands.

use super::*;

pub(crate) async fn run_models(config: &Config, args: ModelsArgs) -> Result<()> {
    use crate::client::ApiClient;

    let client = ApiClient::new(config)?;
    let mut models = client.list_models().await?;
    models.sort_by(|a, b| a.id.cmp(&b.id));

    if args.json {
        println!("{}", serde_json::to_string_pretty(&models)?);
        return Ok(());
    }

    if models.is_empty() {
        println!("No models returned by the API.");
        return Ok(());
    }

    let default_model = config.default_model();

    println!("Available models (default: {default_model})");
    for model in models {
        let marker = if model.id == default_model { "*" } else { " " };
        if let Some(owner) = model.owned_by {
            println!("{marker} {} ({owner})", model.id);
        } else {
            println!("{marker} {}", model.id);
        }
    }

    Ok(())
}

pub(crate) async fn run_speech(config: &Config, args: SpeechArgs) -> Result<()> {
    use crate::client::{ApiClient, SpeechSynthesisRequest};
    use crate::config::ApiProvider;
    use crate::tools::speech::{
        DEFAULT_VOICE, SPEECH_MODEL_EXAMPLES, combine_speech_instructions,
        default_speech_output_name, describe_speech_voice, encode_voice_clone_sample_data_uri,
        infer_speech_model, normalize_speech_format,
    };

    let SpeechArgs {
        text,
        output,
        output_dir,
        model,
        voice,
        instruction,
        voice_prompt,
        clone_voice,
        format,
        json: json_output,
    } = args;

    if config.api_provider() != ApiProvider::OpenAiCompatible {
        bail!(
            "`speech` requires provider = \"xiaomi-mimo\" (current: {}). Run with `--provider xiaomi-mimo` or set it in config.",
            config.api_provider().as_str()
        );
    }

    if text.trim().is_empty() {
        bail!("Speech text cannot be empty");
    }
    let voice_is_data_uri = voice
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| value.starts_with("data:audio/"));
    if clone_voice.is_some() && voice.is_some() {
        bail!("Use either --clone-voice or --voice for cloned voice data, not both");
    }
    let model = infer_speech_model(
        model.as_deref(),
        clone_voice.is_some() || voice_is_data_uri,
        voice_prompt.is_some(),
    );
    let model_lower = model.to_ascii_lowercase();
    if !model_lower.contains("tts") {
        bail!(
            "speech requires a TTS model (examples: {}); got {model}",
            SPEECH_MODEL_EXAMPLES.join(", ")
        );
    }
    let is_voice_design = model_lower.contains("voicedesign");
    let is_voice_clone = model_lower.contains("voiceclone");

    let instruction = combine_speech_instructions(instruction, voice_prompt);
    if is_voice_design
        && instruction
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        bail!(
            "mimo-v2.5-tts-voicedesign requires --voice-prompt or --instruction to describe the voice"
        );
    }

    let voice = if let Some(clone_path) = clone_voice {
        Some(encode_voice_clone_sample_data_uri(&clone_path)?)
    } else if is_voice_design {
        None
    } else if let Some(value) = voice.filter(|value| !value.trim().is_empty()) {
        Some(value)
    } else if is_voice_clone {
        bail!("mimo-v2.5-tts-voiceclone requires --clone-voice <mp3|wav> or --voice <data-uri>");
    } else {
        Some(DEFAULT_VOICE.to_string())
    };
    let format = normalize_speech_format(&format).with_context(|| {
        format!("Unsupported speech format '{format}' (allowed: wav, mp3, pcm16)")
    })?;
    let output = output.unwrap_or_else(|| {
        output_dir
            .or_else(|| config.speech_output_dir())
            .unwrap_or_default()
            .join(default_speech_output_name(&format))
    });

    let client = ApiClient::new(config)?;
    let response = client
        .synthesize_speech(SpeechSynthesisRequest {
            model: model.clone(),
            text,
            instruction,
            audio_format: format.clone(),
            voice,
        })
        .await?;

    if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }
    std::fs::write(&output, &response.audio_bytes)
        .with_context(|| format!("Failed to write audio file {}", output.display()))?;

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mode": "speech",
                "success": true,
                "model": response.model,
                "format": response.audio_format,
                "output": output.display().to_string(),
                "bytes": response.audio_bytes.len(),
                "voice": response.voice.as_deref().map(describe_speech_voice),
                "transcript": response.transcript,
            }))?
        );
    } else {
        println!(
            "Generated speech: {} ({} bytes, model: {}, format: {})",
            output.display(),
            response.audio_bytes.len(),
            response.model,
            response.audio_format
        );
    }

    Ok(())
}

/// Test API connectivity by making a minimal request
pub(crate) async fn test_api_connectivity(config: &Config) -> Result<()> {
    use crate::client::ApiClient;
    use crate::models::{ContentBlock, Message, MessageRequest};

    let client = ApiClient::new(config)?;
    let model = client.model().to_string();

    // Minimal request: single word prompt, 1 max token
    let request = MessageRequest {
        model: model.clone(),
        messages: vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
                cache_control: None,
            }],
        }],
        max_tokens: 1,
        system: None,
        tools: None,
        tool_choice: None,
        metadata: None,
        thinking: None,
        reasoning_effort: None,
        stream: Some(false),
        temperature: None,
        top_p: None,
        response_format: None,
    };

    // Use tokio timeout to catch hanging requests
    let timeout_duration = std::time::Duration::from_secs(15);
    match tokio::time::timeout(timeout_duration, client.create_message(request)).await {
        Ok(Ok(_response)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("Request timeout after 15 seconds"),
    }
}

pub(crate) fn rustc_version() -> String {
    let Some(mut cmd) = crate::dependencies::RustC::command() else {
        return "unknown".to_string();
    };
    let Ok(output) = cmd.arg("--version").output() else {
        return "unknown".to_string();
    };
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
