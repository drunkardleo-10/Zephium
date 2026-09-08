# Retained evidence during progressive inspection

Progressive inspection replaces the active browser observation and its action
references. It must not erase every source the agent already inspected. The
Work controller now retains bounded public read primitives before replacing an
acknowledged observation. Terminal extraction can cite these historical sources
even if the final inspection is empty.

This is a same-document foundation. The collection is owned by one Work state;
navigation clears it before dispatch, and success, cancellation and recovery
release it. Context, document/cancellation generation and trusted source-role
selection must match at admission and mapping. A different account cannot reuse
the original policy taint. Cross-page synthesis remains a separate contract;
this implementation never silently grants it.

## Authority and provenance

`SemanticRetainedReadEvidence` owns safe public fragments derived from exact
acknowledged observations. It holds no DOM, complete snapshot, native handle,
action operation, URL navigation capability, credential or task transcript.
Sensitive fragments are refused; secrets remain withheld by the ordinary read
projection. Frame metadata is shared within a capture instead of copied for
every source string. Debug formatting exposes counts only.

The terminal read is authorized under the **current** acknowledgement, while
each retained source keeps its original observation, observation generation,
frame/origin, invocation, snapshot, reference and monotonic capture time.
Its source guard and the original committed origin/account/reference taint are
independently checked before provider input admission. Removing either the
current baseline or an original source cohort refuses mapping. Historical reads
are rejected by the ordinary nonterminal read path.

Extraction assigns unique result-local `@r` citations across the delivered
inventory. Historical `@a` numbers are source coordinates only. The mapping
encoding labels retained history and emits each source's original coordinates;
they never enter the active observation or authorize actions, locate or native
inspection. The full-request token, policy, cost and deadline gates still apply.

The resulting owned sources preserve individual capture coordinates. Archived
result format v2 records those coordinates for each source and accepts existing
canonical v1 archives without upgrading them into execution authority. The
result-level observation and capture time identify the terminal read baseline;
source-level provenance identifies when each quoted fact was observed.

## Bounds and selection

The evidence owner keeps at most eight nonempty captures, 128 public primitives
and 32 KiB of primitive content in total. Re-admission of an identical capture
is idempotent. An empty capture preserves useful older sources. When admitting a
new capture would exceed the bounds, oldest whole captures are evicted and
omission flags/counts record the loss. Building a replacement can temporarily
hold the old bounded inventory plus one additional STANDARD read-sized copy.
Frame metadata remains independently bounded by the original observation caps.

Terminal mapping prioritizes the current read, followed by retained captures
newest first, under the same 128-fragment/32-KiB read ceiling. Additional omitted
fragments are counted; original privacy and incomplete-source flags survive.
Quotes are copied exactly and never joined, summarized or deduplicated by their
meaning. Contradictory captured values remain distinct historical sources.

This is deterministic bounded retention, not relevance ranking or unlimited
memory. A later broad capture can displace older narrow evidence under pressure.
Decisions still receive the fresh observation and existing content-free capture
progress; retained text is disclosed in the final mapper inventory. A useful
future extension is an explicitly budgeted evidence index for planning and
cross-page synthesis, without restoring old action references.

## Verification

Core tests cover exact source retention after an empty capture, original quote
and timestamp fidelity, duplicate admission, sensitive/role/context refusal,
capture/byte pressure, explicit omissions and cleanup. Policy tests require the
current baseline and every original source cohort, reject foreign accounts and
reject retained history on ordinary reads. Archive tests cover v2 source
coordinates and incomplete metadata while existing v1 tests remain in place.

The shipping controller and loopback-provider regression executes navigation,
an empty progressive capture and terminal extraction. It checks that the mapper
receives the earlier evidence from the current document, never receives the
departure document, preserves the earlier source timestamp, and rejects an
invented result-local citation. Real Luna qualification is recorded separately;
these deterministic tests establish the implementation contract, not answer
quality or general website compatibility.
