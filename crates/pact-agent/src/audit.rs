//! Audit integration — emitting AuditEvents at required audit points.
//!
//! pact-agent uses `hpc_audit::AuditSink` to emit events. The default
//! implementation buffers events in memory and flushes to the journal
//! on reconnect (supports partition resilience, invariant O3).
//!
//! For now, a simple in-process buffer is used. Journal forwarding
//! will be wired when the journal client supports audit append.

use hpc_audit::{
    actions, AuditEvent, AuditOutcome, AuditPrincipal, AuditScope, AuditSink, AuditSource,
};

/// Agent-local audit sink that buffers events for journal forwarding.
///
/// Events are stored in memory and can be flushed to the journal.
/// Supports partition resilience (O3): events are never dropped,
/// only buffered until journal is reachable.
pub struct AgentAuditSink {
    buffer: std::sync::Mutex<Vec<AuditEvent>>,
    node_id: String,
}

impl AgentAuditSink {
    #[must_use]
    pub fn new(node_id: &str) -> Self {
        Self {
            buffer: std::sync::Mutex::new(Vec::new()),
            node_id: node_id.to_string(),
        }
    }

    /// Get buffered events count.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.buffer.lock().expect("audit lock poisoned").len()
    }

    /// Drain buffered events for journal forwarding.
    pub fn drain(&self) -> Vec<AuditEvent> {
        let mut buf = self.buffer.lock().expect("audit lock poisoned");
        std::mem::take(&mut *buf)
    }

    /// Node ID for scoping events.
    #[must_use]
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

impl AuditSink for AgentAuditSink {
    fn emit(&self, event: AuditEvent) {
        tracing::debug!(action = %event.action, "audit event emitted");
        self.buffer
            .lock()
            .expect("audit lock poisoned")
            .push(event);
    }

    fn flush(&self) -> Result<(), hpc_audit::AuditError> {
        // In production, this would forward buffered events to journal.
        // For now, just log the count.
        let count = self.buffered_count();
        if count > 0 {
            tracing::info!(count = count, "audit flush: {count} events buffered");
        }
        Ok(())
    }
}

/// Helper: create a system audit event (from pact-agent internals).
pub fn system_event(
    action: &str,
    node_id: &str,
    outcome: AuditOutcome,
    detail: &str,
) -> AuditEvent {
    AuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        principal: AuditPrincipal::system("pact-agent"),
        action: action.to_string(),
        scope: AuditScope::node(node_id),
        outcome,
        detail: detail.to_string(),
        metadata: serde_json::Value::Null,
        source: AuditSource::PactAgent,
    }
}

/// Helper: create a system audit event with metadata.
pub fn system_event_with_metadata(
    action: &str,
    node_id: &str,
    outcome: AuditOutcome,
    detail: &str,
    metadata: serde_json::Value,
) -> AuditEvent {
    AuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        principal: AuditPrincipal::system("pact-agent"),
        action: action.to_string(),
        scope: AuditScope::node(node_id),
        outcome,
        detail: detail.to_string(),
        metadata,
        source: AuditSource::PactAgent,
    }
}

/// Emit boot phase events via the sink.
pub fn emit_boot_phase_complete(sink: &dyn AuditSink, node_id: &str, phase: &str) {
    sink.emit(system_event(
        actions::BOOT_PHASE_COMPLETE,
        node_id,
        AuditOutcome::Success,
        &format!("boot phase {phase} complete"),
    ));
}

pub fn emit_boot_phase_failed(sink: &dyn AuditSink, node_id: &str, phase: &str, reason: &str) {
    sink.emit(system_event_with_metadata(
        actions::BOOT_PHASE_FAILED,
        node_id,
        AuditOutcome::Failure,
        &format!("boot phase {phase} failed: {reason}"),
        serde_json::json!({"phase": phase, "reason": reason}),
    ));
}

pub fn emit_boot_ready(sink: &dyn AuditSink, node_id: &str, elapsed_ms: u128) {
    sink.emit(system_event_with_metadata(
        actions::BOOT_READY,
        node_id,
        AuditOutcome::Success,
        &format!("node ready in {elapsed_ms}ms"),
        serde_json::json!({"elapsed_ms": elapsed_ms}),
    ));
}

/// Emit service lifecycle events.
pub fn emit_service_start(sink: &dyn AuditSink, node_id: &str, service: &str, pid: Option<u32>) {
    sink.emit(system_event_with_metadata(
        actions::SERVICE_START,
        node_id,
        AuditOutcome::Success,
        &format!("service {service} started"),
        serde_json::json!({"service": service, "pid": pid}),
    ));
}

pub fn emit_service_stop(sink: &dyn AuditSink, node_id: &str, service: &str) {
    sink.emit(system_event(
        actions::SERVICE_STOP,
        node_id,
        AuditOutcome::Success,
        &format!("service {service} stopped"),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_audit_sink_buffers_events() {
        let sink = AgentAuditSink::new("node-001");
        assert_eq!(sink.buffered_count(), 0);

        sink.emit(system_event(
            actions::BOOT_READY,
            "node-001",
            AuditOutcome::Success,
            "test",
        ));
        assert_eq!(sink.buffered_count(), 1);

        let events = sink.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, actions::BOOT_READY);
        assert_eq!(sink.buffered_count(), 0);
    }

    #[test]
    fn agent_audit_sink_flush_ok() {
        let sink = AgentAuditSink::new("node-001");
        sink.emit(system_event(
            actions::SERVICE_START,
            "node-001",
            AuditOutcome::Success,
            "test",
        ));
        assert!(sink.flush().is_ok());
    }

    #[test]
    fn system_event_has_correct_source() {
        let event = system_event(
            actions::BOOT_READY,
            "node-001",
            AuditOutcome::Success,
            "ready",
        );
        assert_eq!(event.source, AuditSource::PactAgent);
        assert_eq!(event.scope.node_id.as_deref(), Some("node-001"));
    }

    #[test]
    fn agent_audit_sink_thread_safe() {
        use std::sync::Arc;
        use std::thread;

        let sink = Arc::new(AgentAuditSink::new("node-001"));
        let mut handles = vec![];

        for _ in 0..5 {
            let s = Arc::clone(&sink);
            handles.push(thread::spawn(move || {
                for _ in 0..20 {
                    s.emit(system_event(
                        actions::SERVICE_START,
                        "node-001",
                        AuditOutcome::Success,
                        "test",
                    ));
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(sink.buffered_count(), 100);
    }
}
