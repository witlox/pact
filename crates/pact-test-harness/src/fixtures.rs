//! Test fixtures and builders for pact types.
//!
//! Uses the builder pattern for constructing test data:
//! ```ignore
//! let config = ConfigEntryBuilder::new()
//!     .vcluster("ml-training")
//!     .author("admin@example.org")
//!     .build();
//! ```

pub use builders::*;

pub mod builders {
    use chrono::Utc;
    use pact_common::types::{
        ConfigEntry, EntryType, Identity, PrincipalType, RestartPolicy, Scope, ServiceDecl,
        StateDelta, VClusterPolicy,
    };

    /// Fluent builder for `ConfigEntry` with sensible test defaults.
    pub struct ConfigEntryBuilder {
        entry_type: EntryType,
        scope: Scope,
        author_name: String,
        author_role: String,
        state_delta: Option<StateDelta>,
        policy_ref: Option<String>,
        ttl_seconds: Option<u32>,
        emergency_reason: Option<String>,
        parent: Option<u64>,
    }

    impl ConfigEntryBuilder {
        #[must_use]
        pub fn new() -> Self {
            Self {
                entry_type: EntryType::Commit,
                scope: Scope::Global,
                author_name: "test-admin@example.com".to_string(),
                author_role: "pact-platform-admin".to_string(),
                state_delta: None,
                policy_ref: None,
                ttl_seconds: None,
                emergency_reason: None,
                parent: None,
            }
        }

        #[must_use]
        pub fn entry_type(mut self, t: EntryType) -> Self {
            self.entry_type = t;
            self
        }

        #[must_use]
        pub fn scope(mut self, s: Scope) -> Self {
            self.scope = s;
            self
        }

        #[must_use]
        pub fn author(mut self, name: &str, role: &str) -> Self {
            self.author_name = name.to_string();
            self.author_role = role.to_string();
            self
        }

        #[must_use]
        pub fn vcluster(self, id: &str) -> Self {
            self.scope(Scope::VCluster(id.to_string()))
        }

        #[must_use]
        pub fn node(self, id: &str) -> Self {
            self.scope(Scope::Node(id.to_string()))
        }

        #[must_use]
        pub fn with_delta(mut self, delta: StateDelta) -> Self {
            self.state_delta = Some(delta);
            self
        }

        #[must_use]
        pub fn parent(mut self, seq: u64) -> Self {
            self.parent = Some(seq);
            self
        }

        #[must_use]
        pub fn policy_ref(mut self, r: &str) -> Self {
            self.policy_ref = Some(r.to_string());
            self
        }

        #[must_use]
        pub fn ttl(mut self, seconds: u32) -> Self {
            self.ttl_seconds = Some(seconds);
            self
        }

        #[must_use]
        pub fn emergency_reason(mut self, reason: &str) -> Self {
            self.emergency_reason = Some(reason.to_string());
            self
        }

        #[must_use]
        pub fn build(self) -> ConfigEntry {
            ConfigEntry {
                sequence: 0,
                timestamp: Utc::now(),
                entry_type: self.entry_type,
                scope: self.scope,
                author: Identity {
                    principal: self.author_name,
                    principal_type: PrincipalType::Human,
                    role: self.author_role,
                },
                parent: self.parent,
                state_delta: self.state_delta,
                policy_ref: self.policy_ref,
                ttl_seconds: self.ttl_seconds,
                emergency_reason: self.emergency_reason,
            }
        }
    }

    impl Default for ConfigEntryBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Fluent builder for `VClusterPolicy` covering all 17 fields.
    pub struct VClusterPolicyBuilder {
        inner: VClusterPolicy,
    }

    impl VClusterPolicyBuilder {
        #[must_use]
        pub fn new() -> Self {
            Self { inner: VClusterPolicy::default() }
        }

        #[must_use]
        pub fn vcluster(mut self, id: &str) -> Self {
            self.inner.vcluster_id = id.to_string();
            self
        }

        #[must_use]
        pub fn drift_sensitivity(mut self, v: f64) -> Self {
            self.inner.drift_sensitivity = v;
            self
        }

        #[must_use]
        pub fn base_commit_window(mut self, seconds: u32) -> Self {
            self.inner.base_commit_window_seconds = seconds;
            self
        }

        #[must_use]
        pub fn enforcement_mode(mut self, mode: &str) -> Self {
            self.inner.enforcement_mode = mode.to_string();
            self
        }

        #[must_use]
        pub fn regulated(mut self, v: bool) -> Self {
            self.inner.regulated = v;
            self
        }

        #[must_use]
        pub fn two_person_approval(mut self, v: bool) -> Self {
            self.inner.two_person_approval = v;
            self
        }

        #[must_use]
        pub fn emergency_allowed(mut self, v: bool) -> Self {
            self.inner.emergency_allowed = v;
            self
        }

        #[must_use]
        pub fn exec_whitelist(mut self, cmds: Vec<String>) -> Self {
            self.inner.exec_whitelist = cmds;
            self
        }

        #[must_use]
        pub fn shell_whitelist(mut self, cmds: Vec<String>) -> Self {
            self.inner.shell_whitelist = cmds;
            self
        }

        #[must_use]
        pub fn supervisor_backend(mut self, backend: &str) -> Self {
            self.inner.supervisor_backend = backend.to_string();
            self
        }

        #[must_use]
        pub fn build(self) -> VClusterPolicy {
            self.inner
        }
    }

    impl Default for VClusterPolicyBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Fluent builder for `ServiceDecl`.
    pub struct ServiceDeclBuilder {
        name: String,
        binary: String,
        args: Vec<String>,
        restart: RestartPolicy,
        restart_delay_seconds: u32,
        depends_on: Vec<String>,
        order: u32,
    }

    impl ServiceDeclBuilder {
        #[must_use]
        pub fn new() -> Self {
            Self {
                name: "test-service".to_string(),
                binary: "/usr/bin/test-service".to_string(),
                args: Vec::new(),
                restart: RestartPolicy::OnFailure,
                restart_delay_seconds: 5,
                depends_on: Vec::new(),
                order: 10,
            }
        }

        #[must_use]
        pub fn name(mut self, n: &str) -> Self {
            self.name = n.to_string();
            self
        }

        #[must_use]
        pub fn binary(mut self, b: &str) -> Self {
            self.binary = b.to_string();
            self
        }

        #[must_use]
        pub fn restart(mut self, r: RestartPolicy) -> Self {
            self.restart = r;
            self
        }

        #[must_use]
        pub fn depends_on(mut self, deps: Vec<String>) -> Self {
            self.depends_on = deps;
            self
        }

        #[must_use]
        pub fn order(mut self, o: u32) -> Self {
            self.order = o;
            self
        }

        #[must_use]
        pub fn build(self) -> ServiceDecl {
            ServiceDecl {
                name: self.name,
                binary: self.binary,
                args: self.args,
                restart: self.restart,
                restart_delay_seconds: self.restart_delay_seconds,
                depends_on: self.depends_on,
                order: self.order,
                cgroup_memory_max: None,
                cgroup_slice: None,
                cgroup_cpu_weight: None,
                health_check: None,
            }
        }
    }

    impl Default for ServiceDeclBuilder {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use pact_common::types::{EntryType, RestartPolicy, Scope};

    use super::builders::*;

    #[test]
    fn config_entry_builder_defaults() {
        let entry = ConfigEntryBuilder::new().build();
        assert_eq!(entry.sequence, 0);
        assert!(matches!(entry.entry_type, EntryType::Commit));
        assert!(matches!(entry.scope, Scope::Global));
        assert_eq!(entry.author.principal, "test-admin@example.com");
    }

    #[test]
    fn config_entry_builder_customization() {
        let entry = ConfigEntryBuilder::new()
            .entry_type(EntryType::Rollback)
            .vcluster("ml-train")
            .author("alice@example.com", "pact-ops-ml")
            .parent(5)
            .build();
        assert!(matches!(entry.entry_type, EntryType::Rollback));
        assert!(matches!(entry.scope, Scope::VCluster(ref id) if id == "ml-train"));
        assert_eq!(entry.author.principal, "alice@example.com");
        assert_eq!(entry.parent, Some(5));
    }

    #[test]
    fn vcluster_policy_builder_defaults() {
        let policy = VClusterPolicyBuilder::new().build();
        assert!(policy.vcluster_id.is_empty());
        assert!((policy.drift_sensitivity - 2.0).abs() < f64::EPSILON);
        assert_eq!(policy.base_commit_window_seconds, 900);
        assert_eq!(policy.enforcement_mode, "observe");
    }

    #[test]
    fn vcluster_policy_builder_chain() {
        let policy = VClusterPolicyBuilder::new()
            .vcluster("regulated-v1")
            .drift_sensitivity(1.0)
            .regulated(true)
            .two_person_approval(true)
            .enforcement_mode("enforce")
            .build();
        assert_eq!(policy.vcluster_id, "regulated-v1");
        assert!((policy.drift_sensitivity - 1.0).abs() < f64::EPSILON);
        assert!(policy.regulated);
        assert!(policy.two_person_approval);
        assert_eq!(policy.enforcement_mode, "enforce");
    }

    #[test]
    fn service_decl_builder_defaults() {
        let decl = ServiceDeclBuilder::new().build();
        assert_eq!(decl.name, "test-service");
        assert_eq!(decl.restart, RestartPolicy::OnFailure);
        assert_eq!(decl.order, 10);
    }

    #[test]
    fn service_decl_builder_customization() {
        let decl = ServiceDeclBuilder::new()
            .name("chronyd")
            .binary("/usr/sbin/chronyd")
            .restart(RestartPolicy::Always)
            .order(1)
            .depends_on(vec!["network".to_string()])
            .build();
        assert_eq!(decl.name, "chronyd");
        assert_eq!(decl.restart, RestartPolicy::Always);
        assert_eq!(decl.order, 1);
        assert_eq!(decl.depends_on, vec!["network"]);
    }
}
