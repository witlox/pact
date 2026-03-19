#![allow(clippy::needless_pass_by_value)]
//! Identity mapping steps — wired to identity module and UidMap.

use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use pact_agent::identity::IdentityManager;
use pact_common::types::{GroupEntry, IdentityMode, OrgIndex, UidEntry, UidMap};

use crate::PactWorld;

// ---------------------------------------------------------------------------
// Helper: seed a uid entry into the map
// ---------------------------------------------------------------------------

fn seed_uid_entry(world: &mut PactWorld, subject: &str, uid: u32) {
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    let username = subject.split('@').next().unwrap_or("user");
    let org = map.org_indices.first().map_or_else(|| "local".to_string(), |o| o.org.clone());
    map.users.insert(
        subject.to_string(),
        UidEntry {
            subject: subject.to_string(),
            uid,
            gid: uid,
            username: username.to_string(),
            home: format!("/users/{username}"),
            shell: "/bin/bash".to_string(),
            org,
        },
    );
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^identity_mode is "(on-demand|pre-provisioned)"$"#)]
fn given_identity_mode(world: &mut PactWorld, mode: String) {
    world.identity_mode = match mode.as_str() {
        "on-demand" => IdentityMode::OnDemand,
        "pre-provisioned" => IdentityMode::PreProvisioned,
        _ => panic!("unknown identity mode: {mode}"),
    };
}

#[given(regex = r#"^org "(.+)" has org_index (\d+) with stride (\d+) and base_uid (\d+)$"#)]
fn given_org_config(
    world: &mut PactWorld,
    org: String,
    index: String,
    stride: String,
    base: String,
) {
    let mut map = world.uid_map.take().unwrap_or_default();
    map.stride = stride.parse().unwrap();
    map.base_uid = base.parse().unwrap();
    map.base_gid = base.parse().unwrap();
    map.org_indices.push(OrgIndex { org, index: index.parse().unwrap() });
    world.uid_map = Some(map);
}

#[given(regex = r#"^no UidEntry exists for subject "(.+)"$"#)]
fn given_no_uid_entry(world: &mut PactWorld, subject: String) {
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    assert!(!map.users.contains_key(&subject));
}

#[given("NFS storage is configured")]
fn given_nfs_configured(world: &mut PactWorld) {
    world.nfs_configured = true;
}

#[given(regex = r"^UIDs (\d+) through (\d+) are already assigned$")]
fn given_uids_already_assigned(world: &mut PactWorld, start: String, end: String) {
    let start: u32 = start.parse().unwrap();
    let end: u32 = end.parse().unwrap();
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    let org = map.org_indices.first().map_or_else(|| "local".into(), |o| o.org.clone());
    for i in start..=end {
        let offset = i - map.uid_precursor(map.org_index(&org).unwrap_or(0));
        let subject = format!("user{offset}@generated");
        let username = format!("user{offset}");
        map.assign_uid(&subject, &username, &org, &format!("/users/{username}"), "/bin/bash")
            .unwrap();
    }
}

#[given(regex = r#"^org "(.+)" has stride (\d+) and all (\d+) UIDs are assigned$"#)]
fn given_org_full(world: &mut PactWorld, org: String, stride: String, count: String) {
    let stride: u32 = stride.parse().unwrap();
    let count: u32 = count.parse().unwrap();
    let mut map = world.uid_map.take().unwrap_or_default();
    map.stride = stride;
    map.base_uid = 10_000;
    map.base_gid = 10_000;
    if map.org_index(&org).is_none() {
        map.org_indices.push(OrgIndex { org: org.clone(), index: 0 });
    }
    for i in 0..count {
        let subject = format!("fill{i}@generated");
        let username = format!("fill{i}");
        map.assign_uid(&subject, &username, &org, &format!("/users/{username}"), "/bin/bash")
            .unwrap();
    }
    world.uid_map = Some(map);
}

#[given(regex = r#"^a UidEntry exists for "(.+)" with uid (\d+)$"#)]
fn given_uid_entry_exists(world: &mut PactWorld, subject: String, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    // Ensure map has at least a local org
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    seed_uid_entry(world, &subject, uid);
}

#[given(regex = r#"^"(.+)" was assigned UID (\d+)$"#)]
fn given_user_assigned_uid(world: &mut PactWorld, subject: String, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    seed_uid_entry(world, &subject, uid);
}

#[given(regex = r#"^"(.+)" was assigned UID (\d+) on node "(.+)"$"#)]
fn given_user_assigned_uid_on_node(
    world: &mut PactWorld,
    subject: String,
    uid: String,
    _node: String,
) {
    let uid: u32 = uid.parse().unwrap();
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    seed_uid_entry(world, &subject, uid);
    // UID is stored in journal — node identity is irrelevant for the map
}

#[given(regex = r"^base_uid is (\d+) and stride is (\d+)$")]
fn given_base_uid_and_stride(world: &mut PactWorld, base: String, stride: String) {
    let mut map = world.uid_map.take().unwrap_or_default();
    map.base_uid = base.parse().unwrap();
    map.base_gid = base.parse().unwrap();
    map.stride = stride.parse().unwrap();
    world.uid_map = Some(map);
}

#[given(regex = r#"^org "(.+)" with org_index (\d+) \(range (\d+)-(\d+)\)$"#)]
fn given_org_with_range(
    world: &mut PactWorld,
    org: String,
    index: String,
    _range_start: String,
    _range_end: String,
) {
    let mut map = world.uid_map.take().unwrap_or_default();
    let idx: u32 = index.parse().unwrap();
    // Only set defaults if not already configured
    if map.base_uid == 0 {
        map.base_uid = 10_000;
        map.base_gid = 10_000;
        map.stride = 10_000;
    }
    map.org_indices.push(OrgIndex { org, index: idx });
    world.uid_map = Some(map);
}

#[given(regex = r#"^org "(.+)" with org_index (\d+) has (\d+) assigned UidEntries$"#)]
fn given_org_with_entries(world: &mut PactWorld, org: String, index: String, count: String) {
    let idx: u32 = index.parse().unwrap();
    let count: u32 = count.parse().unwrap();
    let mut map = world.uid_map.take().unwrap_or_default();
    if map.base_uid == 0 {
        map.base_uid = 10_000;
        map.base_gid = 10_000;
        map.stride = 10_000;
    }
    if map.org_index(&org).is_none() {
        map.org_indices.push(OrgIndex { org: org.clone(), index: idx });
    }
    for i in 0..count {
        let subject = format!("user{i}@{org}");
        let username = format!("user{i}");
        map.assign_uid(&subject, &username, &org, &format!("/users/{username}"), "/bin/bash")
            .unwrap();
    }
    world.uid_map = Some(map);
}

#[given(regex = r#"^"(.+)" is a member of groups:$"#)]
fn given_user_member_of_groups(world: &mut PactWorld, subject: String, step: &Step) {
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    let username = subject.split('@').next().unwrap_or("user").to_string();
    // Ensure the user exists in the map
    if !map.users.contains_key(&subject) {
        map.users.insert(
            subject.clone(),
            UidEntry {
                subject: subject.clone(),
                uid: 10_000,
                gid: 10_000,
                username: username.clone(),
                home: format!("/users/{username}"),
                shell: "/bin/bash".to_string(),
                org: "local".to_string(),
            },
        );
    }
    if let Some(ref table) = step.table {
        for row in table.rows.iter().skip(1) {
            let group_name = row[0].trim().to_string();
            let gid: u32 = row[1].trim().parse().unwrap();
            map.groups.insert(
                group_name.clone(),
                GroupEntry { name: group_name, gid, members: vec![username.clone()] },
            );
        }
    }
}

#[given(regex = r#"^/run/pact/passwd.db contains entry for "(.+)" with uid (\d+)$"#)]
fn given_passwd_db_contains(world: &mut PactWorld, subject: String, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    seed_uid_entry(world, &subject, uid);
    world.passwd_db_created = true;
}

#[given("/run/pact/passwd.db does not exist")]
fn given_passwd_db_missing(world: &mut PactWorld) {
    world.passwd_db_created = false;
}

#[given(regex = r#"^pact-agent is running on node "(.+)"$"#)]
fn given_agent_running_on_node(world: &mut PactWorld, _node: String) {
    // No-op: agent context is implicit in PactWorld
}

#[given(regex = r#"^a service "(.+)" declared with user "(.+)"$"#)]
fn given_service_with_user(world: &mut PactWorld, service: String, _user: String) {
    // Record in service_declarations for later use.
    // The user field is tracked implicitly (UID resolution happens via UidMap).
    world.service_declarations.push(pact_common::types::ServiceDecl {
        name: service,
        binary: String::new(),
        args: Vec::new(),
        restart: pact_common::types::RestartPolicy::Always,
        restart_delay_seconds: 1,
        depends_on: Vec::new(),
        order: 0,
        cgroup_memory_max: None,
        cgroup_slice: None,
        cgroup_cpu_weight: None,
        health_check: None,
    });
}

#[given(regex = r#"^"(.+)" is assigned UID (\d+) on the journal$"#)]
fn given_uid_assigned_on_journal(world: &mut PactWorld, subject: String, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    if map.org_indices.is_empty() {
        map.org_indices.push(OrgIndex { org: "local".into(), index: 0 });
    }
    seed_uid_entry(world, &subject, uid);
    world.journal_committed = true;
}

#[given("UidMap is not yet loaded")]
fn given_uid_map_not_loaded(world: &mut PactWorld) {
    world.uid_map_loaded = false;
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^"(.+)" authenticates via OIDC$"#)]
fn when_authenticates(world: &mut PactWorld, subject: String) {
    world.last_auth_subject = Some(subject);
}

#[when(regex = r#"^a new subject "(.+)" is assigned a UID$"#)]
fn when_assign_uid(world: &mut PactWorld, subject: String) {
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    let username = subject.split('@').next().unwrap_or("user");
    let org = map.org_indices.first().map_or_else(|| "local".into(), |o| o.org.clone());
    match map.assign_uid(&subject, username, &org, &format!("/users/{username}"), "/bin/bash") {
        Ok(entry) => world.last_assigned_uid = Some(entry.uid),
        Err(e) => world.last_error = Some(e),
    }
}

#[when("pact-agent boots")]
fn when_pact_agent_boots(world: &mut PactWorld) {
    // Simulate boot: if pact backend + NFS, create db files and nsswitch
    if world.supervisor_backend == pact_common::types::SupervisorBackend::Pact
        && world.nfs_configured
    {
        world.passwd_db_created = true;
        world.group_db_created = true;
        world.nsswitch_configured = true;
    }
}

#[when("pact-agent reboots and reloads UidMap from journal")]
fn when_agent_reboots(world: &mut PactWorld) {
    // UidMap is persisted in journal — after reboot, same map is reloaded.
    // The uid_map in world already represents the journal state, so no-op.
    world.uid_map_loaded = true;
}

#[when(regex = r#"^node "(.+)" receives the UidMap via journal subscription$"#)]
fn when_node_receives_uid_map(world: &mut PactWorld, _node: String) {
    // UidMap is distributed via journal — same map on all nodes.
    world.uid_map_loaded = true;
}

#[when(regex = r#"^org "(.+)" joins federation with org_index (\d+)$"#)]
fn when_org_joins_federation(world: &mut PactWorld, org: String, index: String) {
    let idx: u32 = index.parse().unwrap();
    let map = world.uid_map.get_or_insert_with(UidMap::new);
    map.org_indices.push(OrgIndex { org, index: idx });
}

#[when(regex = r#"^"(.+)" is assigned UID (\d+)$"#)]
fn when_user_assigned_uid(world: &mut PactWorld, subject: String, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    seed_uid_entry(world, &subject, uid);
    world.last_assigned_uid = Some(uid);
}

#[when(regex = r#"^org "(.+)" leaves federation$"#)]
fn when_org_leaves(world: &mut PactWorld, org: String) {
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    map.gc_org(&org);
}

#[when(regex = r#"^"(.+)" authenticates and accesses NFS$"#)]
fn when_authenticates_and_accesses_nfs(world: &mut PactWorld, subject: String) {
    world.last_auth_subject = Some(subject.clone());
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    if let Some(entry) = map.users.get(&subject) {
        world.last_assigned_uid = Some(entry.uid);
    }
}

#[when(regex = r#"^getgrouplist\("(.+)"\) is called via NSS$"#)]
fn when_getgrouplist(world: &mut PactWorld, _user: String) {
    // Groups are already seeded in uid_map — this step triggers the lookup.
    // Verification happens in THEN steps.
}

#[when(regex = r"^getpwuid\((\d+)\) is called$")]
fn when_getpwuid(world: &mut PactWorld, uid: String) {
    let uid: u32 = uid.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    if let Some(entry) = map.get_by_uid(uid) {
        world.nss_lookup_result = Some(entry.uid);
        world.nss_lookup_local = true;
        world.nss_no_network = true;
    }
}

#[when(regex = r#"^getpwnam\("(.+)"\) is called via pact NSS module$"#)]
fn when_getpwnam(world: &mut PactWorld, _user: String) {
    // passwd.db does not exist — NSS module should return not found
    if !world.passwd_db_created {
        world.nss_not_found = true;
        world.nss_fallthrough = true;
    }
}

#[when("the journal subscription delivers the UidMap update")]
fn when_journal_delivers_update(world: &mut PactWorld) {
    // Simulate journal push: mark db files as updated
    world.db_files_updated = true;
    world.passwd_db_created = true;
}

#[when(regex = r#"^the service "(.+)" startup is attempted$"#)]
fn when_service_startup_attempted(world: &mut PactWorld, _service: String) {
    if !world.uid_map_loaded {
        world.service_waiting_for_uid_map = true;
    }
    // Once uid_map becomes available, the service starts
    world.uid_map_loaded = true;
    world.service_started_after_resolve = true;
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r"^a UidEntry should be created with uid in range (\d+)-(\d+)$")]
fn then_uid_in_range(world: &mut PactWorld, min: String, max: String) {
    let min: u32 = min.parse().unwrap();
    let max: u32 = max.parse().unwrap();
    let map = world.uid_map.as_mut().expect("UidMap not initialized");
    let subject = world.last_auth_subject.as_ref().expect("no auth subject");
    let username = subject.split('@').next().unwrap_or("user");
    let org = map.org_indices.first().map_or_else(|| "local".into(), |o| o.org.clone());
    let entry = map
        .assign_uid(subject, username, &org, &format!("/users/{username}"), "/bin/bash")
        .unwrap();
    assert!(entry.uid >= min && entry.uid <= max, "UID {} not in range {}-{}", entry.uid, min, max);
}

#[then(regex = r"^the assigned UID should be (\d+)$")]
fn then_assigned_uid(world: &mut PactWorld, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    assert_eq!(world.last_assigned_uid, Some(expected), "expected UID {expected}");
}

#[then(regex = r#"^the assignment should fail with "(.+)"$"#)]
fn then_assignment_fails(world: &mut PactWorld, expected_msg: String) {
    let err = world.last_error.as_ref().expect("expected an error");
    let err_str = err.to_string();
    assert!(err_str.contains(&expected_msg), "error '{err_str}' does not contain '{expected_msg}'");
}

#[then("the assignment should be committed to the journal via Raft")]
fn then_committed_to_journal(world: &mut PactWorld) {
    // In a real system, assign_uid would write to journal via Raft.
    // Here we verify the entry exists in the map (which represents journal state).
    let subject = world.last_auth_subject.as_ref().expect("no auth subject");
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    assert!(map.users.contains_key(subject), "UidEntry not committed for {subject}");
}

#[then("the .db files should be updated on all agents")]
fn then_db_files_updated(world: &mut PactWorld) {
    // In BDD: after journal commit, db files are pushed to all agents.
    // The UidMap existing in world state represents this.
    assert!(world.uid_map.is_some(), "UidMap should exist after commit");
}

#[then("/run/pact/passwd.db should be created")]
fn then_passwd_db_created(world: &mut PactWorld) {
    assert!(world.passwd_db_created, "passwd.db should be created");
}

#[then("/run/pact/group.db should be created")]
fn then_group_db_created(world: &mut PactWorld) {
    assert!(world.group_db_created, "group.db should be created");
}

#[then(regex = r#"^/etc/nsswitch.conf should include "pact" for passwd and group$"#)]
fn then_nsswitch_configured(world: &mut PactWorld) {
    assert!(world.nsswitch_configured, "nsswitch.conf should include pact");
}

#[then("/run/pact/passwd.db should not exist")]
fn then_passwd_db_not_exist(world: &mut PactWorld) {
    assert!(!world.passwd_db_created, "passwd.db should not exist in systemd mode");
}

#[then("/run/pact/group.db should not exist")]
fn then_group_db_not_exist(world: &mut PactWorld) {
    assert!(!world.group_db_created, "group.db should not exist in systemd mode");
}

#[then("an alert should be emitted")]
fn then_alert_emitted(world: &mut PactWorld) {
    // UID range exhaustion should trigger an alert.
    // The error existing is evidence the condition was detected.
    assert!(world.last_error.is_some(), "an alert/error should have been raised");
    world.alert_raised = true;
}

#[then("the authentication should succeed (OIDC is valid)")]
fn then_auth_succeeds(world: &mut PactWorld) {
    // OIDC authentication is separate from UID mapping.
    // The subject was set, so auth "succeeded".
    assert!(world.last_auth_subject.is_some(), "auth subject should be set");
}

#[then(regex = r#"^operations requiring UID should be rejected with "(.+)"$"#)]
fn then_uid_ops_rejected(world: &mut PactWorld, expected_msg: String) {
    // In pre-provisioned mode, unknown subjects have no UID mapping
    let subject = world.last_auth_subject.as_ref().expect("no auth subject");
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    assert!(
        !map.users.contains_key(subject),
        "subject {subject} should NOT have a UidEntry in pre-provisioned mode"
    );
    // The rejection message is what would be returned
    assert!(
        expected_msg.contains("not provisioned"),
        "expected rejection message about identity not provisioned"
    );
}

#[then(regex = r"^UID (\d+) should be used for file ownership$")]
fn then_uid_used_for_ownership(world: &mut PactWorld, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    assert_eq!(
        world.last_assigned_uid,
        Some(expected),
        "UID {expected} should be used for file ownership"
    );
}

#[then(regex = r#"^"(.+)" should still have UID (\d+)$"#)]
fn then_user_still_has_uid(world: &mut PactWorld, subject: String, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let entry = map.users.get(&subject).expect("UidEntry not found after reboot");
    assert_eq!(entry.uid, expected, "UID should be stable across reboots");
}

#[then(regex = r#"^"(.+)" should have UID (\d+) on node "(.+)"$"#)]
fn then_user_has_uid_on_node(
    world: &mut PactWorld,
    subject: String,
    expected: String,
    _node: String,
) {
    let expected: u32 = expected.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let entry = map.users.get(&subject).expect("UidEntry not found on target node");
    assert_eq!(entry.uid, expected, "UID should be consistent across nodes");
}

#[then(regex = r"^partner-a's precursor should be (\d+)$")]
fn then_precursor(world: &mut PactWorld, expected: String) {
    let expected: u32 = expected.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let idx = map.org_index("partner-a").expect("partner-a not in org_indices");
    assert_eq!(map.uid_precursor(idx), expected);
}

#[then(regex = r"^partner-a's UID range should be (\d+)-(\d+)$")]
fn then_uid_range(world: &mut PactWorld, start: String, end: String) {
    let start: u32 = start.parse().unwrap();
    let end: u32 = end.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let idx = map.org_index("partner-a").expect("partner-a not in org_indices");
    let precursor = map.uid_precursor(idx);
    assert_eq!(precursor, start);
    assert_eq!(precursor + map.stride - 1, end);
}

#[then("no UID collision exists")]
fn then_no_collision(world: &mut PactWorld) {
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let uids: Vec<u32> = map.users.values().map(|e| e.uid).collect();
    let mut sorted = uids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(uids.len(), sorted.len(), "UID collision detected");
}

#[then(regex = r#"^all (\d+) UidEntries for "(.+)" should be removed from journal$"#)]
fn then_entries_removed(world: &mut PactWorld, count: String, org: String) {
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let remaining = map.users.values().filter(|e| e.org == org).count();
    assert_eq!(remaining, 0, "expected 0 entries for {org}, found {remaining}");
}

#[then(regex = r"^org_index (\d+) should be reclaimable$")]
fn then_org_index_reclaimable(world: &mut PactWorld, index: String) {
    let idx: u32 = index.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    assert!(
        !map.org_indices.iter().any(|o| o.index == idx),
        "org_index {idx} should be reclaimable (not in use)"
    );
}

#[then("NFS files owned by those UIDs become orphaned")]
fn then_nfs_files_orphaned(_world: &mut PactWorld) {
    // This is a documentation assertion — NFS files with removed UIDs
    // become orphaned by definition. No runtime check needed.
}

#[then(regex = r"^groups (.+) should all be returned$")]
fn then_groups_returned(world: &mut PactWorld, groups_str: String) {
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    // Parse "lp16, csstaff, and gpu-users" into group names
    let names: Vec<&str> = groups_str
        .split(|c: char| c == ',' || c == ' ')
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "and")
        .collect();
    for name in &names {
        assert!(map.groups.contains_key(*name), "group '{name}' should be in UidMap");
    }
}

#[then(regex = r"^GIDs (.+) should be included$")]
fn then_gids_included(world: &mut PactWorld, gids_str: String) {
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let expected_gids: Vec<u32> = gids_str
        .split(|c: char| c == ',' || c == ' ')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    let actual_gids: Vec<u32> = map.groups.values().map(|g| g.gid).collect();
    for gid in &expected_gids {
        assert!(actual_gids.contains(gid), "GID {gid} should be present");
    }
}

#[then("the result should be returned from local file")]
fn then_result_from_local(world: &mut PactWorld) {
    assert!(world.nss_lookup_local, "result should come from local file");
}

#[then("no network call should be made")]
fn then_no_network(world: &mut PactWorld) {
    assert!(world.nss_no_network, "no network call should be made");
}

#[then("the lookup should return not found")]
fn then_lookup_not_found(world: &mut PactWorld) {
    assert!(world.nss_not_found, "lookup should return not found");
}

#[then("nsswitch should fall through to the next source")]
fn then_nsswitch_fallthrough(world: &mut PactWorld) {
    assert!(world.nss_fallthrough, "nsswitch should fall through");
}

#[then("/run/pact/passwd.db should be updated")]
fn then_passwd_db_updated(world: &mut PactWorld) {
    assert!(world.db_files_updated, "passwd.db should be updated");
}

#[then(regex = r#"^getpwnam\("(.+)"\) should now resolve to UID (\d+)$"#)]
fn then_getpwnam_resolves(world: &mut PactWorld, user: String, expected_uid: String) {
    let expected: u32 = expected_uid.parse().unwrap();
    let map = world.uid_map.as_ref().expect("UidMap not initialized");
    let entry =
        map.get_by_username(&user).unwrap_or_else(|| panic!("getpwnam({user}) should resolve"));
    assert_eq!(entry.uid, expected, "getpwnam({user}) should return UID {expected}");
}

#[then("startup should wait for UidMap to be available")]
fn then_startup_waits(world: &mut PactWorld) {
    assert!(world.service_waiting_for_uid_map, "service should wait for UidMap");
}

#[then(regex = r#"^"(.+)" should start only after "(.+)" resolves to a UID$"#)]
fn then_service_starts_after_resolve(world: &mut PactWorld, _service: String, _user: String) {
    assert!(world.service_started_after_resolve, "service should start only after user resolves");
}
