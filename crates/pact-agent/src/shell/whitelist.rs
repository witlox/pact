//! Command whitelist management.
//!
//! Enforcement via PATH restriction (ADR-007):
//! - Whitelisted binaries are symlinked into a session-specific directory
//! - PATH is set to only include that directory
//! - rbash prevents changing PATH or running absolute paths
//!
//! State-changing commands are classified for commit window integration.
//! Learning mode captures "command not found" events for whitelist suggestions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tracing::{debug, info};

/// A whitelisted command with metadata.
#[derive(Debug, Clone)]
pub struct WhitelistEntry {
    /// Command name (basename only, e.g., "nvidia-smi").
    pub command: String,
    /// Whether this command can change system state.
    pub state_changing: bool,
    /// Human-readable description.
    pub description: String,
    /// Absolute path to the real binary (resolved at whitelist build time).
    pub binary_path: Option<PathBuf>,
}

/// Manages the command whitelist for exec and shell.
pub struct WhitelistManager {
    /// Commands allowed for `pact exec`.
    exec_whitelist: HashMap<String, WhitelistEntry>,
    /// Commands allowed in `pact shell` (PATH-restricted).
    shell_whitelist: HashMap<String, WhitelistEntry>,
    /// Learning mode: captures denied commands for admin review.
    learning_mode: bool,
    /// Commands denied in learning mode (for admin review).
    denied_commands: Vec<String>,
}

impl WhitelistManager {
    pub fn new(learning_mode: bool) -> Self {
        let mut mgr = Self {
            exec_whitelist: HashMap::new(),
            shell_whitelist: HashMap::new(),
            learning_mode,
            denied_commands: Vec::new(),
        };
        mgr.load_defaults();
        mgr
    }

    /// Load the default whitelist (common HPC diagnostics).
    ///
    /// Security-audited per ADR-007:
    /// - NO vi/vim (`:!bash` escape)
    /// - NO less/more/man (`!cmd` escape) unless LESSSECURE=1
    /// - NO python/perl/ruby (arbitrary code execution)
    /// - NO find (`-exec` flag enables execution)
    fn load_defaults(&mut self) {
        let defaults = vec![
            // GPU diagnostics
            ("nvidia-smi", false, "NVIDIA GPU status and diagnostics"),
            ("rocm-smi", false, "AMD GPU status and diagnostics"),
            // System info (read-only)
            ("echo", false, "Print arguments"),
            ("dmesg", false, "Kernel ring buffer"),
            ("lspci", false, "PCI device listing"),
            ("lsmod", false, "Loaded kernel modules"),
            ("lsblk", false, "Block device listing"),
            ("lscpu", false, "CPU information"),
            ("uname", false, "System information"),
            ("hostname", false, "Show hostname"),
            ("uptime", false, "System uptime"),
            ("free", false, "Memory usage"),
            ("df", false, "Disk usage"),
            ("mount", true, "Mount operations (can mount filesystems)"),
            // Process and resource monitoring
            ("ps", false, "Process listing"),
            ("top", false, "Process monitor"),
            ("htop", false, "Interactive process monitor"),
            // Network diagnostics (ip can modify state with subcommands like 'link set')
            ("ip", true, "Network configuration"),
            ("ss", false, "Socket statistics"),
            ("ping", false, "Network connectivity test"),
            ("traceroute", false, "Network path tracing"),
            ("ethtool", false, "Network interface details"),
            // File inspection (read-only)
            ("cat", false, "Display file contents"),
            ("head", false, "Display first lines"),
            ("tail", false, "Display last lines"),
            ("wc", false, "Word/line count"),
            ("ls", false, "List directory contents"),
            ("stat", false, "File status"),
            ("file", false, "File type identification"),
            ("md5sum", false, "Compute MD5 checksum"),
            ("sha256sum", false, "Compute SHA-256 checksum"),
            ("diff", false, "Compare files"),
            ("grep", false, "Search file contents"),
            // Logs
            ("journalctl", false, "Systemd journal viewer"),
            // Kernel tuning (sysctl -w can modify kernel parameters)
            ("sysctl", true, "Kernel parameter query/set"),
            // State-changing commands
            ("systemctl", true, "Service management"),
            ("modprobe", true, "Load kernel module"),
            ("umount", true, "Unmount filesystem"),
        ];

        for (cmd, state_changing, desc) in defaults {
            let entry = WhitelistEntry {
                command: cmd.into(),
                state_changing,
                description: desc.into(),
                binary_path: None, // resolved at symlink time
            };
            self.exec_whitelist.insert(cmd.into(), entry.clone());
            self.shell_whitelist.insert(cmd.into(), entry);
        }
    }

    /// Check if a command is whitelisted for exec.
    pub fn is_exec_allowed(&self, command: &str) -> bool {
        self.exec_whitelist.contains_key(command)
    }

    /// Check if a command is state-changing.
    pub fn is_state_changing(&self, command: &str) -> bool {
        self.exec_whitelist.get(command).is_some_and(|e| e.state_changing)
    }

    /// Check if a command is whitelisted for shell.
    pub fn is_shell_allowed(&self, command: &str) -> bool {
        self.shell_whitelist.contains_key(command)
    }

    /// Record a denied command (learning mode).
    pub fn record_denied(&mut self, command: &str) {
        if self.learning_mode && !self.denied_commands.contains(&command.to_string()) {
            info!(command, "Learning mode: recording denied command for review");
            self.denied_commands.push(command.into());
        }
    }

    /// Get denied commands for admin review.
    pub fn denied_commands(&self) -> &[String] {
        &self.denied_commands
    }

    /// Clear denied commands (after admin review).
    pub fn clear_denied(&mut self) {
        self.denied_commands.clear();
    }

    /// Update exec whitelist from vCluster policy.
    pub fn update_exec_whitelist(&mut self, commands: &[String]) {
        for cmd in commands {
            if !self.exec_whitelist.contains_key(cmd) {
                debug!(command = %cmd, "Adding command to exec whitelist from policy");
                self.exec_whitelist.insert(
                    cmd.clone(),
                    WhitelistEntry {
                        command: cmd.clone(),
                        state_changing: false, // conservative default
                        description: "Added from vCluster policy".into(),
                        binary_path: None,
                    },
                );
            }
        }
    }

    /// Update shell whitelist from vCluster policy.
    pub fn update_shell_whitelist(&mut self, commands: &[String]) {
        for cmd in commands {
            if !self.shell_whitelist.contains_key(cmd) {
                debug!(command = %cmd, "Adding command to shell whitelist from policy");
                self.shell_whitelist.insert(
                    cmd.clone(),
                    WhitelistEntry {
                        command: cmd.clone(),
                        state_changing: false,
                        description: "Added from vCluster policy".into(),
                        binary_path: None,
                    },
                );
            }
        }
    }

    /// List all exec-whitelisted commands.
    pub fn exec_commands(&self) -> Vec<&WhitelistEntry> {
        let mut entries: Vec<_> = self.exec_whitelist.values().collect();
        entries.sort_by(|a, b| a.command.cmp(&b.command));
        entries
    }

    /// List all shell-whitelisted commands.
    pub fn shell_commands(&self) -> Vec<&WhitelistEntry> {
        let mut entries: Vec<_> = self.shell_whitelist.values().collect();
        entries.sort_by(|a, b| a.command.cmp(&b.command));
        entries
    }

    /// Get the set of shell command names (for symlink creation).
    pub fn shell_command_names(&self) -> HashSet<&str> {
        self.shell_whitelist.keys().map(String::as_str).collect()
    }

    /// Resolve binary paths by searching PATH for each whitelisted command.
    ///
    /// Only resolves commands where `binary_path` is `None`.
    pub fn resolve_binary_paths(&mut self) {
        let path_var = std::env::var("PATH").unwrap_or_default();
        let search_dirs: Vec<&str> = path_var.split(':').collect();

        for entry in self.exec_whitelist.values_mut().chain(self.shell_whitelist.values_mut()) {
            if entry.binary_path.is_some() {
                continue;
            }
            for dir in &search_dirs {
                let candidate = Path::new(dir).join(&entry.command);
                if candidate.exists() {
                    entry.binary_path = Some(candidate);
                    break;
                }
            }
        }
    }

    /// Security check: identify commands with known shell escape vectors.
    ///
    /// Returns commands that should be reviewed before adding to whitelist.
    pub fn audit_escape_vectors(commands: &[&str]) -> Vec<(String, String)> {
        let risky: HashMap<&str, &str> = HashMap::from([
            ("vi", "Can spawn :!bash — use 'view' (read-only) instead"),
            ("vim", "Can spawn :!bash — use 'view' (read-only) instead"),
            ("less", "Can spawn !cmd — set LESSSECURE=1 or exclude"),
            ("more", "Can spawn !cmd — set LESSSECURE=1 or exclude"),
            ("man", "Uses less/more pager — same escape risk"),
            ("python", "Arbitrary code execution"),
            ("python3", "Arbitrary code execution"),
            ("perl", "Arbitrary code execution"),
            ("ruby", "Arbitrary code execution"),
            ("lua", "Arbitrary code execution"),
            ("node", "Arbitrary code execution"),
            ("bash", "Unrestricted shell"),
            ("sh", "Unrestricted shell"),
            ("zsh", "Unrestricted shell"),
            ("find", "-exec flag enables arbitrary execution"),
            ("xargs", "Can execute arbitrary commands"),
            ("awk", "system() function enables execution"),
            ("nawk", "system() function enables execution"),
            ("gawk", "system() function enables execution"),
        ]);

        commands
            .iter()
            .filter_map(|cmd| {
                risky.get(cmd).map(|reason| ((*cmd).to_string(), (*reason).to_string()))
            })
            .collect()
    }

    /// Validate command arguments for sensitive path access (F5 fix).
    ///
    /// Blocks arguments that reference sensitive system paths. This is a
    /// defense-in-depth measure — even whitelisted read-only commands like
    /// `cat` or `grep` should not read private keys or password files.
    pub fn validate_args(command: &str, args: &[String]) -> Result<(), String> {
        /// Paths that must never be accessed via pact exec/shell.
        const BLOCKED_PATHS: &[&str] = &[
            "/etc/shadow",
            "/etc/gshadow",
            "/etc/master.passwd",
            "/root/",
            "/.ssh/",
            "/etc/ssl/private/",
            "/etc/pact/ca/",
            "/run/pact/ca/",
        ];

        /// Path prefixes that require caution — blocked for read commands.
        const SENSITIVE_PREFIXES: &[&str] = &[
            "/etc/shadow",
            "/etc/gshadow",
            "/root",
        ];

        for arg in args {
            // Skip non-path arguments (flags like -c, --help, numbers)
            if !arg.starts_with('/') && !arg.contains("..") {
                continue;
            }

            // Normalize path traversal: resolve .. components
            let normalized = {
                let mut components: Vec<&str> = Vec::new();
                for part in arg.split('/') {
                    match part {
                        ".." => { components.pop(); }
                        "." | "" => {}
                        other => components.push(other),
                    }
                }
                format!("/{}", components.join("/"))
            };

            // Check blocked paths (exact match, prefix match, or contains for patterns like /.ssh/)
            for blocked in BLOCKED_PATHS {
                if normalized == *blocked
                    || normalized.starts_with(blocked)
                    || normalized.contains(blocked)
                {
                    return Err(format!(
                        "access denied: {command} cannot access {arg} (sensitive path)"
                    ));
                }
            }

            // Check sensitive prefixes for file-reading commands
            let is_file_reader = matches!(
                command,
                "cat" | "head" | "tail" | "grep" | "less" | "diff" | "wc" | "file"
                    | "md5sum" | "sha256sum" | "stat"
            );
            if is_file_reader {
                for prefix in SENSITIVE_PREFIXES {
                    if normalized.starts_with(prefix) {
                        return Err(format!(
                            "access denied: {command} cannot read {arg} (sensitive path)"
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_whitelist_includes_diagnostics() {
        let mgr = WhitelistManager::new(false);
        assert!(mgr.is_exec_allowed("nvidia-smi"));
        assert!(mgr.is_exec_allowed("dmesg"));
        assert!(mgr.is_exec_allowed("ps"));
        assert!(mgr.is_exec_allowed("cat"));
        assert!(mgr.is_exec_allowed("ip"));
    }

    #[test]
    fn default_whitelist_excludes_risky_commands() {
        let mgr = WhitelistManager::new(false);
        // These must NOT be in default whitelist (shell escape vectors)
        assert!(!mgr.is_exec_allowed("vi"));
        assert!(!mgr.is_exec_allowed("vim"));
        assert!(!mgr.is_exec_allowed("python"));
        assert!(!mgr.is_exec_allowed("python3"));
        assert!(!mgr.is_exec_allowed("bash"));
        assert!(!mgr.is_exec_allowed("sh"));
        assert!(!mgr.is_exec_allowed("less"));
        assert!(!mgr.is_exec_allowed("find"));
    }

    #[test]
    fn state_changing_classification() {
        let mgr = WhitelistManager::new(false);
        // Read-only commands
        assert!(!mgr.is_state_changing("nvidia-smi"));
        assert!(!mgr.is_state_changing("ps"));
        assert!(!mgr.is_state_changing("cat"));
        assert!(!mgr.is_state_changing("dmesg"));

        // State-changing commands (F1-F3 fix: ip, mount, sysctl reclassified)
        assert!(mgr.is_state_changing("systemctl"));
        assert!(mgr.is_state_changing("modprobe"));
        assert!(mgr.is_state_changing("umount"));
        assert!(mgr.is_state_changing("sysctl"));
        assert!(mgr.is_state_changing("ip"));
        assert!(mgr.is_state_changing("mount"));
    }

    #[test]
    fn state_changing_unknown_command() {
        let mgr = WhitelistManager::new(false);
        assert!(!mgr.is_state_changing("unknown-cmd"));
    }

    #[test]
    fn learning_mode_records_denied() {
        let mut mgr = WhitelistManager::new(true);
        mgr.record_denied("vim");
        mgr.record_denied("python3");
        mgr.record_denied("vim"); // duplicate

        assert_eq!(mgr.denied_commands().len(), 2);
        assert!(mgr.denied_commands().contains(&"vim".to_string()));
        assert!(mgr.denied_commands().contains(&"python3".to_string()));

        mgr.clear_denied();
        assert!(mgr.denied_commands().is_empty());
    }

    #[test]
    fn learning_mode_off_does_not_record() {
        let mut mgr = WhitelistManager::new(false);
        mgr.record_denied("vim");
        assert!(mgr.denied_commands().is_empty());
    }

    #[test]
    fn update_exec_whitelist_from_policy() {
        let mut mgr = WhitelistManager::new(false);
        assert!(!mgr.is_exec_allowed("custom-tool"));

        mgr.update_exec_whitelist(&["custom-tool".into()]);
        assert!(mgr.is_exec_allowed("custom-tool"));
        // Should not be state-changing by default
        assert!(!mgr.is_state_changing("custom-tool"));
    }

    #[test]
    fn update_shell_whitelist_from_policy() {
        let mut mgr = WhitelistManager::new(false);
        assert!(!mgr.is_shell_allowed("custom-diag"));

        mgr.update_shell_whitelist(&["custom-diag".into()]);
        assert!(mgr.is_shell_allowed("custom-diag"));
    }

    #[test]
    fn update_whitelist_does_not_replace_existing() {
        let mut mgr = WhitelistManager::new(false);
        // nvidia-smi already exists with description
        let original_desc = mgr.exec_whitelist.get("nvidia-smi").unwrap().description.clone();

        mgr.update_exec_whitelist(&["nvidia-smi".into()]);
        let after_desc = mgr.exec_whitelist.get("nvidia-smi").unwrap().description.clone();
        assert_eq!(original_desc, after_desc);
    }

    #[test]
    fn exec_commands_sorted() {
        let mgr = WhitelistManager::new(false);
        let commands = mgr.exec_commands();
        let names: Vec<&str> = commands.iter().map(|e| e.command.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }

    #[test]
    fn shell_command_names_returns_set() {
        let mgr = WhitelistManager::new(false);
        let names = mgr.shell_command_names();
        assert!(names.contains("nvidia-smi"));
        assert!(names.contains("ps"));
        assert!(!names.contains("vim"));
    }

    #[test]
    fn audit_escape_vectors_detects_risky() {
        let risky = WhitelistManager::audit_escape_vectors(&["vi", "ps", "python", "cat", "bash"]);
        assert_eq!(risky.len(), 3);

        let risky_names: Vec<&str> = risky.iter().map(|(name, _)| name.as_str()).collect();
        assert!(risky_names.contains(&"vi"));
        assert!(risky_names.contains(&"python"));
        assert!(risky_names.contains(&"bash"));
    }

    #[test]
    fn audit_escape_vectors_safe_commands() {
        let risky = WhitelistManager::audit_escape_vectors(&["ps", "cat", "nvidia-smi", "dmesg"]);
        assert!(risky.is_empty());
    }

    // --- F5: Argument validation ---

    #[test]
    fn validate_args_blocks_shadow() {
        let result = WhitelistManager::validate_args("cat", &["/etc/shadow".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("sensitive path"));
    }

    #[test]
    fn validate_args_blocks_root_home() {
        let result = WhitelistManager::validate_args("cat", &["/root/.bashrc".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn validate_args_blocks_ssh_keys() {
        let result = WhitelistManager::validate_args("cat", &["/home/user/.ssh/id_rsa".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn validate_args_blocks_path_traversal() {
        let result =
            WhitelistManager::validate_args("cat", &["/var/log/../../etc/shadow".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn validate_args_allows_safe_paths() {
        assert!(WhitelistManager::validate_args("cat", &["/var/log/syslog".into()]).is_ok());
        assert!(WhitelistManager::validate_args("cat", &["/proc/cpuinfo".into()]).is_ok());
        assert!(WhitelistManager::validate_args("nvidia-smi", &[]).is_ok());
        assert!(WhitelistManager::validate_args("ps", &["aux".into()]).is_ok());
    }

    #[test]
    fn validate_args_non_reader_allows_sensitive_flag() {
        // Non-file-reading commands can have args that look like paths
        assert!(WhitelistManager::validate_args("ping", &["-c".into(), "1".into()]).is_ok());
    }

    #[test]
    fn validate_args_blocks_ca_keys() {
        let result =
            WhitelistManager::validate_args("cat", &["/etc/pact/ca/key.pem".into()]);
        assert!(result.is_err());
    }
}
