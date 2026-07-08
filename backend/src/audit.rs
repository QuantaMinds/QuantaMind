//! Audit-log SEAM (laid, not implemented — see docs/security.md).
//!
//! The OSS build ships `NoopAudit`: security-relevant events are dropped. The separate
//! enterprise product swaps in a tamper-evident sink behind this trait so the emit sites below
//! never change. This is a no-op today by design — a tamper-evident audit log for an audience
//! of one (a single-user local tool) is cost with near-zero benefit; it earns its place only in
//! the multi-tenant/compliance (SOC 2) context.
//!
//! Emit points: the events that matter for an audit trail — publishing (data leaves the
//! machine), authentication, and settings changes.

/// A security-relevant event worth recording in an audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditEvent {
    /// A publish to the community board succeeded (`rows` metrics rows left the machine).
    Published { rows: usize },
    /// A browser OAuth sign-in completed.
    LoggedIn,
    /// User/remote-backend settings were changed.
    SettingsChanged,
}

/// The audit boundary: `record` is called at each emit point.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}

/// The OSS no-op: events are dropped.
pub struct NoopAudit;

impl AuditSink for NoopAudit {
    fn record(&self, _event: AuditEvent) {}
}

/// The active audit sink. OSS build → `NoopAudit`; the enterprise build returns a tamper-evident
/// sink from the same accessor, so the emit sites never change.
pub fn audit() -> impl AuditSink {
    NoopAudit
}

/// Emit an audit event through the active sink. A free function so call sites don't need the
/// `AuditSink` trait in scope — they just `audit::record(AuditEvent::…)`.
pub fn record(event: AuditEvent) {
    audit().record(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_records_without_effect() {
        // Pins the OSS sink as a true no-op — never panics, does nothing, whatever the event.
        audit().record(AuditEvent::Published { rows: 3 });
        audit().record(AuditEvent::LoggedIn);
        audit().record(AuditEvent::SettingsChanged);
    }
}
