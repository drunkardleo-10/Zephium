// Read-only artifact inventory, not signature verification or a filesystem seal.
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { lstat, readdir, readlink } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export async function bundleManifest(bundle) {
  const root = resolve(bundle);
  if (!(await lstat(root)).isDirectory()) throw new Error('bundle must be a real directory');
  const entries = [];
  async function visit(relative) {
    const path = `${root}/${relative}`;
    const stat = await lstat(path);
    const entry = { path: relative, mode: (stat.mode & 0o7777).toString(8).padStart(4, '0') };
    if (stat.isSymbolicLink()) {
      // Record literal link bytes; never traverse targets outside/inside bundle.
      entries.push({ ...entry, kind: 'symlink', target_hex: (await readlink(path, { encoding: 'buffer' })).toString('hex') });
    } else if (stat.isDirectory()) {
      entries.push({ ...entry, kind: 'directory' });
      const names = await readdir(path, { encoding: 'buffer' });
      names.sort(Buffer.compare);
      for (const name of names) {
        const decoded = name.toString('utf8');
        if (!Buffer.from(decoded).equals(name)) throw new Error('non-UTF8 bundle entry');
        await visit(`${relative}/${decoded}`);
      }
    } else if (stat.isFile()) {
      const hash = createHash('sha256');
      for await (const chunk of createReadStream(path)) hash.update(chunk);
      entries.push({ ...entry, kind: 'file', bytes: stat.size, sha256: hash.digest('hex') });
    } else {
      throw new Error('unsupported bundle entry');
    }
  }
  // Cover Info.plist, executable, resources and every other bundle entry.
  await visit('.');
  entries.sort((left, right) => Buffer.compare(Buffer.from(left.path), Buffer.from(right.path)));
  return `${JSON.stringify({ schema: 'zephium.bundle-manifest.v1', entries })}\n`;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw new Error('usage: bundle-manifest-v1.mjs BUNDLE.app');
  process.stdout.write(await bundleManifest(process.argv[2]));
}
