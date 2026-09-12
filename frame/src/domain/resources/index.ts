export { resourceSession } from "./resources.svelte";
export type { ResourceSession } from "./resources.svelte";
export type {
  ResourceDraft_Deserialize as ResourceDraft,
  ResourceRecord_Serialize as ResourceRecord,
  ResourceSummary,
  DocumentNode_Deserialize as DocumentNode,
  NoteDocument_Deserialize as NoteDocument,
} from "$shared/ipc/bindings";
export { noteReferences } from "./resource-model";
