import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import ts from 'typescript';

// 「始终播放」：系统要求减少动态效果（Windows 关闭「在 Windows 中显示动画」）时也播放，窗口隐藏时仍然暂停。
async function backgroundModule() {
  const source = (await readFile(new URL('../src/lib/background.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const invoke = () => {}; const convertFileSrc = p => p;');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
  const file = new URL(`./.background-motion-${crypto.randomUUID()}.mjs`, import.meta.url);
  await writeFile(file, code);
  try { return await import(file.href); } finally { await unlink(file); }
}

test('always plays despite reduced motion but pauses when hidden', async () => {
  const { shouldPlay } = await backgroundModule();
  assert.equal(shouldPlay(true, 'always', true), true);
  assert.equal(shouldPlay(true, 'always', false), true);
  assert.equal(shouldPlay(false, 'always', true), false);
  assert.equal(shouldPlay(true, 'auto', true), false);
});

test('dynamic background yields to a running game only while the launcher is unfocused', async () => {
  const { yieldsToGame } = await backgroundModule();
  assert.equal(yieldsToGame(true, false), true);
  assert.equal(yieldsToGame(true, true), false);
  assert.equal(yieldsToGame(false, false), false);
});
