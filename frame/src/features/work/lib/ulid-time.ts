const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/** The creation time a ULID carries in its first ten characters, if it is one. */
export function ulidTime(id: string): Date | null {
  if (id.length !== 26) return null;
  let millis = 0;
  for (const char of id.slice(0, 10).toUpperCase()) {
    const digit = CROCKFORD.indexOf(char);
    if (digit < 0) return null;
    millis = millis * 32 + digit;
  }
  return millis > 0 ? new Date(millis) : null;
}
