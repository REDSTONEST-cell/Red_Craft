import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import ts from 'typescript';

async function multiplayerModule() {
  const source = (await readFile(new URL('../src/lib/multiplayer.ts', import.meta.url), 'utf8'))
    .replace(/^import .*;$/m, 'const invoke = () => {};');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const file = new URL(`./.multiplayer-${crypto.randomUUID()}.mjs`, import.meta.url);
  await writeFile(file, code);
  try { return await import(file.href); } finally { await unlink(file); }
}

test('room codes follow the Terracotta / Scaffolding rules', async () => {
  const { normalizeRoom } = await multiplayerModule();
  // 真实陶瓦 0.4.2 生成的房间号
  assert.equal(normalizeRoom('U/0AMH-FGZJ-EF82-C6CA'), 'U/0AMH-FGZJ-EF82-C6CA');
  // 省略 U/、小写、空格；I/O 视为 1/0
  assert.equal(normalizeRoom('  0amh-fgzj-ef82-c6ca '), 'U/0AMH-FGZJ-EF82-C6CA');
  assert.equal(normalizeRoom('U/OAMH-FGZJ-EF82-C6CA'), 'U/0AMH-FGZJ-EF82-C6CA');
  // 校验位不对（陶瓦对它返回 400）与格式错误
  assert.equal(normalizeRoom('U/AAAA-AAAA-AAAA-AAAB'), null);
  assert.equal(normalizeRoom('U/AAAA-AAAA-AAAA-AAAA'), 'U/AAAA-AAAA-AAAA-AAAA');
  assert.equal(normalizeRoom('U/ABCD-EFGH'), null);
  assert.equal(normalizeRoom(''), null);
});
