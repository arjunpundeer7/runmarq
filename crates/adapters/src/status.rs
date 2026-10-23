//! Combines detection, configuration verification and observed events into
//! the integration status shown to users.
//!
//! "Connected" requires end-to-end proof: the configuration is complete *and*
//! the collector has received an event from that agent since setup. A valid
//! configuration alone is reported as `awaiting_first_event`.

use crate::{Adapter, Detection, InstallOptions, Verification};
use chrono::{DateTime, Utc};
use runmarq_core::api::{IntegrationCheck, IntegrationState, IntegrationStatus};

pub fn compute(
    adapter: &dyn Adapter,
    opts: &InstallOptions,
    detection: &Detection,
    verification: &Verification,
    last_event_at: Option<DateTime<Utc>>,
    installed_at: Option<DateTime<Utc>>,
) -> IntegrationStatus {
    let event_since_setup = match (last_event_at, installed_at) {
        (Some(e), Some(i)) => e >= i,
        (Some(_), None) => true,
        _ => false,
    };
    let state = if !detection.detected && !verification.configured {
        IntegrationState::NotDetected
    } else if !verification.configured {
        IntegrationState::NotConnected
    } else if !verification.complete {
        IntegrationState::Error
    } else if event_since_setup {
        IntegrationState::Connected
    } else {
        IntegrationState::AwaitingFirstEvent
    };
    let mut checks: Vec<IntegrationCheck> = verification
        .checks
        .iter()
        .map(|c| IntegrationCheck {
            label: c.label.clone(),
            ok: c.ok,
            detail: c.detail.clone(),
        })
        .collect();
    checks.insert(
        0,
        IntegrationCheck {
            label: format!("{} installed", adapter.source().display_name()),
            ok: detection.detected,
            detail: match (&detection.version, &detection.executable) {
                (Some(v), _) => Some(format!("version {v}")),
                (None, Some(p)) => Some(p.display().to_string()),
                _ => None,
            },
        },
    );
    if verification.configured {
        checks.push(IntegrationCheck {
            label: "Event received since setup".into(),
            ok: event_since_setup,
            detail: last_event_at.map(|t| format!("last event {}", t.format("%Y-%m-%d %H:%M UTC"))),
        });
    }
    let mut pending = verification.pending_steps.clone();
    if verification.complete && !event_since_setup && pending.is_empty() {
        pending.push(format!(
            "Start a new {} session and send a prompt to confirm events arrive.",
            adapter.source().display_name()
        ));
    }
    // Steps such as "trust the hooks in Codex" only make sense after setup.
    if matches!(
        state,
        IntegrationState::Connected
            | IntegrationState::NotConnected
            | IntegrationState::NotDetected
    ) {
        pending.clear();
    }
    let mut limitations = adapter.limitations();
    limitations.extend(detection.notes.iter().cloned());
    IntegrationStatus {
        agent: adapter.source(),
        display_name: adapter.source().display_name().into(),
        detected: detection.detected,
        installed_version: detection.version.clone(),
        executable: detection
            .executable
            .as_ref()
            .map(|p| p.display().to_string()),
        state,
        support: adapter.support(),
        capabilities: adapter.capabilities(),
        limitations,
        config_locations: vec![adapter
            .config_path(&opts.scope, &opts.home())
            .display()
            .to_string()],
        checks,
        pending_steps: pending,
        last_event_at,
    }
}
