import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import ts from 'typescript';
import { compileModule } from 'svelte/compiler';

async function moduleFrom(source, runes = false) {
  let code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
  if (runes) code = compileModule(code, { generate: 'server' }).js.code;
  const file = new URL(`./.background-${crypto.randomUUID()}.mjs`, import.meta.url);
  await writeFile(file, code);
  try { return await import(file.href); } finally { await unlink(file); }
}

async function backgroundModule() {
  const source = (await readFile(new URL('../src/lib/background.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const invoke = () => {}; const convertFileSrc = p => p;');
  return moduleFrom(source);
}

test('native hidden, static and reduced-motion policies all pause video', async () => {
  const { shouldPlay } = await backgroundModule();
  for (const visible of [false, true]) for (const motion of ['auto', 'static']) for (const reduced of [false, true]) {
    assert.equal(shouldPlay(visible, motion, reduced), visible && motion === 'auto' && !reduced);
  }
});

test('appearance defaults allocate fresh values with no automatic wallpaper selection', async () => {
  const { defaults } = await backgroundModule();
  const a = defaults(); a.selection = 'preview';
  assert.equal(defaults().selection, null);
  assert.equal(defaults().fit, 'cover');
  assert.equal(defaults().panelOpacity, 0.94);
});

test('WE picker only lists videos outside bundled defaultprojects while local images stay available', async () => {
  const { isVisibleBackgroundAsset } = await backgroundModule();
  const base = { title: 'fixture', source: 'wallpaperEngine', media: 'clip.mp4', cover: null, reason: null };
  const entries = [
    { ...base, id: 'workshop', kind: 'video', root: 'C:\\Steam\\steamapps\\workshop\\content\\431960\\123' },
    { ...base, id: 'own', kind: 'video', root: 'C:/Steam/steamapps/common/wallpaper_engine/projects/myprojects/own' },
    { ...base, id: 'bundled', kind: 'video', root: 'C:/Steam/steamapps/common/wallpaper_engine/projects/DEFAULTPROJECTS/demo' },
    { ...base, id: 'scene', kind: 'scene' },
    { ...base, id: 'web', kind: 'web' },
    { ...base, id: 'we-image', kind: 'image' },
    { ...base, id: 'local-image', source: 'local', kind: 'image' },
  ];
  assert.deepEqual(entries.filter(isVisibleBackgroundAsset).map(a => a.id), ['workshop', 'own', 'local-image']);
});

test('background apply failure keeps persisted selection and does not claim success', async () => {
  const { defaults } = await backgroundModule();
  globalThis.__backgroundTest = { defaults, backgroundApi: { apply: async () => { throw new Error('disk full'); } } };
  let source = (await readFile(new URL('../src/lib/background-state.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const { defaults, backgroundApi } = globalThis.__backgroundTest;');
  try {
    const mod = await moduleFrom(source, true);
    mod.background.saved.selection = 'previous';
    mod.background.preview = { ...defaults(), selection: 'new' };
    await assert.rejects(mod.applyBackground(mod.background.preview), /disk full/);
    assert.equal(mod.background.saved.selection, 'previous');
  } finally { delete globalThis.__backgroundTest; }
});

test('background apply publishes backend result and clears preview only after success', async () => {
  const { defaults } = await backgroundModule();
  let commit;
  globalThis.__backgroundTest = { defaults, backgroundApi: { apply: appearance => new Promise(resolve => {
    commit = () => resolve({ ...appearance, blurPx: 24 });
  }) } };
  let source = (await readFile(new URL('../src/lib/background-state.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const { defaults, backgroundApi } = globalThis.__backgroundTest;');
  try {
    const mod = await moduleFrom(source, true);
    mod.background.preview = { ...defaults(), selection: 'new', blurPx: 99 };
    const pending = mod.applyBackground(mod.background.preview);
    assert.equal(mod.background.saved.selection, null);
    assert.equal(mod.background.preview.selection, 'new');
    commit(); await pending;
    assert.equal(mod.background.saved.selection, 'new');
    assert.equal(mod.background.saved.blurPx, 24);
    assert.equal(mod.background.preview, null);
  } finally { delete globalThis.__backgroundTest; }
});

test('an earlier save does not discard a newer preview', async () => {
  const { defaults } = await backgroundModule();
  let commit;
  globalThis.__backgroundTest = { defaults, backgroundApi: { apply: value => new Promise(resolve => { commit = () => resolve(value); }) } };
  const source = (await readFile(new URL('../src/lib/background-state.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const { defaults, backgroundApi } = globalThis.__backgroundTest;');
  try {
    const mod = await moduleFrom(source, true);
    mod.background.preview = { ...defaults(), selection: 'first' };
    const pending = mod.applyBackground(mod.background.preview);
    mod.background.preview = { ...defaults(), selection: 'second' };
    commit(); await pending;
    assert.equal(mod.background.saved.selection, 'first');
    assert.equal(mod.background.preview.selection, 'second');
  } finally { delete globalThis.__backgroundTest; }
});

test('video is muted before play and hiding pauses without unloading the source', async () => {
  const { controlVideo } = await backgroundModule();
  const events = [];
  const video = { muted: false, defaultMuted: false, volume: 1,
    play() { events.push(['play', this.muted, this.defaultMuted, this.volume]); return Promise.resolve(); },
    pause() { events.push(['pause']); } };
  const cleanup = controlVideo(video, true, () => assert.fail('unexpected playback failure'));
  assert.deepEqual(events, [['play', true, true, 0]]);
  cleanup();
  controlVideo(video, false, () => assert.fail('hidden video must not play'))();
  assert.deepEqual(events, [['play', true, true, 0], ['pause'], ['pause'], ['pause']]);
});

test('switching releases the captured old video rather than the new node', async () => {
  const { releaseVideo } = await backgroundModule();
  const events = [];
  const old = { pause: () => events.push('pause'), removeAttribute: name => events.push(`remove:${name}`), load: () => events.push('load') };
  releaseVideo(old);
  assert.deepEqual(events, ['pause', 'remove:src', 'load']);
});

test('a rejected play from a disposed video cannot clear the new background', async () => {
  const { controlVideo } = await backgroundModule();
  let rejectPlay;
  const failures = [];
  const video = { play: () => new Promise((_, reject) => { rejectPlay = reject; }), pause() {} };
  const cleanup = controlVideo(video, true, error => failures.push(error));
  cleanup();
  rejectPlay(new Error('old decoder failed'));
  await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(failures, []);
});

test('a late library read cannot roll back a newer successful background save', async () => {
  const { defaults } = await backgroundModule();
  let finishRead;
  globalThis.__backgroundTest = { defaults, backgroundApi: {
    list: () => new Promise(resolve => { finishRead = resolve; }),
    apply: async value => value,
  } };
  const source = (await readFile(new URL('../src/lib/background-state.svelte.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const { defaults, backgroundApi } = globalThis.__backgroundTest;');
  try {
    const mod = await moduleFrom(source, true);
    const reading = mod.loadBackground();
    await mod.applyBackground({ ...defaults(), selection: 'newer' });
    finishRead({ appearance: { ...defaults(), selection: 'old-snapshot' }, assets: [], roots: [], scanNotes: [] });
    await reading;
    assert.equal(mod.background.saved.selection, 'newer');
  } finally { delete globalThis.__backgroundTest; }
});

test('a native hide event wins over an older pending visibility query', async () => {
  const { observeVisibility } = await backgroundModule();
  let event;
  let finishQuery;
  let unsubscribed = 0;
  const values = [];
  const observer = observeVisibility({
    query: () => new Promise(resolve => { finishQuery = resolve; }),
    subscribe: async fn => { event = fn; return () => { unsubscribed++; }; },
    hidden: () => false,
    change: value => values.push(value),
  });
  await Promise.resolve(); await Promise.resolve();
  event(false);
  finishQuery(true);
  await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(values, [false]);
  event(true);
  assert.deepEqual(values, [false, true]);
  observer.dispose();
  assert.equal(unsubscribed, 1);
});
