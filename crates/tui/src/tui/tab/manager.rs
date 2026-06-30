//! Tab manager for handling multiple agent sessions

// WIP collaboration surface. The collab/UI pass lives in PR #2753; this
// file's public items are part of the narrow tab-core harvest. The
// `dead_code` allow at the file root makes that explicit.
#![allow(dead_code)]

use super::cross_tab::CrossTabLink;
use super::delegator::{DelegationResult, TaskDelegator};
use super::meeting::{Meeting, MeetingDecision, MeetingManager, MeetingMessage};
use super::{Priority, TabId, TabMetadata, TabStatus, TabType};
use std::collections::HashMap;

/// Maximum number of tabs allowed
pub const MAX_TABS: usize = 9;

/// Tab state including metadata and status
#[derive(Debug, Clone)]
pub struct TabState {
    pub metadata: TabMetadata,
    pub status: TabStatus,
    pub pending_tasks: Vec<String>,
}

impl TabState {
    pub fn new(id: TabId, title: String, tab_type: TabType) -> Self {
        Self {
            metadata: TabMetadata::new(id, title, tab_type),
            status: TabStatus::Idle,
            pending_tasks: Vec::new(),
        }
    }
}

/// Manages multiple tabs and their interactions
pub struct TabManager {
    tabs: Vec<TabState>,
    active_tab: Option<usize>,
    max_tabs: usize,
    cross_tab_links: HashMap<TabId, Vec<CrossTabLink>>,
    delegator: TaskDelegator,
    meeting_manager: MeetingManager,
    groups: super::group::TabGroupManager,
    /// Monotonic counter for assigning unique tab IDs. Independent of wall
    /// clock so two `create_tab` calls in the same nanosecond on fast
    /// machines still get distinct IDs.
    next_tab_id: u64,
}

impl TabManager {
    /// Create a new tab manager
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: None,
            max_tabs: MAX_TABS,
            cross_tab_links: HashMap::new(),
            delegator: TaskDelegator::new(),
            meeting_manager: MeetingManager::new(),
            groups: super::group::TabGroupManager::new(),
            next_tab_id: 1,
        }
    }

    /// Get the number of tabs
    /// Returns the number of tabs currently open.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Returns true if no tabs are open.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Returns the index of the currently active tab, or `None` if no tabs exist.
    pub fn active_index(&self) -> Option<usize> {
        self.active_tab
    }

    /// Returns the ID of the currently active tab, or `None` if no tabs exist.
    pub fn active_id(&self) -> Option<TabId> {
        self.active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.metadata.id)
    }

    /// Returns metadata for all tabs, in tab order.
    pub fn all_tabs(&self) -> Vec<&TabMetadata> {
        self.tabs.iter().map(|t| &t.metadata).collect()
    }

    /// Returns the tab at `index`, or `None` if out of range.
    pub fn get(&self, index: usize) -> Option<&TabState> {
        self.tabs.get(index)
    }

    /// Returns a mutable reference to the tab at `index`, or `None` if out of range.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut TabState> {
        self.tabs.get_mut(index)
    }

    /// Iterate over tabs by `(index, &TabState)` without allocating.
    /// Cheaper than [`Self::all_tabs`] for hot paths like the tab bar renderer.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &TabState)> {
        self.tabs.iter().enumerate()
    }

    /// Create a new tab. The new tab becomes active.
    ///
    /// # Arguments
    /// * `title` - Display title shown in the tab bar
    /// * `tab_type` - Type of tab (Chat/Delegation/Review/Meeting)
    ///
    /// # Returns
    /// The new tab's `TabId`, or `None` if `max_tabs` (default 9) is reached.
    pub fn create_tab(&mut self, title: String, tab_type: TabType) -> Option<TabId> {
        if self.tabs.len() >= self.max_tabs {
            return None;
        }

        let id = TabId::new(self.next_tab_id);
        self.next_tab_id += 1;
        let tab = TabState::new(id, title, tab_type);
        self.tabs.push(tab);
        self.active_tab = Some(self.tabs.len() - 1);
        Some(id)
    }

    /// Create a new chat tab with a default `"Tab N"` title where `N` is the
    /// 1-indexed slot for the new tab. Returns `None` if `max_tabs` is reached.
    pub fn create_default_chat_tab(&mut self) -> Option<TabId> {
        let title = format!("Tab {}", self.tabs.len() + 1);
        self.create_tab(title, TabType::Chat)
    }

    /// Get mutable access to the group manager
    pub fn groups_mut(&mut self) -> &mut super::group::TabGroupManager {
        &mut self.groups
    }

    /// Get immutable access to the group manager
    pub fn groups(&self) -> &super::group::TabGroupManager {
        &self.groups
    }

    /// Create a new tab group
    pub fn create_group(&mut self, name: String, color: super::group::GroupColor) -> String {
        self.groups.create_group(name, color)
    }

    /// Delete a tab group
    pub fn delete_group(&mut self, group_id: &str) -> bool {
        self.groups.delete_group(group_id)
    }

    /// Assign a tab to a group
    pub fn assign_tab_to_group(&mut self, tab_id: TabId, group_id: &str) -> bool {
        self.groups.assign_tab(tab_id, group_id)
    }

    /// Get the group a tab is assigned to
    pub fn tab_group(&self, tab_id: TabId) -> Option<&super::group::TabGroup> {
        self.groups.group_of(tab_id)
    }

    /// List all groups
    pub fn all_groups(&self) -> Vec<&super::group::TabGroup> {
        self.groups.all_groups()
    }

    /// Cycle a tab to the next group
    pub fn cycle_tab_group(&mut self, tab_id: TabId) {
        self.groups.cycle_tab_group(tab_id);
    }

    /// Snapshot the current manager state for persistence
    pub fn snapshot(&self) -> super::persistence::PersistedTabState {
        use super::persistence::PersistedTab;
        let tabs: Vec<PersistedTab> = self
            .tabs
            .iter()
            .map(|t| super::persistence::from_metadata(&t.metadata))
            .collect();
        let delegations: Vec<super::persistence::PersistedDelegation> = self
            .delegator
            .all_pending()
            .iter()
            .map(|t| super::persistence::PersistedDelegation {
                task_id: t.task_id.clone(),
                from_tab: t.from_tab.0,
                to_tab: t.to_tab.0,
                description: t.description.clone(),
                priority: t.priority,
                status: t.status,
                created_at: t.created_at,
                completed_at: t.completed_at,
                result: t.result.clone(),
                was_successful: None,
            })
            .collect();
        let groups: Vec<super::persistence::PersistedGroup> = self
            .groups
            .all_groups()
            .into_iter()
            .map(|g| super::persistence::PersistedGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                color: g.color,
                tab_ids: g.tab_ids.clone(),
                created_at: g.created_at,
            })
            .collect();
        super::persistence::PersistedTabState {
            version: 1,
            saved_at: chrono::Utc::now(),
            active_tab_index: self.active_tab,
            tabs,
            delegations,
            groups,
        }
    }

    /// Save the current state to a file
    pub fn save_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        let state = self.snapshot();
        super::persistence::save_to_file(&state, path)
    }

    /// Restore state from a persisted snapshot. Existing tabs are cleared.
    pub fn restore_from_snapshot(&mut self, state: &super::persistence::PersistedTabState) {
        self.tabs.clear();
        self.active_tab = None;
        self.cross_tab_links.clear();
        self.delegator = super::delegator::TaskDelegator::new();
        self.meeting_manager = super::meeting::MeetingManager::new();
        self.groups = super::group::TabGroupManager::new();
        self.next_tab_id = 1;

        let mut max_seen_id: u64 = 0;
        for p in &state.tabs {
            let meta = super::persistence::to_metadata(p);
            // Advance the monotonic counter past any restored ID so freshly
            // created tabs can never collide with restored ones.
            if meta.id.0 > max_seen_id {
                max_seen_id = meta.id.0;
            }
            let tab = TabState {
                metadata: meta,
                status: TabStatus::Idle,
                pending_tasks: Vec::new(),
            };
            self.tabs.push(tab);
        }
        self.next_tab_id = max_seen_id + 1;

        // Restore active delegations so cross-tab work survives restarts.
        // We honour the persisted status (`InProgress` is preserved) so
        // work-in-progress isn't silently demoted to `Pending` on restart.
        for d in &state.delegations {
            let task = super::delegator::DelegationTask {
                task_id: d.task_id.clone(),
                from_tab: TabId(d.from_tab),
                to_tab: TabId(d.to_tab),
                description: d.description.clone(),
                priority: d.priority,
                status: d.status,
                created_at: d.created_at,
                deadline: None,
                completed_at: d.completed_at,
                result: d.result.clone(),
            };
            self.delegator.pending_tasks.push(task);
        }
        self.delegator.advance_next_id_past_existing_tasks();

        // Restore groups AFTER tabs so tab_ids can reference real tabs
        for g in &state.groups {
            let group = super::group::TabGroup {
                id: g.id.clone(),
                name: g.name.clone(),
                color: g.color,
                tab_ids: g.tab_ids.clone(),
                created_at: g.created_at,
            };
            self.groups.groups.insert(group.id.clone(), group);
            for tab_id in &g.tab_ids {
                self.groups.tab_to_group.insert(*tab_id, g.id.clone());
            }
        }
        self.groups.advance_next_id_past_existing_groups();

        if let Some(idx) = state.active_tab_index {
            if idx < self.tabs.len() {
                self.active_tab = Some(idx);
            } else if !self.tabs.is_empty() {
                self.active_tab = Some(self.tabs.len() - 1);
            }
        } else if !self.tabs.is_empty() {
            self.active_tab = Some(0);
        }
    }

    /// Restore state from a file. Missing file is treated as empty.
    pub fn restore_from_file(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        let state = super::persistence::load_from_file(path)?;
        self.restore_from_snapshot(&state);
        Ok(())
    }

    /// Close a tab by index
    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        // Unassign from any group before removing
        let tab_id = self.tabs[index].metadata.id;
        self.groups.unassign_tab(tab_id);

        self.tabs.remove(index);

        // Adjust active tab index
        if let Some(active) = self.active_tab {
            if index < active {
                self.active_tab = Some(active - 1);
            } else if index == active {
                self.active_tab = if self.tabs.is_empty() {
                    None
                } else if active >= self.tabs.len() {
                    Some(self.tabs.len() - 1)
                } else {
                    Some(active)
                };
            }
        }

        true
    }

    /// Close a tab by ID
    pub fn close_tab_by_id(&mut self, id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.metadata.id == id) {
            self.close_tab(index)
        } else {
            false
        }
    }

    /// Switch to a tab by index
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab = Some(index);
            if let Some(tab) = self.tabs.get_mut(index) {
                tab.metadata.touch();
                tab.metadata.clear_unread();
            }
            true
        } else {
            false
        }
    }

    /// Switch to the next tab
    pub fn switch_to_next(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }

        let next = match self.active_tab {
            Some(i) => (i + 1) % self.tabs.len(),
            None => 0,
        };
        self.switch_to(next)
    }

    /// Switch to the previous tab
    pub fn switch_to_prev(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }

        let prev = match self.active_tab {
            Some(i) => {
                if i == 0 {
                    self.tabs.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.tabs.len() - 1,
        };
        self.switch_to(prev)
    }

    /// Switch to tab by ID
    pub fn switch_to_by_id(&mut self, id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.metadata.id == id) {
            self.switch_to(index)
        } else {
            false
        }
    }

    /// Update tab title
    pub fn update_title(&mut self, index: usize, title: &str) -> bool {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.metadata.title = title.to_string();
            true
        } else {
            false
        }
    }

    /// Update tab status
    pub fn update_status(&mut self, index: usize, status: TabStatus) -> bool {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.status = status;
            true
        } else {
            false
        }
    }

    /// Mark a tab as having unread content
    pub fn mark_unread(&mut self, index: usize) -> bool {
        if let Some(tab) = self.tabs.get_mut(index) {
            if self.active_tab != Some(index) {
                tab.metadata.increment_unread();
            }
            true
        } else {
            false
        }
    }

    /// Get completed delegation results for a tab.
    ///
    /// Despite the historical name, this returns **completed** results
    /// (via `delegator.results_for_tab`), not in-flight tasks. Use
    /// [`Self::pending_delegations`] for tasks that are still pending or
    /// in progress.
    pub fn completed_delegations(&self, id: TabId) -> Vec<&DelegationResult> {
        self.delegator.results_for_tab(id)
    }

    /// Create a link between tabs for cross-tab events
    pub fn create_link(&mut self, from: TabId, to: TabId) {
        let link = CrossTabLink {
            from,
            to,
            created_at: chrono::Utc::now(),
        };
        self.cross_tab_links.entry(from).or_default().push(link);
    }

    /// Remove link between tabs
    pub fn remove_link(&mut self, from: TabId, to: TabId) -> bool {
        if let Some(links) = self.cross_tab_links.get_mut(&from) {
            let initial_len = links.len();
            links.retain(|l| l.to != to);
            links.len() < initial_len
        } else {
            false
        }
    }

    /// Get all links from a tab
    pub fn get_links(&self, from: TabId) -> Vec<&CrossTabLink> {
        self.cross_tab_links
            .get(&from)
            .map(|l| l.iter().collect())
            .unwrap_or_default()
    }

    /// Delegate a task from one tab to another.
    ///
    /// Returns `None` if either the `from` or `to` tab does not currently
    /// exist in the manager. This defensive check prevents orphaned
    /// delegations from being created with stale tab IDs after a tab
    /// has been closed.
    pub fn delegate_task(
        &mut self,
        from: TabId,
        to: TabId,
        description: String,
        priority: Priority,
    ) -> Option<String> {
        let has_from = self.tabs.iter().any(|t| t.metadata.id == from);
        let has_to = self.tabs.iter().any(|t| t.metadata.id == to);
        if !has_from || !has_to {
            return None;
        }
        self.delegator
            .create_delegation(from, to, description, priority)
    }

    /// Complete a delegated task
    pub fn complete_delegation(&mut self, task_id: &str, result: String) {
        self.delegator.complete(task_id, result);
    }

    /// Get pending delegations for a tab
    pub fn pending_delegations(&self, tab_id: TabId) -> Vec<&super::delegator::DelegationTask> {
        self.delegator.pending_for_tab(tab_id)
    }

    /// Take the highest-priority pending delegation for a tab.
    /// The task is marked as in-progress in place (it is not removed until
    /// `complete_delegation` / `fail_delegation` / `cancel_delegation` is
    /// called). Returns the task.
    pub fn take_next_delegation(
        &mut self,
        tab_id: TabId,
    ) -> Option<super::delegator::DelegationTask> {
        self.delegator.take_pending_for_tab(tab_id)
    }

    /// Peek at the next pending delegation for a tab
    pub fn peek_next_delegation(&self, tab_id: TabId) -> Option<&super::delegator::DelegationTask> {
        self.delegator.peek_pending_for_tab(tab_id)
    }

    /// Check if a tab has any pending delegations
    pub fn has_pending_delegations(&self, tab_id: TabId) -> bool {
        self.delegator.peek_pending_for_tab(tab_id).is_some()
    }

    /// Start a meeting.
    ///
    /// Returns `None` if any participant tab does not currently exist in
    /// the manager. This defensive check prevents meetings from being
    /// created with stale tab IDs after a tab has been closed.
    pub fn start_meeting(&mut self, topic: String, participants: Vec<TabId>) -> Option<String> {
        for p in &participants {
            if !self.tabs.iter().any(|t| t.metadata.id == *p) {
                return None;
            }
        }
        self.meeting_manager.start_meeting(topic, participants)
    }

    /// End a meeting
    pub fn end_meeting(&mut self, meeting_id: &str) -> Option<super::meeting::MeetingSummary> {
        self.meeting_manager.end_meeting(meeting_id)
    }

    /// Add a message to a meeting
    pub fn add_meeting_message(&mut self, meeting_id: &str, msg: MeetingMessage) {
        self.meeting_manager.add_message(meeting_id, msg);
    }

    /// Add a decision to a meeting
    pub fn add_meeting_decision(&mut self, meeting_id: &str, decision: MeetingDecision) {
        self.meeting_manager.add_decision(meeting_id, decision);
    }

    /// Get active meeting for a tab
    pub fn active_meeting(&self, tab_id: TabId) -> Option<&Meeting> {
        self.meeting_manager.active_meeting_for(tab_id)
    }

    /// Get an active meeting by ID.
    pub fn meeting(&self, meeting_id: &str) -> Option<&Meeting> {
        self.meeting_manager.get_meeting(meeting_id)
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {}
