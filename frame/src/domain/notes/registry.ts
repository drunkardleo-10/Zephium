import { NoteSession } from "./notes.svelte";

// Keeps unsaved text across a host being hidden and shown. Bounded; a clean
// inactive session can always be rebuilt from the files. Not reactive: which
// sessions exist is never drawn.
const sessions = new Map<string, NoteSession>();
const MAX_SESSIONS = 12;

export function noteSession(profile: string, host: string): NoteSession | null {
  const key = `${profile}:${host}`;
  const existing = sessions.get(key);
  if (existing) return existing;
  if (sessions.size >= MAX_SESSIONS) {
    const clean = [...sessions].find(([, session]) => session.disposable);
    if (!clean) return null;
    clean[1].stop();
    clean[1].removeCloseTask();
    sessions.delete(clean[0]);
  }
  const session = new NoteSession(profile, host);
  sessions.set(key, session);
  return session;
}
