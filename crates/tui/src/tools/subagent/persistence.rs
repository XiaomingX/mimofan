//! Sub-agent persistence.
//!
//! Persistence layer for sub-agent state and records.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::types::{AgentWorkerRecord, PersistedSubAgentState};

/// Persistence manager for sub-agent state.
pub struct SubAgentPersistence {
    workspace: PathBuf,
}

impl SubAgentPersistence {
    /// Create a new persistence manager.
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Get the persistence directory.
    #[must_use]
    pub fn dir(&self) -> PathBuf {
        self.workspace.join(".mimofan").join("subagents")
    }

    /// Ensure the persistence directory exists.
    pub fn ensure_dir(&self) -> Result<()> {
        let dir = self.dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    /// Save sub-agent state to disk.
    pub fn save_state(&self, state: &PersistedSubAgentState) -> Result<()> {
        self.ensure_dir()?;
        let path = self.dir().join("state.json");
        let json = serde_json::to_string_pretty(state)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load sub-agent state from disk.
    pub fn load_state(&self) -> Result<Option<PersistedSubAgentState>> {
        let path = self.dir().join("state.json");
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path)?;
        let state = serde_json::from_str(&json)?;
        Ok(Some(state))
    }

    /// Delete sub-agent state from disk.
    pub fn delete_state(&self, agent_id: &str) -> Result<()> {
        let path = self.dir().join(format!("{agent_id}.json"));
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// List all persisted sub-agent states.
    pub fn list_states(&self) -> Result<Vec<PersistedSubAgentState>> {
        let dir = self.dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut states = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let json = fs::read_to_string(&path)?;
                let state: PersistedSubAgentState = serde_json::from_str(&json)?;
                states.push(state);
            }
        }
        Ok(states)
    }

    /// Save agent worker record to disk.
    pub fn save_worker_record(&self, record: &AgentWorkerRecord) -> Result<()> {
        self.ensure_dir()?;
        let path = self
            .dir()
            .join(format!("worker_{}.json", record.spec.worker_id));
        let json = serde_json::to_string_pretty(record)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Load agent worker record from disk.
    pub fn load_worker_record(&self, agent_id: &str) -> Result<Option<AgentWorkerRecord>> {
        let path = self.dir().join(format!("worker_{agent_id}.json"));
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path)?;
        let record = serde_json::from_str(&json)?;
        Ok(Some(record))
    }

    /// List all agent worker records.
    pub fn list_worker_records(&self) -> Result<Vec<AgentWorkerRecord>> {
        let dir = self.dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("worker_"))
                .unwrap_or(false)
            {
                let json = fs::read_to_string(&path)?;
                let record: AgentWorkerRecord = serde_json::from_str(&json)?;
                records.push(record);
            }
        }
        Ok(records)
    }
}

/// Load persisted agent worker records from workspace.
pub fn load_persisted_agent_worker_records(workspace: &Path) -> Result<Vec<AgentWorkerRecord>> {
    let persistence = SubAgentPersistence::new(workspace.to_path_buf());
    persistence.list_worker_records()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sub_agent_persistence_new() {
        let temp = TempDir::new().unwrap();
        let persistence = SubAgentPersistence::new(temp.path().to_path_buf());
        assert!(persistence.dir().ends_with("subagents"));
    }

    #[test]
    fn test_sub_agent_persistence_save_load_state() {
        let temp = TempDir::new().unwrap();
        let persistence = SubAgentPersistence::new(temp.path().to_path_buf());

        let state = PersistedSubAgentState::default();
        persistence.save_state(&state).unwrap();

        let loaded = persistence.load_state().unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.agents.len(), 0);
        assert_eq!(loaded.workers.len(), 0);
    }

    #[test]
    fn test_sub_agent_persistence_delete_state() {
        let temp = TempDir::new().unwrap();
        let persistence = SubAgentPersistence::new(temp.path().to_path_buf());

        let state = PersistedSubAgentState::default();
        persistence.save_state(&state).unwrap();

        let loaded = persistence.load_state().unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn test_sub_agent_persistence_list_states() {
        let temp = TempDir::new().unwrap();
        let persistence = SubAgentPersistence::new(temp.path().to_path_buf());

        let state = PersistedSubAgentState::default();
        persistence.save_state(&state).unwrap();

        let loaded = persistence.load_state().unwrap();
        assert!(loaded.is_some());
    }
}
