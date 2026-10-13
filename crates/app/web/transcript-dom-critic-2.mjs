// Blind-critic checks on the DOM transcript (round 2): what the browser's own find, a drag-selection, the word
// panel on a tablet and a screen reader's tree do with it. FAILING BY DESIGN until the gaps are closed.
// Usage: node crates/app/web/transcript-dom-critic-2.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8794));
let browser;

async function open(viewport, query = 'demo=1&theme=light&tray=0') {
  const ctx = await browser.newContext({ viewport, bypassCSP: true });
  const page = await ctx.newPage();
  await page.goto(`http://127.0.0.1:${port}/?${query}`, { waitUntil: 'load' });
  for (let i = 0; i < 300 && !(await page.evaluate(() => window.__ready === true || !!window.__error)); i++) await page.waitForTimeout(100);
  const pump = async (n = 5) => { for (let i = 0; i < n; i++) { await page.mouse.move(viewport.width - 30 + (i % 3), viewport.height - 30 + i); await page.waitForTimeout(70); } };
  await pump(8);
  await page.waitForSelector('#controls p.tx', { timeout: 20000 });
  await pump(6);
  return { ctx, page, pump };
}
const top = (page, text) => page.evaluate((t) => {
  const p = [...document.querySelectorAll('#controls p.tx')].find((e) => e.textContent.includes(t));
  const r = p.getBoundingClientRect(); const c = document.querySelector('#controls .tx-clip').getBoundingClientRect();
  return { top: r.top, bottom: r.bottom, clipTop: c.top, clipBottom: c.bottom };
}, text);

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. The word panel is a sheet over the page on a phone or a tablet: no transcript text shows or takes the
  //    pointer through it (the transcript is DOM, laid over the canvas the panel is drawn on)
  for (const [w, h, fixed] of [[390, 844, 3], [768, 1024, 0]]) {
    const { ctx, page, pump } = await open({ width: w, height: h });
    for (let k = 0; k < fixed; k++) { await page.mouse.move(200, 500); await page.mouse.wheel(0, 350); await pump(4); await page.waitForTimeout(300); }
    for (let k = 0; k < (fixed ? 0 : 8); k++) {
      const seen = await page.evaluate(() => { const b = document.querySelector('#controls p.tx button.w'); const r = b.getBoundingClientRect(); const c = document.querySelector('#controls .tx-clip').getBoundingClientRect(); return r.top > c.top + 20 && r.bottom < c.bottom - 20; });
      if (seen) break;
      await page.mouse.move(w / 2, h / 2); await page.mouse.wheel(0, 300); await pump(3); await page.waitForTimeout(250);
    }
    const at = await page.evaluate(() => { const r = document.querySelector('#controls p.tx button.w').getBoundingClientRect(); return [r.x + r.width / 2, r.y + r.height / 2]; });
    await page.mouse.click(at[0], at[1]);
    await pump(4); await page.waitForTimeout(1200); await pump(3);
    const r = await page.evaluate(() => {
      const t = document.querySelector('#controls [data-id="tray-title"]');
      if (!t) return { none: true };
      const y0 = Math.max(0, t.getBoundingClientRect().top - 4);
      let leak = 0, n = 0;
      for (let x = 4; x < innerWidth - 4; x += 6) for (let y = y0; y < innerHeight - 2; y += 6) { n++; const e = document.elementFromPoint(x, y); if (e && e.closest && e.closest('p.tx')) leak++; }
      return { leak, n, y0 };
    });
    expect(!r.none && r.leak === 0, `${w}x${h}: transcript text does not show through the word panel: ` + JSON.stringify(r));
    await ctx.close();
  }

  // 2. Find-in-page on a line below the fold brings it into view (the transcript scrolls inside the canvas, the
  //    document cannot scroll, so the browser has nothing to scroll for the hit)
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 450 });
    const before = await top(page, 'likely wide');
    await page.evaluate(() => { getSelection().removeAllRanges(); window.find('likely wide'); });
    await pump(10); await page.waitForTimeout(800);
    const after = await top(page, 'likely wide');
    expect(before.top > before.clipBottom && after.bottom <= after.clipBottom && after.top >= after.clipTop,
      `a find hit below the fold is scrolled into view: ${Math.round(before.top)} -> ${Math.round(after.top)} (visible area ${Math.round(after.clipTop)}-${Math.round(after.clipBottom)})`);
    await ctx.close();
  }

  // 3. Dragging a selection past the bottom edge scrolls (a scrolling pane does, in the JS app)
  {
    const { ctx, page } = await open({ width: 1280, height: 450 });
    const p0 = await page.evaluate(() => { const r = document.querySelectorAll('#controls p.tx')[0].getBoundingClientRect(); return [r.left + 4, r.top + 10, r.top]; });
    await page.mouse.move(p0[0], p0[1]); await page.mouse.down();
    await page.mouse.move(900, 440, { steps: 5 });
    for (let i = 0; i < 20; i++) { await page.mouse.move(900, 449 - (i % 2)); await page.waitForTimeout(100); }
    await page.mouse.up();
    const t1 = await page.evaluate(() => document.querySelectorAll('#controls p.tx')[0].getBoundingClientRect().top);
    expect(t1 < p0[2] - 40, `a drag-selection held at the bottom edge scrolls the transcript: ${Math.round(p0[2])} -> ${Math.round(t1)}`);
    await ctx.close();
  }

  // 4. A formula is spoken once: its TeX text (kept for copy) is not in a screen reader's tree under the
  //    role=math element that already has the spoken name
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const cdp = await ctx.newCDPSession(page);
    const { nodes } = await cdp.send('Accessibility.getFullAXTree');
    const byId = new Map(nodes.map((n) => [n.nodeId, n]));
    const exposed = [];
    const walk = (n) => { for (const id of n.childIds || []) { const k = byId.get(id); if (!k) continue; if (!k.ignored && k.role?.value === 'StaticText') exposed.push(k.name?.value); walk(k); } };
    for (const n of nodes) if (!n.ignored && n.role?.value === 'math') walk(n);
    expect(exposed.length === 0, 'no text node is exposed inside role=math (name read, then the TeX read again): ' + JSON.stringify(exposed));
    await ctx.close();
  }

  // 5. Paragraph spacing from a user stylesheet (WCAG 1.4.12) makes room between lines, as it does on the JS
  //    app's block lines
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    await page.addStyleTag({ content: '#controls p.tx { margin-bottom: 24px !important; }' });
    await pump(10); await page.waitForTimeout(800); await pump(4);
    const gaps = await page.evaluate(() => { const ls = [...document.querySelectorAll('#controls p.tx')].map((p) => p.getBoundingClientRect()); return ls.slice(1).map((r, i) => Math.round(r.top - ls[i].bottom)); });
    expect(gaps.every((g) => g >= 16), 'margin-bottom of 24px on each line leaves at least 16px between lines: ' + JSON.stringify(gaps));
    await ctx.close();
  }
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  if (browser) await browser.close().catch(() => {});
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall DOM transcript critic round 2 checks passed');
process.exit(failures.length ? 1 : 0);
