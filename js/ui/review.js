/* Review: page image + the current student's page strip on the left, transcript and the
   words-to-check tray on the right. Bound to #doc-pane, #tx-pane and the tray host. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, R = OCR.render, W = OCR.words, G = OCR.groups, Q = OCR.queue;

  let scanHead, scanTitle, btnBoxes, btnRotate, btnLevel, btnReplace, btnRemove, strip, stage, img, overlay, scanEmpty;
  let who, txText, foot, trayHost;
  const ZOOMS = [1, 1.5, 2, 3, 4];
  let zoom = 1, zoomOutBtn, zoomInBtn, zoomLabel;
  let imgUrl = null, imgToken = 0, streamEl = null, dragId = null, wasActive = false;
  const thumbUrls = new Map();
  let sel = null; /* { id, key }: the word open in the tray */
  let trayOpen = true, trayGroup = null; /* the panel can be closed once the choices are made */

  const plural = (n, one, many) => n + ' ' + (n === 1 ? one : many);
  const group = () => (S.d.selectedId ? G.of(S.d.selectedId) : null);
  const effText = (p) => (S.d.textVersion === 'original' ? p.raw : W.effective(p));

  /* ================= left pane ================= */
  function initScan() {
    const pane = document.getElementById('doc-pane');
    btnBoxes = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Show line boxes', title: 'Show line boxes', 'aria-pressed': 'true', html: U.icon('boxes', 22), onclick: cycleBoxes });
    btnLevel = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Straighten this page and redo the line boxes', title: 'Straighten page and redo line boxes', html: U.icon('level', 22), onclick: levelPage });
    btnRotate = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Rotate page 90 degrees', title: 'Rotate 90 degrees', html: U.icon('rotate', 22), onclick: rotate });
    btnReplace = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Replace this page', title: 'Replace this page', html: U.icon('swap', 22), onclick: () => { const p = S.current(); if (p) U.upload.replace(p.id); } });
    btnRemove = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Remove this page', title: 'Remove this page', html: U.icon('trash', 22), onclick: () => removePage(S.d.selectedId) });
    zoomOutBtn = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Zoom out', title: 'Zoom out', html: U.icon('zoomOut', 22), onclick: () => stepZoom(-1) });
    zoomInBtn = h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Zoom in', title: 'Zoom in', html: U.icon('zoomIn', 22), onclick: () => stepZoom(1) });
    zoomLabel = h('button', { class: 'zoom-label', type: 'button', 'aria-label': 'Fit the page to the pane', title: 'Fit to pane', text: '100%', onclick: () => setZoom(1) });
    scanTitle = h('b');
    scanHead = h('div', { class: 'scan-head' }, scanTitle, h('div', { class: 'scan-tools' }, zoomOutBtn, zoomLabel, zoomInBtn, btnBoxes, btnLevel, btnRotate, btnReplace, btnRemove));
    strip = h('div', { class: 'scan-pages', role: 'group', 'aria-label': 'Pages' });
    img = h('img', { alt: 'The scanned handwritten page' });
    overlay = h('div', { class: 'overlay', 'aria-hidden': 'true' });
    stage = h('div', { class: 'scan-stage' }, h('div', { class: 'scan-sheet' }, img, overlay));
    scanEmpty = h('div', { class: 'scan-stage' }, h('p', { class: 'scan-empty', text: 'No page selected.' }));
    pane.append(scanHead, strip, stage, scanEmpty);
    strip.addEventListener('keydown', onStripKey);
    bindDrag();
    stage.addEventListener('wheel', (e) => { if (e.ctrlKey || e.metaKey) { e.preventDefault(); stepZoom(e.deltaY < 0 ? 1 : -1, e.clientX, e.clientY); } }, { passive: false });
    stage.addEventListener('dblclick', (e) => { setZoom(zoom === 1 ? 2 : 1, e.clientX, e.clientY); });
    stage.addEventListener('keydown', (e) => {
      if (e.key === '+' || e.key === '=') { e.preventDefault(); stepZoom(1); }
      else if (e.key === '-') { e.preventDefault(); stepZoom(-1); }
      else if (e.key === '0') { e.preventDefault(); setZoom(1); }
    });
    stage.tabIndex = 0;
    stage.setAttribute('aria-label', 'Page image. Press plus or minus to zoom, zero to fit.');
  }

  /* Zoom keeps the point under the pointer (or the middle of the view) where it was. */
  function setZoom(z, cx, cy) {
    z = Math.max(ZOOMS[0], Math.min(ZOOMS[ZOOMS.length - 1], z));
    if (z === zoom) return;
    const sheet = stage.firstElementChild, r = sheet.getBoundingClientRect(), s = stage.getBoundingClientRect();
    const px = cx == null ? s.left + s.width / 2 : cx, py = cy == null ? s.top + s.height / 2 : cy;
    const fx = r.width ? (px - r.left) / r.width : 0.5, fy = r.height ? (py - r.top) / r.height : 0;
    zoom = z;
    applyZoom();
    const n = sheet.getBoundingClientRect();
    stage.scrollLeft += n.left + fx * n.width - px;
    stage.scrollTop += n.top + fy * n.height - py;
  }
  function stepZoom(d, cx, cy) {
    const i = ZOOMS.findIndex((z) => z >= zoom - 0.001), next = ZOOMS[Math.max(0, Math.min(ZOOMS.length - 1, (i < 0 ? 0 : i) + d))];
    setZoom(next, cx, cy);
  }
  function applyZoom() {
    stage.firstElementChild.style.width = zoom * 100 + '%';
    stage.classList.toggle('is-zoomed', zoom > 1);
    zoomLabel.textContent = Math.round(zoom * 100) + '%';
    zoomOutBtn.disabled = zoom <= ZOOMS[0]; zoomInBtn.disabled = zoom >= ZOOMS[ZOOMS.length - 1];
  }

  async function removePage(id) {
    const p = S.page(id);
    if (!p) return;
    if (p.raw && !(await U.confirm('Remove “' + p.name + '”? Its transcript will be deleted too.', { danger: true, okLabel: 'Remove page', title: 'Remove page' }))) return;
    const g = G.of(id), sibling = g && g.pages.find((x) => x.id !== id);
    U.inboxThumbs.drop(id);
    const u = thumbUrls.get(id); if (u) { URL.revokeObjectURL(u); thumbUrls.delete(id); }
    S.removePage(id);
    if (sibling) S.select(sibling.id); else U.nav.inbox();
  }

  function thumbFor(btn, id) {
    const img2 = btn.querySelector('img');
    if (thumbUrls.has(id)) { img2.src = thumbUrls.get(id); return; }
    OCR.fs.loadImage(id).then((b) => b && OCR.image.thumb(b, 112)).then((u) => {
      if (!u) return;
      thumbUrls.set(id, u);
      if (img2.isConnected) img2.src = u;
    }).catch(() => {});
  }

  function dotClass(p) {
    if (p.status === 'error') return 'bad';
    if (p.status !== 'done') return '';
    return W.count(p) > 0 ? 'warn' : 'ok';
  }

  function renderStrip() {
    const g = group();
    strip.textContent = '';
    if (!g) return;
    g.pages.forEach((p, i) => {
      const on = p.id === S.d.selectedId, n = W.count(p);
      const b = h('button', { class: 'pthumb', type: 'button', draggable: 'true', dataset: { id: p.id }, 'aria-pressed': on ? 'true' : 'false', 'data-multi': S.d.multi.has(p.id) ? '1' : null,
        'aria-label': 'Page ' + (i + 1) + (p.status === 'error' ? ', could not transcribe' : p.status !== 'done' ? ', not transcribed yet' : n ? ', ' + plural(n, 'word', 'words') + ' to check' : ', all words checked'),
        onclick: (e) => {
          if (e.ctrlKey || e.metaKey || e.shiftKey) {
            const m = new Set(S.d.multi);
            if (e.shiftKey && S.d.selectedId) {
              const a = g.pages.findIndex((x) => x.id === S.d.selectedId), lo = Math.min(a, i), hi = Math.max(a, i);
              g.pages.slice(lo, hi + 1).forEach((x) => m.add(x.id));
            } else { if (!m.size && S.d.selectedId) m.add(S.d.selectedId); if (m.has(p.id)) m.delete(p.id); else m.add(p.id); }
            S.set({ multi: m }); renderStrip();
          } else S.select(p.id);
        } },
      h('img', { alt: '', draggable: 'false' }), h('span', { class: 'n', text: String(i + 1) }), h('span', { class: 'dot ' + dotClass(p) }));
      strip.append(b);
      thumbFor(b, p.id);
    });
    if (S.d.multi.size) {
      strip.append(h('button', { class: 'btn', type: 'button', text: 'Export ' + S.d.multi.size + ' selected', onclick: () => U.openExport({ scope: 'selected' }) }),
        h('button', { class: 'btn', type: 'button', text: 'Review again', onclick: () => U.reReview(Array.from(S.d.multi)) }),
        h('button', { class: 'btn ghost', type: 'button', text: 'Clear', onclick: () => { S.set({ multi: new Set() }); renderStrip(); } }));
    }
    strip.append(h('button', { class: 'btn ghost', type: 'button', 'aria-label': 'Add a page for ' + g.name, onclick: () => U.nav.add({ student: g.name }) },
      h('span', { html: U.icon('plus', 20) }), 'Add page'));
    const i = g.pages.findIndex((p) => p.id === S.d.selectedId);
    scanTitle.textContent = i >= 0 ? 'Page ' + (i + 1) + ' of ' + g.pages.length : '';
  }

  function onStripKey(e) {
    const b = e.target.closest('.pthumb');
    if (!b) return;
    const id = b.dataset.id, g = group(), i = g.pages.findIndex((p) => p.id === id);
    const focus = (pid) => { const el = strip.querySelector('[data-id="' + pid + '"]'); if (el) el.focus(); };
    if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      const t = g.pages[i + (e.key === 'ArrowLeft' ? -1 : 1)];
      if (t) { e.preventDefault(); S.movePage(S.indexOf(id), S.indexOf(t.id)); setTimeout(() => focus(id), 0); }
      return;
    }
    const go = (j) => { const t = g.pages[Math.max(0, Math.min(g.pages.length - 1, j))]; e.preventDefault(); S.select(t.id); setTimeout(() => focus(t.id), 0); };
    if (e.key === 'ArrowLeft') go(i - 1);
    else if (e.key === 'ArrowRight') go(i + 1);
    else if (e.key === 'Home') go(0);
    else if (e.key === 'End') go(g.pages.length - 1);
    else if (e.key === 'Delete') { e.preventDefault(); removePage(id); }
  }

  function bindDrag() {
    const clear = () => strip.querySelectorAll('.drop-before,.drop-after').forEach((x) => x.classList.remove('drop-before', 'drop-after'));
    const half = (e, el) => { const r = el.getBoundingClientRect(); return e.clientX < r.left + r.width / 2; };
    strip.addEventListener('dragstart', (e) => {
      const it = e.target.closest('.pthumb');
      if (!it) return;
      dragId = it.dataset.id;
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', dragId);
      it.classList.add('dragging');
    });
    strip.addEventListener('dragover', (e) => {
      if (!dragId) return;
      e.preventDefault(); clear();
      const it = e.target.closest('.pthumb');
      if (it && it.dataset.id !== dragId) it.classList.add(half(e, it) ? 'drop-before' : 'drop-after');
    });
    strip.addEventListener('drop', (e) => {
      if (!dragId) return;
      e.preventDefault();
      const it = e.target.closest('.pthumb');
      if (it && it.dataset.id !== dragId) {
        const from = S.indexOf(dragId);
        let to = S.indexOf(it.dataset.id) + (half(e, it) ? 0 : 1);
        if (from < to) to--;
        S.movePage(from, to);
      }
      dragId = null;
    });
    strip.addEventListener('dragend', () => { dragId = null; clear(); strip.querySelectorAll('.dragging').forEach((x) => x.classList.remove('dragging')); });
  }

  /* Line boxes: off, only the lines that hold a flagged word (default), or every line. We have line
     boxes, not word boxes, so a flagged word is shown by the line it sits on. */
  let boxAll = false;
  const readLines = new Map(); /* page id -> index of the transcript line being read */
  const readLine = () => (readLines.has(S.d.selectedId) ? readLines.get(S.d.selectedId) : -1);
  function setReadLine(i, opt) {
    if (i == null || i < 0) readLines.delete(S.d.selectedId); else readLines.set(S.d.selectedId, i);
    paintReading();
    if (!(opt && opt.quiet)) drawBoxes();
  }
  /* Underline the line in the transcript, and scroll it into view. */
  function paintReading() {
    const i = readLine();
    txText.querySelectorAll('.ln.reading').forEach((n) => n.classList.remove('reading'));
    if (i < 0) return;
    const ln = txText.querySelector('.ln[data-i="' + i + '"]');
    if (ln) { ln.classList.add('reading'); if (ln.scrollIntoView) ln.scrollIntoView({ block: 'nearest' }); }
  }
  function moveReadLine(d) {
    const n = txText.querySelectorAll('.ln').length;
    if (!n) return;
    const cur = readLine(), next = cur < 0 ? (d > 0 ? 0 : n - 1) : Math.max(0, Math.min(n - 1, cur + d));
    setReadLine(next);
  }
  function cycleBoxes() {
    if (!S.d.showBoxes) { boxAll = false; S.set({ showBoxes: true }); }
    else if (!boxAll) { boxAll = true; drawBoxes(); }
    else { boxAll = false; S.set({ showBoxes: false }); }
  }

  /* Boxes from a failed scan pile up in one spot. Real lines cover a decent stretch of the page. */
  const spreadOut = (boxes) => {
    if (boxes.length < 2) return boxes.length === 1;
    const top = Math.min.apply(null, boxes.map((b) => b.y)), bottom = Math.max.apply(null, boxes.map((b) => b.y + b.h));
    return bottom - top >= 0.12 || boxes.length < 3 && bottom - top >= 0.04;
  };
  const MATH_RE = /\$\$[\s\S]+?\$\$|\$[^$\n]+?\$/g, esc2 = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  /* Which line (transcript unit) each flagged word is on: { unitIndex: 'warn' | 'bad' | 'ok' }, plus the selected one. */
  function flaggedLines(p) {
    const units = R.units(p.raw || ''), kinds = {}, rank = { warn: 2, bad: 3, ok: 1 };
    let selUnit = -1;
    let seen = 0;
    const items = W.items(p);
    items.forEach((it) => {
      const kind = it.via ? 'ok' : it.kind === 'illegible' ? 'bad' : 'warn';
      const put = (i) => { if (!kinds[i] || rank[kind] > rank[kinds[i]]) kinds[i] = kind; if (sel && sel.id === p.id && sel.key === it.key) selUnit = i; };
      if (it.kind === 'illegible') {
        let n = 0, at = -1;
        units.some((u, i) => { const c = (u.replace(MATH_RE, '').match(/\[illegible\]/gi) || []).length; if (it.index < n + c) { at = i; return true; } n += c; return false; });
        if (at >= 0) put(at);
      } else {
        const re = new RegExp('\\b' + esc2(it.word) + '\\b');
        units.forEach((u, i) => { if (re.test(u.replace(MATH_RE, ''))) put(i); });
      }
      seen++;
    });
    return { kinds, selUnit, count: seen };
  }

  /* Where each transcript line sits on the page, as { x, y, w, h } fractions, for ANY page.
     Exact when the scan found as many lines as the transcript has; otherwise the lines are spread evenly
     over the area where writing was found, so a page always gets its underline (approx = true). */
  const geomCache = new Map();
  async function geometry(p) {
    const n = R.units(p.raw || '').length;
    if (!n || p.status !== 'done') return null;
    if (p.boxes && p.boxes.length === n && spreadOut(p.boxes)) return { lines: p.boxes, approx: false };
    const key = p.id + ':' + (p.rotation || 0) + ':' + n + ':' + (p.boxes ? p.boxes.length : 0);
    if (geomCache.has(key)) return geomCache.get(key);
    let bands = [];
    try { const b = await OCR.fs.loadImage(p.id); if (b) bands = await OCR.image.lineBands(b); } catch (e) { bands = []; }
    let out;
    if (bands.length === n && spreadOut(bands)) out = { lines: bands, approx: false };
    else if (bands.length > n && bands.length - n <= Math.max(2, Math.round(n * 0.35)) && spreadOut(bands)) out = { lines: R.mapBands(bands, n), approx: false }; /* drop the strays */
    else {
      const top = bands.length >= 2 ? Math.min.apply(null, bands.map((b) => b.y)) : 0.08;
      const bottom = bands.length >= 2 ? Math.max.apply(null, bands.map((b) => b.y + b.h)) : 0.92;
      const left = bands.length >= 2 ? Math.min.apply(null, bands.map((b) => b.x)) : 0.08;
      const right = bands.length >= 2 ? Math.max.apply(null, bands.map((b) => b.x + b.w)) : 0.92;
      const step = (bottom - top) / n;
      out = { lines: Array.from({ length: n }, (_, i) => ({ x: left, y: top + i * step + step * 0.1, w: right - left, h: step * 0.8, r: 0 })), approx: true };
    }
    geomCache.set(key, out);
    return out;
  }

  let drawToken = 0;
  const drawBoxes = async () => {
    const p = S.current(), tok = ++drawToken;
    const label = !S.d.showBoxes ? 'Marks on the page: off' : boxAll ? 'Marks on the page: underlines and a box on every line' : 'Marks on the page: underlines';
    btnBoxes.setAttribute('aria-pressed', S.d.showBoxes ? 'true' : 'false');
    btnBoxes.title = label + '. Click to change.'; btnBoxes.setAttribute('aria-label', label + '. Click to change.');
    const g = p ? await geometry(p) : null;
    if (tok !== drawToken) return; /* a newer draw superseded this one */
    overlay.textContent = '';
    if (!g) return;
    const put = (cls, b, below) => {
      const d = document.createElement('div');
      d.className = cls;
      d.style.cssText = 'left:' + b.x * 100 + '%;top:calc(' + (b.y + b.h) * 100 + '% + ' + below + 'px);width:' + b.w * 100 + '%';
      overlay.append(d);
      return d;
    };
    const ri = readLine();
    if (S.d.showBoxes) {
      const fl = flaggedLines(p);
      if (boxAll && !g.approx) {
        g.lines.forEach((b, i) => {
          const d = document.createElement('div');
          d.className = 'box'; d.dataset.i = i;
          d.style.cssText = 'left:' + b.x * 100 + '%;top:' + b.y * 100 + '%;width:' + b.w * 100 + '%;height:' + b.h * 100 + '%';
          overlay.append(d);
        });
      }
      Object.keys(fl.kinds).forEach((k) => {
        const i = +k, b = g.lines[i];
        if (!b) return;
        const d = put('mark mark-' + fl.kinds[k] + (fl.selUnit === i ? ' sel' : ''), b, i === ri ? 9 : 1);
        d.dataset.i = i; if (g.approx) d.dataset.approx = '1';
      });
    }
    if (ri >= 0 && g.lines[ri]) { const d = put('read-line', g.lines[ri], 1); d.dataset.i = ri; }
    const s = overlay.querySelector('.mark.sel, .read-line');
    if (s && s.scrollIntoView) s.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  };

  async function showImage() {
    const p = S.current(), tok = ++imgToken, has = !!p;
    stage.hidden = !has; scanEmpty.hidden = has; strip.hidden = !has; scanHead.hidden = !has;
    if (!has) { img.removeAttribute('src'); drawBoxes(); return; }
    const b = await OCR.fs.loadImage(p.id).catch(() => null);
    if (tok !== imgToken) return;
    const old = imgUrl;
    imgUrl = b ? URL.createObjectURL(b) : null;
    if (imgUrl) img.src = imgUrl; else img.removeAttribute('src');
    if (old) URL.revokeObjectURL(old);
    drawBoxes();
  }

  /* Straightens the stored image if it is tilted, then redraws the line boxes from what is there now.
     Also the way to refresh boxes on a page added before straightening existed. */
  async function levelPage() {
    const p = S.current();
    if (!p || p.status === 'transcribing') return;
    const blob = await OCR.fs.loadImage(p.id).catch(() => null);
    if (!blob) return;
    btnLevel.disabled = true;
    try {
      const r = await OCR.image.deskew(blob);
      let cur = blob;
      if (r.angle) { cur = r.blob; OCR.fs.saveImage(p.id, cur); }
      const boxes = p.status === 'done' && p.raw ? await R.computeBoxes(cur, p.raw) : [];
      S.updatePage(p.id, { boxes });
      if (r.angle) S.emit('image:changed', { id: p.id });
      OCR.toast(r.angle ? 'Straightened by ' + Math.abs(r.angle) + '°' + (boxes.length ? ' and redrew the line boxes.' : '.') : (boxes.length ? 'The page was already level. Line boxes redrawn.' : 'The page was already level.'));
    } catch (e) { OCR.toast('Could not straighten this page.', { kind: 'error' }); }
    finally { btnLevel.disabled = false; }
  }

  async function rotate() {
    const p = S.current();
    if (!p || p.status === 'transcribing') return;
    const b = await OCR.fs.loadImage(p.id);
    if (!b) return;
    const r = await OCR.image.rotate(b, 90);
    OCR.fs.saveImage(p.id, r.blob);
    S.updatePage(p.id, { rotation: ((p.rotation || 0) + 90) % 360, boxes: [] });
    S.emit('image:changed', { id: p.id });
  }

  /* ================= right pane ================= */
  function initText() {
    const pane = document.getElementById('tx-pane');
    who = h('div'); txText = h('div', { class: 'tx-body' }); foot = h('div', { class: 'tx-foot' });
    const inner = h('div', { class: 'tx-inner' }, who, txText, foot);
    trayHost = h('div', { class: 'tray-wrap' });
    pane.append(inner, trayHost);
    txText.tabIndex = 0;
    txText.setAttribute('aria-label', 'Transcript. Use the up and down arrow keys to move the underline to the line you are reading.');
    txText.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') { e.preventDefault(); moveReadLine(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); moveReadLine(-1); }
      else if (e.key === 'Escape' && readLine() >= 0) { e.stopPropagation(); setReadLine(-1); }
    });
    pane.addEventListener('click', (e) => {
      const ln = e.target.closest && e.target.closest('.tx-text .ln');
      if (ln && ln.dataset.i != null && !e.target.closest('button')) setReadLine(+ln.dataset.i);
      const w = e.target.closest && e.target.closest('.w');
      if (w && ln && ln.dataset.i != null) setReadLine(+ln.dataset.i);
      if (!w) return;
      sel = { id: S.d.selectedId, key: w.dataset.key };
      trayOpen = true;
      syncSel(); renderTray();
    });
    R.hoverSync(overlay, txText);
    pane.addEventListener('scroll', OCR.debounce(() => {
      const P = S.d.project;
      if (P && S.d.selectedId) { P.meta.ui = Object.assign({}, P.meta.ui, { scroll: pane.scrollTop, scrollId: S.d.selectedId }); OCR.fs.saveMeta(); }
    }, 600));
  }

  const pillFor = (g) => {
    if (g.status === 'failed') return ['bad', 'Couldn’t transcribe'];
    if (g.status === 'processing') return ['processing', 'Transcribing'];
    if (g.status === 'queued' || g.status === 'waiting') return ['queued', 'Waiting'];
    const n = g.pages.reduce((k, p) => k + W.open(p).length, 0);
    if (n) return ['warn', plural(n, 'word', 'words') + ' to check'];
    return ['ok', g.pages.some((p) => W.items(p).length) ? 'All words checked' : 'No words to check'];
  };

  function renderWho() {
    const g = group();
    who.textContent = '';
    if (!g) return;
    const [cls, label] = pillFor(g);
    const meta = plural(g.pages.length, 'page', 'pages') + (g.words ? ', ' + g.words + ' words' : '');
    who.append(h('div', { class: 'who-head' },
      h('div', null, h('h1', { text: g.name }), h('p', { text: meta + (g.source === 'file' ? '. No name was found on the page.' : '') }),
        h('button', { class: 'btn ghost', type: 'button', text: 'Change student', style: 'margin:4px 0 0 -16px', onclick: () => changeStudent(g) })),
      h('span', { class: 'pill ' + cls, role: 'status', text: label })),
    h('div', { class: 'legend-row' },
      h('div', { class: 'legend' },
        h('span', null, h('span', { class: 'k warn', text: 'word' }), 'Spelling to check'),
        h('span', null, h('span', { class: 'k bad', text: 'word' }), 'Couldn’t read'),
        h('span', null, h('span', { class: 'k ok', text: 'word' }), 'You checked it')),
      versionSeg()));
  }

  function versionSeg() {
    const p = S.current(), same = !p || !p.raw || W.effective(p) === p.raw; /* nothing corrected yet: both views read the same */
    const mk = (v, label) => h('button', { type: 'button', text: label, disabled: same && v === 'original' ? true : null, 'aria-pressed': S.d.textVersion === v ? 'true' : 'false', onclick: () => S.set({ textVersion: v }) });
    const seg = h('div', { class: 'seg', role: 'group', 'aria-label': 'Which text to show', title: same ? 'Nothing has been corrected on this page yet, so the original and corrected text are the same.' : null }, mk('corrected', 'Corrected'), mk('original', 'Original'));
    if (same && S.d.textVersion === 'original') S.d.textVersion = 'corrected';
    return seg;
  }

  function emptyState(title, text, btn) {
    return h('div', { class: 'state-pane' }, h('span', { html: U.icon('page', 40) }), h('h3', { text: title }), text ? h('p', { text }) : null, btn || null);
  }

  function makeWord(p) {
    const items = W.items(p), original = S.d.textVersion === 'original';
    return (key) => {
      const it = items.find((x) => x.key === key);
      const checked = it && it.via && !original;
      const kind = it ? it.kind : 'suggest';
      const text = it ? (original ? (kind === 'illegible' ? '[illegible]' : it.word) : it.shown) : key;
      return h('button', { type: 'button', class: 'w ' + (checked ? 'checked' : kind), dataset: { key },
        'aria-label': checked ? 'Checked word: ' + text + '. Show options.' : kind === 'illegible' ? 'Word that could not be read. Show options.' : 'Spelling to check: ' + text + '. Show options.' });
    };
  }

  function renderText() {
    const p = S.current(), g = group();
    streamEl = null;
    txText.textContent = '';
    foot.textContent = '';
    if (!p || !g) { txText.append(emptyState('No page selected', 'Add pages and the transcript appears here.')); return; }

    if (p.status === 'transcribing') {
      streamEl = h('div', { class: 'stream', text: p.raw });
      txText.append(streamEl);
      foot.append(h('span', { text: 'Reading this page. The text appears as it is read.' }));
    } else if (p.status === 'pending' && !p.raw) {
      txText.append(Q.isQueued(p.id)
        ? emptyState('Waiting for its turn', 'This page is in line and will be read soon.')
        : emptyState('This page has no transcript yet', 'Transcribe this page now, or run every page from the assignment inbox.',
          h('button', { class: 'btn primary', type: 'button', text: 'Transcribe this page', onclick: () => Q.start([p.id], { force: true }) })));
    } else {
      if (p.status === 'error') {
        txText.append(h('div', { class: 'errbox', role: 'alert' },
          h('div', null, h('span', { html: U.icon('warn', 22) }), h('p', { text: p.error || 'The page could not be transcribed.' })),
          h('button', { class: 'btn', type: 'button', text: 'Retry this page', onclick: () => Q.start([p.id], { force: true }) })));
      }
      const plain = S.d.textView === 'raw';
      if (p.raw) {
        const wrap = h('div', { class: 'tx-text' + (plain ? ' plain' : '') });
        if (plain) wrap.textContent = effText(p).replace(/[-]/g, '');
        else {
          R.renderFormatted(wrap, W.marked(p, S.d.textVersion));
          wrap.querySelectorAll('.ln.margin').forEach((ln) => {
            const t = document.createTreeWalker(ln, NodeFilter.SHOW_TEXT).nextNode();
            if (t) t.nodeValue = t.nodeValue.replace(/^\s*Margin:\s*/i, '');
          });
          W.wrap(wrap, makeWord(p));
        }
        txText.append(wrap);
        paintReading();
        const stats = p.timings && p.timings.tokens ? p.timings.tokens + ' tokens in ' + (p.timings.ms / 1000).toFixed(1) + 's' : '';
        if (stats) foot.append(h('span', { text: stats }));
        if (!plain && readLine() < 0) foot.append(h('span', { text: 'Click a line, or use the up and down arrow keys, to underline where you are.' }));
        if (!trayOpen && g.pages.some((q) => W.items(q).length)) foot.append(h('button', { class: 'btn', type: 'button', text: 'Check words', onclick: () => { trayOpen = true; renderTray(); renderText(); } }));
        foot.append(
          h('button', { class: 'btn ghost', type: 'button', text: plain ? 'Show formatted text' : 'Show plain text', onclick: () => S.set({ textView: plain ? 'formatted' : 'raw' }) }),
          h('button', { class: 'btn ghost', type: 'button', text: 'Transcribe this page again', disabled: S.d.run.active || null, onclick: () => again(p) }));
      }
    }
    syncSel();
  }

  async function again(p) {
    const touched = (p.edits && p.edits.length) || Object.keys(p.resolved || {}).length;
    if (touched && !(await U.confirm('Reading this page again replaces the transcript and the choices you made on it.', { okLabel: 'Transcribe again', title: 'Transcribe again' }))) return;
    Q.start([p.id], { force: true });
  }

  /* ================= student change ================= */
  function changeStudent(g) {
    const names = G.names().filter((n) => n !== g.name);
    const cur = S.current();
    const sel2 = h('select', { class: 'select', id: 'cs-sel' }, h('option', { value: '', text: 'Choose a student' }),
      names.map((n) => h('option', { value: n, text: n })), h('option', { value: '__new', text: 'A new student…' }));
    const input = h('input', { class: 'input', id: 'cs-new', type: 'text', placeholder: 'Student name', autocomplete: 'off' });
    const newField = h('div', { class: 'field', hidden: true }, h('label', { for: 'cs-new', text: 'New student name' }), input);
    sel2.addEventListener('change', () => { newField.hidden = sel2.value !== '__new'; if (!newField.hidden) input.focus(); });
    let scope = 'page';
    const scopeBox = g.pages.length > 1 ? h('div', { class: 'scope', role: 'radiogroup', 'aria-label': 'Which pages' },
      h('label', null, h('input', { type: 'radio', name: 'cs-scope', checked: true, onchange: () => { scope = 'page'; } }), h('span', { text: 'Only this page' })),
      h('label', null, h('input', { type: 'radio', name: 'cs-scope', onchange: () => { scope = 'all'; } }), h('span', { text: 'All ' + g.pages.length + ' pages for ' + g.name }))) : null;
    const m = U.modal({ title: 'Change student', narrow: true,
      body: [h('div', { class: 'field' }, h('label', { for: 'cs-sel', text: 'This page belongs to' }), sel2), newField, scopeBox],
      footer: [h('button', { class: 'btn', type: 'button', text: 'Cancel', onclick: () => m.close() }),
        h('button', { class: 'btn primary', type: 'button', text: 'Save', onclick: () => {
          const name = (sel2.value === '__new' ? input.value : sel2.value).trim();
          if (!name) { (sel2.value === '__new' ? input : sel2).focus(); return; }
          (scope === 'all' ? g.pages : [cur]).forEach((p) => S.updatePage(p.id, { student: { name, source: 'chosen', confirmed: true } }));
          m.close();
        } })] });
  }

  /* ================= tray ================= */
  const cropCache = new Map();
  const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const MATH = /\$\$[\s\S]+?\$\$|\$[^$\n]+?\$/g;

  /* Finds the line a flagged word sits on, then crops that box from the page image.
     Only when the ink scan found exactly as many line bands as the transcript has lines;
     otherwise the boxes were stretched to fit and a crop could land on the wrong line. */
  async function cropFor(p, item) {
    const units = R.units(p.raw);
    if (!units.length || !p.boxes || p.boxes.length !== units.length) return null;
    let idx = -1;
    if (item.kind === 'suggest') { const re = new RegExp('\\b' + esc(item.word) + '\\b'); idx = units.findIndex((u) => re.test(u.replace(MATH, ''))); }
    else {
      let n = 0;
      units.some((u, i) => { const c = (u.replace(MATH, '').match(/\[illegible\]/gi) || []).length; if (item.index < n + c) { idx = i; return true; } n += c; return false; });
    }
    if (idx < 0 || !p.boxes[idx]) return null;
    const blob = await OCR.fs.loadImage(p.id).catch(() => null);
    if (!blob) return null;
    const ck = p.id + ':' + units.length + ':' + (p.rotation || 0);
    if (!cropCache.has(ck)) { try { cropCache.set(ck, (await OCR.image.lineBands(blob)).length === units.length); } catch (e) { cropCache.set(ck, false); } }
    if (!cropCache.get(ck)) return null;
    const b = p.boxes[idx];
    if (b.h > 0.2 || b.w < 0.05) return null;
    const bmp = await createImageBitmap(blob);
    const padY = b.h * 0.25, x = Math.max(0, b.x - 0.01), y = Math.max(0, b.y - padY), w = Math.min(1 - x, b.w + 0.02), hh = Math.min(1 - y, b.h + padY * 2);
    const c = document.createElement('canvas');
    c.width = Math.max(1, Math.round(w * bmp.width)); c.height = Math.max(1, Math.round(hh * bmp.height));
    c.getContext('2d').drawImage(bmp, x * bmp.width, y * bmp.height, c.width, c.height, 0, 0, c.width, c.height);
    if (bmp.close) bmp.close();
    return c;
  }

  function allItems(g) {
    const out = [];
    g.pages.forEach((p) => W.items(p).forEach((it) => out.push({ id: p.id, item: it })));
    return out;
  }

  function syncSel() {
    txText.querySelectorAll('.w').forEach((b) => b.classList.toggle('sel', !!sel && sel.id === S.d.selectedId && b.dataset.key === sel.key));
    drawBoxes();
  }

  function advance(g, from) {
    const list = allItems(g), i = list.findIndex((x) => x.id === from.id && x.item.key === from.key);
    for (let k = 1; k <= list.length; k++) {
      const c = list[(i + k) % list.length];
      if (!c.item.via) return { id: c.id, key: c.item.key };
    }
    return from;
  }

  function renderTray() {
    const g = group();
    trayHost.textContent = '';
    if (!g) return;
    const list = allItems(g);
    if (!list.length) { sel = null; trayHost.hidden = true; return; }
    if (g.key !== trayGroup) { trayGroup = g.key; trayOpen = list.some((x) => !x.item.via); } /* a new student opens it only if words are waiting */
    trayHost.hidden = !trayOpen;
    if (!trayOpen) return;
    let idx = sel ? list.findIndex((x) => x.id === sel.id && x.item.key === sel.key) : -1;
    if (idx < 0) {
      const onPage = list.findIndex((x) => x.id === S.d.selectedId && !x.item.via);
      const firstOpen = list.findIndex((x) => !x.item.via);
      idx = onPage >= 0 ? onPage : firstOpen >= 0 ? firstOpen : list.findIndex((x) => x.id === S.d.selectedId);
      if (idx < 0) idx = 0;
      sel = { id: list[idx].id, key: list[idx].item.key };
      syncSel();
    }
    const cur = list[idx], it = cur.item, page = S.page(cur.id);
    const original = S.d.textVersion === 'original';
    const choose = (via, text) => {
      const next = advance(g, { id: cur.id, key: it.key });
      if (!W.choose(cur.id, it.key, via, text)) return;
      sel = next.id === cur.id && next.key === it.key ? sel : next;
      if (sel.id !== S.d.selectedId) S.select(sel.id);
    };
    const go = (d) => {
      const t = list[(idx + d + list.length) % list.length];
      sel = { id: t.id, key: t.item.key };
      if (t.id !== S.d.selectedId) S.select(t.id); else { syncSel(); renderTray(); }
    };

    const unresolvedSuggest = list.filter((x) => x.item.kind === 'suggest' && !x.item.via).length;
    const allDone = !list.some((x) => !x.item.via);
    const close = () => { trayOpen = false; renderTray(); renderText(); const w = txText.querySelector('.w.sel') || txText.querySelector('.w'); if (w) w.focus(); };
    const head = h('div', { class: 'tray-head' }, h('h2', { text: allDone ? 'All words checked' : 'Check this word' }),
      h('div', { class: 'tray-ctl' },
        unresolvedSuggest >= 2 ? h('button', { class: 'btn useall', type: 'button', text: 'Use all ' + unresolvedSuggest + ' suggestions', onclick: () => { W.useAll(g.pages.map((p) => p.id)); sel = null; renderTray(); } }) : null,
        h('div', { class: 'nav' },
          h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Previous word to check', html: U.icon('left', 22), onclick: () => go(-1) }),
          h('span', { class: 'count', text: (idx + 1) + ' of ' + list.length }),
          h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Next word to check', html: U.icon('right', 22), onclick: () => go(1) })),
        allDone ? h('button', { class: 'btn primary', type: 'button', text: 'Done', onclick: close })
          : h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Close the word panel', title: 'Close', html: U.icon('x', 22), onclick: close })));

    let note;
    if (original) note = 'Showing the original transcript. Switch to Corrected to see your choices.';
    else if (it.via === 'typed') note = 'You typed “' + it.shown + '”. Choose another option to change it.';
    else if (it.via) note = 'You chose “' + it.shown + '”. Choose another option to change it.';
    else if (it.kind === 'illegible') note = 'The model could not read this word. Pick one of its best guesses, type it yourself, or keep the marker.';
    else note = 'The model wrote “' + it.word + '”, which is close to a course word.';

    const opts = it.kind === 'suggest'
      ? [{ label: it.term, hint: 'Course word, ' + Math.round((it.conf || 0) * 100) + '% match', via: 'suggested' },
        { label: it.word, hint: 'Keep it as read from the page', via: 'written' }]
      : (((page.guesses || {})[it.key]) || []).map((g) => ({ label: g.text, hint: g.percent + '% likely', via: 'typed', text: g.text, on: it.via === 'typed' && it.shown === g.text }))
        .concat([{ label: '[illegible]', hint: 'Keep the marker so you can find it later', via: 'marked' }]);
    const alts = h('div', { class: 'alts' }, opts.map((o) => h('button', { class: 'alt', type: 'button', 'aria-pressed': (o.on != null ? o.on : it.via === o.via) ? 'true' : 'false', onclick: () => choose(o.via, o.text) },
      h('span', { class: 'radio' }), h('span', null, h('b', { text: o.label }), h('span', { text: o.hint })))));
    if (it.kind === 'illegible') {
      const has = ((page.guesses || {})[it.key] || []).length > 0;
      if (W.isGuessing(page.id)) alts.append(h('p', { class: 'hint', role: 'status', text: 'Looking at the handwriting for likely words…' }));
      else if (has) alts.append(h('p', { class: 'hint', text: 'The percentages are the model’s own estimate, not a measured probability.' }));
      else alts.append(h('button', { class: 'btn', type: 'button', text: page.guessed ? 'Look again for likely words' : 'Suggest likely words', onclick: () => { page.guessed = false; W.guess(page.id); } }));
    }
    const input = h('input', { class: 'input', id: 'typed-word', type: 'text', placeholder: 'Type what it says', 'aria-label': 'Type what the word says', autocomplete: 'off', spellcheck: 'false' });
    const use = () => { const v = input.value.trim(); if (v) choose('typed', v); else input.focus(); };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); use(); } });
    alts.append(h('div', { class: 'typed' }, input, h('button', { class: 'btn primary', type: 'button', text: 'Use this', onclick: use })));

    const crop = h('div', { class: 'crop', hidden: true }, h('div', { class: 'crop-img' }), h('div', { class: 'cap', text: 'This line, as the student wrote it' }));
    trayHost.append(h('div', { class: 'tray', onkeydown: (e) => { if (e.key === 'Escape') { e.stopPropagation(); close(); } } }, head, h('p', { class: 'tray-note', text: note }), h('div', { class: 'tray-body' }, crop, alts)));

    const token = idx + ':' + cur.id + ':' + it.key;
    crop.dataset.t = token;
    cropFor(page, it).then((c) => {
      if (c && crop.isConnected && crop.dataset.t === token) { crop.querySelector('.crop-img').append(c); crop.hidden = false; }
    }).catch(() => {});
  }

  /* ================= wiring ================= */
  function renderAll() { renderStrip(); showImage(); renderWho(); renderText(); renderTray(); }
  /* Batches redraws into one frame. Every distinct request runs: a boxes-only redraw must not swallow a full one. */
  const queued = new Set();
  const later = (fn) => {
    if (!queued.size) requestAnimationFrame(() => { const run = Array.from(queued); queued.clear(); run.forEach((f) => f()); });
    queued.add(fn);
  };

  U.review = {
    init() {
      initScan(); initText();
      const live = () => S.d.view === 'review';
      const all = () => { if (live()) renderAll(); };
      S.on('change:view', () => { if (live()) all(); });
      S.on('change:selectedId', () => { if (!live()) return; if (!sel || sel.id !== S.d.selectedId) sel = null; renderAll(); const pane = document.getElementById('tx-pane'); const ui = S.d.project && S.d.project.meta.ui; pane.scrollTop = ui && ui.scrollId === S.d.selectedId ? ui.scroll || 0 : 0; });
      S.on('change:project', () => { zoom = 1; applyZoom(); sel = null; cropCache.clear(); thumbUrls.forEach((u) => URL.revokeObjectURL(u)); thumbUrls.clear(); });
      S.on('pages:structure', () => { if (live()) later(renderAll); });
      S.on('change:textVersion', () => { if (live()) { renderWho(); renderText(); renderTray(); } });
      S.on('change:textView', () => { if (live()) renderText(); });
      S.on('change:showBoxes', drawBoxes);
      S.on('image:changed', (e) => {
        const u = thumbUrls.get(e.id); if (u) { URL.revokeObjectURL(u); thumbUrls.delete(e.id); }
        cropCache.clear();
        if (live() && e.id === S.d.selectedId) later(renderAll); else if (live()) later(renderStrip);
      });
      S.on('page:update', (e) => {
        if (!live()) return;
        const g = group();
        if (!g || !g.pages.some((p) => p.id === e.id)) return;
        const k = e.keys;
        const text = ['status', 'corrected', 'corrections', 'error', 'raw', 'resolved', 'edits', 'guesses', 'guessing'].some((x) => k.indexOf(x) >= 0);
        later(() => {
          if (text) { renderStrip(); renderWho(); if (e.id === S.d.selectedId) renderText(); renderTray(); }
          if (e.id === S.d.selectedId && (text || k.indexOf('boxes') >= 0)) drawBoxes();
          if (k.indexOf('student') >= 0) { renderStrip(); renderWho(); renderTray(); }
        });
      });
      S.on('page:stream', (e) => {
        if (!live() || e.id !== S.d.selectedId) return;
        if (streamEl && streamEl.isConnected) streamEl.append(document.createTextNode(e.chunk)); else renderText();
      });
      /* Only a run starting or ending changes what an untranscribed page says; progress ticks do not. */
      S.on('change:run', () => { const a = !!S.d.run.active; if (a === wasActive) return; wasActive = a; if (live() && S.current()) later(renderText); });
    }
  };
})();
