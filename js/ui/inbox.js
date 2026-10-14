/* Inbox: one row per student, a run bar, a segmented status bar and filter tabs. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, Q = OCR.queue, G = OCR.groups;
  let root, filter = 'all', raf = 0;
  const thumbs = new Map(); /* page id -> object URL; one at a time so a big stack never decodes at once */
  let chain = Promise.resolve();

  U.inboxThumbs = { drop(id) { const u = thumbs.get(id); if (u) URL.revokeObjectURL(u); thumbs.delete(id); } };

  function loadThumb(img, id) {
    if (thumbs.has(id)) { img.src = thumbs.get(id); return; }
    chain = chain.then(async () => {
      if (!img.isConnected) return;
      try {
        const b = await OCR.fs.loadImage(id);
        if (!b) return;
        const u = await OCR.image.thumb(b, 104);
        thumbs.set(id, u);
        if (img.isConnected) img.src = u;
      } catch (e) { /* the ruled-paper placeholder stays */ }
    }, () => {});
  }

  const plural = (n, one, many) => n + ' ' + (n === 1 ? one : many);

  function chip(g) {
    if (g.status === 'ready') return ['ready', 'Ready to read'];
    if (g.status === 'check') return ['check', plural(g.open, 'word to check', 'words to check')];
    if (g.status === 'processing') return ['processing', 'Transcribing'];
    if (g.status === 'queued' || g.status === 'waiting') return ['queued', 'Waiting'];
    return ['failed', 'Couldn’t transcribe'];
  }

  const picked = (g) => g.pages.every((p) => S.d.multi.has(p.id));
  function toggle(g, on) {
    const m = new Set(S.d.multi);
    g.pages.forEach((p) => { if (on) m.add(p.id); else m.delete(p.id); });
    S.set({ multi: m });
  }

  /* Puts the chosen pages back to "needs a check": decisions cleared, suggestions un-applied. */
  U.reReview = async (ids) => {
    const pages = ids.map((id) => S.page(id)).filter((p) => p && p.status === 'done');
    if (!pages.length) { OCR.toast('Only pages with a transcript can be reviewed again.', { kind: 'warn' }); return; }
    const touched = pages.some((p) => (p.edits && p.edits.length) || Object.keys(p.resolved || {}).length || p.reviewed);
    if (touched && !(await U.confirm('Review ' + plural(pages.length, 'page', 'pages') + ' again? Your choices on ' + (pages.length === 1 ? 'it are' : 'them are') + ' cleared and the words come back to check. The transcripts stay.', { okLabel: 'Review again', title: 'Review again' }))) return;
    pages.forEach((p) => {
      (p.corrections || []).forEach((c, i) => { if (c.applied) OCR.judge.toggle(p.id, i, false); });
      S.updatePage(p.id, { resolved: {}, edits: [], reviewed: false });
    });
    const first = G.list().find((g) => g.pages.some((p) => ids.indexOf(p.id) >= 0));
    S.set({ multi: new Set() });
    if (first) U.nav.review(first.pages[0].id);
  };

  function selectionBar(list) {
    const n = S.d.multi.size;
    if (!n) return null;
    const names = list.filter((g) => g.pages.some((p) => S.d.multi.has(p.id))).length;
    return h('div', { class: 'runbar', role: 'status' },
      h('div', { class: 'txt' }, h('b', { text: plural(names, 'student', 'students') + ' selected' }), h('div', { class: 'sub', text: plural(n, 'page', 'pages') })),
      h('div', { class: 'btns' },
        h('button', { class: 'btn', type: 'button', text: 'Clear selection', onclick: () => S.set({ multi: new Set() }) }),
        h('button', { class: 'btn', type: 'button', text: 'Review again', onclick: () => U.reReview(Array.from(S.d.multi)) }),
        h('button', { class: 'btn primary', type: 'button', text: 'Export selected', onclick: () => U.openExport({ scope: 'selected' }) })));
  }

  function row(g) {
    const [cls, label] = chip(g), idle = g.status === 'processing' || g.status === 'queued' || g.status === 'waiting' || g.status === 'failed';
    const meta = idle ? plural(g.pages.length, 'page', 'pages') : plural(g.pages.length, 'page', 'pages') + ', ' + g.words + ' words';
    const img = h('img', { alt: '' });
    const first = g.pages[0];
    loadThumb(img, first.id);
    const open = h('button', { class: 'srow-open', type: 'button', 'aria-label': 'Open ' + g.name + ', ' + label, onclick: () => U.nav.review(first.id), dataset: { k: 'row:' + g.key } },
      h('span', { class: 'sthumb' + (idle && g.status !== 'failed' ? ' dim' : ''), 'aria-hidden': 'true' }, img),
      h('span', { class: 'who' }, h('b', { text: g.name }), h('span', { text: meta })),
      h('span', { class: 'pill ' + cls, text: label }),
      h('span', { html: U.icon('right', 20) }));
    const box = h('input', { type: 'checkbox', checked: picked(g), 'aria-label': 'Select ' + g.name, dataset: { k: 'sel:' + g.key }, onchange: (e) => toggle(g, e.target.checked) });
    const r = h('div', { class: 'srow' }, h('label', { class: 'srow-check' }, box), open);
    if (g.status === 'failed') {
      r.append(h('button', { class: 'btn retry', type: 'button', 'aria-label': 'Retry ' + g.name, text: 'Retry', disabled: S.d.run.active || null, dataset: { k: 'retry:' + g.key },
        onclick: () => Q.start(g.failed, { force: true }) }));
    }
    return r;
  }

  function runBar(list, tally) {
    const run = S.d.run, c = S.counts();
    if (!c.total) return null;
    const box = (children) => h('div', { class: 'runbar', role: 'status' }, children);
    const failedBtn = c.error ? h('button', { class: 'btn', type: 'button', text: 'Retry ' + c.error + ' failed', disabled: run.active || null, onclick: () => Q.retryFailed() }) : null;
    if (run.active) {
      const cur = run.currentId && S.page(run.currentId), g = cur && G.of(cur.id, list);
      const left = Math.max(0, run.total - run.done);
      const pct = run.total ? Math.min(100, Math.round((100 * run.done) / run.total)) : 0;
      return box([
        h('div', { class: 'txt' },
          h('b', { text: g ? 'Reading ' + g.name + '’s page, ' + Math.min(run.total, run.done + 1) + ' of ' + run.total : 'Working, ' + run.done + ' of ' + run.total }),
          h('div', { class: 'sub', text: plural(left, 'page', 'pages') + ' left' + (run.etaMs > 0 ? ', about ' + OCR.fmtDur(run.etaMs) : '') + '' }),
          h('div', { class: 'meter', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': String(pct) }, h('i', { style: 'width:' + pct + '%' }))),
        h('div', { class: 'btns' }, failedBtn, h('button', { class: 'btn', type: 'button', text: 'Stop', onclick: () => Q.stop() }))]);
    }
    if (c.pending > 0) {
      const stopped = S.d.run.paused ? 'Paused. Fix the server, then retry.' : c.done > 0 ? 'Stopped. ' + plural(c.pending, 'page', 'pages') + ' left.' : plural(c.pending, 'page needs', 'pages need') + ' a transcript.';
      return box([
        h('div', { class: 'txt' }, h('b', { text: stopped }),
          h('div', { class: 'sub', text: c.done + ' of ' + c.total + ' pages have a transcript. The rest will pick up where they left off.' })),
        h('div', { class: 'btns' }, failedBtn,
          h('button', { class: 'btn primary', type: 'button', text: c.done > 0 ? 'Resume (' + c.pending + ' left)' : 'Transcribe ' + plural(c.pending, 'page', 'pages'), onclick: () => U.resumeAll() }))]);
    }
    if (c.error) {
      return box([h('div', { class: 'txt' }, h('b', { text: plural(c.error, 'page', 'pages') + ' couldn’t be transcribed.' }),
        h('div', { class: 'sub', text: 'Retry them, or open a student to see what went wrong.' })), h('div', { class: 'btns' }, failedBtn)]);
    }
    if (tally.check > 0) {
      const first = list.find((g) => g.status === 'check');
      return box([h('div', { class: 'txt' }, h('b', { text: 'All ' + plural(c.total, 'page is', 'pages are') + ' transcribed.' }),
        h('div', { class: 'sub', text: plural(tally.check, 'student has', 'students have') + ' words to check before you read them.' })),
      h('button', { class: 'btn primary', type: 'button', text: 'Start with the first one', onclick: () => U.nav.review(first.pages[0].id) })]);
    }
    return null;
  }

  function render() {
    if (S.d.view !== 'inbox' || !S.d.project) return;
    const P = S.d.project, list = G.list(), t = G.tally(list);
    const ae = document.activeElement, fk = ae && root.contains(ae) && ae.dataset ? ae.dataset.k : null;
    root.textContent = '';
    const inner = h('div', { class: 'page-inner' });
    inner.append(h('div', { class: 'inbox-head' },
      h('div', null, h('h1', { class: 'page-title', text: P.meta.name }),
        h('p', { class: 'page-sub', text: (P.meta.topic ? P.meta.topic + ', ' : '') + plural(t.total, 'student', 'students') })),
      h('div', { class: 'inbox-actions' },
        h('button', { class: 'btn ghost', type: 'button', text: 'Assignment settings', onclick: () => U.openAssignmentSettings() }),
        h('button', { class: 'btn', type: 'button', text: 'Export transcripts', onclick: () => U.openExport() }),
        h('button', { class: 'btn primary', type: 'button', text: 'Add pages', onclick: () => U.nav.add() }))));

    if (!list.length) {
      inner.append(h('div', { class: 'empty', style: 'margin-top:28px' }, h('span', { html: U.icon('upload', 40) }),
        h('h3', { text: 'No pages in this assignment' }), h('p', { text: 'Drop photos or scans here, or choose files. PNG, JPG and WebP work.' }),
        h('button', { class: 'btn primary', type: 'button', text: 'Add pages', onclick: () => U.nav.add() })));
      root.append(inner);
      return;
    }

    const bar = runBar(list, t);
    if (bar) inner.append(bar);
    const sb = selectionBar(list);
    if (sb) { sb.style.marginTop = '16px'; inner.append(sb); }

    const seg = (n, cls) => h('i', { class: cls, style: 'width:' + (n / t.total) * 100 + '%' });
    inner.append(h('div', { class: 'stackbar', role: 'img',
      'aria-label': t.ready + ' ready to read, ' + t.check + ' need a check, ' + t.failed + ' failed, ' + t.processing + ' not done yet' },
    seg(t.ready, 'ok'), seg(t.check, 'warn'), seg(t.failed, 'bad'), seg(t.processing, '')));

    const tab = (key, label, n, dot) => h('button', { class: 'tab', type: 'button', 'aria-pressed': filter === key ? 'true' : 'false', dataset: { k: 'tab:' + key }, onclick: () => { filter = key; render(); } },
      dot ? h('span', { class: 'dot ' + dot }) : null, h('span', { text: label }), h('span', { class: 'n', text: String(n) }));
    inner.append(h('div', { class: 'tabs', role: 'group', 'aria-label': 'Filter students' },
      tab('all', 'All', t.total), tab('ready', 'Ready to read', t.ready, 'ok'), tab('check', 'Needs a check', t.check, 'warn'),
      tab('processing', 'Transcribing', t.processing, ''), tab('failed', 'Failed', t.failed, 'bad')));

    const shown = list.filter((g) => G.matches(g, filter));
    if (shown.length) inner.append(h('div', { class: 'srows' }, shown.map(row)));
    else inner.append(h('div', { class: 'empty', style: 'margin-top:20px' }, h('span', { html: U.icon('check', 40) }),
      h('h3', { text: 'Nothing left to check' }), h('p', { text: 'Every student in this filter has been read. Pick another filter to see the rest.' }),
      h('button', { class: 'btn', type: 'button', text: 'Show all students', onclick: () => { filter = 'all'; render(); } })));
    root.append(inner);
    if (fk) { const el = root.querySelector('[data-k="' + fk.replace(/"/g, '') + '"]'); if (el && !el.disabled) el.focus(); }
  }

  /* Rows are rebuilt, so renders are spaced out: a run reports progress many times a second. */
  const schedule = () => { if (!raf) raf = setTimeout(() => { raf = 0; render(); }, 200); };

  U.inbox = {
    init() {
      root = document.getElementById('inbox');
      ['pages:structure', 'page:update', 'stats', 'change'].forEach((e) => S.on(e, schedule));
      S.on('change:view', () => { if (S.d.view === 'inbox') render(); });
      S.on('change:project', () => { filter = 'all'; thumbs.forEach((u) => URL.revokeObjectURL(u)); thumbs.clear(); });
      S.on('image:changed', (e) => { U.inboxThumbs.drop(e.id); schedule(); });
    }
  };
})();
