/** Public research transmits only the exact objective saved by Create. */
export function publicResearchQueryValid(objective: string) {
  return (
    objective.length > 0 &&
    [...objective].length <= 512 &&
    new TextEncoder().encode(objective).byteLength <= 2048
  );
}
