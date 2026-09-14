import type { MediaAssetV1_Deserialize as MediaAssetV1 } from "$shared/ipc/bindings";
import { IS_MAC } from "$shared/platform";

export type { MediaAssetV1 };

const DIGEST = /^[0-9a-f]{64}$/;
const PROFILE = /^[0-9A-HJKMNP-TV-Z]{26}$/;

/**
 * The privileged media route for one admitted blob. Rust serves only images
 * from the profile's own store to main chrome; anything else is a 404.
 */
export function mediaUrl(profile: string, digest: string): string | null {
  if (!DIGEST.test(digest) || !PROFILE.test(profile)) return null;
  const base = IS_MAC ? "zephium-media://localhost/" : "http://zephium-media.localhost/";
  return `${base}${profile}/${digest}`;
}

export function mediaSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
