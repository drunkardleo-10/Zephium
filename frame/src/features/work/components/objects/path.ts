/** A file's folder and its name, for a header that sets the name and quiets the rest. */
export function splitPath(path: string): { folder: string; name: string } {
  const at = path.lastIndexOf("/");
  return at < 0
    ? { folder: "", name: path }
    : { folder: path.slice(0, at + 1), name: path.slice(at + 1) };
}
