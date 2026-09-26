export const loadNotes = () => import("./components/Notes.svelte");
export const loadNotesPage = () => import("./components/NotesPage.svelte");
export const loadNoteHost = () => import("./components/NoteHost.svelte");
/** The notes Markdown parser, for a view that renders Markdown it did not write. */
export { parse as parseMarkdown } from "./lib/markdown/parse";
