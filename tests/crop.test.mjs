import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, writeFile, unlink } from 'node:fs/promises';
import ts from 'typescript';

async function geometry() {
  const source = await readFile(new URL('../src/lib/crop.ts', import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext } }).outputText;
  const file = new URL(`./.crop-${crypto.randomUUID()}.mjs`, import.meta.url);
  await writeFile(file, code);
  try { return await import(file.href); } finally { await unlink(file); }
}
test('draw in either direction selects the same normalized area and clamps to the image', async () => {
  const { drawCrop } = await geometry();
  assert.deepEqual(drawCrop(.2, .25, .8, .75), drawCrop(.8, .75, .2, .25));
  assert.deepEqual(drawCrop(0, 0, 2, 2), { x: 0, y: 0, width: 1, height: 1 });
  const click = drawCrop(1, 1, 1, 1);
  assert.ok(click.width > 0 && click.height > 0 && click.x + click.width <= 1);
});
test('moving the crop preserves its size and stays inside the image', async () => {
  const { moveCrop } = await geometry();
  const rect = { x: .1, y: .2, width: .4, height: .3 };
  assert.deepEqual(moveCrop(rect, -5, 5), { ...rect, x: 0, y: .7 });
});
test('corner resizing preserves the opposite corner and cannot invert or escape', async () => {
  const { resizeCrop } = await geometry();
  const rect = { x: .25, y: .25, width: .5, height: .5 };
  const resized = resizeCrop(rect, 'nw', -.5, -.5);
  assert.deepEqual(resized, { x: 0, y: 0, width: .75, height: .75 });
  for (const corner of ['nw','ne','sw','se']) {
    const next = resizeCrop(rect, corner, 5, -5);
    assert.ok(next.width > 0 && next.height > 0 && next.x >= 0 && next.y >= 0);
    assert.ok(next.x + next.width <= 1 && next.y + next.height <= 1);
  }
});
