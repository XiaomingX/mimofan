//! Custom agent definition loading from Markdown files.
//!
//! Scans `~/.mimofan/agents/` and `.mimofan/agents/` directories for
//! Markdown files with YAML frontmatter defining custom agent types.
//!
//! Format:
//! ```markdown
//! ---
//! name: react-expert
//! description: React component specialist
//! tools: read_file, write_file, edit_file, grep_files
//! model: fast
//! ---
//!
//! You are a React component expert. Focus on...
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

/// A custom agent definition loaded from a Markdown file.
#[derive(Debug, Clone)]
pub struct CustomAgentDef {
    /// Unique name identifier (from filename or frontmatter `name` field).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Comma-separated list of allowed tools. Empty = inherit all.
    pub tools: Vec<String>,
    /// Model routing hint: "inherit", "fast", or a specific model ID.
    pub model: String,
    /// The system prompt body (Markdown content after frontmatter).
    pub prompt: String,
    /// Source file path for debugging.
    pub source: PathBuf,
}

/// Registry of custom agent definitions.
#[derive(Debug, Clone, Default)]
pub struct CustomAgentRegistry {
    pub agents: HashMap<String, CustomAgentDef>,
}

impl CustomAgentRegistry {
    /// Scan standard directories and build the registry.
    pub fn load() -> Self {
        let mut agents = HashMap::new();

        // Scan ~/.mimofan/agents/
        if let Some(home) = dirs::home_dir() {
            let global_dir = home.join(".mimofan").join("agents");
            Self::scan_dir(&global_dir, &mut agents);
        }

        // Scan .mimofan/agents/ (project-local)
        let local_dir = PathBuf::from(".mimofan").join("agents");
        Self::scan_dir(&local_dir, &mut agents);

        Self { agents }
    }

    /// Look up a custom agent by name.
    pub fn get(&self, name: &str) -> Option<&CustomAgentDef> {
        self.agents.get(name)
    }

    /// List all available custom agent names.
    pub fn list_names(&self) -> Vec<&str> {
        self.agents.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a name refers to a custom agent.
    pub fn contains(&self, name: &str) -> bool {
        self.agents.contains_key(name)
    }

    pub fn scan_dir(dir: &Path, agents: &mut HashMap<String, CustomAgentDef>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Ok(Some(agent)) = parse_agent_file(&path) {
                agents.insert(agent.name.clone(), agent);
            }
        }
    }
}

/// Parse a Markdown agent file with optional YAML frontmatter.
///
/// Frontmatter format is simple key: value pairs (no nested YAML needed):
/// ```yaml
/// ---
/// name: react-expert
/// description: React specialist
/// tools: read_file, write_file
/// model: fast
/// ---
/// ```
pub fn parse_agent_file(path: &Path) -> Result<Option<CustomAgentDef>> {
    let content = fs::read_to_string(path)?;

    // Extract filename stem as default name
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Check for YAML frontmatter (--- delimited)
    let (frontmatter, body) = if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("---") {
            let yaml_part = &rest[..end];
            let body_part = &rest[end + 3..];
            (Some(yaml_part.trim()), body_part.trim())
        } else {
            (None, content.trim())
        }
    } else {
        (None, content.trim())
    };

    // Parse simple key: value frontmatter
    let mut meta = FrontMatter::default();
    if let Some(yaml) = frontmatter {
        for line in yaml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_lowercase();
                let value = value.trim().to_string();
                match key.as_str() {
                    "name" => meta.name = Some(value),
                    "description" => meta.description = Some(value),
                    "tools" => meta.tools = Some(value),
                    "model" => meta.model = Some(value),
                    _ => {} // Ignore unknown keys
                }
            }
        }
    }

    let name = meta.name.unwrap_or(stem);
    let description = meta.description.unwrap_or_default();
    let tools: Vec<String> = meta
        .tools
        .map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let model = meta.model.unwrap_or_else(|| "inherit".to_string());

    if body.is_empty() {
        return Ok(None);
    }

    Ok(Some(CustomAgentDef {
        name,
        description,
        tools,
        model,
        prompt: body.to_string(),
        source: path.to_path_buf(),
    }))
}

#[derive(Debug, Default)]
struct FrontMatter {
    name: Option<String>,
    description: Option<String>,
    tools: Option<String>,
    model: Option<String>,
}
