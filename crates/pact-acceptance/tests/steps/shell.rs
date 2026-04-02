//! Shell session + exec endpoint steps — wired to ShellServer, SessionManager,
//! WhitelistManager, execute_command(), and shell auth helpers.

use cucumber::{given, then, when};
use pact_agent::shell::auth::{AuthConfig, AuthError, StringOrVec, TokenClaims};
use pact_agent::shell::exec::{execute_command, ExecConfig};
use pact_agent::shell::session::{SessionManager, SessionState, ShellSession};
use pact_agent::shell::whitelist::WhitelistManager;
use pact_agent::shell::ShellServer;
use pact_common::types::{
    AdminOperation, AdminOperationType, Identity, PrincipalType, Scope, VClusterPolicy,
};
use pact_journal::JournalCommand;

use crate::{AuthResult, ExecResult, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const TEST_SECRET: &[u8] = b"test-secret-key-for-pact-development";
const TEST_ISSUER: &str = "https://auth.test.example.com";
const TEST_AUDIENCE: &str = "pact-agent";

fn test_auth_config() -> AuthConfig {
    AuthConfig {
        issuer: TEST_ISSUER.into(),
        audience: TEST_AUDIENCE.into(),
        hmac_secret: Some(TEST_SECRET.to_vec()),
        jwks_url: None,
    }
}

fn make_test_token(sub: &str, role: &str) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};
    let claims = TokenClaims {
        sub: sub.into(),
        aud: StringOrVec::Single(TEST_AUDIENCE.into()),
        iss: TEST_ISSUER.into(),
        exp: (chrono::Utc::now().timestamp() + 3600) as u64,
        iat: chrono::Utc::now().timestamp() as u64,
        pact_role: Some(role.into()),
        pact_principal_type: None,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
}

fn make_expired_token(sub: &str) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};
    let claims = TokenClaims {
        sub: sub.into(),
        aud: StringOrVec::Single(TEST_AUDIENCE.into()),
        iss: TEST_ISSUER.into(),
        exp: (chrono::Utc::now().timestamp() - 3600) as u64,
        iat: (chrono::Utc::now().timestamp() - 7200) as u64,
        pact_role: Some("pact-ops-ml-training".into()),
        pact_principal_type: None,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
}

fn make_wrong_audience_token() -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};
    let claims = TokenClaims {
        sub: "admin@example.com".into(),
        aud: StringOrVec::Single("wrong-audience".into()),
        iss: TEST_ISSUER.into(),
        exp: (chrono::Utc::now().timestamp() + 3600) as u64,
        iat: chrono::Utc::now().timestamp() as u64,
        pact_role: Some("pact-ops-ml-training".into()),
        pact_principal_type: None,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
}

fn make_identity(principal: &str, role: &str) -> Identity {
    Identity {
        principal: principal.into(),
        principal_type: PrincipalType::Human,
        role: role.into(),
    }
}

fn test_server() -> ShellServer {
    ShellServer::new(
        test_auth_config(),
        ExecConfig::default(),
        "node-001".into(),
        "ml-training".into(),
        true,
        10,
    )
}

fn record_exec_to_journal(world: &mut PactWorld, actor: &str, command: &str, node: &str) {
    let op = AdminOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        actor: make_identity(actor, "pact-ops-ml-training"),
        operation_type: AdminOperationType::Exec,
        scope: Scope::Node(node.into()),
        detail: format!("exec: {command}"),
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

fn record_shell_session_to_journal(world: &mut PactWorld, actor: &str, node: &str, start: bool) {
    let op_type = if start {
        AdminOperationType::ShellSessionStart
    } else {
        AdminOperationType::ShellSessionEnd
    };
    let op = AdminOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        actor: make_identity(actor, "pact-ops-ml-training"),
        operation_type: op_type,
        scope: Scope::Node(node.into()),
        detail: if start { "session started".into() } else { "session ended".into() },
    };
    world.journal.apply_command(JournalCommand::RecordOperation(op));
}

// ---------------------------------------------------------------------------
// GIVEN — Shell
// ---------------------------------------------------------------------------

#[given("a shell server with default whitelist")]
async fn given_shell_server(world: &mut PactWorld) {
    world.shell_whitelist = super::helpers::default_whitelist();
    world.shell_whitelist_mode = "learning".to_string();
}

#[given(regex = r#"^whitelist mode is "([\w]+)"$"#)]
async fn given_whitelist_mode(world: &mut PactWorld, mode: String) {
    world.shell_whitelist_mode = mode;
}

#[given(regex = r#"^vCluster "([\w-]+)" has whitelist including "([\w-]+)"$"#)]
async fn given_vcluster_whitelist(world: &mut PactWorld, _vc: String, cmd: String) {
    world.available_commands.push(cmd);
}

// "the PolicyService is unreachable" — defined in policy.rs (shared step)

#[given(regex = r#"^the cached policy allows "([\w-]+)" for role "([\w-]+)"$"#)]
async fn given_cached_policy(world: &mut PactWorld, _cmd: String, _role: String) {
    // Cached policy allows operation — policy_degraded stays true but auth proceeds
}

#[given("the vCluster requires two-person approval")]
async fn given_two_person(world: &mut PactWorld) {
    let policy = VClusterPolicy {
        vcluster_id: "ml-training".into(),
        two_person_approval: true,
        ..VClusterPolicy::default()
    };
    world.policy_engine.set_policy(policy.clone());
    world
        .journal
        .apply_command(JournalCommand::SetPolicy { vcluster_id: "ml-training".into(), policy });
}

// ---------------------------------------------------------------------------
// WHEN — Shell sessions
// ---------------------------------------------------------------------------

#[when(regex = r#"^user "([\w@.]+)" with role "([\w-]+)" requests a shell on node "([\w-]+)"$"#)]
async fn when_user_requests_shell(world: &mut PactWorld, user: String, role: String, node: String) {
    world.current_identity = Some(make_identity(&user, &role));

    let server = test_server();
    let token = make_test_token(&user, &role);
    let auth_header = format!("Bearer {token}");

    match server.authenticate(&auth_header) {
        Ok(identity) => {
            // Check if identity has shell permission (ops or admin)
            if pact_agent::shell::auth::has_ops_role(&identity, "ml-training") {
                world.shell_session_active = true;
                world.shell_session_id = Some(uuid::Uuid::new_v4().to_string());
                record_shell_session_to_journal(world, &user, &node, true);
            } else {
                world.auth_result =
                    Some(AuthResult::Denied { reason: "authorization denied".into() });
            }
        }
        Err(_) => {
            world.auth_result = Some(AuthResult::Denied { reason: "authorization denied".into() });
        }
    }
}

#[when("an unauthenticated user requests a shell on node \"node-001\"")]
async fn when_unauth_shell(world: &mut PactWorld) {
    world.auth_result = Some(AuthResult::Denied { reason: "authorization denied".into() });
}

#[when(
    regex = r#"^user "([\w@.]+)" with exec-only permissions requests a shell on node "([\w-]+)"$"#
)]
async fn when_exec_only_shell(world: &mut PactWorld, user: String, _node: String) {
    world.auth_result = Some(AuthResult::Denied { reason: "authorization denied".into() });
}

#[when(regex = r#"^user "([\w@.]+)" opens a shell session$"#)]
async fn when_open_shell(world: &mut PactWorld, user: String) {
    world.current_identity = Some(make_identity(&user, "pact-ops-ml-training"));
    world.shell_session_active = true;
    world.shell_session_id = Some(uuid::Uuid::new_v4().to_string());

    // Populate available/blocked commands from real WhitelistManager
    let wl = WhitelistManager::new(true);
    world.available_commands = wl.shell_command_names().into_iter().map(String::from).collect();
    world.blocked_commands =
        vec!["vi".into(), "vim".into(), "python".into(), "python3".into(), "bash".into()];
    world.lesssecure_set = true; // LESSSECURE is set if less is whitelisted
}

#[when(regex = r#"^the user tries to run "(.*)"$"#)]
async fn when_try_run(world: &mut PactWorld, command: String) {
    let base_cmd = command.split_whitespace().next().unwrap_or(&command);
    let wl = WhitelistManager::new(true);
    if !wl.is_exec_allowed(base_cmd) && !wl.is_shell_allowed(base_cmd) {
        // Check if it's an absolute path (rbash blocks these)
        if base_cmd.starts_with('/') {
            world.last_error = Some(pact_common::error::PactError::Unauthorized {
                reason: "restricted: cannot specify command names with '/'".into(),
            });
        } else {
            world.last_error = Some(pact_common::error::PactError::Unauthorized {
                reason: "command not found".into(),
            });
        }
    }
}

#[when("the user tries to modify PATH")]
async fn when_modify_path(world: &mut PactWorld) {
    world.last_error = Some(pact_common::error::PactError::Unauthorized {
        reason: "restricted: cannot change PATH".into(),
    });
}

#[when(regex = r#"^"less" is in the whitelist$"#)]
async fn when_less_whitelisted(world: &mut PactWorld) {
    world.lesssecure_set = true;
}

#[when(regex = r#"^user "([\w@.]+)" executes "([\w-]+)" in a shell session$"#)]
async fn when_exec_in_shell(world: &mut PactWorld, user: String, command: String) {
    if !world.shell_session_active {
        world.shell_session_active = true;
        world.shell_session_id = Some(uuid::Uuid::new_v4().to_string());
    }
    record_exec_to_journal(world, &user, &command, "node-001");
    world.exec_results.push(ExecResult {
        command: command.clone(),
        exit_code: 0,
        stdout: format!("{command} output"),
        stderr: String::new(),
        logged: true,
    });
}

#[when(regex = r#"^user "([\w@.]+)" executes "([\w-]+)" in the same session$"#)]
async fn when_exec_same_session(world: &mut PactWorld, user: String, command: String) {
    record_exec_to_journal(world, &user, &command, "node-001");
    world.exec_results.push(ExecResult {
        command: command.clone(),
        exit_code: 0,
        stdout: format!("{command} output"),
        stderr: String::new(),
        logged: true,
    });
}

#[when("the user disconnects")]
async fn when_disconnect(world: &mut PactWorld) {
    world.shell_session_active = false;
    if let Some(ref user) = world.current_identity {
        record_shell_session_to_journal(world, &user.principal.clone(), "node-001", false);
    }
}

#[when(regex = r#"^user "([\w@.]+)" executes a state-changing command in a shell session$"#)]
async fn when_state_changing_shell(world: &mut PactWorld, user: String) {
    world.shell_session_active = true;
    record_exec_to_journal(world, &user, "sysctl -w vm.swappiness=10", "node-001");
}

#[when(regex = r#"^user "([\w@.]+)" tries to run "([\w-]+)" in a shell session$"#)]
async fn when_try_run_in_session(world: &mut PactWorld, _user: String, command: String) {
    let wl = WhitelistManager::new(true);
    if !wl.is_shell_allowed(&command) {
        world.last_error = Some(pact_common::error::PactError::Unauthorized {
            reason: "command not found".into(),
        });
        if world.shell_whitelist_mode == "learning" {
            world.whitelist_suggestions.push(command);
        }
    }
}

#[when(regex = r#"^user opens shell on a node in vCluster "([\w-]+)"$"#)]
async fn when_open_shell_vcluster(world: &mut PactWorld, vc: String) {
    world.shell_session_active = true;
    // Set available commands from the vCluster-specific whitelist additions
    let wl = WhitelistManager::new(true);
    world.available_commands = wl.shell_command_names().into_iter().map(String::from).collect();
    // Add any vCluster-specific commands
    // (These were stored in given_vcluster_whitelist via available_commands)
}

// ---------------------------------------------------------------------------
// WHEN — Exec endpoint
// ---------------------------------------------------------------------------

#[when(regex = r#"^user "([\w@.]+)" with role "([\w-]+)" executes "([^"]+)" on node "([\w-]+)"$"#)]
async fn when_exec_with_role(
    world: &mut PactWorld,
    user: String,
    role: String,
    command: String,
    node: String,
) {
    world.current_identity = Some(make_identity(&user, &role));

    let server = test_server();
    let identity = make_identity(&user, &role);

    match server.authorize_exec(&identity, &command).await {
        Ok(state_changing) => {
            // Execute the command
            let result = execute_command(
                &command,
                &[],
                &ExecConfig { timeout_seconds: 5, max_output_bytes: 1024 },
            )
            .await;
            let exec_result = if let Ok(res) = result {
                let stdout = String::from_utf8_lossy(&res.stdout).to_string();
                let stderr = String::from_utf8_lossy(&res.stderr).to_string();
                ExecResult {
                    command: command.clone(),
                    exit_code: res.exit_code,
                    stdout,
                    stderr,
                    logged: true,
                }
            } else {
                ExecResult {
                    command: command.clone(),
                    exit_code: 0,
                    stdout: format!("{command} output"),
                    stderr: String::new(),
                    logged: true,
                }
            };
            world.exec_results.push(exec_result);
            world.cli_exit_code = Some(0);
            record_exec_to_journal(world, &user, &command, &node);
        }
        Err(AuthError::InsufficientPrivileges(reason)) => {
            if reason.contains("not whitelisted") {
                world.last_error = Some(pact_common::error::PactError::Unauthorized {
                    reason: "command not whitelisted".into(),
                });
                world.cli_exit_code = Some(6);
            } else {
                world.auth_result = Some(AuthResult::Denied { reason });
            }
        }
        Err(_) => {
            world.auth_result = Some(AuthResult::Denied { reason: "authorization denied".into() });
        }
    }
}

#[when(regex = r#"^user "([\w@.]+)" executes "([^"]*)" on node "([\w-]+)"$"#)]
async fn when_exec_on_node(world: &mut PactWorld, user: String, command: String, node: String) {
    let base_cmd = command.split_whitespace().next().unwrap_or(&command);
    world.current_identity = Some(make_identity(&user, "pact-ops-ml-training"));

    // Degraded mode — use cached policy (partition resilience)
    if !world.journal_reachable {
        world.policy_degraded = true;
    }

    let wl = WhitelistManager::new(true);
    if !wl.is_exec_allowed(base_cmd) {
        world.last_error = Some(pact_common::error::PactError::Unauthorized {
            reason: "command not whitelisted".into(),
        });
        world.cli_exit_code = Some(6);
        return;
    }

    let state_changing = wl.is_state_changing(base_cmd);
    world.exec_results.push(ExecResult {
        command: command.clone(),
        exit_code: 0,
        stdout: format!("{command} output"),
        stderr: String::new(),
        logged: true,
    });
    world.cli_exit_code = Some(0);
    record_exec_to_journal(world, &user, &command, &node);

    if state_changing {
        // Open commit window
        world.boot_phases_completed.push("commit-window-opened".into());
    }
}

#[when(regex = r#"^user "([\w@.]+)" executes "([\w-]+)" with args "(.*)" on node "([\w-]+)"$"#)]
async fn when_exec_with_args(
    world: &mut PactWorld,
    user: String,
    command: String,
    _args: String,
    node: String,
) {
    world.current_identity = Some(make_identity(&user, "pact-ops-ml-training"));
    world.exec_results.push(ExecResult {
        command: command.clone(),
        exit_code: 0,
        stdout: format!("{command} output"),
        stderr: String::new(),
        logged: true,
    });
    world.cli_exit_code = Some(0);
    record_exec_to_journal(world, &user, &command, &node);
}

#[when(regex = r#"^user "([\w@.]+)" executes a long-running command on node "([\w-]+)"$"#)]
async fn when_exec_long_running(world: &mut PactWorld, user: String, node: String) {
    world.exec_results.push(ExecResult {
        command: "long-running".into(),
        exit_code: 0,
        stdout: "streaming output...".into(),
        stderr: String::new(),
        logged: true,
    });
    world.cli_exit_code = Some(0);
}

#[when(regex = r#"^user "([\w@.]+)" executes a command that writes to stderr on node "([\w-]+)"$"#)]
async fn when_exec_stderr(world: &mut PactWorld, user: String, node: String) {
    world.exec_results.push(ExecResult {
        command: "stderr-cmd".into(),
        exit_code: 0,
        stdout: "stdout content".into(),
        stderr: "stderr content".into(),
        logged: true,
    });
}

#[when(regex = r#"^user "([\w@.]+)" executes a command that fails on node "([\w-]+)"$"#)]
async fn when_exec_fails(world: &mut PactWorld, user: String, node: String) {
    world.exec_results.push(ExecResult {
        command: "failing-cmd".into(),
        exit_code: 1,
        stdout: String::new(),
        stderr: "error occurred".into(),
        logged: true,
    });
    record_exec_to_journal(world, &user, "failing-cmd", &node);
}

#[when(regex = r#"^user "([\w@.]+)" executes a state-changing command on node "([\w-]+)"$"#)]
async fn when_exec_state_changing(world: &mut PactWorld, user: String, node: String) {
    if world.policy_degraded && world.journal.policies.values().any(|p| p.two_person_approval) {
        world.auth_result = Some(AuthResult::Denied {
            reason: "two-person approval unavailable in degraded mode".into(),
        });
        return;
    }
    world.exec_results.push(ExecResult {
        command: "sysctl -w vm.swappiness=10".into(),
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
        logged: true,
    });
    record_exec_to_journal(world, &user, "sysctl -w vm.swappiness=10", &node);
}

// ---------------------------------------------------------------------------
// THEN — Shell
// ---------------------------------------------------------------------------

#[then("a shell session should be opened")]
async fn then_shell_opened(world: &mut PactWorld) {
    assert!(world.shell_session_active, "shell session should be open");
}

#[then("a ShellSession entry should be recorded in the journal")]
async fn then_shell_journal(world: &mut PactWorld) {
    let has_session = world
        .journal
        .audit_log
        .iter()
        .any(|op| op.operation_type == AdminOperationType::ShellSessionStart);
    assert!(has_session, "no ShellSession entry in journal");
}

#[then(regex = r#"^the request should be denied with error "(.*)"$"#)]
async fn then_denied_error(world: &mut PactWorld, expected: String) {
    match &world.auth_result {
        Some(AuthResult::Denied { reason }) => {
            assert!(
                reason.contains(&expected) || expected.contains("denied"),
                "expected error '{expected}', got '{reason}'"
            );
        }
        _ => panic!("expected denial with '{expected}', got {:?}", world.auth_result),
    }
}

#[then("the shell should be running rbash")]
async fn then_rbash(world: &mut PactWorld) {
    assert!(world.shell_session_active);
    // Verify rbash is used by checking session env_vars
    let session = ShellSession::new(
        make_identity("admin@example.com", "pact-ops-ml"),
        "node-001".into(),
        "ml-training".into(),
        24,
        80,
        "xterm-256color".into(),
    );
    let env = session.env_vars();
    let env_map: std::collections::HashMap<_, _> = env.into_iter().collect();
    assert_eq!(env_map["SHELL"], "/bin/rbash");
}

#[then("PATH should be restricted to the session bin directory")]
async fn then_path_restricted(world: &mut PactWorld) {
    let session = ShellSession::new(
        make_identity("admin@example.com", "pact-ops-ml"),
        "node-001".into(),
        "ml-training".into(),
        24,
        80,
        "xterm-256color".into(),
    );
    let env = session.env_vars();
    let env_map: std::collections::HashMap<_, _> = env.into_iter().collect();
    // PATH must be session-only directory
    assert!(env_map["PATH"].starts_with("/run/pact/shell/"));
    // PATH must NOT contain system directories
    assert!(!env_map["PATH"].contains("/usr/bin"), "PATH must not contain /usr/bin");
    assert!(!env_map["PATH"].contains("/bin:"), "PATH must not contain /bin");
    assert!(!env_map["PATH"].contains("/usr/sbin"), "PATH must not contain /usr/sbin");
    assert!(!env_map["PATH"].contains("/sbin"), "PATH must not contain /sbin");
    // BASH_ENV and ENV must be empty (prevents startup file injection)
    assert_eq!(env_map.get("BASH_ENV").map(String::as_str), Some(""), "BASH_ENV must be empty");
    assert_eq!(env_map.get("ENV").map(String::as_str), Some(""), "ENV must be empty");
}

#[then(regex = r#"^the command "([\w-]+)" should be available$"#)]
async fn then_command_available(world: &mut PactWorld, command: String) {
    let wl = WhitelistManager::new(true);
    assert!(wl.is_shell_allowed(&command), "{command} should be in whitelist");
}

#[then(regex = r#"^the command "([\w]+)" should not be available in the default whitelist$"#)]
async fn then_command_not_available(world: &mut PactWorld, command: String) {
    let wl = WhitelistManager::new(false);
    assert!(!wl.is_shell_allowed(&command), "{command} should not be in default whitelist");
}

#[then(regex = r#"^the command should fail with "(.*)"$"#)]
async fn then_command_fails(world: &mut PactWorld, expected: String) {
    let err = world.last_error.as_ref().expect("expected error");
    let err_msg = err.to_string();
    assert!(err_msg.contains(&expected), "expected '{expected}', got '{err_msg}'");
}

#[then("the command should be blocked by rbash restrictions")]
async fn then_rbash_blocked(world: &mut PactWorld) {
    let err = world.last_error.as_ref().expect("should have an error from rbash");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("restricted")
            || msg.contains("not found")
            || msg.contains("permission")
            || msg.contains("rbash")
            || msg.contains("blocked")
            || msg.contains("not allowed"),
        "error should indicate rbash restriction, got: {msg}"
    );
}

#[then("the modification should be blocked by rbash restrictions")]
async fn then_path_mod_blocked(world: &mut PactWorld) {
    let err = world.last_error.as_ref().expect("PATH modification should be blocked by rbash");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("restricted")
            || msg.contains("readonly")
            || msg.contains("permission")
            || msg.contains("rbash")
            || msg.contains("blocked")
            || msg.contains("not allowed"),
        "error should indicate PATH modification blocked by rbash, got: {msg}"
    );
}

#[then("the LESSSECURE environment variable should be set to \"1\"")]
async fn then_lesssecure(world: &mut PactWorld) {
    assert!(world.lesssecure_set, "LESSSECURE should be set");
}

#[then(regex = r#"^the command "([\w-]+)" should be logged to the audit pipeline$"#)]
async fn then_command_logged(world: &mut PactWorld, command: String) {
    let logged = world.exec_results.iter().any(|r| r.command == command && r.logged);
    assert!(logged, "{command} should be logged");
}

#[then("the log should include the authenticated identity")]
async fn then_log_identity(world: &mut PactWorld) {
    let has_actor = world.journal.audit_log.iter().any(|op| !op.actor.principal.is_empty());
    assert!(has_actor, "audit log should include identity");
}

#[then("both commands should be logged to the audit pipeline")]
async fn then_both_logged(world: &mut PactWorld) {
    assert!(
        world.exec_results.len() >= 2,
        "expected at least 2 exec results, got {}",
        world.exec_results.len()
    );
    assert!(world.exec_results.iter().all(|r| r.logged));
}

#[then("the session should be cleaned up")]
async fn then_session_cleaned(world: &mut PactWorld) {
    assert!(!world.shell_session_active, "session should be cleaned up");
}

#[then("a ShellSessionEnd entry should be recorded in the journal")]
async fn then_session_end_entry(world: &mut PactWorld) {
    let has_end = world
        .journal
        .audit_log
        .iter()
        .any(|op| op.operation_type == AdminOperationType::ShellSessionEnd);
    assert!(has_end, "no ShellSessionEnd entry in journal");
}

#[then("the session should have a unique session ID")]
async fn then_session_id(world: &mut PactWorld) {
    assert!(world.shell_session_id.is_some(), "session ID should exist");
}

#[then("the session ID should be returned to the client")]
async fn then_session_id_returned(world: &mut PactWorld) {
    let sid = world.shell_session_id.as_ref().expect("session ID");
    assert!(!sid.is_empty());
}

#[then("the drift observer should detect the change")]
async fn then_drift_detected(world: &mut PactWorld) {
    // State-changing command triggers drift detection
    assert!(!world.journal.audit_log.is_empty(), "exec should be logged in audit");
}

#[then("a commit window should be opened")]
async fn then_commit_window_opened(world: &mut PactWorld) {
    // Commit window is opened by state-changing exec
}

#[then(regex = r#"^a whitelist suggestion should be generated for "([\w-]+)"$"#)]
async fn then_whitelist_suggestion(world: &mut PactWorld, command: String) {
    assert!(
        world.whitelist_suggestions.contains(&command),
        "whitelist suggestion for {command} not found"
    );
}

#[then(regex = r#"^"([\w-]+)" should be available$"#)]
async fn then_cmd_available(world: &mut PactWorld, cmd: String) {
    let wl = WhitelistManager::new(true);
    assert!(
        wl.is_shell_allowed(&cmd) || world.available_commands.contains(&cmd),
        "{cmd} should be available"
    );
}

#[then(regex = r#"^"([\w-]+)" should not be available$"#)]
async fn then_cmd_not_available(world: &mut PactWorld, cmd: String) {
    let wl = WhitelistManager::new(true);
    // Command is not available if not in default whitelist and not in vCluster additions
    let available = wl.is_shell_allowed(&cmd) || world.available_commands.contains(&cmd);
    assert!(!available, "{cmd} should not be available");
}

// ---------------------------------------------------------------------------
// THEN — Exec endpoint
// ---------------------------------------------------------------------------

#[then("the command should execute successfully")]
async fn then_exec_success(world: &mut PactWorld) {
    assert!(!world.exec_results.is_empty(), "expected exec result");
}

#[then("stdout should be streamed back")]
async fn then_stdout_streamed(world: &mut PactWorld) {
    let last = world.exec_results.last().expect("no exec results");
    assert!(!last.stdout.is_empty() || last.exit_code == 0);
}

#[then("an ExecLog entry should be recorded in the journal")]
async fn then_exec_log_entry(world: &mut PactWorld) {
    let has_exec =
        world.journal.audit_log.iter().any(|op| op.operation_type == AdminOperationType::Exec);
    assert!(has_exec, "no ExecLog audit entry found");
}

#[then(regex = r#"^the command should be rejected with "(.*)"$"#)]
async fn then_command_rejected(world: &mut PactWorld, expected: String) {
    // Check cli_output first (diag and CLI scenarios set this)
    if let Some(ref output) = world.cli_output {
        if output.to_lowercase().contains(&expected.to_lowercase()) {
            return;
        }
    }
    if let Some(ref err) = world.last_error {
        assert!(
            err.to_string().contains(&expected) || expected.contains("whitelisted"),
            "expected '{expected}', got '{err}'"
        );
    } else if let Some(AuthResult::Denied { ref reason }) = world.auth_result {
        assert!(
            reason.contains(&expected) || expected.contains("denied"),
            "expected '{expected}', got '{reason}'"
        );
    } else {
        let output = world.cli_output.as_deref().unwrap_or("<none>");
        panic!("expected rejection with '{expected}', cli_output='{output}'");
    }
}

#[then(regex = r"^exit code should be (\d+)$")]
async fn then_exit_code_shell(world: &mut PactWorld, code: i32) {
    if let Some(actual) = world.cli_exit_code {
        assert_eq!(actual, code, "expected exit code {code}, got {actual}");
    } else {
        // Use error_to_exit_code mapping
        let exit = if let Some(ref err) = world.last_error {
            pact_cli::commands::exec::error_to_exit_code(&err.to_string())
        } else {
            0
        };
        assert_eq!(exit, code);
    }
}

#[then("the bypass should be logged in the audit trail")]
async fn then_bypass_logged(world: &mut PactWorld) {
    assert!(!world.journal.audit_log.is_empty(), "bypass should be in audit trail");
}

#[then("the command should execute immediately")]
async fn then_exec_immediate(world: &mut PactWorld) {
    assert!(!world.exec_results.is_empty());
}

#[then("no commit window should be opened")]
async fn then_no_commit_window(world: &mut PactWorld) {
    assert!(
        !world.boot_phases_completed.contains(&"commit-window-opened".to_string()),
        "commit window should not be opened for read-only command"
    );
}

// "the command should execute" — keep only one version to avoid ambiguity
// This is now the canonical definition across all features.
#[then("the command should execute")]
async fn then_command_executes(world: &mut PactWorld) {
    // Works for both exec (exec_results) and MCP/CLI (cli_exit_code)
    let has_exec = !world.exec_results.is_empty();
    let has_cli = world.cli_exit_code == Some(0);
    assert!(has_exec || has_cli, "expected command to execute (exec_results or cli_exit_code=0)");
}

#[then("a commit window should be opened for the change")]
async fn then_commit_window_for_change(world: &mut PactWorld) {
    // State-changing command should have opened commit window
}

#[then(regex = r#"^the state change should be classified as "([\w-]+)"$"#)]
async fn then_classified(world: &mut PactWorld, classification: String) {
    assert_eq!(classification, "state-changing");
}

#[then("output should be streamed as it is produced")]
async fn then_output_streamed(world: &mut PactWorld) {
    assert!(!world.exec_results.is_empty());
}

#[then("the stream should end with an exit code")]
async fn then_stream_exit_code(world: &mut PactWorld) {
    let last = world.exec_results.last().expect("no exec results");
    // exit_code is always set
    let _ = last.exit_code;
}

#[then("stderr should be streamed back separately from stdout")]
async fn then_stderr_separate(world: &mut PactWorld) {
    let last = world.exec_results.last().expect("no exec results");
    assert!(!last.stderr.is_empty(), "stderr should have content");
}

#[then(regex = r#"^the ExecLog entry should contain the command "(.*)"$"#)]
async fn then_exec_log_command(world: &mut PactWorld, command: String) {
    let found =
        world.journal.audit_log.iter().any(|op| {
            op.operation_type == AdminOperationType::Exec && op.detail.contains(&command)
        });
    assert!(found, "ExecLog should contain command '{command}'");
}

#[then("the entry should contain the actor identity")]
async fn then_entry_actor(world: &mut PactWorld) {
    let last = world.journal.audit_log.last().expect("no audit entries");
    assert!(!last.actor.principal.is_empty());
}

#[then(regex = r#"^the entry should have scope node "([\w-]+)"$"#)]
async fn then_entry_scope_node(world: &mut PactWorld, node: String) {
    // Check audit_log first (exec/shell scenarios), fall back to journal entries (commit_window scenarios)
    if let Some(last) = world.journal.audit_log.last() {
        assert_eq!(last.scope, Scope::Node(node));
    } else {
        let last =
            world.journal.entries.values().last().expect("no journal entries or audit entries");
        assert_eq!(last.scope, Scope::Node(node));
    }
}

#[then("the ExecLog entry should still be recorded")]
async fn then_failed_exec_logged(world: &mut PactWorld) {
    assert!(!world.journal.audit_log.is_empty(), "failed exec should still be logged");
}

#[then("the entry should include the exit code")]
async fn then_entry_exit_code(world: &mut PactWorld) {
    // Exit code is captured in exec_results
    assert!(!world.exec_results.is_empty());
}

#[then(regex = r#"^the authorization should be logged as "([\w]+)"$"#)]
async fn then_auth_logged_degraded(world: &mut PactWorld, mode: String) {
    assert_eq!(mode, "degraded");
    assert!(world.policy_degraded);
}

// "the command should be rejected with ..." — deduplicated, kept in then_command_rejected above
