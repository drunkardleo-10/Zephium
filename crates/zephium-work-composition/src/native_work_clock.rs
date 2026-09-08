//! Native Work admission and controller effects must share one process epoch.
use std::time::Instant;
use zephium_agent_controller::{TerraControllerClock, TerraControllerClockError};
use zephium_agentic::AgentPolicyInstant;

pub(crate) struct NativeWorkClock;
impl TerraControllerClock for NativeWorkClock {
    fn now(&self) -> Result<AgentPolicyInstant, TerraControllerClockError> {
        zephium_engine::work_browser_monotonic_now().ok_or(TerraControllerClockError::Invalid)
    }
}

pub(crate) fn authority_window(
    deadline: Instant,
) -> Result<(AgentPolicyInstant, AgentPolicyInstant), &'static str> {
    let issued = zephium_engine::work_browser_monotonic_now().ok_or("clock")?;
    let expires = zephium_engine::work_browser_monotonic_deadline(deadline).ok_or("deadline")?;
    if Instant::now() >= deadline || expires <= issued {
        return Err("deadline");
    }
    Ok((issued, expires))
}

#[cfg(test)]
pub(crate) fn assert_native_timing(
    input: &zephium_agent_controller::AgentWorkRunInput,
    deadline: Instant,
) {
    let spec = input.retained_resource_spec().unwrap();
    let before = zephium_engine::work_browser_monotonic_now().unwrap();
    let policy_now = spec.clock.now().unwrap();
    let after = zephium_engine::work_browser_monotonic_now().unwrap();
    assert!(
        before <= policy_now && policy_now <= after,
        "controller action timestamps must be in the native ingress epoch"
    );
    assert_eq!(
        spec.deadline, deadline,
        "credentials/admission cannot restart wall time"
    );
    assert_eq!(
        Some(spec.expires_at),
        zephium_engine::work_browser_monotonic_deadline(deadline)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn elapsed_or_past_deadlines_cannot_create_fresh_authority() {
        let now = Instant::now();
        assert!(authority_window(now).is_err());
        assert!(
            authority_window(now.checked_sub(std::time::Duration::from_secs(37)).unwrap()).is_err()
        );
        let deadline = now + std::time::Duration::from_secs(113);
        let (_, expires) = authority_window(deadline).unwrap();
        assert_eq!(
            Some(expires),
            zephium_engine::work_browser_monotonic_deadline(deadline)
        );
        assert!(
            expires
                < zephium_engine::work_browser_monotonic_deadline(
                    now + std::time::Duration::from_secs(150)
                )
                .unwrap()
        );
    }
}
