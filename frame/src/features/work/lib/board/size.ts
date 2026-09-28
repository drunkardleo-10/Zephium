/** The room every legacy block keeps around its title and body. */
export const PAD = 20;
/** A gallery shows this many before it counts the rest. */
export const GALLERY = 6;
/** Rows a table shows before it opens; a gallery's card; a code block's lines. */
export const TABLE_ROWS = 8;
export const CODE_LINES = 16;
export const CARD = { width: 216, gap: 12 } as const;
/** A card with a picture is as wide as its picture; one with a mark beside its words, wider and fewer across. */
export const cardWidth = (block: { entities: readonly { image?: unknown }[] }) =>
  block.entities.some((entity) => entity.image) ? CARD.width : 300;
