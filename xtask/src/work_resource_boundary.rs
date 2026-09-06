//! Mechanical guards for persistent resource ownership and its opt-in adapter.

use std::path::Path;

pub(crate) fn check(root: &Path) -> Result<(), String> {
    let source =
        std::fs::read_to_string(root.join("crates/zephium-agentic/src/work_browser_resource.rs"))
            .map_err(|e| e.to_string())?;
    let port = std::fs::read_to_string(root.join("crates/zephium-agentic/src/context_port.rs"))
        .map_err(|e| e.to_string())?;
    validate(&source, &port)?;
    for (path, required, forbidden) in ADAPTER_RULES {
        let source = std::fs::read_to_string(root.join(path)).map_err(|e| e.to_string())?;
        validate_adapter(&source, required, forbidden).map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(())
}

const ADAPTER_RULES: &[(&str, &[&str], &[&str])] = &[
    (
        "crates/zephium-app/src/work_resources_probe.rs",
        &[
            "owner:WorkResourceOwner", "WorkResourceOwner::new(WorkId::generate(),profile,wake,factory)",
            "self.owner.retained_browser(lease,now)", "SnapshotReleaseBrowser::new(browser,release)",
            "AgentWorkRetainedController::try_new(", "PendingScopedAgentRuntime::spawn_suspended(",
            "AgentProviderTransportConfig::STANDARD", "self.owner.poll_native_event()", "self.owner.shutdown_audit(audit)",
            "ifself.constructed||self.pending.is_some()", "ifself.acquired||self.pending.is_some()", "ifself.started||self.pending.is_some()",
        ],
        &["AgentNativeShutdownProof", "AgentBrowserShutdownOutcome", "ContextRegistry::new", "WorkBrowserResources::new", "Serialize", "Deserialize", "attach_successor", "browser_loop", "persist_extraction_result", "try_new_for_probe"],
    ),
    (
        "crates/zephium-engine/src/platform/macos/agentic_resource_composition_probe.rs",
        &[
            "port:ResourceWitnessPort,resource:WorkBrowserResourceJoin", "MainThreadMarker::new()?",
            "if!admission.remains_current()", "engine.agent_context_port.resource_witness_port()?",
            "Request{resource:self.resource.clone(),operation,}", "ifactual==expected",
            "self.schedule(Operation::Acquire,completion)", "self.schedule(Operation::Retire,completion)",
        ],
        &["AgentProvider", "AgentBrowserPort", "WorkBrowserResources", "evaluateJavaScript", "setActivationPolicy", "makeKey", "makeMain", "NSRunLoop", "requestAnimationFrame", "unsafe", "Serialize", "Deserialize"],
    ),
    (
        "crates/zephium-work-composition/src/retained_qualification.rs",
        &[
            "owner:RetainedWorkProbeOwner", "admission:ForegroundRenderingAdmission",
            "load_macos_probe_openai_credential()", "native.take_agent_browser_port(move|event|sink(event))",
            "self.store.clone()", "self.owner.start(", "lifecycle.drain_until(deadline)",
            "mpsc::sync_channel(4)", "TOTAL:Duration=Duration::from_secs(150)", "CLEANUP:Duration=Duration::from_secs(5)",
            "ifstate==ForegroundRenderingState::Retiring{return;}",
            "deliver_snapshot_retirement(&release,state,&signal)", "self.owner.presentation_returned()",
            "task::verify_owned(extraction,expected)", "self.owner.seal(", "self.owner.locally_retired()",
            "self.admission.remains_current()", "drop(self)", "fixture.shutdown()",
            "fnpoll_actor(&mutself)->Result<(),&'staticstr>{self.drain_actor_metadata()?;if!self.completion.as_ref().is_some_and(AgentRuntimeCompletion::is_stopped){returnOk(());}",
        ],
        &["AgentNativeShutdownProof", "AgentBrowserShutdownOutcome", "ContextRegistry", "WorkBrowserResources", "browser_loop", "AgentBrowserSession", "evaluateJavaScript", "NSRunLoop", "std::thread::sleep", "OPENAI_API_KEY", "Command::new", "attach_successor", "persist_extraction_result"],
    ),
    (
        "crates/zephium-work-composition/src/retained_qualification_task.rs",
        &[
            "snapshot.completeness()==SemanticCompleteness::Complete", "observation.request().context().identity()==self.context",
            "!sample.current||!sample.complete||sample.boundaries!=0||sample.markers.into_iter().any(|present|!present)",
            "node.role()==SemanticRole::Paragraph", "fragment.provenance().reference()!=expected.reference",
            "fragment.provenance().frame()!=&expected.frame", "source.reference==expected.reference",
            "source.frame==expected.frame", "source.role==SemanticRole::Paragraph",
            "AgentRunBudget::try_new(8,100_000,50_000,1)", "AgentBrowserModel::Luna", "AgentAccountScope::Anonymous",
        ],
        &["allows_actions_before_extraction", "navigation_target", "with_source_roles", "with_subtree_extraction", "AgentBrowserPort", "Serialize", "Deserialize"],
    ),
    (
        "crates/zephium-app/src/work_resources_snapshot_probe.rs",
        &[
            "browser:RetainedBrowser",
            "resource:WorkBrowserResourceJoin",
            "browser.binding().lease().resource()!=&release.resource",
            "self.observation=Some(observation);self.release.start()?",
            "ifself.release.returned()?{Ok(self.observation.take())}",
            "ifself.observed{returnErr(AgentWorkFailure::Contract);}",
            "letreturned=ReleaseCompletion(Some(self.clone()))",
            "implDropforReleaseCompletion",
            "let_=slot.complete(false)",
            "listener:Mutex<Option<Arc<Waker>>>",
            "std::panic::catch_unwind",
        ],
        &["AgentBrowserPort", "AgentProviderTransport", "ContextRegistry", "AgentNativeShutdownProof", "Serialize", "Deserialize", "evaluateJavaScript", "browser_loop", "tokio::spawn", "std::thread", "WorkBrowserResources::new"],
    ),
    (
        "crates/zephium-agent-controller/src/work.rs",
        &[
            "ifretained.is_some()&&(input.durable_result||extraction_schema.is_none()||actions_before_extraction||subtree_extraction||navigation_target.is_some()||navigation_route.is_some())",
            "resources:retained.is_none().then(||WorkContextResources{",
            "ifstate.native.retained.is_some(){journal.emit(AgentWorkEventKind::ContextActive)?;self.start_session()?;self.browser_loop(worker,browser).await?;returnself.close_retained(worker,None).await;}",
            "state.transport.take().ok_or(AgentWorkFailure::Contract)?",
            "worker.try_claim_scoped_terminal(class)",
            "claim.commit(delivery,settlement,provider)",
            "state.native.retained.take()",
            "Self::reconcile_deferred_audit(state)",
            "delivery.proof()==settlement.proof()",
        ],
        &["WorkBrowserResources::new", "implAgentWorkRetainedBrowser", "attach_successor_work"],
    ),
    (
        "crates/zephium-agent-controller/src/work_retained.rs",
        &[
            "pubtraitAgentWorkRetainedBrowser:Send", "fnregister_listener(&mutself,waker:Waker)",
            "fncheck_health(&self,now:AgentPolicyInstant)",
            "fnbinding(&self)->&WorkBrowserReadBinding",
            "implzephium_agent_runtime::AgentRuntimeScopedControllerforAgentWorkRetainedController",
            "controller.execute(&mutworker,&WorkBrowser::Retained).await",
            "AgentWorkController::with_transport(",
            "browser.register_listener(cx.waker().clone())",
            "binding.document()!=&input.context.target",
            "binding.storage()!=input.context.storage",
            "session.try_finish_unsuccessful()", "session.try_finish()",
            "provider:Some(terminal.provider)",
            "state.native.retained_delivery=Some(delivery)",
            "controller.drain_recovery(&mutworker,&WorkBrowser::Retained).await",
            "controller.publish_terminal(&mutworker,false).await",
            "journal.audit.is_quiescent()",
            "Self::Retained=>ContextDispatch::Unsupported",
        ],
        &["AgentBrowserPort", "ContextRegistry::new", "AgentNativeShutdownProof", "AgentNativeShutdownCoordinator", "AgentWorkJournal", "Serialize", "Deserialize", "std::thread", "tokio::spawn", "try_prove_shutdown"],
    ),
    (
        "crates/zephium-app/src/work_resources_controller.rs",
        &[
            "implAgentWorkRetainedBrowserforRetainedBrowser", "browser:LeaseBrowser",
            "binding:WorkBrowserReadBinding",
            "lease:WorkBrowserExecutionLease", "retired:Arc<LeaseRetirement>",
            "self.retired.fail()", "self.browser.refusal()",
            "actors.len()>=MAX_LIVE_CONTEXTS", "actors.push(Arc::downgrade(&listener))",
            "self.pending.swap(false,Ordering::AcqRel)", "self.pending.swap(true,Ordering::AcqRel)",
            "if!self.pending.swap(true,Ordering::AcqRel)&&std::panic::catch_unwind", "std::panic::catch_unwind",
            "read:Option<(PendingRead,SemanticObservationRequest)>", "revoke:Option<PendingLifecycle>",
            "ticket.register_waker(listener.into())", "Some(ticket)",
            "self.browser.shared.lock_rows()", "revoke_with_delivery(&self.browser.lease)",
            "observe_initial(&self.browser.lease,now)", "correlation.invocation().get()",
            "Some(LifecycleResult::Delivered(proof))",
        ],
        &["pubstruct", "pubfn", "pub(crate)", "AgentBrowserPort", "ContextRegistry", "AgentNativeShutdownProof", "AgentWorkJournal", "Serialize", "Deserialize", "std::thread", "tokio::spawn", "port.dispatch", "port.invoke_semantic", "port.seal_for_shutdown", "running.swap"],
    ),
    (
        "crates/zephium-agentic/src/work_browser_observation.rs",
        &[
            "#[derive(Debug)]pubstructWorkBrowserReadBinding{lease:WorkBrowserExecutionLease,frame:SemanticFrameJoin,document:Arc<ContextNavigationTarget>,storage:ContextProfileStorageClass,}",
            "self.admits_lease(lease,now)?;letrow=self.row_mut(lease.resource())?;letdocument=row.document.as_ref()",
            "ContextJoin::work_execution(ContextIdentity::new(row.join.identity.context,lease.run,row.join.identity.profile,ContextKind::Owned,)",
            "Ok(WorkBrowserReadBinding{lease:lease.clone(),frame,document:Arc::clone(document),storage:row.storage,})",
            "letbinding=self.read_binding(lease,now)?;letrow=self.row_mut(lease.resource())?;ifrow.observation.is_some()",
            "letframe=binding.frame;letcontext=frame.context();",
            "SemanticObservationId::new(u64::from(sequence))",
            "SemanticInvocationId::new(u64::from(sequence))",
            "*sequence<=MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS",
        ],
        &[
            "Serialize", "Deserialize", "AgentBrowserPort", "AgentNativeShutdownProof",
            "pubfnnew(", "pubconstfnnew(", "evaluateJavaScript",
        ],
    ),
    (
        "crates/zephium-app/src/work_resources.rs",
        &[
            "typeNativeFactory=Box<dynFnOnce(NativeSink)->Option<Arc<dynAgentBrowserPort>>>",
            "slots:Mutex<Vec<OwnedSlot>>", "slots.len()>=3", "mpsc::sync_channel(1)",
            "mpsc::sync_channel(2)", "self.pending.swap(true,Ordering::AcqRel)",
            "self.shared.notifications.pending.swap(false,Ordering::AcqRel)",
            "Arc::downgrade(resource)", "resource.retain(OwnedSlot::Lifecycle(slot.clone()))?",
            "resource.retain(OwnedSlot::Read(slot.clone()))?", "state.flight.rejected(&resource)",
            "self.shared.lock_rows()?.admits_lease(&self.lease,now)?",
            "self.shared.lock_rows()?.revoke_with_delivery(&self.lease)?",
            "self.delivery.is_some()&&resource.reads.load(Ordering::Acquire)!=0",
            "self.flight.finish(resource);letproof=ended.join_delivery(receipt)",
            "implDropforWorkResourceOwner", "implDropforLeaseBrowser",
            "implDropforLeaseBrowser{fndrop(&mutself){self.retired.fail();}}",
            "structLeaseRetirement{state:AtomicU8,resource:Weak<Resource>,}",
            "self.state.compare_exchange(Self::ACTIVE,Self::FAILED,Ordering::AcqRel,Ordering::Acquire,)",
            "self.state.compare_exchange(Self::ACTIVE,Self::RETIRED,Ordering::AcqRel,Ordering::Acquire,)",
            "Ok(_)|Err(Self::FAILED)=>Some(LeaseFailure(self))",
            "implDropforLeaseFailure<'_>{fndrop(&mutself){ifletSome(resource)=self.0.resource.upgrade(){resource.fail();}}}",
            "self.retired.check_active()?",
            "retirement.retire()?;}resource.reusable.store(true,Ordering::Release)",
            "fnlocally_retired(&self)->bool",
            "self.shared.global_current()&&self.shared.lock_rows()",
            "self.rows.is_poisoned()||self.resources.is_poisoned()",
            "self.resources.lock().map_err(|_|self.refusal())",
            "mutex.lock().map_err(|_|self.refusal())",
            "slots.iter().any(OwnedSlot::is_poisoned)",
            "drop(request.take_resource_health_reporter());}letmutstate=resource.lock_local(&slot)?",
            "Err(TryLockError::WouldBlock)=>false",
            "letmutslot=resource.lock_local(slot)?",
            "Err(mpsc::TryRecvError::Empty)=>failure.map_or(Ok(None),Err)",
        ],
        &[
            "pubstruct", "pubfn", "pub(crate)", "ContextRegistry", "ContextIdentity::",
            "NativeEventSink", "AgentNativeShutdownProof", "AgentBrowserShutdownOutcome",
            "attach_successor_work", "Serialize", "Deserialize", "port.dispatch(",
            "port.invoke_semantic(", "port.execute_semantic_action(", "port.transfer_cookies(",
            "AgentWorkController", "PendingAgentRuntime", "AgentWorkJournal",
            "state.store(Self::ACTIVE",
        ],
    ),
    (
        "crates/zephium-agentic/src/work_browser_health.rs",
        &[
            "resource:WorkBrowserResourceJoin,state:Arc<HealthState>,installed:AtomicBool",
            "resource!=&self.resource", "self.installed.swap(true,Ordering::AcqRel)",
            "self.state.fetch_max(state,Ordering::AcqRel)",
            "self.pending_wake.swap(true,Ordering::AcqRel)",
            "self.state.pending_wake.swap(false,Ordering::AcqRel)",
            "waker:Mutex<Option<Arc<Waker>>>", "std::panic::catch_unwind",
            "self.state.receiver_alive.store(false,Ordering::Release)",
        ],
        &["ContextJoin", "ContextIdentity", "Serialize", "VecDeque", "std::thread", "AgentNativeShutdownProof"],
    ),
    (
        "crates/zephium-agentic/src/work_browser_delivery.rs",
        &[
            "Arc::ptr_eq(&self.0,&other.0)",
            "DeliveryBinding(Arc::new(DeliveryState{state:AtomicU8::new(PENDING)",
            "pubfnregister_waker(", "coordination:Mutex<ListenerRegistration>",
            "self.binding.0.listener.coordination.is_poisoned()",
            "NotificationPhase::Running|NotificationPhase::Completed=>{self.binding.0.listener.fail();Err(WorkBrowserLeaseDeliveryPollError::RegistrationClosed)}",
            "_=>{self.binding.0.listener.fail();Err(WorkBrowserLeaseDeliveryPollError::Notification)}",
            "Err(poisoned)=>{let_registration=poisoned.into_inner();self.binding.0.listener.fail();Err(WorkBrowserLeaseDeliveryPollError::Notification)}",
            "drop(proposed);result",
            "ifregistration.notifier_taken||registration.phase!=NotificationPhase::Pending",
            "ifterminal==RETURNED&&registration.waker.is_some()&&!registration.notifier_taken",
            "ifpublished&&!registration.notifier_taken{",
            "registration.phase=NotificationPhase::Running;registration.waker.clone()};",
            "letreturned=waker.is_none_or(|waker|{std::panic::catch_unwind",
            "ifreturned&&!self.binding.0.listener.failed.load(Ordering::Acquire){registration.phase=NotificationPhase::Completed;true}",
            "self.binding.0.publish(RETURNED)", "self.binding.0.publish(UNPROVEN)",
            "self.binding.0.listener.failed.load(Ordering::Acquire)",
            "pubstructWorkBrowserLeaseDeliveryNotification",
            "PENDING=>Ok(None)",
            "compare_exchange(state,CONSUMED,Ordering::AcqRel,Ordering::Acquire)",
            "(state!=CONSUMED&&state!=ABANDONED).then_some(ABANDONED)",
            "compare_exchange(PENDING,terminal,Ordering::AcqRel,Ordering::Acquire)",
            "compare_exchange(PENDING,UNPROVEN,Ordering::AcqRel,Ordering::Acquire,)",
            "receipt.returned&&self.lease==receipt.lease&&self.delivery.as_ref().is_some_and(|binding|binding.matches(&receipt.binding))",
            "Err(Box::new(WorkBrowserLeaseDeliveryRefusal{ended:self,receipt,}))",
        ],
        &[
            "AgentNativeShutdownProof", "AgentBrowserShutdownOutcome", "Serialize", "Deserialize",
            "std::thread", "tokio::", "Condvar", "FnOnce", "is_clean(",
            "signaled.swap(",
        ],
    ),
    (
        "crates/zephium-engine/src/agent_work_resource_probe_port.rs",
        &[
            "self.resource==next.resource", "self.view==next.view", "self.world==next.world", "self.document==next.document",
            "self.completed.checked_add(1)==Some(next.completed)", "next.invocation>self.invocation",
            "self.admission.witness_resource(&request.resource)", "self.admission.reserve()",
            "completion(self.request.clone(),evidence)", "self.permit.release()",
            "if!state.taken||state.sealed||state.factory.is_some()",
        ],
        &["ContextJoin::", "ContextIdentity::", "#[derive(Serialize", "reqwest::", "evaluateJavaScript"],
    ),
    (
        "crates/zephium-engine/src/host/work_resource_witness.rs",
        &[
            "url.host_str()==Some(\"127.0.0.1\")", "url.path()==\"/semantic-rendering-v1.html\"",
            "url.query().is_none()", "url.fragment().is_none()", "url.username().is_empty()", "url.password().is_none()",
            "Arc::ptr_eq(&resource.guard,&guard)", "self.witness_attempted=true",
            "self.retire_witness_state()==State::Retired", "ifstate==State::Retired", "holder.samples>=8", "holder.samples+=1",
            "view.semantic()?.witness_identity()?", "view.work_navigation()?.witness_document()?",
            "resource.last_invocation!=0||guard.execution_reserved()",
            "resource.witness=Some(RenderingHolder", "lease.present_resource(guard.resource())",
            "letSome(permit)=guard.notification_permit()", "Duration::from_millis(50)",
            "self.action.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()",
            "queued.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()",
        ],
        &["ContextJoin", "ContextIdentity", "cancel_run(", "evaluateJavaScript", "load_url(", "setActivationPolicy", "activateIgnoringOtherApps", "makeKeyAndOrderFront", "setInactiveSchedulingPolicy"],
    ),
    (
        "crates/zephium-engine/src/platform/macos/agentic_resource_driver.rs",
        &[
            "letForegroundRenderingAdmission(human)=admission", "if!human.is_current()",
            "mpsc::sync_channel(4)", "Duration::from_secs(15)", "Duration::from_secs(5)",
            "[u64;7]=[0,50,100,200,400,800,1600]", "Vec::with_capacity(8)",
            "self.rows.construct_document(", "self.rows.observation_dispatch_refused(*request)",
            "failure:ContextPortFailure::Stale", "old.run()!=lease.run()", "old.resource()==lease.resource()",
            "prior.retained_after_one_read(&stamp)", "self.ended!=2", "self.port.seal_for_shutdown(audit)",
            "!rendering_reply_matches(&self.phase,request)", "*index<2&&request==expected&&request.operation==RenderOp::Inspect",
            "sample_foreground_snapshot(&snapshot,correlation.frame().context(),&origin)",
            "self.rows.is_quiescent()", "self.human.is_current()", "drop(self)",
        ],
        &["ContextIdentity::", "ContextRegistry::", "invoke_semantic(", "ContextNativeRequest::", "evaluateJavaScript", "NSApplication::", "reqwest::", "OPENAI_API_KEY", "std::thread::sleep", "runUntilDate", "activateIgnoringOtherApps", "makeKeyAndOrderFront"],
    ),
    (
        "crates/zephium-agentic/src/work_browser_observation.rs",
        &[
            "self.admits_lease(lease,now)?",
            "row.observation_sequence.checked_add(1)",
            "*sequence<=MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS",
            "ContextGeneration::new(lease.generation)",
            "row.observation_sequence=sequence;row.observation=Some(join.clone())",
            "row.observation.as_ref()!=Some(&completion.join)",
            "row.lease.as_ref()!=Some(&completion.join.lease)",
            "now>=completion.join.lease.deadline",
            "SemanticRuntimeSettlement::try_new(self.join.correlation.clone(),outcome)",
            "SemanticObservationBudget::INITIAL_FILTERED",
            "SemanticRuntimeBudget::INITIAL_FILTERED",
        ],
        &[".cancel_run(", "evaluateJavaScript", "ForegroundRendering"],
    ),
    (
        "crates/zephium-engine/src/work_resource_port.rs",
        &[
            "request.resource()!=&self.resource",
            "request.document()!=self.document.as_ref()",
            "construction_pending:true",
            "state.phase==Phase::Constructing&&state.construction_pending&&!state.uncertain",
            "!state.construction_pending&&state.retirement_delivery.is_none()&&state.reads==0&&state.callbacks==0&&!state.notification_pending",
            "letremove=guard.construction_returned()",
            "self.admission.work_construction_returned(&guard)",
            "state.phase=Phase::Revoking",
            "state.callbacks==0",
            "now<lease.deadline()",
            "state.reads==0&&state.callbacks==0&&!state.notification_pending",
            "admission.reserve_audit().ok()",
            "state.notification_pending||state.phase==Phase::Destroyed",
            "completion(request.complete(outcome))",
            "self.guard.read_terminal_begin()",
            "self.guard.read_terminal_end()",
            "self.permit.release()",
            "state.phase==Phase::Retained&&!state.uncertain&&state.lease.is_none()&&state.retirement_delivery.is_none()",
            "state.phase==Phase::Acquiring&&state.retirement_delivery.is_none()",
            "state.lease.is_some()||state.retirement_delivery.is_some()",
            "state.retirement_delivery.as_ref()==Some(lease)",
            "exact&&callback_returned&&permit_released&&port_open&&self.health_current()&&!state.uncertain&&state.phase==Phase::Retained",
            "health:Option<WorkBrowserResourceHealthReporter>,health_permit:Option<AgentTaskPermit>",
            "guard.health=health;guard.health_permit=health_permit",
            "health.is_current(&self.resource)",
            "ifrequest.operation()==Operation::Construct{guard.install_health();}",
            "letpublished=ifretained{delivery.is_none_or(|owner|owner.publish_returned())}else{drop(delivery);false}",
            "drop(state);letnotified=notification.is_none_or(|notification|notification.notify())",
            "if!notified{state.uncertain=true;",
            "ifexact&&permit_released&&state.retirement_delivery.as_ref()==Some(lease){",
            "self.permit.release();ifletSome(lease)=&revocation{self.guard.finish_revocation_delivery(",
            "self.permit.released&&self.permit.admission.counts().is_some()",
            "(construction||revocation.is_some())&&self.guard.destruction_started()",
            "Arc::ptr_eq(current,guard)",
            "ingress.rows.len()>=MAX_LIVE_CONTEXTS",
            ">=zephium_agentic::MAX_EXECUTING_CONTEXTS",
        ],
        &[
            "ContextIdentity::new(",
            "ContextJoin",
            "cancel_run(",
            "ForegroundRendering",
        ],
    ),
    (
        "crates/zephium-engine/src/host/work_resource.rs",
        &[
            "self.agent_contexts.len()+self.work_resources.len()>=MAX_LIVE_CONTEXTS",
            "execution_count<=MAX_EXECUTING_CONTEXTS",
            "guard.acquisition_current(lease,now)",
            "if!guard.construction_current()",
            "operation==Operation::Destroy&&!self.work_resources.contains_key(&id)&&!guard.callbacks_drained()",
            "WorkNativeResource::unconstructed(guard.clone(),reservation)",
            "ifresource.prepare_destruction()",
            "self.retire_construction();ifletSome(task)=self.revocation.take(){task.complete(Outcome::Refused);}self.destruction_drained()",
            "self.retirement_clean&&self.view.is_none()&&self.guard.callbacks_drained()&&self.observation.is_none()",
            "!self.erasure_tombstones.contains(&resource.profile())",
            "native_resource.reclassify(NativeResourceClass::AgentContext)",
            "retirement_clean:false",
            "crate::platform::imp::build_owned_work_view(",
            "resource_retained:true",
            "guard.lease_drained(lease)",
            "guard.callbacks_drained()",
            "resource.observation.is_none()",
            "view.semantic_pending_for_audit()==Some(false)",
            "request.invocation().invocation().get()>resource.last_invocation",
            "request.invocation().scope()==zephium_agentic::SemanticRuntimeScopeClass::Initial",
            "dispatch2::DispatchQueue::main().exec_async(move||",
            "Arc::ptr_eq(&resource.guard,&guard)",
            "request.operation()==self.operation&&request.lease()==self.lease.as_ref()",
            "resource.lifecycle_deadline.is_some_and(|deadline|Instant::now()>=deadline)",
        ],
        &[
            "ContextJoin",
            "view_origin",
            "cancel_run(",
            "cancel_semantic(",
            "AgentOwnedContext",
            "ContextNativePermit",
            "dispatch_action(",
            "evaluateJavaScript",
            "ForegroundRendering",
            "probe_",
        ],
    ),
    (
        "crates/zephium-engine/src/platform/work_document_navigation.rs",
        &[
            "state.phase!=Phase::Bootstrap||!state.bootstrap_finished||state.target.is_some()",
            "expected.as_url().as_str()==target",
            "state.native_id==Some(event.id)",
            "Some(target.as_url().as_str())==current",
            "ifstate.phase==Phase::Ready{state.phase=Phase::Refused",
            "state.phase=Phase::Retired",
        ],
        &[
            "ContextRunId",
            "ContextJoin",
            "AgentNavigationOperation",
            "evaluateJavaScript",
        ],
    ),
    (
        "crates/zephium-engine/src/agent_context_port.rs",
        &[
            "if!self.work_is_absent(){return;}",
            "if!self.work_is_absent(){returnErr(ContextPortFailure::ProfileBusy);}",
            "self.schedule_work_lifecycle(request,completion)",
            "self.schedule_work_observation(request,completion)",
        ],
        &[],
    ),
    (
        "crates/zephium-engine/src/host/agent_context.rs",
        &[
            "checked_add(self.work_resources.len())",
            ".filter(|resource|resource.resident())",
            ".filter(|resource|resource.pending())",
            "task.work_ingress_matches(",
            ".any(|resource|resource.profile()==profile)",
            "self.force_shutdown_work_resources()&&self.agent_contexts.is_empty()",
        ],
        &[],
    ),
    (
        "crates/zephium-engine/src/host/content_rules.rs",
        &[
            "self.work_resources",
            "resource.replace_content_policy_registration(registration)",
        ],
        &[],
    ),
];

fn validate_adapter(source: &str, required: &[&str], forbidden: &[&str]) -> Result<(), String> {
    let source = compact(production(source));
    for needle in required {
        require(&source, needle)?;
    }
    for needle in forbidden {
        if source.contains(needle) {
            return Err(format!("Work native boundary forbids {needle}"));
        }
    }
    Ok(())
}

fn production(source: &str) -> &str {
    source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(production, _)| production)
}

fn compact(source: &str) -> String {
    source.chars().filter(|c| !c.is_whitespace()).collect()
}
fn require(source: &str, needle: &str) -> Result<(), String> {
    if source.contains(needle) {
        Ok(())
    } else {
        Err(format!("Work resource boundary missing {needle}"))
    }
}
fn validate(source: &str, port: &str) -> Result<(), String> {
    let source = compact(source);
    let port = compact(port);
    require(&source, "pubstructWorkBrowserResourceIdentity{work:WorkId,resource:WorkBrowserResourceId,profile:ProfileId,context:ContextId,}")?;
    for needle in [
        "Arc::ptr_eq(&self.0,&other.0)",
        "self.sequence.checked_add(1)",
        "self.rows.len()>=MAX_LIVE_CONTEXTS",
        "row.lease.is_some()).count()>=MAX_EXECUTING_CONTEXTS",
        "row.phase=WorkBrowserResourcePhase::Revoking;row.pending=Some(operation.clone())",
        "row.destruction_attempted=true;row.destruction=Some(operation.clone())",
        "sequence:lease.generation",
        "sequence:resource.incarnation",
        "ifsealed||now>=lease.deadline",
        "WorkBrowserResourceEvent::RevocationRequired(lease.clone())",
        "ifdebt.bounded()&&debt.is_empty()&&resource_retained",
        "resource_retained&&row.observation.is_none()",
        "row.pending.is_none()&&row.destruction.is_none()&&row.lease.is_none()",
        "row.pending.as_ref()!=Some(&completion.operation)",
        "row.destruction.as_ref()==Some(&completion.operation)",
        "WorkBrowserResourceEvent::DebtSettled(row.join.clone())",
        "request:Box<WorkBrowserResourceRequest>",
    ] {
        require(&source, needle)?;
    }
    for forbidden in [
        "ContextIdentity::new(",
        "AgentNativeShutdownProof",
        "std::thread",
        "tokio::",
        "ContextNativeResourceSnapshot",
        "cancel_run(",
        "begin_document_load(",
        "evaluateJavaScript",
        "SEMANTIC_RUNTIME_CHANNEL_STOP",
    ] {
        if source.contains(forbidden) {
            return Err(format!("Work resource boundary forbids {forbidden}"));
        }
    }
    require(&port, "fnwork_resource_lifecycle(&self,request:crate::WorkBrowserResourceRequest,_completion:crate::WorkBrowserResourceCompletionCallback,)->crate::WorkBrowserResourceDispatch{crate::WorkBrowserResourceDispatch::Rejected{request:Box::new(request),failure:ContextPortFailure::Unsupported,}}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = include_str!("../../crates/zephium-agentic/src/work_browser_resource.rs");
    const PORT: &str = include_str!("../../crates/zephium-agentic/src/context_port.rs");
    #[test]
    fn resource_boundary_rejects_run_ownership_false_drain_and_probe_authority() {
        validate(SOURCE, PORT).unwrap();
        let source = compact(production(SOURCE));
        for changed in [
            source.replace("work:WorkId,", "work:ContextRunId,"),
            source.replace("Arc::ptr_eq(&self.0,&other.0)", "true"),
            source.replace("debt.is_empty()&&resource_retained", "true"),
            source.replace("sealed||now>=lease.deadline", "false"),
            source.replace("row.destruction.is_none()", "true"),
            source.replace("sequence:lease.generation", "sequence:self.next()?"),
            format!("{source}\nlet fake = AgentNativeShutdownProof;"),
            format!("{source}\ncancel_run();"),
        ] {
            assert!(validate(&changed, PORT).is_err());
        }
        assert!(validate(
            SOURCE,
            &PORT.replace(
                "failure: ContextPortFailure::Unsupported,",
                "failure: ContextPortFailure::Cancelled,"
            )
        )
        .is_err());
    }
    #[test]
    fn native_boundary_rejects_missing_ownership_guards_and_legacy_authority() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for (path, required, forbidden) in ADAPTER_RULES {
            let original = std::fs::read_to_string(root.join(path)).unwrap();
            let source = compact(production(&original));
            validate_adapter(&source, required, forbidden)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            for needle in *required {
                let changed = source.replace(needle, "false");
                assert!(
                    validate_adapter(&changed, required, forbidden).is_err(),
                    "{path}: {needle}"
                );
            }
            for needle in *forbidden {
                assert!(
                    validate_adapter(&format!("{source}{needle}"), required, forbidden).is_err(),
                    "{path}: {needle}"
                );
            }
        }
    }
}
