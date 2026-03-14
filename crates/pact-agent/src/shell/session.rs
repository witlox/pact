//! Interactive shell session management (PTY + restricted bash).
//!
//! Shell sessions use restricted bash (rbash) with:
//! - Session-specific PATH: `/run/pact/shell/<sid>/bin/` with symlinks
//! - rbash prevents: changing PATH, running absolute paths, output redirection
//! - PROMPT_COMMAND: logs each command via `$(history 1)` to audit pipeline
//! - Optional mount namespace: hide sensitive paths (configurable per vCluster)
//! - Session-level cgroup for resource limits
//!
//! PTY allocation is Linux-only (via nix crate). macOS uses stubs.

use chrono::{DateTime, Utc};
#[cfg(target_os = "linux")]
use tracing::debug;
use tracing::{info, warn};
use uuid::Uuid;

use pact_common::types::Identity;

/// Session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Session is being set up (PTY allocation, environment).
    Initializing,
    /// Session is active — user is connected.
    Active,
    /// Session is being torn down.
    Closing,
    /// Session is closed.
    Closed,
}

/// Metadata for an active shell session.
#[derive(Debug, Clone)]
pub struct ShellSession {
    /// Unique session identifier.
    pub session_id: String,
    /// Authenticated user identity.
    pub user: Identity,
    /// Node this session is on.
    pub node_id: String,
    /// vCluster context.
    pub vcluster_id: String,
    /// When the session started.
    pub started_at: DateTime<Utc>,
    /// Current state.
    pub state: SessionState,
    /// Number of commands executed (from PROMPT_COMMAND audit).
    pub commands_executed: u32,
    /// Terminal dimensions.
    pub rows: u16,
    pub cols: u16,
    /// TERM environment variable.
    pub term: String,
}

impl ShellSession {
    /// Create a new session.
    pub fn new(
        user: Identity,
        node_id: String,
        vcluster_id: String,
        rows: u16,
        cols: u16,
        term: String,
    ) -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            user,
            node_id,
            vcluster_id,
            started_at: Utc::now(),
            state: SessionState::Initializing,
            commands_executed: 0,
            rows,
            cols,
            term,
        }
    }

    /// Mark session as active.
    pub fn activate(&mut self) {
        self.state = SessionState::Active;
        info!(
            session_id = %self.session_id,
            user = %self.user.principal,
            "Shell session activated"
        );
    }

    /// Mark session as closing.
    pub fn close(&mut self) {
        self.state = SessionState::Closing;
        info!(
            session_id = %self.session_id,
            user = %self.user.principal,
            commands = self.commands_executed,
            "Shell session closing"
        );
    }

    /// Mark session as fully closed.
    pub fn finalize(&mut self) {
        self.state = SessionState::Closed;
    }

    /// Record a command executed in this session.
    pub fn record_command(&mut self) {
        self.commands_executed += 1;
    }

    /// Get session duration.
    pub fn duration_seconds(&self) -> i64 {
        (Utc::now() - self.started_at).num_seconds()
    }

    /// Path to this session's bin directory (for PATH restriction).
    pub fn bin_dir(&self) -> String {
        format!("/run/pact/shell/{}/bin", self.session_id)
    }

    /// Build the restricted bash environment variables.
    pub fn env_vars(&self) -> Vec<(String, String)> {
        vec![
            ("PATH".into(), self.bin_dir()),
            ("HOME".into(), "/tmp".into()),
            ("TERM".into(), self.term.clone()),
            ("LANG".into(), "C.UTF-8".into()),
            ("SHELL".into(), "/bin/rbash".into()),
            (
                "PROMPT_COMMAND".into(),
                format!(
                    "echo \"PACT_AUDIT session={} user={} cmd=$(history 1)\" >> /run/pact/shell/{}/audit.log",
                    self.session_id, self.user.principal, self.session_id
                ),
            ),
            // Prevent escape via bash startup files
            ("BASH_ENV".into(), String::new()),
            ("ENV".into(), String::new()),
        ]
    }
}

/// Manages active shell sessions on this node.
pub struct SessionManager {
    /// Active sessions keyed by session_id.
    sessions: std::collections::HashMap<String, ShellSession>,
    /// Maximum concurrent sessions per node.
    max_sessions: usize,
}

impl SessionManager {
    pub fn new(max_sessions: usize) -> Self {
        Self { sessions: std::collections::HashMap::new(), max_sessions }
    }

    /// Create a new session. Returns error if max sessions exceeded.
    pub fn create_session(
        &mut self,
        user: Identity,
        node_id: String,
        vcluster_id: String,
        rows: u16,
        cols: u16,
        term: String,
    ) -> Result<&ShellSession, SessionError> {
        if self.sessions.len() >= self.max_sessions {
            return Err(SessionError::MaxSessionsExceeded(self.max_sessions));
        }

        let session = ShellSession::new(user, node_id, vcluster_id, rows, cols, term);
        let id = session.session_id.clone();
        self.sessions.insert(id.clone(), session);
        Ok(self.sessions.get(&id).unwrap())
    }

    /// Get a session by ID.
    pub fn get(&self, session_id: &str) -> Option<&ShellSession> {
        self.sessions.get(session_id)
    }

    /// Get a mutable session by ID.
    pub fn get_mut(&mut self, session_id: &str) -> Option<&mut ShellSession> {
        self.sessions.get_mut(session_id)
    }

    /// Remove a closed session.
    pub fn remove(&mut self, session_id: &str) -> Option<ShellSession> {
        self.sessions.remove(session_id)
    }

    /// Get all active sessions.
    pub fn active_sessions(&self) -> Vec<&ShellSession> {
        self.sessions.values().filter(|s| s.state == SessionState::Active).collect()
    }

    /// Get session count.
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// Clean up any sessions that have been closing for too long.
    pub fn cleanup_stale(&mut self, max_age_seconds: i64) -> Vec<String> {
        let now = Utc::now();
        let stale: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                s.state == SessionState::Closing
                    && (now - s.started_at).num_seconds() > max_age_seconds
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in &stale {
            warn!(session_id = %id, "Cleaning up stale session");
            self.sessions.remove(id);
        }

        stale
    }
}

/// Session errors.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("maximum concurrent sessions ({0}) exceeded")]
    MaxSessionsExceeded(usize),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("PTY allocation failed: {0}")]
    PtyFailed(String),
    #[error("session setup failed: {0}")]
    SetupFailed(String),
}

/// Create the session bin directory and symlink whitelisted commands.
///
/// This is Linux-only. On macOS, this is a no-op stub.
#[cfg(target_os = "linux")]
pub fn setup_session_bin_dir(
    session: &ShellSession,
    whitelisted_commands: &std::collections::HashSet<&str>,
) -> Result<(), SessionError> {
    use std::os::unix::fs::symlink;

    let bin_dir = session.bin_dir();
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| SessionError::SetupFailed(format!("mkdir {bin_dir}: {e}")))?;

    let path_var = std::env::var("PATH").unwrap_or_default();
    let search_dirs: Vec<&str> = path_var.split(':').collect();

    for cmd in whitelisted_commands {
        // Find the real binary
        for dir in &search_dirs {
            let real_path = std::path::Path::new(dir).join(cmd);
            if real_path.exists() {
                let link_path = std::path::Path::new(&bin_dir).join(cmd);
                if let Err(e) = symlink(&real_path, &link_path) {
                    debug!(command = %cmd, error = %e, "Failed to symlink command");
                }
                break;
            }
        }
    }

    Ok(())
}

/// Stub for macOS — does not create real symlinks.
#[cfg(not(target_os = "linux"))]
pub fn setup_session_bin_dir(
    _session: &ShellSession,
    _whitelisted_commands: &std::collections::HashSet<&str>,
) -> Result<(), SessionError> {
    Ok(())
}

/// Clean up a session's bin directory.
#[cfg(target_os = "linux")]
pub fn cleanup_session_bin_dir(session: &ShellSession) {
    let bin_dir = session.bin_dir();
    if let Err(e) = std::fs::remove_dir_all(&bin_dir) {
        debug!(session_id = %session.session_id, error = %e, "Failed to clean up session bin dir");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn cleanup_session_bin_dir(_session: &ShellSession) {}

// ---------------------------------------------------------------------------
// PTY allocation — Linux implementation
// ---------------------------------------------------------------------------

/// Handle to an allocated PTY with the child shell process.
#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct PtyHandle {
    /// Master file descriptor of the PTY pair.
    master_fd: std::os::unix::io::RawFd,
    /// PID of the child shell process.
    child_pid: nix::unistd::Pid,
}

#[cfg(target_os = "linux")]
impl PtyHandle {
    /// Write data to the master side of the PTY (sends input to the shell).
    pub fn write(&self, data: &[u8]) -> Result<usize, SessionError> {
        nix::unistd::write(self.master_fd, data)
            .map_err(|e| SessionError::PtyFailed(format!("write to master fd: {e}")))
    }

    /// Read data from the master side of the PTY (receives output from the shell).
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, SessionError> {
        nix::unistd::read(self.master_fd, buf)
            .map_err(|e| SessionError::PtyFailed(format!("read from master fd: {e}")))
    }

    /// Resize the PTY terminal window.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), SessionError> {
        let ws = nix::pty::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: TIOCSWINSZ is a well-defined ioctl for setting terminal window size.
        // The winsize struct is stack-allocated and valid for the duration of the call.
        let ret =
            unsafe { nix::libc::ioctl(self.master_fd, nix::libc::TIOCSWINSZ, &ws as *const _) };
        if ret == -1 {
            Err(SessionError::PtyFailed(format!(
                "TIOCSWINSZ ioctl failed: {}",
                std::io::Error::last_os_error()
            )))
        } else {
            Ok(())
        }
    }

    /// Close the PTY session: send SIGHUP to the child and close the master fd.
    pub fn close(self) -> Result<(), SessionError> {
        // Send SIGHUP to the child process (standard terminal hangup signal).
        let _ = nix::sys::signal::kill(self.child_pid, nix::sys::signal::Signal::SIGHUP);

        // Close the master fd.
        let _ = nix::unistd::close(self.master_fd);

        // Reap the child to avoid zombies.
        let _ = nix::sys::wait::waitpid(self.child_pid, None);

        info!(child_pid = %self.child_pid, "PTY session closed");
        Ok(())
    }

    /// Returns the master file descriptor (for use with poll/select/epoll).
    pub fn master_fd(&self) -> std::os::unix::io::RawFd {
        self.master_fd
    }

    /// Returns the child process PID.
    pub fn child_pid(&self) -> nix::unistd::Pid {
        self.child_pid
    }
}

/// Allocate a PTY pair and fork a restricted bash shell for the given session.
///
/// The child process:
/// - Creates a new session with `setsid()`
/// - Sets the slave PTY as stdin/stdout/stderr via `dup2()`
/// - Configures environment variables from the session
/// - Exec's `/bin/rbash` (restricted bash)
///
/// The parent process returns a [`PtyHandle`] holding the master fd and child pid.
#[cfg(target_os = "linux")]
pub fn allocate_pty(session: &ShellSession) -> Result<PtyHandle, SessionError> {
    use nix::pty::openpty;
    use nix::unistd::{dup2, execve, fork, setsid, ForkResult};
    use std::ffi::CString;

    // Set initial window size from session dimensions.
    let win_size = Some(nix::pty::Winsize {
        ws_row: session.rows,
        ws_col: session.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    });

    // Allocate PTY pair.
    let pty = openpty(win_size.as_ref(), None)
        .map_err(|e| SessionError::PtyFailed(format!("openpty: {e}")))?;

    let master_fd = pty.master;
    let slave_fd = pty.slave;

    // Fork.
    // SAFETY: We are single-threaded at the point of fork in the child path
    // (the child immediately sets up fds and exec's). The parent path is safe
    // because we only record the child pid and close the slave fd.
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // --- Child process ---

            // Close the master fd in the child — only the parent uses it.
            let _ = nix::unistd::close(master_fd);

            // Create a new session and set the slave as the controlling terminal.
            setsid().map_err(|e| SessionError::PtyFailed(format!("setsid: {e}")))?;

            // Set slave PTY as stdin/stdout/stderr.
            dup2(slave_fd, 0).map_err(|e| SessionError::PtyFailed(format!("dup2 stdin: {e}")))?;
            dup2(slave_fd, 1).map_err(|e| SessionError::PtyFailed(format!("dup2 stdout: {e}")))?;
            dup2(slave_fd, 2).map_err(|e| SessionError::PtyFailed(format!("dup2 stderr: {e}")))?;

            // Close the original slave fd (now duplicated to 0/1/2).
            if slave_fd > 2 {
                let _ = nix::unistd::close(slave_fd);
            }

            // Build environment from session.
            let env_vars: Vec<CString> = session
                .env_vars()
                .into_iter()
                .filter_map(|(k, v)| CString::new(format!("{k}={v}")).ok())
                .collect();

            let shell =
                CString::new("/bin/rbash").expect("CString::new failed for /bin/rbash path");
            let argv = [shell.clone()];

            // Exec rbash — this does not return on success.
            let _ = execve(&shell, &argv, &env_vars);

            // If execve fails, exit the child immediately.
            std::process::exit(127);
        }
        Ok(ForkResult::Parent { child }) => {
            // --- Parent process ---

            // Close the slave fd in the parent — only the child uses it.
            let _ = nix::unistd::close(slave_fd);

            info!(
                session_id = %session.session_id,
                child_pid = %child,
                "PTY allocated for shell session"
            );

            Ok(PtyHandle { master_fd, child_pid: child })
        }
        Err(e) => Err(SessionError::PtyFailed(format!("fork: {e}"))),
    }
}

// ---------------------------------------------------------------------------
// PTY allocation — non-Linux stub
// ---------------------------------------------------------------------------

/// Stub PTY handle for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
#[derive(Debug)]
pub struct PtyHandle {
    _private: (),
}

#[cfg(not(target_os = "linux"))]
impl PtyHandle {
    /// Always returns an error on non-Linux platforms.
    pub fn write(&self, _data: &[u8]) -> Result<usize, SessionError> {
        Err(SessionError::PtyFailed("PTY not supported on this platform".into()))
    }

    /// Always returns an error on non-Linux platforms.
    pub fn read(&self, _buf: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::PtyFailed("PTY not supported on this platform".into()))
    }

    /// Always returns an error on non-Linux platforms.
    pub fn resize(&self, _rows: u16, _cols: u16) -> Result<(), SessionError> {
        Err(SessionError::PtyFailed("PTY not supported on this platform".into()))
    }

    /// No-op on non-Linux platforms.
    pub fn close(self) -> Result<(), SessionError> {
        Err(SessionError::PtyFailed("PTY not supported on this platform".into()))
    }
}

/// Stub for non-Linux: always returns an error.
#[cfg(not(target_os = "linux"))]
pub fn allocate_pty(_session: &ShellSession) -> Result<PtyHandle, SessionError> {
    Err(SessionError::PtyFailed("PTY allocation not supported on this platform".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::PrincipalType;

    fn test_user() -> Identity {
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml".into(),
        }
    }

    #[test]
    fn session_lifecycle() {
        let mut session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        assert_eq!(session.state, SessionState::Initializing);
        assert_eq!(session.commands_executed, 0);
        assert!(!session.session_id.is_empty());

        session.activate();
        assert_eq!(session.state, SessionState::Active);

        session.record_command();
        session.record_command();
        assert_eq!(session.commands_executed, 2);

        session.close();
        assert_eq!(session.state, SessionState::Closing);

        session.finalize();
        assert_eq!(session.state, SessionState::Closed);
    }

    #[test]
    fn session_env_vars() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        let env = session.env_vars();
        let env_map: std::collections::HashMap<_, _> = env.into_iter().collect();

        // PATH should be session-specific bin directory
        assert_eq!(env_map["PATH"], format!("/run/pact/shell/{}/bin", session.session_id));
        // TERM preserved
        assert_eq!(env_map["TERM"], "xterm-256color");
        // SHELL should be rbash
        assert_eq!(env_map["SHELL"], "/bin/rbash");
        // Escape prevention
        assert_eq!(env_map["BASH_ENV"], "");
        assert_eq!(env_map["ENV"], "");
        // PROMPT_COMMAND should include session_id and user
        assert!(env_map["PROMPT_COMMAND"].contains(&session.session_id));
        assert!(env_map["PROMPT_COMMAND"].contains("admin@example.com"));
    }

    #[test]
    fn session_bin_dir_includes_session_id() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        let bin_dir = session.bin_dir();
        assert!(bin_dir.starts_with("/run/pact/shell/"));
        assert!(bin_dir.ends_with("/bin"));
        assert!(bin_dir.contains(&session.session_id));
    }

    #[test]
    fn session_duration() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );
        // Duration should be very small (just created)
        assert!(session.duration_seconds() >= 0);
        assert!(session.duration_seconds() < 2);
    }

    #[test]
    fn session_manager_create_and_query() {
        let mut mgr = SessionManager::new(10);
        assert_eq!(mgr.count(), 0);

        let session = mgr
            .create_session(
                test_user(),
                "node-001".into(),
                "ml-training".into(),
                24,
                80,
                "xterm-256color".into(),
            )
            .unwrap();
        let sid = session.session_id.clone();

        assert_eq!(mgr.count(), 1);
        assert!(mgr.get(&sid).is_some());
        assert_eq!(mgr.get(&sid).unwrap().user.principal, "admin@example.com");
    }

    #[test]
    fn session_manager_max_sessions() {
        let mut mgr = SessionManager::new(2);

        mgr.create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into()).unwrap();
        mgr.create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into()).unwrap();

        let result =
            mgr.create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into());
        assert!(matches!(result, Err(SessionError::MaxSessionsExceeded(2))));
    }

    #[test]
    fn session_manager_active_sessions() {
        let mut mgr = SessionManager::new(10);

        let s1 = mgr
            .create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into())
            .unwrap();
        let sid1 = s1.session_id.clone();
        let s2 = mgr
            .create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into())
            .unwrap();
        let sid2 = s2.session_id.clone();

        // No active sessions yet (all Initializing)
        assert!(mgr.active_sessions().is_empty());

        // Activate one
        mgr.get_mut(&sid1).unwrap().activate();
        assert_eq!(mgr.active_sessions().len(), 1);

        // Activate second
        mgr.get_mut(&sid2).unwrap().activate();
        assert_eq!(mgr.active_sessions().len(), 2);

        // Close one
        mgr.get_mut(&sid1).unwrap().close();
        assert_eq!(mgr.active_sessions().len(), 1);
    }

    #[test]
    fn session_manager_remove() {
        let mut mgr = SessionManager::new(10);
        let session = mgr
            .create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into())
            .unwrap();
        let sid = session.session_id.clone();

        assert_eq!(mgr.count(), 1);
        let removed = mgr.remove(&sid);
        assert!(removed.is_some());
        assert_eq!(mgr.count(), 0);
        assert!(mgr.get(&sid).is_none());
    }

    #[test]
    fn cleanup_stale_removes_old_closing_sessions() {
        let mut mgr = SessionManager::new(10);

        // Create and activate, then close
        let s1 = mgr
            .create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into())
            .unwrap();
        let sid1 = s1.session_id.clone();
        mgr.get_mut(&sid1).unwrap().activate();
        mgr.get_mut(&sid1).unwrap().close();

        // Create another that stays active
        let s2 = mgr
            .create_session(test_user(), "n1".into(), "vc".into(), 24, 80, "xterm".into())
            .unwrap();
        let sid2 = s2.session_id.clone();
        mgr.get_mut(&sid2).unwrap().activate();

        assert_eq!(mgr.count(), 2);

        // Cleanup with very large max_age — nothing should be removed
        // (sessions are just created, so their age is ~0 seconds)
        let stale = mgr.cleanup_stale(999_999);
        assert!(stale.is_empty());
        assert_eq!(mgr.count(), 2);

        // Manually backdate the closing session's started_at so it appears stale
        mgr.get_mut(&sid1).unwrap().started_at =
            chrono::Utc::now() - chrono::Duration::seconds(120);

        // Cleanup with max_age=60 — session started 120s ago should be removed
        let stale = mgr.cleanup_stale(60);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], sid1);
        assert_eq!(mgr.count(), 1);

        // Active session still there
        assert!(mgr.get(&sid2).is_some());
        assert_eq!(mgr.get(&sid2).unwrap().state, SessionState::Active);
    }

    #[test]
    fn env_vars_prevent_bash_escape_vectors() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        let env = session.env_vars();
        let env_map: std::collections::HashMap<_, _> = env.into_iter().collect();

        // rbash restriction: SHELL must be rbash
        assert_eq!(env_map["SHELL"], "/bin/rbash");
        // Prevent startup file injection
        assert!(
            env_map["BASH_ENV"].is_empty(),
            "BASH_ENV must be empty to prevent startup injection"
        );
        assert!(env_map["ENV"].is_empty(), "ENV must be empty to prevent startup injection");
        // PATH must NOT contain standard system dirs (only session bin dir)
        assert!(!env_map["PATH"].contains("/usr/bin"));
        assert!(!env_map["PATH"].contains("/usr/sbin"));
        assert!(env_map["PATH"].starts_with("/run/pact/shell/"));
        // HOME must not be a real user home
        assert_eq!(env_map["HOME"], "/tmp");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn allocate_pty_stub_returns_error() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        let result = allocate_pty(&session);
        assert!(result.is_err());
        assert!(matches!(result, Err(SessionError::PtyFailed(_))));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn pty_handle_stub_methods_return_errors() {
        let handle = PtyHandle { _private: () };
        assert!(handle.write(b"test").is_err());
        assert!(handle.read(&mut [0u8; 64]).is_err());
        assert!(handle.resize(40, 120).is_err());
        assert!(handle.close().is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn allocate_pty_creates_handle() {
        // This test requires /bin/sh at minimum (rbash may not exist in CI).
        // We test the PTY allocation path by forking a simple shell.
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        // allocate_pty requires /bin/rbash — if it doesn't exist, the child
        // exits with code 127. The parent still gets a valid PtyHandle.
        let handle = allocate_pty(&session);

        // On systems without /bin/rbash, the fork still succeeds (child exits 127).
        // On systems with /bin/rbash, we get a real shell.
        // Either way, PtyHandle should be returned successfully.
        if let Ok(h) = handle {
            assert!(h.master_fd() >= 0);
            assert!(h.child_pid().as_raw() > 0);
            // Clean up
            let _ = h.close();
        }
        // If handle is Err, openpty itself failed (e.g., no /dev/ptmx) — acceptable in CI.
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pty_handle_resize() {
        let session = ShellSession::new(
            test_user(),
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );

        if let Ok(h) = allocate_pty(&session) {
            // Resize should succeed on a valid PTY.
            let resize_result = h.resize(40, 120);
            assert!(resize_result.is_ok());
            let _ = h.close();
        }
    }
}
