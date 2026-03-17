//! Contract tests for shell and exec invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Shell & Exec Invariants (S1-S6)

// ---------------------------------------------------------------------------
// S1: Whitelist enforcement
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S1
/// Spec: invariants.md § S1 — non-whitelisted commands are rejected
/// If this test didn't exist: arbitrary commands could be executed on nodes,
/// bypassing security policy.
#[test]
fn s1_non_whitelisted_command_rejected() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist();
    let caller = ops_user("ml-training");

    let non_whitelisted_commands = [
        "rm -rf /",
        "curl http://malicious.example.com",
        "python -c 'import os; os.system(\"id\")'",
        "dd if=/dev/zero of=/dev/sda",
        "/usr/bin/gcc exploit.c -o exploit",
    ];

    for cmd in &non_whitelisted_commands {
        let result = shell.exec(cmd, &whitelist, &caller);
        assert_matches!(result, Err(PactError::CommandNotWhitelisted { .. }),
            "non-whitelisted command '{}' must be rejected", cmd);
    }
}

/// Contract: enforcement-map.md § S1
/// Spec: invariants.md § S1 — whitelisted commands are allowed
/// If this test didn't exist: the whitelist could be over-restrictive,
/// blocking legitimate admin operations.
#[test]
fn s1_whitelisted_command_allowed() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist(); // includes default allowed commands
    let caller = ops_user("ml-training");

    // Default whitelist includes common diagnostic commands
    let whitelisted_commands = whitelist.allowed_commands();
    assert!(!whitelisted_commands.is_empty(), "whitelist must have default commands");

    for cmd in &whitelisted_commands {
        let result = shell.exec(cmd, &whitelist, &caller);
        assert!(result.is_ok(),
            "whitelisted command '{}' must be allowed for ops user", cmd);
    }
}

// ---------------------------------------------------------------------------
// S2: Platform admin whitelist bypass
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S2
/// Spec: invariants.md § S2 — platform-admin can exec non-whitelisted
/// If this test didn't exist: platform admins would be locked out of emergency
/// diagnostics requiring non-standard tools.
#[test]
fn s2_platform_admin_bypasses_whitelist() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist();
    let admin = platform_admin();

    let non_whitelisted = "strace -p 1";
    // Verify this IS non-whitelisted for normal users
    let ops = ops_user("ml-training");
    assert_matches!(shell.exec(non_whitelisted, &whitelist, &ops),
        Err(PactError::CommandNotWhitelisted { .. }),
        "command must be non-whitelisted for ops user (precondition)");

    // Platform admin bypasses
    let result = shell.exec(non_whitelisted, &whitelist, &admin);
    assert!(result.is_ok(),
        "platform admin must be able to execute non-whitelisted commands");
}

/// Contract: enforcement-map.md § S2
/// Spec: invariants.md § S2 — bypassed commands still in audit log
/// If this test didn't exist: platform admin bypass could become an unaudited
/// backdoor, violating compliance requirements.
#[test]
fn s2_platform_admin_bypass_still_logged() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist();
    let admin = platform_admin();

    let audit_log_before = shell.audit_log().len();

    let non_whitelisted = "strace -p 1";
    shell.exec(non_whitelisted, &whitelist, &admin).unwrap();

    let audit_log_after = shell.audit_log();
    assert_eq!(audit_log_after.len(), audit_log_before + 1,
        "bypassed exec must still produce an audit log entry");

    let entry = audit_log_after.last().unwrap();
    assert_eq!(entry.command, non_whitelisted);
    assert_eq!(entry.principal, "admin@example.com");
    assert!(entry.whitelist_bypassed,
        "audit entry must indicate whitelist was bypassed");
}

// ---------------------------------------------------------------------------
// S3: Restricted bash environment
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S3
/// Spec: invariants.md § S3 — shell session spawns restricted bash
/// If this test didn't exist: shell could spawn unrestricted bash, giving full
/// system access that bypasses all pact controls.
#[test]
fn s3_shell_uses_rbash() {
    let shell = stub_shell_service();
    let caller = ops_user("ml-training");

    let session = shell.open_session(&caller).unwrap();

    assert_eq!(session.shell_binary(), "/bin/rbash",
        "shell session must use restricted bash");
    assert!(session.env_contains("SHELL", "/bin/rbash"),
        "SHELL env var must be set to rbash");
}

/// Contract: enforcement-map.md § S3
/// Spec: invariants.md § S3 — PATH modification blocked in rbash
/// If this test didn't exist: users could escape rbash restrictions by
/// modifying PATH to include arbitrary directories.
#[test]
fn s3_rbash_cannot_change_path() {
    let shell = stub_shell_service();
    let caller = ops_user("ml-training");
    let session = shell.open_session(&caller).unwrap();

    let result = session.execute("export PATH=/usr/bin:$PATH");
    assert!(result.is_err() || result.unwrap().exit_code != 0,
        "rbash must prevent PATH modification");
}

/// Contract: enforcement-map.md § S3
/// Spec: invariants.md § S3 — output redirection blocked in rbash
/// If this test didn't exist: users could write arbitrary files via redirection,
/// bypassing drift detection for file creation.
#[test]
fn s3_rbash_cannot_redirect() {
    let shell = stub_shell_service();
    let caller = ops_user("ml-training");
    let session = shell.open_session(&caller).unwrap();

    let redirect_attempts = [
        "echo test > /tmp/escape.txt",
        "echo test >> /tmp/escape.txt",
        "cat /etc/passwd > /tmp/passwd.copy",
    ];

    for cmd in &redirect_attempts {
        let result = session.execute(cmd);
        assert!(result.is_err() || result.unwrap().exit_code != 0,
            "rbash must block output redirection: {}", cmd);
    }
}

// ---------------------------------------------------------------------------
// S4: Session audit
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S4
/// Spec: invariants.md § S4 — every exec produces AdminOperation entry
/// If this test didn't exist: exec commands could go unaudited, violating
/// compliance and forensic requirements.
#[test]
fn s4_exec_logged_to_journal() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist();
    let caller = ops_user("ml-training");

    let audit_log_before = shell.audit_log().len();

    // Execute a whitelisted command
    let whitelisted = whitelist.allowed_commands().first().unwrap().clone();
    shell.exec(&whitelisted, &whitelist, &caller).unwrap();

    let audit_log_after = shell.audit_log();
    assert_eq!(audit_log_after.len(), audit_log_before + 1,
        "each exec must produce exactly one AdminOperation audit entry");

    let entry = audit_log_after.last().unwrap();
    assert_eq!(entry.operation_type, "exec");
    assert_eq!(entry.command, whitelisted);
    assert_eq!(entry.principal, "ops@example.com");
    assert!(entry.timestamp.is_some(), "audit entry must have a timestamp");
}

/// Contract: enforcement-map.md § S4
/// Spec: invariants.md § S4 — PROMPT_COMMAND hook logs each shell command
/// If this test didn't exist: interactive shell sessions could execute commands
/// without any audit trail.
#[test]
fn s4_shell_command_logged_via_prompt_command() {
    let shell = stub_shell_service();
    let caller = ops_user("ml-training");
    let session = shell.open_session(&caller).unwrap();

    // Verify PROMPT_COMMAND is set in the session environment
    assert!(session.env_contains_key("PROMPT_COMMAND"),
        "shell session must set PROMPT_COMMAND for audit logging");

    // Execute a command in the session
    let audit_log_before = shell.audit_log().len();
    session.execute("hostname").unwrap();

    let audit_log_after = shell.audit_log();
    assert!(audit_log_after.len() > audit_log_before,
        "PROMPT_COMMAND must log each shell command to the audit trail");
}

// ---------------------------------------------------------------------------
// S5: State-changing commands trigger commit windows
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S5
/// Spec: invariants.md § S5 — state change via exec opens a commit window
/// If this test didn't exist: exec-driven state changes could persist
/// indefinitely without commit, leaving the node in undeclared state.
#[test]
fn s5_state_changing_exec_opens_commit_window() {
    let shell = stub_shell_service();
    let whitelist = stub_whitelist();
    let caller = ops_user("ml-training");
    let commit_manager = stub_commit_window_manager();

    assert!(commit_manager.active_window().is_none(),
        "precondition: no active commit window");

    // Execute a state-changing command (e.g., modifying /etc/hosts)
    // The drift observer detects the change and opens a window
    shell.exec_with_drift_observer("echo '10.0.0.1 test' >> /etc/hosts", &whitelist, &caller, &commit_manager).unwrap();

    // Observer detects the file change → drift → commit window
    assert!(commit_manager.active_window().is_some(),
        "state-changing exec must result in a commit window via drift detection");
}

// ---------------------------------------------------------------------------
// S6: Shell does not pre-classify commands
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § S6
/// Spec: invariants.md § S6 — commands pass through directly, observer detects
/// If this test didn't exist: a pre-classification system could block
/// legitimate commands or create a false sense of security by missing
/// novel state-changing patterns.
#[test]
fn s6_shell_does_not_pre_classify() {
    let shell = stub_shell_service();
    let caller = ops_user("ml-training");
    let session = shell.open_session(&caller).unwrap();

    // Commands are not parsed or classified before execution.
    // The shell passes them through to rbash; the observer detects state changes.
    assert!(!session.has_command_classifier(),
        "shell must NOT have a pre-execution command classifier");

    // Even a clearly state-changing command is not blocked by the shell itself
    // (only by rbash restrictions or whitelist). The observer handles the rest.
    assert!(!session.pre_classifies_commands(),
        "shell must NOT analyze commands before execution — \
         drift observer handles state change detection post-execution");
}
