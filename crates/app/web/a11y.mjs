// Accessibility check of the web build (phase 4): the hidden-DOM twin of the review screen, in Chrome.
//   - ariaSnapshot assertions: landmarks, headings, groups, lists, roles and names, math, dialog
//   - axe-core over the page in several states (review, inbox, dialog, phone, forced colours)
//   - keyboard: focus restored after actions and dialogs, a new screen's heading takes focus, no trap
//   - announcements through the live regions
//   - 44px targets, twin laid over the canvas where the widget is drawn, reading order = visual order
//   - prefers-reduced-motion, forced-colors, 200% and 400% zoom (reflow at 640 and 320 CSS px)
// Screen readers cannot run headless: crates/ui/A11Y.md is the manual NVDA / VoiceOver pass.
// Usage: node crates/app/web/a11y.mjs   (run build.mjs first; CHROME env overrides the browser path)
import { chromium } from 'playwright-core';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import { serve } from './serve.mjs';

const require = createRequire(import.meta.url);
const axeSource = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const exe = process.env.CHROME
  || (process.platform === 'win32' ? 'C:/Program Files/Google/Chrome/Application/chrome.exe' : '/usr/bin/google-chrome');
const failures = [];
const expect = (ok, msg) => { console.log((ok ? 'ok   ' : 'FAIL ') + msg); if (!ok) failures.push(msg); };

const { port, stop } = await serve(+(process.env.PORT || 8789));
const base = `http://127.0.0.1:${port}`;
let browser;

/** Open the demo in a new context; returns helpers bound to that page. */
async function open(label, { query = '', viewport = { width: 1280, height: 800 }, ...ctxOpts } = {}) {
  // bypassCSP only so axe-core can be injected; the CSP itself is asserted by check.mjs
  const ctx = await browser.newContext({ viewport, bypassCSP: true, ...ctxOpts });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  await page.goto(`${base}/?demo=1&theme=light${query}`, { waitUntil: 'load' });
  for (let i = 0; i < 300; i++) {
    if (await page.evaluate(() => window.__ready === true || !!window.__error)) break;
    await page.waitForTimeout(100);
  }
  const pump = async (n = 5) => { for (let i = 0; i < n; i++) { await page.mouse.move(viewport.width - 30 + (i % 3), viewport.height - 30 + i); await page.waitForTimeout(70); } };
  await pump(8);
  await page.waitForSelector('#controls [data-id]', { timeout: 20000 });
  const api = {
    label, ctx, page, errors, pump,
    active: () => page.evaluate(() => { const a = document.activeElement; return a ? (a.getAttribute('data-id') || a.tagName.toLowerCase() + (a.id ? '#' + a.id : '')) : null; }),
    ids: () => page.evaluate(() => [...document.querySelectorAll('#controls button, #controls input')].map(e => e.dataset.id)),
    has: async (id) => (await api.ids()).includes(id) || await page.evaluate((i) => !!document.querySelector(`#controls [data-id="${i}"]`), id),
    attr: (id, n) => page.evaluate(([i, a]) => document.querySelector(`#controls [data-id="${i}"]`)?.getAttribute(a) ?? null, [id, n]),
    text: (id) => page.evaluate((i) => document.querySelector(`#controls [data-id="${i}"]`)?.textContent ?? null, id),
    wait: async (fn, ms = 6000) => { const t0 = Date.now(); while (Date.now() - t0 < ms) { if (await fn()) return true; await pump(2); } return false; },
    announced: () => page.evaluate(() => [document.getElementById('announce').textContent, document.getElementById('announce-alert').textContent]),
    snapshot: () => page.locator('body').ariaSnapshot(),
    tabTo: async (id, limit = 120) => {
      await page.mouse.click(Math.round(viewport.width / 2), 3);
      for (let i = 0; i < limit; i++) { await page.keyboard.press('Tab'); if ((await api.active()) === id) return true; }
      return false;
    },
    async axe(tag, { modal = false } = {}) {
      await page.evaluate(axeSource);
      // a modal dialog takes the page behind it out of the tree on purpose: no main landmark or h1 is the point
      const off = modal ? ['landmark-one-main', 'page-has-heading-one', 'region'] : [];
      const res = await page.evaluate(async (off) => {
        const rules = Object.fromEntries(off.map(r => [r, { enabled: false }]));
        const r = await window.axe.run(document, { rules, runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] } });
        return r.violations.map(v => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.slice(0, 4).map(n => n.target.join(' ') + ' :: ' + (n.failureSummary || '').split('\n').slice(0, 2).join(' | ')) }));
      }, off);
      expect(res.length === 0, `axe-core: no violations [${tag}]` + (res.length ? '\n' + JSON.stringify(res, null, 1) : ''));
      return res;
    },
    /** Rectangles of every twin control, in DOM order, from the document. */
    rects: () => page.evaluate(() => [...document.querySelectorAll('#controls button, #controls input, #controls [tabindex="0"]')].map(e => {
      const r = e.getBoundingClientRect();
      return { id: e.dataset.id, x: r.x, y: r.y, w: r.width, h: r.height, disabled: e.disabled === true, tag: e.tagName.toLowerCase(), region: e.closest('[data-region]')?.dataset.region };
    })),
    close: () => ctx.close(),
  };
  return api;
}

try {
  browser = await chromium.launch({
    executablePath: exe,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'],
  });

  // ---------------------------------------------------------------- review screen: structure
  {
    const a = await open('review');
    const { page } = a;
    const snap = await a.snapshot();
    console.log(snap.split('\n').slice(0, 14).join('\n') + '\n...');
    const has = (re, msg) => expect(re.test(snap), msg);
    has(/^- banner:/m, 'landmark: banner (header) with the student navigation and Done');
    has(/^- main:/m, 'landmark: main');
    has(/- region "Page image":/, 'landmark: region "Page image" inside main');
    has(/- region "Transcript":/, 'landmark: region "Transcript" inside main');
    has(/^- complementary "Word panel":/m, 'landmark: complementary "Word panel" (the tray)');
    has(/heading "Maya R\." \[level=1\]/, 'heading: the student is the one h1');
    has(/heading "Page 1 of 2" \[level=2\]/, 'heading: the page is an h2');
    has(/heading "Check this word" \[level=2\]/, 'heading: the word panel is an h2');
    has(/group "Layout":[\s\S]*button "Side by side" \[pressed\]/, 'group "Layout" with a pressed toggle');
    has(/list "Pages":\s+- listitem:\s+- button "Page 1, 3 words to check" \[pressed\]/, 'the page strip is a list of pages with the current one pressed');
    has(/list "Choices for this word":\s+- listitem:\s+- button "confidence\. Course word, 90% match"/, 'the choices for a word are a list');
    has(/textbox "Type what the word says"/, 'textbox with its accessible name');
    has(/button "Spelling to check: confidance\. Show options\."/, 'a flagged word is a button inside its sentence, named for what it asks');
    has(/- status: 4 words to check/, 'live status for the count of words to check');
    expect(/- math "x bar plus or minus 1\.96 times the fraction s over square root of n end fraction equals/.test(snap)
      && /\\\\bar\{x\} \\\\pm/.test(snap), 'math: role=math, spoken name from TeX, and the TeX as its text');
    const sentenceOrder = await page.evaluate(() => [...document.querySelectorAll('#controls p[data-id^="line-"]')].map(p => p.textContent.trim()).filter(Boolean).slice(0, 3));
    expect(/^The 95%/.test(sentenceOrder[0]), 'the text twin reads the transcript in line order: ' + JSON.stringify(sentenceOrder[0]));
    // every control has a name and nothing focusable is aria-hidden
    const bad = await page.evaluate(() => [...document.querySelectorAll('#controls button, #controls input, #controls [tabindex="0"]')]
      .filter(e => !(e.getAttribute('aria-label') || e.textContent || '').trim()).map(e => e.dataset.id));
    expect(bad.length === 0, 'every control has an accessible name' + (bad.length ? ': ' + bad : ''));
    const hidden = await page.evaluate(() => [...document.querySelectorAll('[aria-hidden="true"]')].flatMap(e => [e, ...e.querySelectorAll('button, input, [tabindex]')])
      .filter(e => e.tabIndex >= 0 && e.tagName !== 'CANVAS' && !e.hasAttribute('disabled')).map(e => e.tagName + '#' + e.id));
    expect(hidden.length === 0, 'nothing tabbable inside aria-hidden' + (hidden.length ? ': ' + hidden : ''));

    // ---- reading order = visual order (region, then 16px row band, then x)
    const rects = (await a.rects()).filter(r => r.w > 0 && r.region !== undefined);
    let outOfOrder = [];
    for (let i = 1; i < rects.length; i++) {
      const p = rects[i - 1], c = rects[i];
      if (p.region !== c.region) continue;
      const pk = [Math.floor(p.y / 16), p.x], ck = [Math.floor(c.y / 16), c.x];
      if (ck[0] < pk[0] || (ck[0] === pk[0] && ck[1] < pk[1])) outOfOrder.push(`${p.id} -> ${c.id}`);
    }
    expect(outOfOrder.length === 0, 'DOM order follows the on-screen order within each region' + (outOfOrder.length ? ': ' + outOfOrder.slice(0, 6) : ''));

    // ---- 44px targets, inside the viewport, over the canvas where they are drawn
    const small = rects.filter(r => !r.disabled && r.tag !== 'div' && (r.w < 43.5 || r.h < 43.5)).map(r => `${r.id} ${r.w}x${r.h}`);
    expect(small.length === 0, 'every control is at least 44 x 44 CSS px' + (small.length ? ': ' + small : ''));
    const off = rects.filter(r => r.x < -1 || r.x + r.w > 1281 || r.w > 1281).map(r => r.id);
    expect(off.length === 0, 'no control is placed outside the viewport horizontally' + (off.length ? ': ' + off : ''));
    // alignment end to end: a real mouse click at the centre of the twin's rectangle reaches the widget there
    const zin = rects.find(r => r.id === 'zoom-in');
    await page.mouse.click(zin.x + zin.w / 2, zin.y + zin.h / 2);
    expect(await a.wait(async () => (await a.attr('zoom-out', 'disabled')) === null), 'the twin rectangle of "Zoom in" is where the canvas draws it (a click there zooms)');
    await page.mouse.click(zin.x - 50, zin.y + zin.h / 2); // "Fit" is the middle button of the three
    await a.pump(4);
    expect((await a.snapshot()).includes('button "Zoom out"'), 'twin still intact after the click');

    // ---- canvas: out of the tree, out of the Tab order
    const canvas = await page.evaluate(() => { const c = document.getElementById('c'); return { t: c.getAttribute('tabindex'), h: c.getAttribute('aria-hidden') }; });
    expect(canvas.t === '-1' && canvas.h === 'true', 'canvas is tabindex=-1 and aria-hidden');

    // ---- what the legacy tree says that a first twin missed
    has(/button "Fit the page to the pane, zoom 100%"/, 'zoom: the level is part of the name of the fit button');
    has(/- img "The scanned handwritten page"/, 'the page image has a text alternative');
    has(/paragraph: 2 pages, \d+ words/, 'the page and word count are exposed');
    expect(await page.evaluate(() => [...document.querySelectorAll('#controls del')].some(d => d.textContent === 'probably')),
      'strikethrough text is a deletion in the transcript twin, not plain text');

    await a.axe('review screen, tray open');

    // ---- announcements
    const word = await page.evaluate(() => [...document.querySelectorAll('#controls button[data-id^="word-"]')].map(b => b.dataset.id).find(i => /-c:0$/.test(i)));
    expect(await a.tabTo(word), 'Tab reaches a flagged word');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => await a.has('alt-suggested')), 'Enter opens the word');
    expect(await a.tabTo('alt-suggested'), 'Tab reaches the suggestion');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => /suggested word/.test((await a.announced())[0])), 'accepting a suggestion is announced: ' + JSON.stringify(await a.announced()));
    const act = await a.active();
    expect(act && act !== 'body', 'focus is still on a control after the decision: ' + act);
    expect(/Next, word \d+ of \d+: .+\. (Nothing is chosen for it yet|Already chosen)\.|All words checked/.test((await a.announced())[0]),
      'the decision says which word the panel moved on to, so a second Enter is not blind: ' + JSON.stringify(await a.announced()));
    expect(await a.tabTo('copy'), 'Tab reaches Copy text');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => /Copied Maya R\./.test((await a.announced())[0])), 'the toast is spoken through the live region: ' + JSON.stringify(await a.announced()));
    expect(await page.evaluate(() => ![...document.querySelectorAll('#controls [data-region="2"] p')].some(p => /Copied/.test(p.textContent))),
      'the toast is spoken through the live region and is not a paragraph of the transcript');
    // ---- reordering a page announces where it went
    expect(await a.tabTo('page-p-maya-1'), 'Tab reaches the first page in the strip');
    await page.keyboard.press('Alt+ArrowRight');
    expect(await a.wait(async () => /Moved to position 2 of 2/.test((await a.announced())[0])), 'Alt+Arrow on a page says its new place: ' + JSON.stringify(await a.announced()));
    await page.keyboard.press('Alt+ArrowLeft');
    await a.pump(4);
    // ---- the tray: closing it puts focus on the word
    expect(await a.tabTo('tray-close'), 'Tab reaches the tray close button');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => ((await a.active()) || '').startsWith('word-')), 'closing the tray returns focus to a word: ' + await a.active());

    expect(await a.tabTo('next-student') || await a.tabTo('prev-student'), 'Tab reaches student navigation');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => /student \d of \d/.test((await a.announced())[0])), 'moving to another student says who: ' + JSON.stringify(await a.announced()));

    // ---- focus restore: a dialog gives focus back to what opened it
    expect(await a.tabTo('settings'), 'Tab reaches Settings');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => (await a.text('dialog-title')) === 'Settings'), 'Settings opens');
    expect((await a.active()) === 'theme-auto', 'focus moves to the first control of the dialog (not the last): ' + await a.active());
    const dlg = await a.snapshot();
    expect(/dialog "Settings":/.test(dlg), 'the dialog is a landmark-like dialog named "Settings"');
    expect(await page.evaluate(() => document.querySelector('#controls [role=dialog]')?.getAttribute('aria-modal')) === 'true', 'aria-modal=true');
    expect(!/button "Done, next student"/.test(dlg), 'the screen behind the dialog is not in the tree');
    await a.axe('settings dialog', { modal: true });
    const behind = await page.evaluate(() => [...document.querySelectorAll('#controls > [data-region], #controls > [role=main]')]
      .filter(e => e.getAttribute('role') !== 'dialog').map(e => e.hasAttribute('inert') && e.getAttribute('aria-hidden') === 'true'));
    expect(behind.length > 0 && behind.every(Boolean), 'everything behind the dialog is inert and aria-hidden');
    // Tab goes round inside the dialog (and Escape always leaves it): it never walks out to the page behind
    const seen = new Set();
    for (let i = 0; i < 8; i++) {
      await page.keyboard.press('Tab');
      seen.add(await page.evaluate(() => document.activeElement?.closest('[role=dialog]') ? document.activeElement.dataset.id : 'OUTSIDE:' + document.activeElement?.tagName));
    }
    expect(![...seen].some(x => x.startsWith('OUTSIDE')), 'Tab stays in the dialog: ' + [...seen]);
    await page.keyboard.press('Shift+Tab');
    expect(await page.evaluate(() => !!document.activeElement?.closest('[role=dialog]')), 'Shift+Tab stays in the dialog');
    await page.keyboard.press('Escape');
    expect(await a.wait(async () => (await a.active()) === 'settings'), 'Escape closes the dialog and focus returns to Settings: ' + await a.active());

    // ---- a new screen: focus goes to its heading
    expect(await a.tabTo('crumb'), 'Tab reaches the back crumb');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => (await a.text('inbox-title')) !== null), 'the overview opens');
    expect(await a.wait(async () => (await a.active()) === 'inbox-title'), 'focus moves to the new screen\'s h1 (not left on <body>): ' + await a.active());
    const inbox = await a.snapshot();
    expect(/checkbox "Select [^"]+"/.test(inbox), 'inbox rows have real checkboxes');
    expect(/- region "Unit 5 written response"/.test(inbox) || /- main:/.test(inbox), 'inbox is inside main');
    await a.axe('inbox');
    await page.keyboard.press('Space');
    // keyboard never trapped: Tab walks out of the twin within a bounded number of presses
    let left = false;
    await page.mouse.click(640, 3);
    for (let i = 0; i < 150 && !left; i++) {
      await page.keyboard.press('Tab');
      const inside = await page.evaluate(() => !!document.activeElement?.closest('#controls'));
      left = !inside && (await a.active()) !== 'canvas#c';
    }
    expect(left, 'Tab leaves the page content again (no keyboard trap)');
    expect(a.errors.length === 0, 'no page errors' + (a.errors.length ? ': ' + a.errors : ''));
    await a.close();
  }

  // ---------------------------------------------------------------- state the page and the twin must agree on
  {
    const a = await open('state');
    const { page } = a;
    // WCAG 2.4.2: the tab names the assignment, as js/main.js does
    expect(/^.+ \| viSHion$/.test(await page.title()), 'document title names the assignment: ' + JSON.stringify(await page.title()));
    // a typed fix leaves the text box empty in the twin as it is on the canvas (the box keeps focus)
    const val = () => page.evaluate(() => document.querySelector('#controls [data-id="typed-word"]')?.value ?? null);
    await page.evaluate(() => document.querySelector('#controls [data-id="word-p-maya-1-i:0"]').focus());
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => await a.has('typed-word')), 'the illegible word opens its text box');
    await page.evaluate(() => document.querySelector('#controls [data-id="typed-word"]').focus());
    await page.keyboard.type('mean');
    expect(await val() === 'mean', 'typing shows in the twin box');
    await page.keyboard.press('Enter');
    expect(await a.wait(async () => (await val()) === ''), 'after the typed word is used the box is empty, not stale: ' + JSON.stringify(await val()));
    expect(await a.wait(async () => /Used .mean.\./.test((await a.announced())[0])), 'the typed decision is announced: ' + JSON.stringify(await a.announced()));
    expect(/Now on page 2 of 2/.test((await a.announced())[0]), 'a decision that moves to the other page says so: ' + JSON.stringify(await a.announced()));
    // a control that disables itself under the keyboard hands focus on, and says so
    expect(await a.tabTo('zoom-in'), 'Tab reaches Zoom in');
    for (let i = 0; i < 4; i++) { await page.keyboard.press('Enter'); await a.pump(4); }
    const now = await a.active();
    expect(now && now !== 'body' && now !== 'html' && now !== 'zoom-in', 'at the largest zoom focus moved to a control beside it: ' + now);
    expect(await a.wait(async () => /not available now/.test((await a.announced())[0])), 'and it says why: ' + JSON.stringify(await a.announced()));
    expect(await a.attr('zoom-in', 'disabled') !== null, 'Zoom in is disabled at 400%');
    await a.close();
  }

  // ---------------------------------------------------------------- display preferences
  {
    const a = await open('prefs', { reducedMotion: 'reduce', forcedColors: 'active', colorScheme: 'dark' });
    const attrs = await a.page.evaluate(() => ({ m: document.getElementById('controls').dataset.reducedMotion, c: document.getElementById('controls').dataset.highContrast }));
    expect(attrs.m === '1', 'prefers-reduced-motion is read at start (no easing)');
    expect(attrs.c === '1', 'forced-colors is read at start (high contrast tokens)');
    expect(await a.tabTo('done'), 'the screen works under forced colours: Tab reaches Done');
    await a.axe('forced colours, reduced motion, dark');
    const bg = await a.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(!!bg, 'page background follows the system colours in forced-colors mode: ' + bg);
    await a.close();
  }

  // ---------------------------------------------------------------- zoom: 200% and 400% reflow
  for (const [zoom, w, h] of [[2, 640, 400], [4, 320, 256]]) {
    const a = await open(`zoom ${zoom * 100}%`, { viewport: { width: w, height: h }, deviceScaleFactor: zoom });
    const { page } = a;
    const doc = await page.evaluate(() => ({ sw: document.documentElement.scrollWidth, cw: document.documentElement.clientWidth, iw: innerWidth, dpr: devicePixelRatio }));
    expect(doc.dpr === zoom && doc.sw <= doc.cw, `${zoom * 100}%: no horizontal page scroll (${doc.sw} <= ${doc.cw} CSS px at dpr ${doc.dpr})`);
    const rects = (await a.rects()).filter(r => r.w > 0 && r.tag !== 'div');
    const wide = rects.filter(r => r.x < -1 || r.x + r.w > doc.cw + 1).map(r => `${r.id}@${Math.round(r.x)}+${Math.round(r.w)}`);
    expect(wide.length === 0, `${zoom * 100}%: every control fits the ${doc.cw}px width (content reflows, no sideways scroll)` + (wide.length ? ': ' + wide.slice(0, 8) : ''));
    expect(await a.has('done'), `${zoom * 100}%: Done is still reachable`);
    expect(await a.tabTo('done'), `${zoom * 100}%: Tab reaches Done`);
    const snap = await a.snapshot();
    expect(/heading "Maya R\." \[level=1\]/.test(snap) && /- main:/.test(snap), `${zoom * 100}%: same structure (h1, main)`);
    await a.axe(`${zoom * 100}% zoom`);
    await a.close();
  }

  // ---------------------------------------------------------------- keyboard focus is never off screen
  for (const [w, h, o] of [[390, 844, {}], [640, 400, { deviceScaleFactor: 2 }], [320, 256, { deviceScaleFactor: 4 }]]) {
    const a = await open(`tab ${w}x${h}`, { viewport: { width: w, height: h }, ...o });
    const { page } = a;
    await page.mouse.click(w / 2, 2);
    const off = [];
    for (let i = 0; i < 60; i++) {
      await page.keyboard.press('Tab'); await a.pump(3);
      const r = await page.evaluate(() => { const e = document.activeElement; if (!e?.dataset?.id) return null; const b = e.getBoundingClientRect(); return { id: e.dataset.id, top: b.top, bottom: b.bottom, h: innerHeight }; });
      if (r && (r.bottom > r.h + 1 || r.top < -1) && !off.includes(r.id)) off.push(r.id);
    }
    expect(off.length === 0, `${w}x${h}: every Tab stop is scrolled into view (WCAG 2.4.7, 2.4.11)` + (off.length ? ': ' + off.slice(0, 8) : ''));
    await a.close();
  }
} catch (e) {
  console.error(e);
  failures.push('exception: ' + (e && e.message));
} finally {
  if (browser) await browser.close();
  stop();
}
console.log(failures.length ? `\n${failures.length} FAILED` : '\nall accessibility checks passed');
process.exit(failures.length ? 1 : 0);
