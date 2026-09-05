# Profile-owned Work extraction artifacts

## Decision and scope

Small, bounded extraction results use a separate private body table in the
existing profile-aware SQLite Store. A result and its successful run terminal
must commit in one FULL-synchronous transaction. This avoids an independently
committed blob/terminal pair and a second writer or recovery protocol. Large
immutable artifacts still require the planned content-addressed blob vault;
this is not its general-purpose replacement.

Persistence is explicit trusted product intent, not a model choice. Ordinary
extraction remains memory-only. `AgentWorkRunInput::persist_extraction_result`
requires a durable Work-owned context and an extraction task. Before native
creation, admission persists the exact result profile and Store checks that
profile against its authoritative registry. An ephemeral/incognito context
cannot opt in. No ordinary tab, browser profile, execution port, or policy
authority is recreated by artifact retrieval.

## Immutable publication

The original successful controller outcome retains its owned result until
native teardown, runtime/provider settlement, policy/accounting closure and
audit delivery have produced the existing terminal owners. Only their
proof-bearing successful mutation can prepare an artifact publication. The
publication binds the original run key, exact profile, schema, result values
and cited public source fragments; it receives one identity and canonical body
digest. Retransmission shares that original immutable allocation.

Admission's result-profile intent is immutable. A promised result cannot be
downgraded to a bodyless `Succeeded` write. Store inserts the body and applies
the exact terminal compare-and-set in one transaction. Repeating the same
publication acknowledges the same body and terminal; a different identity,
digest, body, profile or expected record is refused. No action is replayed.

A pre-commit crash leaves neither body nor success; the existing restart
protocol classifies the old run as interrupted. A post-commit crash leaves
both immutable facts, even if the application did not receive the ACK or the
consumer did not receive its in-memory result. Failed/uncertain publication
retains the original application owner for bounded, explicit storage-only
reconciliation; it is never reported as clean success.

## Retrieval is data, not restored authority

The current process incarnation, exact successful record and authorized
profile must join the stored body. Decoding enforces version, length, digest,
canonical encoding, bounded field/citation shapes, public source constraints
and the existing secret-text filter. Historical source identity components
remain primitive archived data: the decoder cannot create a live context join,
opaque action ref, trusted task contract, successful policy proof or runnable
session. Archived values remain explicitly `ModelMapped`, not factual truth.

The application exposes a dedicated one-shot archived-result handoff separate
from content-free snapshots/events/journal/audit. A failed or timed-out read
does not consume a future admission. A stale callback cannot fill a newer
read slot. One unread result applies backpressure instead of being overwritten.

## Bounds and privacy

- One immutable result per run; at most 256 KiB canonical body.
- At most 32 MiB of result bodies in this Store, within the existing 1,024-run
  inventory ceiling. Pressure refuses publication; there is no silent eviction.
- Existing field/value/citation bounds apply; only referenced source fragments
  are copied, not observations, pages, transcripts or provider payloads.
- The existing Store worker and four shared optional Work permits serve
  journal and artifact requests. The application retains one durable request
  and allows at most four explicit reconciliation attempts. No new worker,
  background timer or default Browse dependency is introduced.
- The body is private product data, never diagnostics. Debug is redacted;
  objectives, credentials and raw provider/page payloads are not persisted.
- Authorized profile deletion removes its artifact bodies through the same
  registry transaction. Content-free run facts and required-result intent
  remain for reconciliation; an absent/deleted profile cannot publish or read.

The current database uses the existing OS file-permission boundary, not
application-level encryption. Neither SQLite deletion nor profile deletion is
a forensic secure-erase promise. General artifact deletion/export, large blob
storage, UI, and execution restoration are not part of this vertical.

## Cold native cleanup finding

The first public artifact run published and reloaded its exact cited result,
but the no-UI qualifier's normal Shell deletion request correctly refused:
that host has no Browse bootstrap. Excluded cleanup now composes the existing
Store deletion authorization, exact native-namespace obligation, engine absence
proof and Store finalization ports; it does not weaken Shell policy or create
ordinary tabs. Failure retains the exact private test recovery directory.

A separate cold recovery exposed a pre-existing WebKit initialization defect:
identifier enumeration crashed in the WebsiteDataStore I/O queue before any view
had initialized WebKit's main run loop. Upstream [enumeration code](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/WebsiteData/Cocoa/WebsiteDataStoreCocoa.mm)
dispatches to the main singleton without initialization, whereas the
[API object constructor](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/Shared/API/APIObject.cpp)
initializes WebKit. The scoped production erasure adapter now constructs and
immediately releases only a `WKWebViewConfiguration` before enumeration. It
never accesses a lazy data-store/process-pool property or creates a page. The
same retained deletion subsequently passed native absence verification and
Store finalization in a cold process. No raw crash report or profile identity
is committed; this is scoped recovery evidence, not general platform qualification.

The following same-process run exposed a second, qualifier-only defect: its
manual native dispatcher executed outside an autorelease pool. The pool around
`NSRunLoop` pumping could not release objects created outside that pool. Work
dispatch now uses one pool per operation, matching Cocoa event dispatch;
production scheduling and native actions are unchanged. The corrected full
Luna application run independently verified ten values, a 5,296-byte artifact,
native profile absence, Store finalization and clean Shell shutdown with zero
focus theft. Both earlier retained public-test deletion obligations were also
recovered and finalized, without repeating any provider/native task action.

If the trusted task declares an extraction schema but incorrectly reports
completion without a result, publication preparation refuses and retains the
original clean execution owner. It cannot report durable success or clean
application closure. There is currently no in-process discard/repair authority
for this invalid trusted contract; restart classifies the uncommitted run as
interrupted. This is distinct from a retained, explicitly reconcilable Store
publication timeout/refusal.
