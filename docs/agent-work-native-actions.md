# Retained native semantic actions

The macOS engine implements `AgentBrowserPort::work_resource_act` on the existing
retained WKWebView. The accepted native recipes are Click, Fill, and Select.
These reuse the immutable isolated runtime and the separately attested fixed
page-world fill shim. This path neither synthesizes trusted OS input nor grants
page activation. Other platforms retain explicit `Unsupported` until they have
their own verified adapter.

The request must already contain an independently policy-authorized native
effect. Page labels, proposed effect classes, account sessions and the resource
lease cannot authorize an effect. Public discovery remains read-only.

## Admission and execution

The native ingress joins the original resource incarnation, lease, last observed
context, document epoch, native deadline and exclusive operation slot. It
retires observation authority before queueing. The main-thread host separately
checks the exact last invocation and snapshot, native document stamp, profile
erasure state and semantic runtime readiness on that retained view. No legacy
tab/context lookup is used.

The original native execution deadline includes queueing and presentation time.
It is never rebased when a task reaches the main thread. Each action owns a
temporary presentation with the same captured main-window and responder fence
used by retained observations. Presentation does not activate the app or receive
pointer input. The fixed semantic recipe still revalidates its target at the
point of effect.

A runtime request may wait for a page pull. Retained actions therefore carry a
native-only predicate checked immediately before handing the recipe to that
pull. It checks lease ownership, port health, the original deadline, captured
human ownership and the exact native document stamp. Refusal returns the
original correlated cancellation terminal without handing effect bytes to the
page. The predicate never reaches the model or page.

Once the recipe has been handed to the page, cancellation cannot claim that an
effect did not occur. Cancellation, ownership loss, expiry or destruction closes
resource reuse and drains the original callback. An uncertain effect is never
automatically replayed. Native results remain provisional until the controller
observes fresh state and verifies the declared effect through policy.

## Return and retirement

The resource retains the task, presentation, bounded wake owner and native
terminal separately. It delivers the terminal only after presentation retirement,
the native callback's next-main-queue return barrier, and semantic channel idle.
Cleanup has bounded wake opportunities and cannot manufacture a missing terminal.

The app's terminal callback can wake another thread before it returns. An exact
action return ticket prevents that thread from issuing the next snapshot too
early. The engine holds action/callback debt through the callback, releases its
original task permit, clears that exact debt, publishes the physical return fact
under the guard, then notifies outside locks. The app waits for that fact before
continuation. A late callback can establish physical return after revocation;
it cannot establish current authority. A callback panic publishes no successful
return.

Pending action debt blocks observation, navigation, lease retirement, resource
reuse, destruction completion and global shutdown. Dropping a dispatched owner
keeps explicit debt and quarantines the resource. An idle retained resource gains
no timer, worker, provider request or action payload from this implementation.

This is the native integration contract. Passing deterministic tests or a Rust
build does not constitute a bundled real-site action witness; that qualification
must exercise the complete app, controller, policy and native path.
