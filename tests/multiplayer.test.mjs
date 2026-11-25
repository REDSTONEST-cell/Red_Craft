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

test('port reservation failures get concrete advice', async () => {
  const { exceptionTips } = await multiplayerModule();
  const tips = exceptionTips(2, { portBlocked: true, port: 61823 });
  assert.match(tips[0], /端口 61823/);
  assert.ok(tips.some(t => t.includes('25565')));
  assert.ok(tips.some(t => t.includes('net stop winnat')));
  // 没有诊断结果时给出通用建议
  assert.ok(exceptionTips(2, null).some(t => t.includes('导出联机日志')));
  assert.ok(exceptionTips(4, null)[0].includes('房主'));
});

test('connection route says whether IPv4 or IPv6 is used', async () => {
  const { describeLink } = await multiplayerModule();
  assert.equal(describeLink({ ipVersion: 'IPv6', lan: false, protocol: 'tcp', latencyMs: 23.5, relayed: false }), 'IPv6 直连 · TCP · 延迟 24 ms');
  assert.equal(describeLink({ ipVersion: 'IPv4', lan: true, protocol: 'udp', latencyMs: null, relayed: false }), 'IPv4 局域网直连 · UDP');
  assert.equal(describeLink({ ipVersion: null, lan: false, protocol: '', latencyMs: 40, relayed: true }), '经其他成员转发 · 延迟 40 ms');
});
