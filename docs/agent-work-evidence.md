# Immutable Work evidence foundation

`WorkEvidenceBuilder` collects already-validated source-carrying extractions
from multiple retained browser documents/resources. It is a data-domain join,
not another browser controller, model loop, supervisor or publication path.

Each `WorkEvidenceEntry` joins every cited source to the exact historical
`WorkBrowserReadBinding`. The native-finalized document, existing resource and
Work/profile identity remain attached to the immutable extraction. Entry
metadata retains the original durable/ephemeral storage class; collecting
evidence does not authorize persistence of ephemeral browser data. Entry
admission is public-only; a result with sensitive citations, foreign document
frames or a secret-like document URL is refused without consuming the caller's
original result. The collection freezes the first contribution's original
`ContextRunId` and rejects different Work/profile/run contributions. It does
not infer a manifest, producer node or execution-attempt identity. Those belong
to the original orchestration completion join.

The result's read omission flags and counts survive borrowed-to-owned
extraction. They describe the original delivered read, not merely its cited
subset. No-omission means no known omission within that bounded projection;
it does not establish full-document coverage. Values remain `ModelMapped`
and `NeedsReview`: citation validity is neither entailment, agreement nor
independent verification. Contradictory contributions coexist unchanged.

Fragment identifiers are local to an entry's exact result. A source identity
is therefore entry plus result-local fragment, never a collection-global
`@r1`. Scalar, list-item and collection-level spans retain the original result
guard. The collection does not concatenate transcripts, rewrite citations,
merge facts or restore old observations as actionable baselines.

## Bounds and ownership

- At most 16 contributions and 1 MiB aggregate retained strings/source
  primitives, with smaller caller-selected bounds. Structural metadata remains
  independently bounded by the existing per-extraction limits.
- Exact duplicates and exhaustion refuse insertion atomically. No prior entry
  is overwritten or silently evicted; the caller keeps its original input.
- Freezing a nonempty builder creates an immutable set. Set/entry sharing uses
  `Arc`, not copies of pages or unbounded source history.
- No browser lease/incarnation owner, native callback, provider body, worker or
  clock remains in the evidence entry. Historical source coordinates are data,
  not proof that a page is still live or an effect can execute.
- `WorkEvidenceDescriptor` exposes only identity, existing Work/profile/run,
  ordered integrity digest and counts. It cannot retrieve content, publish an
  artifact or certify successful task completion. Debug paths omit identities,
  digests, document URLs, model values and quoted text.

This slice provides no codec, database schema, artifact publication, restart
restoration, model-input encoder or new data-flow authority. Later persistence
must preserve the same lineage and omissions through an explicitly versioned
private-data codec and its original transactional publication owner. Later
synthesis must separately admit each source-to-provider disclosure and validate
entry-qualified references; possession of a set/descriptor grants neither.
The existing single-result artifact format and publication protocol are
unchanged.
