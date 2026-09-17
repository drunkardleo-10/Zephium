/** Display helpers for granted folders and the files a run touched. */
const HOME = /^(?:\/Users\/[^/]+|\/home\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)(?=[/\\]|$)/u;

/** The path as a person reads it: their home folder is `~`. */
export function homePath(path: string): string {
  return path.replace(HOME, "~");
}
