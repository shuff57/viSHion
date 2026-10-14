// Blind-critic checks on the DOM transcript (round 4): keyboard scrolling from the transcript, hover link to the
// page's line boxes, the rest of the pane being real text, math structure for assistive technology, label in name.
// (Closed by the DOM transcript round 4 fixes; kept as a regression check.)
// Usage: node crates/app/web/transcript-dom-critic-4.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8796));
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

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. With focus on the transcript, the keys that scroll a pane scroll it (js: #tx-pane is the scroller of the
  //    focused .tx-body); here PageDown, Space, End and Home do nothing at all
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 500 });
    const top = () => page.evaluate(() => Math.round(document.querySelector('#controls p.tx').getBoundingClientRect().top));
    await page.evaluate(() => document.querySelector('#controls [data-id="transcript"]').focus());
    await pump(3); await page.waitForTimeout(200);
    const t0 = await top();
    await page.keyboard.press('PageDown'); await pump(5); await page.waitForTimeout(500);
    const t1 = await top();
    expect(t1 < t0 - 60, `PageDown with the transcript focused scrolls it: ${t0} -> ${t1}`);
    await page.keyboard.press('End'); await pump(5); await page.waitForTimeout(500);
    const t2 = await top();
    expect(t2 < t1 - 20, `End with the transcript focused scrolls to the foot: ${t1} -> ${t2}`);
    await page.keyboard.press('Home'); await pump(5); await page.waitForTimeout(500);
    const t3 = await top();
    expect(t3 > t2 + 20, `Home with the transcript focused scrolls back to the top: ${t2} -> ${t3}`);
    await ctx.close();
  }

  // 2. Hovering a transcript line lights its box on the page (js R.hoverSync: `.box.on { border-color: accent }`)
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const clip = { x: 0, y: 240, width: 640, height: 420 };
    await page.mouse.move(1250, 780); await page.waitForTimeout(400);
    const a = await page.screenshot({ clip });
    const at = await page.evaluate(() => { const r = document.querySelectorAll('#controls p.tx')[4].getBoundingClientRect(); return [r.left + 120, r.top + 10]; });
    for (let i = 0; i < 8; i++) { await page.mouse.move(at[0] + i, at[1]); await page.waitForTimeout(80); }
    await page.waitForTimeout(300);
    const b = await page.screenshot({ clip });
    expect(Buffer.compare(a, b) !== 0, 'hovering a transcript line changes its box on the page');
    await ctx.close();
  }

  // 3. The rest of the transcript pane is text too: the student's name, the count line, the token stat and the
  //    hint are painted by the canvas with an invisible copy in the DOM, so a user stylesheet cannot reach them
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const r = await page.evaluate(() => ['student-name', 'student-meta', 'foot-stats', 'foot-hint', 'legend'].map((id) => {
      const e = document.querySelector(`#controls [data-id="${id}"]`);
      if (!e) return [id, 'missing'];
      let o = 1; for (let n = e; n && n.id !== 'controls'; n = n.parentElement) o *= parseFloat(getComputedStyle(n).opacity);
      return [id, o];
    }));
    const hidden = r.filter(([, o]) => o === 0).map(([id]) => id);
    expect(hidden.length === 0, 'text beside the transcript is visible DOM text (user CSS reaches it): invisible copies ' + JSON.stringify(hidden));
    await ctx.close();
  }

  // 4. A formula reaches assistive technology as MathML (structure to walk, Nemeth/UEB braille), not only as
  //    an English sentence in a label with the typeset MathML hidden
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const cdp = await ctx.newCDPSession(page);
    const { nodes } = await cdp.send('Accessibility.getFullAXTree');
    const byId = new Map(nodes.map((n) => [n.nodeId, n]));
    const roles = [];
    const walk = (n) => { for (const id of n.childIds || []) { const k = byId.get(id); if (!k) continue; if (!k.ignored) roles.push(k.role?.value); walk(k); } };
    for (const n of nodes) if (!n.ignored && n.role?.value === 'math') walk(n);
    expect(roles.some((r) => /^MathMLFraction$|^MathMLSquareRoot$/.test(r)), 'a formula exposes its structure (fraction, root) in the accessibility tree: ' + JSON.stringify([...new Set(roles)]));
    await ctx.close();
  }

  // 5. WCAG 2.5.3 Label in Name: voice control says "click illegible"; the name must hold the word that is shown
  {
    const { ctx, page } = await open({ width: 1280, height: 800 });
    const r = await page.evaluate(() => [...document.querySelectorAll('#controls p.tx button.w')].map((b) => [b.textContent.trim(), b.getAttribute('aria-label')]));
    const bad = r.filter(([t, n]) => !n.toLowerCase().includes(t.replace(/[\[\]]/g, '').toLowerCase()));
    expect(bad.length === 0, 'every flagged word\'s name contains the text it shows: ' + JSON.stringify(bad));
    await ctx.close();
  }
} finally {
  await browser?.close();
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall passed');
process.exit(failures.length ? 1 : 0);
