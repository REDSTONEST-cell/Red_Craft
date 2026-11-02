import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import ts from 'typescript';

async function filterModule() {
  const source = await readFile(new URL('../src/lib/versionFilter.ts', import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
  const file = new URL(`./.version-filter-${crypto.randomUUID()}.mjs`, import.meta.url);
  await writeFile(file, code);
  try { return await import(file.href); } finally { await unlink(file); }
}

const v = (id, gameVersions, loaders, releaseType) => ({ id, gameVersions, loaders, releaseType });
const versions = [
  v('a', ['1.21.10'], ['fabric'], 'alpha'),
  v('b', ['1.21.10', '1.21.9'], ['fabric'], 'release'),
  v('c', ['1.20.1'], ['forge'], 'release'),
  v('d', ['25w14a'], ['fabric'], 'beta'),
  v('e', ['1.21.2'], ['neoforge'], 'release'),
];

test('game versions are sorted newest first, snapshots last', async () => {
  const { filterOptions } = await filterModule();
  const o = filterOptions(versions, { game: null, loader: null, type: null });
  assert.deepEqual(o.game.map(([g]) => g), ['1.21.10', '1.21.9', '1.21.2', '1.20.1', '25w14a']);
  assert.equal(o.gameTotal, 5);
  assert.deepEqual(o.type.map(([t]) => t), ['release', 'beta', 'alpha']);
});

test('counts of each category follow the other selections', async () => {
  const { filterOptions, matchesFilter } = await filterModule();
  const f = { game: '1.21.10', loader: null, type: null };
  const o = filterOptions(versions, f);
  assert.deepEqual(o.loader, [['fabric', 2]]);
  assert.deepEqual(o.type, [['release', 1], ['alpha', 1]]);
  assert.equal(o.gameTotal, 5);
  assert.deepEqual(versions.filter((x) => matchesFilter(x, { ...f, type: 'release' })).map((x) => x.id), ['b']);
});
