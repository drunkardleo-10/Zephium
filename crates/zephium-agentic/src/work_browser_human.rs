//! Human input and actor execution are mutually exclusive on a retained page.
use super::*;

/// Maximum total waiting/presentation interval; caller deadlines can shorten it.
/// A sign-in with a second factor takes minutes; the wait never spends the run.
pub const MAX_WORK_HUMAN_WAIT_MILLIS: u64 = 540_000;

/// Human navigation stays on the original scheme, port and registrable site.
/// Private suffixes keep unrelated hosted tenants separate; IPs stay exact.
pub fn same_work_human_site(
    source: &ContextNavigationTarget,
    target: &ContextNavigationTarget,
) -> bool {
    let (source, target) = (source.as_url(), target.as_url());
    if source.scheme() != target.scheme()
        || source.port_or_known_default() != target.port_or_known_default()
    {
        return false;
    }
    if source.host() == target.host() {
        return true;
    }
    let (Some(url::Host::Domain(source)), Some(url::Host::Domain(target))) =
        (source.host(), target.host())
    else {
        return false;
    };
    fn domain(host: &str) -> Option<psl::Domain<'_>> {
        psl::domain(host.as_bytes()).filter(|domain| domain.suffix().is_known())
    }
    domain(source)
        .zip(domain(target))
        .is_some_and(|(source, target)| source == target)
}

/// Logical points from the top-left of the native host's content region.
/// The native adapter additionally checks containment in the actual window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkBrowserHumanRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
impl WorkBrowserHumanRegion {
    /// Reject empty, overflowing or unbounded presentation requests.
    pub fn try_new(x: u32, y: u32, width: u32, height: u32) -> Option<Self> {
        (width >= 64
            && height >= 64
            && x.checked_add(width).is_some_and(|edge| edge <= 8192)
            && y.checked_add(height).is_some_and(|edge| edge <= 8192))
        .then_some(Self {
            x,
            y,
            width,
            height,
        })
    }
    /// Validated top-left position and size in logical points.
    pub const fn components(self) -> [u32; 4] {
        [self.x, self.y, self.width, self.height]
    }
}

#[derive(Clone, Debug)]
pub(super) struct HumanWindow {
    /// Absent when the page is handed over without being shown: the person
    /// decides elsewhere and the page only needs a fresh actor lease.
    pub(super) region: Option<WorkBrowserHumanRegion>,
    pub(super) deadline: AgentPolicyInstant,
    pub(super) source: Arc<ContextNavigationTarget>,
    pub(super) progress: WorkBrowserHumanProgress,
    /// A sign-in: the person may pass through another site's sign-in pages.
    pub(super) sign_in: bool,
}

/// Content-free native document revision; observational, never execution authority.
#[derive(Clone, Debug, Default)]
pub struct WorkBrowserHumanProgress(
    Arc<std::sync::atomic::AtomicU64>,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<std::sync::atomic::AtomicBool>,
);
impl WorkBrowserHumanProgress {
    /// Native readiness is advisory; continuation rechecks the live document.
    pub fn ready(&self) -> bool {
        self.1.load(std::sync::atomic::Ordering::Acquire)
    }
    /// Native owner reports completion of the current main-document navigation.
    pub fn record_ready(&self, ready: bool) {
        self.1.store(ready, std::sync::atomic::Ordering::Release);
    }
    /// Monotonic revision for frontend document-change events.
    pub fn revision(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
    /// The settled page is back on its own site and off any sign-in path.
    pub fn clear_of_sign_in(&self) -> bool {
        self.2.load(std::sync::atomic::Ordering::Acquire)
    }
    /// Native owner reports where the person's navigation settled.
    pub fn record_clear_of_sign_in(&self, clear: bool) {
        self.2.store(clear, std::sync::atomic::Ordering::Release);
    }
    /// Native owner publishes only increasing revisions of its retained page.
    pub fn record_revision(&self, revision: u64) {
        self.0
            .fetch_max(revision, std::sync::atomic::Ordering::Release);
    }
}

impl WorkBrowserResources {
    /// Descriptive current document only when no actor or human operation owns it.
    pub fn retained_document(
        &self,
        resource: &WorkBrowserResourceJoin,
    ) -> Option<&ContextNavigationTarget> {
        let row = self.rows.get(&resource.identity.resource)?;
        (row.join == *resource
            && row.phase == WorkBrowserResourcePhase::Retained
            && row.failure.is_none()
            && row.document_available
            && row.pending.is_none()
            && row.lease.is_none())
        .then_some(row.effective_document.as_deref())
        .flatten()
    }
    /// Read the original native presentation's content-free progress.
    pub fn human_progress(
        &self,
        resource: &WorkBrowserResourceJoin,
    ) -> Option<WorkBrowserHumanProgress> {
        let row = self.rows.get(&resource.identity.resource)?;
        (row.join == *resource)
            .then(|| row.human.as_ref().map(|human| human.progress.clone()))
            .flatten()
    }
    /// Reserve human presentation only after the actor lease and its work ended.
    /// The application must independently prove the original human-handoff ACK.
    pub fn present_human(
        &mut self,
        resource: &WorkBrowserResourceJoin,
        region: WorkBrowserHumanRegion,
        now: AgentPolicyInstant,
        deadline: AgentPolicyInstant,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        self.present_human_for(resource, region, now, deadline, false)
    }
    /// Presents a page for a sign-in: the person's navigation may pass through
    /// another site's sign-in pages but must settle back on the page's site.
    pub fn present_human_for(
        &mut self,
        resource: &WorkBrowserResourceJoin,
        region: WorkBrowserHumanRegion,
        now: AgentPolicyInstant,
        deadline: AgentPolicyInstant,
        sign_in: bool,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        self.hand_over(resource, Some(region), now, deadline, sign_in)
    }
    /// Hands the page over without showing it, for a decision the person
    /// makes elsewhere; continuing it re-admits the unchanged document.
    pub fn hand_over_unpresented(
        &mut self,
        resource: &WorkBrowserResourceJoin,
        now: AgentPolicyInstant,
        deadline: AgentPolicyInstant,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        self.hand_over(resource, None, now, deadline, false)
    }
    fn hand_over(
        &mut self,
        resource: &WorkBrowserResourceJoin,
        region: Option<WorkBrowserHumanRegion>,
        now: AgentPolicyInstant,
        deadline: AgentPolicyInstant,
        sign_in: bool,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        if self.sealed {
            return Err(WorkBrowserResourceError::Sealed);
        }
        if deadline <= now || deadline.millis() - now.millis() > MAX_WORK_HUMAN_WAIT_MILLIS {
            return Err(WorkBrowserResourceError::Expired);
        }
        let sequence = self.next()?;
        let row = self.row_mut(resource)?;
        row.tick(now)?;
        if row.failure.is_some() {
            return Err(WorkBrowserResourceError::Quarantined);
        }
        if row.phase != WorkBrowserResourcePhase::Retained
            || row.pending.is_some()
            || row.lease.is_some()
            || row.observation.is_some()
            || row.navigation.is_some()
            || row.action.is_some()
            || row.human.is_some()
            || !row.document_available
            || row.effective_document.is_none()
        {
            return Err(WorkBrowserResourceError::Phase);
        }
        row.human = Some(Box::new(HumanWindow {
            progress: WorkBrowserHumanProgress::default(),
            region,
            deadline,
            source: row
                .effective_document
                .clone()
                .ok_or(WorkBrowserResourceError::Source)?,
            sign_in,
        }));
        row.document_available = false;
        row.observed = false;
        row.phase = WorkBrowserResourcePhase::PresentingHuman;
        Ok(request(
            row,
            sequence,
            WorkBrowserResourceOperation::PresentHuman,
        ))
    }

    /// Reserve one hide/rebind operation; it never starts or resumes an actor.
    pub fn continue_after_human(
        &mut self,
        resource: &WorkBrowserResourceJoin,
        now: AgentPolicyInstant,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        if self.sealed {
            return Err(WorkBrowserResourceError::Sealed);
        }
        let sequence = self.next()?;
        let row = self.row_mut(resource)?;
        row.tick(now)?;
        if row.phase != WorkBrowserResourcePhase::PresentedHuman || row.pending.is_some() {
            return Err(WorkBrowserResourceError::Phase);
        }
        if row.human.as_ref().is_none_or(|human| now >= human.deadline) {
            return Err(WorkBrowserResourceError::Expired);
        }
        row.phase = WorkBrowserResourcePhase::ContinuingAfterHuman;
        Ok(request(
            row,
            sequence,
            WorkBrowserResourceOperation::ContinueAfterHuman,
        ))
    }
}

fn request(
    row: &mut Resource,
    sequence: u64,
    kind: WorkBrowserResourceOperation,
) -> WorkBrowserResourceRequest {
    let operation = OperationJoin {
        resource: row.join.clone(),
        sequence,
        kind,
        lease: None,
    };
    row.pending = Some(operation.clone());
    WorkBrowserResourceRequest {
        construction: None,
        operation,
        storage: row.storage,
        isolated_public: row.isolated_public,
        document: row.document.clone(),
        document_policy: row.document_policy,
        delivery: None,
        health: None,
        human: row.human.clone(),
    }
}

pub(super) fn settle(
    row: &mut Resource,
    completion: WorkBrowserResourceCompletion,
    now: AgentPolicyInstant,
    sealed: bool,
) -> WorkBrowserResourceEvent {
    use WorkBrowserResourceNativeOutcome as Outcome;
    use WorkBrowserResourceOperation as Operation;
    if !sealed && row.human.as_ref().is_some_and(|human| now < human.deadline) {
        match (completion.operation.kind, completion.outcome) {
            (Operation::PresentHuman, Outcome::HumanPresented)
                if row.phase == WorkBrowserResourcePhase::PresentingHuman =>
            {
                row.phase = WorkBrowserResourcePhase::PresentedHuman;
                return WorkBrowserResourceEvent::HumanPresented(row.join.clone());
            }
            (Operation::ContinueAfterHuman, Outcome::HumanContinued)
                if row.phase == WorkBrowserResourcePhase::ContinuingAfterHuman =>
            {
                if let Some((effective, epoch, generation)) = completion
                    .effective_document
                    .filter(|effective| {
                        row.effective_document
                            .as_ref()
                            .is_some_and(|prior| same_work_human_site(prior, effective))
                    })
                    .zip(row.navigation_epoch.next())
                    .zip(row.frame_generation.next())
                    .map(|((target, epoch), generation)| (target, epoch, generation))
                {
                    row.current_requested_document = Some(effective.clone());
                    row.admission_document = Some(effective.clone());
                    row.effective_document = Some(effective);
                    row.navigation_epoch = epoch;
                    row.admission_epoch = epoch;
                    row.frame_generation = generation;
                    row.document_available = true;
                    row.observed = false;
                    row.human = None;
                    row.phase = WorkBrowserResourcePhase::Retained;
                    return WorkBrowserResourceEvent::HumanContinued(row.join.clone());
                }
            }
            _ => {}
        }
    }
    let failure = if completion.outcome == Outcome::Refused {
        WorkBrowserResourceFailure::NativeRefused
    } else {
        WorkBrowserResourceFailure::Contract
    };
    row.quarantine(failure);
    WorkBrowserResourceEvent::Quarantined(failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_site_uses_private_suffixes_and_never_changes_scheme_port_or_ip() {
        for (source, target, allowed) in [
            (
                "https://travel.state.gov/",
                "https://login.state.gov/",
                true,
            ),
            (
                "https://www.example.co.uk/",
                "https://auth.example.co.uk/",
                true,
            ),
            ("https://alice.github.io/", "https://bob.github.io/", false),
            ("https://example.co.uk/", "https://attacker.co.uk/", false),
            (
                "https://example.com/",
                "https://example.com.attacker.test/",
                false,
            ),
            ("https://example.com/", "http://example.com/", false),
            ("https://example.com/", "https://example.com:8443/", false),
            ("http://127.0.0.1:8000/a", "http://127.0.0.1:8000/b", true),
            ("http://127.0.0.1:8000/", "http://127.0.0.2:8000/", false),
            (
                "https://a.example.invalid/",
                "https://b.example.invalid/",
                false,
            ),
        ] {
            assert_eq!(
                same_work_human_site(
                    &ContextNavigationTarget::parse(source).unwrap(),
                    &ContextNavigationTarget::parse(target).unwrap()
                ),
                allowed
            );
        }
    }
    fn now(value: u64) -> AgentPolicyInstant {
        AgentPolicyInstant::from_millis(value)
    }
    fn region() -> WorkBrowserHumanRegion {
        WorkBrowserHumanRegion::try_new(200, 80, 900, 640).unwrap()
    }
    fn fixture() -> (WorkBrowserResources, WorkBrowserResourceJoin) {
        let mut rows = WorkBrowserResources::new(WorkId::generate(), ProfileId::generate());
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("https://example.test/challenge").unwrap(),
                now(0),
            )
            .unwrap();
        let resource = request.resource().clone();
        assert!(matches!(
            rows.settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Constructed),
                now(1)
            ),
            Ok(WorkBrowserResourceEvent::Retained(_))
        ));
        (rows, resource)
    }
    fn acquire(
        rows: &mut WorkBrowserResources,
        resource: &WorkBrowserResourceJoin,
        time: u64,
    ) -> Result<WorkBrowserResourceRequest, WorkBrowserResourceError> {
        rows.acquire(resource, ContextRunId::generate(), now(time), now(1000))
    }

    #[test]
    fn human_and_actor_authority_never_overlap_and_continue_requires_fresh_observation() {
        let (mut rows, resource) = fixture();
        let request = acquire(&mut rows, &resource, 2).unwrap();
        assert!(rows
            .present_human(&resource, region(), now(3), now(900))
            .is_err());
        let lease = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now(4),
            )
            .unwrap();
        assert!(rows
            .present_human(&resource, region(), now(5), now(900))
            .is_err());
        let revoke = rows.revoke(&lease).unwrap();
        let _ = rows
            .settle_at(
                revoke.complete(WorkBrowserResourceNativeOutcome::Revoked {
                    debt: WorkBrowserLeaseNativeDebt::default(),
                    resource_retained: true,
                }),
                now(6),
            )
            .unwrap();
        let present = rows
            .present_human(&resource, region(), now(7), now(900))
            .unwrap();
        assert!(acquire(&mut rows, &resource, 8).is_err());
        let _ = rows
            .settle_at(
                present.complete(WorkBrowserResourceNativeOutcome::HumanPresented),
                now(9),
            )
            .unwrap();
        assert!(acquire(&mut rows, &resource, 10).is_err());
        let hide = rows.continue_after_human(&resource, now(11)).unwrap();
        assert_eq!(hide.document().unwrap().as_url().path(), "/challenge");
        assert!(rows.continue_after_human(&resource, now(12)).is_err());
        assert!(acquire(&mut rows, &resource, 13).is_err());
        let target = ContextNavigationTarget::parse("https://example.test/en.html").unwrap();
        assert!(matches!(
            rows.settle_at(hide.complete_human_document(target.clone()), now(14)),
            Ok(WorkBrowserResourceEvent::HumanContinued(_))
        ));
        assert!(rows.admits_lease(&lease, now(15)).is_err());
        assert_eq!(rows.retained_document(&resource), Some(&target));
        let request = acquire(&mut rows, &resource, 16).unwrap();
        assert_eq!(request.document().unwrap().as_url().path(), "/challenge");
        let fresh = request.lease().unwrap().clone();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::Acquired),
                now(17),
            )
            .unwrap();
        let binding = rows.read_binding(&fresh, now(18)).unwrap();
        assert_eq!(binding.requested_document(), &target);
        assert!(binding.is_admission_document());
        assert_eq!(binding.frame().context().navigation_epoch().get(), 2);
        assert!(!rows
            .automation_state(&fresh, now(19))
            .unwrap()
            .can_automate());
        assert_eq!(
            rows.rows[&resource.identity.resource]
                .effective_document
                .as_deref(),
            Some(&target)
        );
    }

    #[test]
    fn foreign_origin_and_late_native_success_never_restore_agent_authority() {
        for (url, time) in [("https://other.test/", 4), ("https://example.test/", 100)] {
            let (mut rows, resource) = fixture();
            let request = rows
                .present_human(&resource, region(), now(2), now(100))
                .unwrap();
            let _ = rows
                .settle_at(
                    request.complete(WorkBrowserResourceNativeOutcome::HumanPresented),
                    now(3),
                )
                .unwrap();
            let request = rows.continue_after_human(&resource, now(4)).unwrap();
            assert!(matches!(
                rows.settle_at(
                    request.complete_human_document(ContextNavigationTarget::parse(url).unwrap()),
                    now(time)
                ),
                Ok(WorkBrowserResourceEvent::Quarantined(_))
            ));
            assert!(acquire(&mut rows, &resource, time + 1).is_err());
            let destroy = rows.destroy(&resource).unwrap();
            assert!(matches!(
                rows.settle_at(
                    destroy.complete(WorkBrowserResourceNativeOutcome::Destroyed),
                    now(time + 2)
                ),
                Ok(WorkBrowserResourceEvent::Destroyed(_))
            ));
        }
    }

    #[test]
    fn destruction_wins_over_pending_human_presentation_without_erasing_callback_debt() {
        let (mut rows, resource) = fixture();
        let present = rows
            .present_human(&resource, region(), now(2), now(100))
            .unwrap();
        let destroy = rows.destroy(&resource).unwrap();
        let _ = rows
            .settle_at(
                destroy.complete(WorkBrowserResourceNativeOutcome::Destroyed),
                now(3),
            )
            .unwrap();
        rows.seal();
        assert!(!rows.is_quiescent());
        assert!(matches!(
            rows.settle_at(
                present.complete(WorkBrowserResourceNativeOutcome::HumanPresented),
                now(4)
            ),
            Ok(WorkBrowserResourceEvent::DebtSettled(_))
        ));
        assert!(rows.is_quiescent());
    }

    #[test]
    fn human_window_is_bounded_and_cannot_be_reentered_or_renewed() {
        assert!(WorkBrowserHumanRegion::try_new(u32::MAX, 0, 64, 64).is_none());
        assert!(WorkBrowserHumanRegion::try_new(0, 0, 0, 64).is_none());
        let (mut rows, resource) = fixture();
        assert!(rows
            .present_human(&resource, region(), now(2), now(2))
            .is_err());
        assert!(rows
            .present_human(
                &resource,
                region(),
                now(2),
                now(3 + MAX_WORK_HUMAN_WAIT_MILLIS)
            )
            .is_err());
        let request = rows
            .present_human(&resource, region(), now(2), now(100))
            .unwrap();
        let _ = rows
            .settle_at(
                request.complete(WorkBrowserResourceNativeOutcome::HumanPresented),
                now(3),
            )
            .unwrap();
        assert!(rows
            .present_human(&resource, region(), now(4), now(200))
            .is_err());
        assert!(rows.continue_after_human(&resource, now(100)).is_err());
        assert!(rows.destroy(&resource).is_ok());
    }
}
