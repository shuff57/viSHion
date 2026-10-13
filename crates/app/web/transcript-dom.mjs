// The transcript is real DOM text on the web (units, inline markdown, math as MathML, flagged words as inline
// buttons), laid over the canvas: the person's own CSS reaches it, as it does in the JS app (WCAG 1.4.12 text
// spacing, 1.4.4 resize, 1.4.10 reflow), and find-in-page and selection work on it.
// Usage: node crates/app/web/transcript-dom.mjs   (run build.mjs first)
import { chromium } from 'playwright-core';
import { serve } from './serve.mjs';

const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };
const { port, stop } = await serve(+(process.env.PORT || 8791));
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

// the lines as the browser lays them out, and the foot of the page (drawn by egui below the transcript)
const lines = (page) => page.evaluate(() => [...document.querySelectorAll('#controls p.tx[data-line]')].map((p) => {
  const r = p.getBoundingClientRect();
  return { id: p.dataset.id, top: r.top, bottom: r.bottom, left: r.left, width: r.width, height: r.height, fits: p.scrollHeight <= Math.ceil(r.height) + 1, text: p.textContent };
}));
const foot = (page) => page.evaluate(() => { const b = document.querySelector('#controls [data-id="text-view"]'); return b ? b.getBoundingClientRect().top : null; });

try {
  browser = await chromium.launch({ executablePath: exe, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] });

  // 1. The transcript is visible text in the page, not an invisible twin of a picture of text
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const vis = await page.evaluate(() => {
      const p = document.querySelector('#controls p.tx[data-line]');
      let o = 1, hidden = false;
      for (let e = p; e; e = e.parentElement) {
        const cs = getComputedStyle(e);
        o *= parseFloat(cs.opacity);
        if (cs.visibility === 'hidden' || cs.display === 'none') hidden = true;
      }
      const cs = getComputedStyle(p);
      return { o, hidden, font: cs.fontFamily, size: cs.fontSize, lh: cs.lineHeight, color: cs.color, text: p.innerText };
    });
    expect(vis.o === 1 && !vis.hidden, 'a transcript line is painted (opacity 1, visible): ' + JSON.stringify({ o: vis.o, hidden: vis.hidden }));
    expect(/Source Serif 4/.test(vis.font) && vis.size === '22px' && vis.lh === '37.4px', 'in the JS app\'s face and measure (22px / 1.7 serif): ' + [vis.font, vis.size, vis.lh]);
    expect(/^The 95% confidance interval/.test(vis.text), 'its text reads as a sentence with the flagged word in place: ' + JSON.stringify(vis.text));
    const math = await page.evaluate(() => ({ ml: document.querySelectorAll('#controls p.tx math').length, src: document.querySelectorAll('#controls p.tx .tx-math.src').length }));
    expect(math.ml >= 2 && math.src === 0, 'formulas are MathML in the line (KaTeX output): ' + JSON.stringify(math));
    const twin = await page.evaluate(() => document.querySelectorAll('#controls p[data-id^="line-"]').length);
    expect(twin === (await lines(page)).length, 'there is one element per line (no hidden twin of the transcript to read twice): ' + twin);
    const sel = await page.evaluate(() => { const p = document.querySelector('#controls p.tx[data-line]'); getSelection().selectAllChildren(p); return getSelection().toString(); });
    expect(/95% confidance interval/.test(sel), 'text can be selected: ' + JSON.stringify(sel.slice(0, 40)));
    await page.evaluate(() => getSelection().removeAllRanges());
    // find-in-page: the browser's own search, through window.find, lands in the transcript
    const found = await page.evaluate(() => { getSelection().removeAllRanges(); return window.find('commute time') && !!getSelection().anchorNode && !!getSelection().anchorNode.parentElement.closest('p.tx'); });
    expect(found, 'find-in-page finds words in the transcript');
    await pump(2);
    await ctx.close();
  }

  // 2. A user stylesheet with the WCAG 1.4.12 text spacing (and a larger size) reflows the transcript, and the
  //    page below it moves down with it
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const before = await lines(page);
    const footBefore = await foot(page);
    const wBefore = before[0].width;
    await page.addStyleTag({ content: '#controls p.tx { line-height: 2.2 !important; letter-spacing: 0.12em !important; word-spacing: 0.16em !important; font-size: 26px !important; }' });
    await pump(10);
    await page.waitForTimeout(600);
    await pump(4);
    const after = await lines(page);
    expect(after.length === before.length, 'same lines after the stylesheet: ' + after.length);
    const grew = after.filter((l, i) => l.height > before[i].height + 4).length;
    expect(grew >= after.length - 1, `lines are taller (${grew} of ${after.length}) and wrap onto more rows`);
    expect(after[0].width === wBefore, 'the column keeps its width (it reflows inside it)');
    const tall = (await page.evaluate(() => getComputedStyle(document.querySelector('#controls p.tx[data-line]')).lineHeight));
    expect(parseFloat(tall) >= 57, 'the user\'s line-height applies (2.2 x 26px): ' + tall);
    const clipped = after.filter((l) => !l.fits).map((l) => l.id);
    expect(clipped.length === 0, 'no line is clipped: ' + clipped);
    let overlap = 0;
    for (let i = 1; i < after.length; i++) if (after[i].top < after[i - 1].bottom - 1) overlap++;
    expect(overlap === 0, 'lines do not overlap after reflow: ' + overlap);
    const spans = after.every((l, i) => i === 0 || l.top - after[i - 1].bottom > -1 && l.top - after[i - 1].bottom < 16);
    expect(spans, 'the screen made room: each line starts where the one above ends (a margin note has 14px above it)');
    const footAfter = await foot(page);
    // the page below the transcript is drawn by egui: it follows the browser's layout
    expect(footBefore !== null && footAfter > footBefore + 30, `content after the transcript moved down: ${footBefore} -> ${footAfter}`);
    // a smaller size comes back to the original layout
    await page.evaluate(() => { for (const s of document.querySelectorAll('style')) if (/line-height: 2\.2/.test(s.textContent)) s.remove(); });
    await pump(8);
    await page.waitForTimeout(500);
    await pump(3);
    const back = await lines(page);
    expect(back.every((l, i) => Math.abs(l.height - before[i].height) < 2), 'removing the stylesheet restores the layout');
    await ctx.close();
  }

  // 3. Minimum font size and zoom: the same text honours the browser's own settings
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const before = await lines(page);
    await page.addStyleTag({ content: '#controls p.tx { font-size: 34px !important; }' });
    await pump(10);
    await page.waitForTimeout(600);
    const after = await lines(page);
    expect(after[0].height > before[0].height * 1.2 && after.every((l) => l.fits), 'a larger font size reflows without clipping: ' + [before[0].height, after[0].height]);
    await ctx.close();
  }

  // 4. A word in the DOM transcript still selects through the control layer; the wheel over the text scrolls the page
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 800 });
    const w = await page.evaluate(() => { const b = document.querySelector('#controls p.tx button.w'); const r = b.getBoundingClientRect(); return { id: b.dataset.id, x: r.x + r.width / 2, y: r.y + r.height / 2, text: b.textContent }; });
    expect(w.text === 'confidance', 'a flagged word is an inline button with its word as text: ' + w.text);
    const trayBefore = await page.evaluate(() => !!document.querySelector('#controls [data-id="tray-count"]'));
    await page.mouse.click(w.x, w.y);
    await pump(6);
    await page.waitForTimeout(300);
    const tray = await page.evaluate(() => ({ tray: !!document.querySelector('#controls [data-id="tray-count"]'), sel: document.querySelector('#controls button.w.sel')?.dataset.id }));
    expect(!trayBefore && tray.tray && tray.sel === w.id, 'a click on the word opens the word panel with that word selected: ' + JSON.stringify(tray));
    await ctx.close();
  }
  {
    const { ctx, page, pump } = await open({ width: 1280, height: 600 });
    const last = async () => (await lines(page)).at(-1).top;
    const t0 = await last();
    const p0 = await page.evaluate(() => { const r = document.querySelector('#controls p.tx[data-line]').getBoundingClientRect(); return [r.x + 80, r.y + 10]; });
    await page.mouse.move(p0[0], p0[1]);
    await page.mouse.wheel(0, 240);
    await pump(6);
    await page.waitForTimeout(400);
    const t1 = await last();
    expect(t1 < t0 - 50, `the wheel over the text scrolls the page: ${t0} -> ${t1}`);
    // a click on a line (not on a word) underlines it, as in the JS app
    const ln = await page.evaluate(() => { const p = document.querySelectorAll('#controls p.tx[data-line]')[3]; const r = p.getBoundingClientRect(); return { id: p.dataset.id, x: r.x + 40, y: r.y + 8 }; });
    await page.mouse.click(ln.x, ln.y);
    await pump(6);
    await page.waitForTimeout(300);
    const reading = await page.evaluate(() => document.querySelector('#controls p.tx[data-reading]')?.dataset.id || null);
    expect(reading === ln.id, 'a click on a line underlines it: ' + reading + ' / ' + ln.id);
    await ctx.close();
  }
} catch (e) {
  failures.push(String(e.stack || e));
  console.error(e);
} finally {
  if (browser) await browser.close().catch(() => {});
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall DOM transcript checks passed');
process.exit(failures.length ? 1 : 0);
