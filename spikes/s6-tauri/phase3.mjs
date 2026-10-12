// Phase 3: real OS-level drag and drop (OLE file drag from a WinForms window + real mouse events),
// default dragDropEnabled=true vs disabled (S6_NO_DND=1 -> disable_drag_drop_handler()).
import fs from 'node:fs';
import path from 'node:path';
import { launch, sleep, save, OUT, ps, psAsync } from './lib.mjs';

const R = {};
const file = path.join(OUT, 'dropme.png');
fs.writeFileSync(file, Buffer.from('89504e470d0a1a0a', 'hex'));
const folder = path.join(OUT, 'dropdir'); fs.mkdirSync(folder, { recursive: true });
fs.writeFileSync(path.join(folder, 'inner.txt'), 'x');

async function scenario(name, env) {
  const S = {};
  const app = await launch(env);
  const { page } = app;
  await sleep(500);
  const geo = async sel => page.evaluate(s => { const r = document.querySelector(s).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; }, sel);
  const dpr = await page.evaluate(() => devicePixelRatio);
  const [ox, oy] = ps('win.ps1', 'pos', 'viSHion S6').split(',').map(Number);
  const toScreen = p => [Math.round(ox + p.x * dpr), Math.round(oy + p.y * dpr)];
  S.dpr = dpr; S.clientOrigin = [ox, oy];
  const mark = () => page.evaluate(() => s6.LOG.length);
  const since = async m => (await page.evaluate(m => s6.LOG.slice(m), m)).map(l => JSON.parse(l)).filter(e => e.tag !== 'ready');

  // a) OS file drop on #zone
  let m = await mark();
  const [zx, zy] = toScreen(await geo('#zone'));
  S.fileDropDriver = await psAsync('filedrag.ps1', file, zx, zy);
  await sleep(500);
  S.fileDropEvents = await since(m);

  // b) OS folder drop
  m = await mark();
  S.folderDropDriver = await psAsync('filedrag.ps1', folder, zx, zy);
  await sleep(500);
  S.folderDropEvents = await since(m);

  // c) in-page HTML5 reorder with real mouse: A -> C
  m = await mark();
  const [ax, ay] = toScreen(await geo('#item-a')); const [cx, cy] = toScreen(await geo('#item-c'));
  await psAsync('win.ps1', 'drag', ax, ay, cx, cy);
  await sleep(500);
  S.reorderEvents = await since(m);
  S.orderAfter = await page.evaluate(() => [...document.querySelectorAll('#list li')].map(l => l.textContent));
  await app.close();
  R[name] = S;
}
await scenario('dragDropEnabled_true_default', {});
await scenario('dragDropEnabled_false', { S6_NO_DND: '1' });
save('phase3.json', R);
console.log(JSON.stringify(R, null, 1));
