import { taskLists, taskSession } from "$domain/resources";
import { parseCapture, parseTokens } from "./task-language";
import { todayKey } from "./task-sections";

/** Adds one task from a line typed anywhere, such as the launcher, reading it
 *  exactly as the composer does. Resolves to the title it was saved under, or
 *  null when nothing was saved; a failed capture stays with the session and is
 *  retried, never duplicated, by the same words. */
export async function captureTask(profile: string, text: string): Promise<string | null> {
  const today = todayKey();
  const tokens = parseTokens(text.trim(), await taskLists(profile, today));
  const read = parseCapture(tokens.rest, today);
  const title = (read.matched ? read.title : tokens.rest).trim();
  if (!title) return null;
  const id = await taskSession(profile, "launcher").create({
    title,
    dueDate: read.matched ? read.dueDate : null,
    dueTime: read.matched ? read.dueTime : null,
    duration: tokens.duration,
    list: tokens.list,
    inbox: tokens.list === null,
    priority: tokens.priority ?? "none",
  });
  return id ? title : null;
}
