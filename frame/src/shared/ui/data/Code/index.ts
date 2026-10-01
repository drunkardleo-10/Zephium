/** The block and its tokenizer arrive together, only when code is shown. */
export const loadCodeBlock = () => import("./CodeBlock.svelte");
export { CODE_CARD_LINES, codeLines, type CodeNoteView } from "./code";
