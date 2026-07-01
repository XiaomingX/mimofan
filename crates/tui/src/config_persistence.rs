//! Config file path resolution and TOML persistence helpers.
//!
//! These helpers are used by command handlers and non-command UI code, so
//! persistence lives outside the command tree.

use std::path::{Path, PathBuf};

use crate::config::{ApiProvider, StatusItem, effective_home_dir, expand_path};

pub(crate) fn persist_status_items(items: &[StatusItem]) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(None)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };

    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    let tui_entry = table
        .entry("tui".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let tui_table = tui_entry
        .as_table_mut()
        .context("`tui` section in config.toml must be a table")?;
    let array = items
        .iter()
        .map(|item| toml::Value::String(item.key().to_string()))
        .collect::<Vec<_>>();
    tui_table.insert("status_items".to_string(), toml::Value::Array(array));

    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

pub(crate) fn persist_root_string_key(
    config_path: Option<&Path>,
    key: &str,
    value: &str,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(config_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    table.insert(key.to_string(), toml::Value::String(value.to_string()));
    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

pub(crate) fn persist_root_bool_key(
    config_path: Option<&Path>,
    key: &str,
    value: bool,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(config_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    table.insert(key.to_string(), toml::Value::Boolean(value));
    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

pub(crate) fn persist_tui_integer_key(
    config_path: Option<&Path>,
    key: &str,
    value: u64,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(config_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    let tui_entry = table
        .entry("tui".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let tui_table = tui_entry
        .as_table_mut()
        .context("`tui` section in config.toml must be a table")?;
    let value = i64::try_from(value).context("integer value is too large for TOML")?;
    tui_table.insert(key.to_string(), toml::Value::Integer(value));
    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

pub(crate) fn persist_subagents_bool_key(
    config_path: Option<&Path>,
    key: &str,
    value: bool,
) -> anyhow::Result<PathBuf> {
    persist_subagents_value_key(config_path, key, toml::Value::Boolean(value))
}

pub(crate) fn persist_subagents_integer_key(
    config_path: Option<&Path>,
    key: &str,
    value: u64,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;

    let value = i64::try_from(value).context("integer value is too large for TOML")?;
    persist_subagents_value_key(config_path, key, toml::Value::Integer(value))
}

fn persist_subagents_value_key(
    config_path: Option<&Path>,
    key: &str,
    value: toml::Value,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(config_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    let subagents_entry = table
        .entry("subagents".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
    let subagents_table = subagents_entry
        .as_table_mut()
        .context("`subagents` section in config.toml must be a table")?;
    subagents_table.insert(key.to_string(), value);

    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

pub(crate) fn persist_provider_base_url_key(
    config_path: Option<&Path>,
    provider: ApiProvider,
    value: &str,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    use std::fs;

    let path = config_toml_path(config_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let (mut doc, original_raw) = if path.exists() {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let doc: toml::Value = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        (doc, Some(raw))
    } else {
        (toml::Value::Table(toml::value::Table::new()), None)
    };
    let table = doc
        .as_table_mut()
        .context("config.toml root must be a table")?;
    let providers = table
        .entry("providers".to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .context("`providers` must be a table")?;
    let provider_key = provider_base_url_table_key(provider)?;
    let entry = providers
        .entry(provider_key.to_string())
        .or_insert_with(|| toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .with_context(|| format!("`providers.{provider_key}` must be a table"))?;
    entry.insert(
        "base_url".to_string(),
        toml::Value::String(value.to_string()),
    );

    if let Some(raw) = original_raw {
        save_toml_preserving_comments(&path, &doc, &raw)?;
    } else {
        let body = toml::to_string_pretty(&doc).context("failed to serialize config.toml")?;
        fs::write(&path, body)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
    }
    Ok(path)
}

fn provider_base_url_table_key(provider: ApiProvider) -> anyhow::Result<&'static str> {
    match provider {
        ApiProvider::XiaomiMimo | ApiProvider::XiaomiMimo => {
            anyhow::bail!("DeepSeek uses the root base_url setting")
        }
        ApiProvider::XiaomiMimo => Ok("deepseek_anthropic"),
        ApiProvider::XiaomiMimo => Ok("nvidia_nim"),
        ApiProvider::XiaomiMimo => Ok("openai"),
        ApiProvider::XiaomiMimo => Ok("anthropic"),
        ApiProvider::XiaomiMimo => Ok("atlascloud"),
        ApiProvider::XiaomiMimo => Ok("wanjie_ark"),
        ApiProvider::XiaomiMimo => Ok("volcengine"),
        ApiProvider::XiaomiMimo => Ok("openrouter"),
        ApiProvider::XiaomiMimo => Ok("xiaomi_mimo"),
        ApiProvider::XiaomiMimo => Ok("novita"),
        ApiProvider::XiaomiMimo => Ok("fireworks"),
        ApiProvider::XiaomiMimo | ApiProvider::XiaomiMimo => Ok("siliconflow"),
        ApiProvider::XiaomiMimo => Ok("arcee"),
        ApiProvider::XiaomiMimo => Ok("huggingface"),
        ApiProvider::XiaomiMimo => Ok("deepinfra"),
        ApiProvider::XiaomiMimo => Ok("moonshot"),
        ApiProvider::XiaomiMimo => Ok("together"),
        ApiProvider::XiaomiMimo => Ok("qianfan"),
        ApiProvider::XiaomiMimo => Ok("openai_codex"),
        ApiProvider::XiaomiMimo => Ok("zai"),
        ApiProvider::XiaomiMimo => Ok("stepfun"),
        ApiProvider::XiaomiMimo => Ok("minimax"),
        // Custom providers live under a user-chosen `[providers.<name>]` table,
        // not a fixed key. Persisting base_url through this static-key path is
        // out of scope for the #1519 constrained slice; users edit the named
        // table directly.
        ApiProvider::Custom => {
            anyhow::bail!("custom providers store base_url in their named [providers.<name>] table")
        }
    }
}

pub(crate) fn config_toml_path(config_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    use anyhow::Context;

    if let Some(path) = config_path {
        return Ok(expand_path(path.to_string_lossy().as_ref()));
    }
    if let Ok(env) = std::env::var("MIMOFAN_CONFIG_PATH") {
        let trimmed = env.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    if let Ok(env) = std::env::var("DEEPSEEK_CONFIG_PATH") {
        let trimmed = env.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home =
        effective_home_dir().context("failed to resolve home directory for config.toml path")?;
    let primary = home.join(".mimofan").join("config.toml");
    if primary.exists() {
        return Ok(primary);
    }
    let legacy = home.join(".deepseek").join("config.toml");
    if legacy.exists() {
        return Ok(legacy);
    }
    Ok(primary)
}

/// Write `doc` to `path`, merging comments from `original_raw` so user
/// annotations survive the rewrite.
fn save_toml_preserving_comments(
    path: &Path,
    doc: &toml::Value,
    original_raw: &str,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let serialized = toml::to_string_pretty(doc).context("failed to serialize config.toml")?;
    let body = mimofan_config::merge_and_preserve_comments(&serialized, original_raw)
        .unwrap_or_else(|e| {
            tracing::warn!("failed to merge config comments, saving without them: {e:#}");
            serialized
        });
    std::fs::write(path, body)
        .with_context(|| format!("failed to write config at {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {}
