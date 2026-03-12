//! Mock implementations of pact service traits.
//!
//! Pattern: `Arc<Mutex<Vec<MockCall>>>` for call recording with
//! configurable responses via builder chain.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pact_common::types::{ConfigEntry, ConfigState, NodeId};

/// Recorded mock call for verification.
#[derive(Debug, Clone)]
pub enum MockCall {
    AppendEntry { entry_type: String },
    GetEntry { sequence: u64 },
    ListEntries { scope: String },
    GetNodeState { node_id: String },
    Evaluate { action: String },
}

/// Mock journal client — records calls and returns configurable responses.
#[derive(Debug, Clone)]
pub struct MockJournalClient {
    calls: Arc<Mutex<Vec<MockCall>>>,
    entries: Arc<Mutex<Vec<ConfigEntry>>>,
    node_states: Arc<Mutex<HashMap<NodeId, ConfigState>>>,
}

impl MockJournalClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            entries: Arc::new(Mutex::new(Vec::new())),
            node_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Pre-populate with entries for responses.
    #[must_use]
    pub fn with_entries(self, entries: Vec<ConfigEntry>) -> Self {
        *self.entries.lock().unwrap() = entries;
        self
    }

    /// Pre-populate with node states for responses.
    #[must_use]
    pub fn with_node_states(self, states: HashMap<NodeId, ConfigState>) -> Self {
        *self.node_states.lock().unwrap() = states;
        self
    }

    /// Record an `append_entry` call and return the next sequence.
    pub fn append_entry(&self, entry: &ConfigEntry) -> u64 {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::AppendEntry { entry_type: format!("{:?}", entry.entry_type) });
        let mut entries = self.entries.lock().unwrap();
        let seq = entries.len() as u64;
        entries.push(entry.clone());
        seq
    }

    /// Record a `get_entry` call.
    pub fn get_entry(&self, sequence: u64) -> Option<ConfigEntry> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::GetEntry { sequence });
        let entries = self.entries.lock().unwrap();
        entries.iter().find(|e| e.sequence == sequence).cloned()
    }

    /// Record a `list_entries` call.
    pub fn list_entries(&self, scope: &str) -> Vec<ConfigEntry> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::ListEntries { scope: scope.to_string() });
        self.entries.lock().unwrap().clone()
    }

    /// Record a `get_node_state` call.
    pub fn get_node_state(&self, node_id: &str) -> Option<ConfigState> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::GetNodeState { node_id: node_id.to_string() });
        self.node_states.lock().unwrap().get(node_id).cloned()
    }

    /// Get all recorded calls for assertion.
    #[must_use]
    pub fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get count of recorded calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Default for MockJournalClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Policy evaluation result for the mock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockPolicyResult {
    Allow,
    Deny(String),
    RequireApproval(String),
}

/// Mock policy engine — configurable allow/deny with call recording.
#[derive(Debug, Clone)]
pub struct MockPolicyEngine {
    calls: Arc<Mutex<Vec<MockCall>>>,
    default_result: Arc<Mutex<MockPolicyResult>>,
    action_overrides: Arc<Mutex<HashMap<String, MockPolicyResult>>>,
}

impl MockPolicyEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            default_result: Arc::new(Mutex::new(MockPolicyResult::Allow)),
            action_overrides: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Allow all actions (default).
    #[must_use]
    pub fn allow_all(self) -> Self {
        *self.default_result.lock().unwrap() = MockPolicyResult::Allow;
        self
    }

    /// Deny all actions.
    #[must_use]
    pub fn deny_all(self) -> Self {
        *self.default_result.lock().unwrap() = MockPolicyResult::Deny("denied by mock".into());
        self
    }

    /// Deny a specific action.
    #[must_use]
    pub fn deny_action(self, action: &str) -> Self {
        self.action_overrides
            .lock()
            .unwrap()
            .insert(action.to_string(), MockPolicyResult::Deny(format!("{action} denied")));
        self
    }

    /// Require approval for a specific action.
    #[must_use]
    pub fn require_approval(self, action: &str) -> Self {
        self.action_overrides.lock().unwrap().insert(
            action.to_string(),
            MockPolicyResult::RequireApproval(format!("approval-{action}")),
        );
        self
    }

    /// Evaluate a policy request.
    pub fn evaluate(&self, action: &str) -> MockPolicyResult {
        let mut calls = self.calls.lock().unwrap();
        calls.push(MockCall::Evaluate { action: action.to_string() });
        let overrides = self.action_overrides.lock().unwrap();
        if let Some(result) = overrides.get(action) {
            return result.clone();
        }
        self.default_result.lock().unwrap().clone()
    }

    /// Get all recorded calls.
    #[must_use]
    pub fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get count of recorded calls.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Default for MockPolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::builders::{ConfigEntryBuilder, VClusterPolicyBuilder};

    #[test]
    fn mock_journal_records_calls() {
        let client = MockJournalClient::new();
        let entry = ConfigEntryBuilder::new().build();
        let seq = client.append_entry(&entry);
        assert_eq!(seq, 0);
        assert_eq!(client.call_count(), 1);
        assert!(matches!(&client.calls()[0], MockCall::AppendEntry { .. }));
    }

    #[test]
    fn mock_journal_with_entries() {
        let entry = ConfigEntryBuilder::new().build();
        let client = MockJournalClient::new().with_entries(vec![entry]);
        let result = client.list_entries("global");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn mock_journal_node_states() {
        let mut states = HashMap::new();
        states.insert("node-1".to_string(), ConfigState::Committed);
        let client = MockJournalClient::new().with_node_states(states);
        assert_eq!(client.get_node_state("node-1"), Some(ConfigState::Committed));
        assert_eq!(client.get_node_state("node-2"), None);
    }

    #[test]
    fn mock_policy_allow_all() {
        let engine = MockPolicyEngine::new().allow_all();
        assert_eq!(engine.evaluate("commit"), MockPolicyResult::Allow);
        assert_eq!(engine.evaluate("exec"), MockPolicyResult::Allow);
    }

    #[test]
    fn mock_policy_deny_all() {
        let engine = MockPolicyEngine::new().deny_all();
        assert!(matches!(engine.evaluate("commit"), MockPolicyResult::Deny(_)));
    }

    #[test]
    fn mock_policy_deny_specific_action() {
        let engine = MockPolicyEngine::new().deny_action("exec");
        assert_eq!(engine.evaluate("commit"), MockPolicyResult::Allow);
        assert!(matches!(engine.evaluate("exec"), MockPolicyResult::Deny(_)));
    }

    #[test]
    fn mock_policy_require_approval() {
        let engine = MockPolicyEngine::new().require_approval("commit");
        assert!(matches!(engine.evaluate("commit"), MockPolicyResult::RequireApproval(_)));
        assert_eq!(engine.evaluate("exec"), MockPolicyResult::Allow);
        assert_eq!(engine.call_count(), 2);
    }

    #[test]
    fn builders_are_usable_from_mocks() {
        // Verify the builder → mock integration works
        let _policy = VClusterPolicyBuilder::new().vcluster("test").build();
        let entry = ConfigEntryBuilder::new().vcluster("test").build();
        let client = MockJournalClient::new();
        client.append_entry(&entry);
        assert_eq!(client.call_count(), 1);
    }
}
