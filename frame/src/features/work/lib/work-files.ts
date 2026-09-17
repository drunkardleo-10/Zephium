/** Display helpers for granted folders and the files a run touched. */
const SEPARATOR = /[/\\]/u;
const HOME = /^(?:\/Users\/[^/]+|\/home\/[^/]+|[A-Za-z]:\\Users\\[^\\]+)(?=[/\\]|$)/u;

/** The last segment of a path: what a person calls the file or the folder. */
export function fileName(path: string): string {
  return path.split(SEPARATOR).filter(Boolean).at(-1) ?? path;
}

/** The folder a file sits in, as a person reads it. */
export function fileFolder(path: string): string {
  const parts = path.split(SEPARATOR);
  parts.pop();
  return homePath(parts.join("/"));
}

/** The path as a person reads it: their home folder is `~`. */
export function homePath(path: string): string {
  return path.replace(HOME, "~");
}
