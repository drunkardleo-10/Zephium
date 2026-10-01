import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, symlink, rm, chmod } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { bundleManifest } from './bundle-manifest-v1.mjs';

test('manifest is location/order independent and binds bytes, modes and literal links', async () => {
  const temporary = await mkdtemp(join(tmpdir(), 'zephium-bundle-manifest-'));
  try {
    const bundles = [join(temporary, 'one.app'), join(temporary, 'two.app')];
    for (const [index, bundle] of bundles.entries()) {
      await mkdir(join(bundle, 'Contents'), { recursive: true, mode: 0o700 });
      for (const name of index ? ['z resource', 'a\nresource'] : ['a\nresource', 'z resource']) {
        await writeFile(join(bundle, 'Contents', name), name, { mode: 0o600 });
      }
      await symlink('../external-not-followed', join(bundle, 'link'));
    }
    const original = await bundleManifest(bundles[0]);
    assert.equal(original, await bundleManifest(bundles[1]));
    assert.equal(JSON.parse(original).entries.filter(entry => entry.kind === 'symlink').length, 1);
    await writeFile(join(bundles[1], 'Contents', 'z resource'), 'changed');
    assert.notEqual(original, await bundleManifest(bundles[1]));
    await writeFile(join(bundles[1], 'Contents', 'z resource'), 'z resource');
    await chmod(join(bundles[1], 'Contents', 'z resource'), 0o400);
    assert.notEqual(original, await bundleManifest(bundles[1]));
    await symlink(bundles[0], join(temporary, 'alias.app'));
    await assert.rejects(bundleManifest(join(temporary, 'alias.app')), /real directory/);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});
