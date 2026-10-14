/* Top header. One bar for every screen; its contents follow S.d.view.
   Also owns navigation between views (U.nav), so the leave-while-running check lives in one place. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, G = OCR.groups;
  let host, cur = null, raf = 0;

  const TITLE = 'viSHion';
  const ibtn = (label, ic, fn) => h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': label, title: label, html: U.icon(ic, 22), onclick: fn });
  const sbtn = () => ibtn('Settings', 'gear', () => U.openSettings());

  /* ---------- navigation ---------- */
  U.nav = {
    async home() {
      if (S.d.view === 'home') return;
      if (OCR.queue.isRunning() && !(await U.confirm('Transcription is running. Stop it and go back to your assignments? The page in progress stays untranscribed.', { okLabel: 'Stop and leave', title: 'Leave assignment' }))) return;
      if (U.add && U.add.reset) U.add.reset();
      OCR.fs.closeProject();
    },
    inbox() { if (U.add && U.add.reset) U.add.reset(); S.set({ view: 'inbox' }); },
    review(pageId) { if (pageId) S.select(pageId); S.set({ view: 'review' }); },
    add(opt) { if (U.add && U.add.reset) U.add.reset(opt); S.set({ view: 'add' }); }
  };

  /* Animated wordmark. The scan-in plays once per page load; hover replays it (CSS). */
  let introPlayed = false;
  const logo = () => {
    const intro = !introPlayed;
    introPlayed = true;
    const el = h('span', {
      html: '<svg class="logo' + (intro ? ' intro' : '') + '" viewBox="24 18 312 84" aria-hidden="true">'
        + '<text class="lg-hand" x="30" y="82" textLength="300" lengthAdjust="spacingAndGlyphs" transform="rotate(-3 180 60)">' + TITLE + '</text>'
        + '<text class="lg-clean" x="30" y="82" textLength="300" lengthAdjust="spacingAndGlyphs">vi<tspan>SH</tspan>ion</text>'
        + '<rect class="lg-bar" x="28" y="22" width="4" height="76" rx="2"/></svg>'
    });
    /* Drop .intro once it has played; otherwise hover reuses the same animation name and never restarts it. */
    if (intro) el.addEventListener('animationend', () => el.firstChild.classList.remove('intro'), { once: true });
    return el;
  };
  const brandLink = () => h('button', { class: 'brand', type: 'button', 'aria-label': TITLE, onclick: () => U.nav.home() }, logo());
  const back = (label, fn) => h('button', { class: 'crumb', type: 'button', onclick: fn }, h('span', { html: U.icon('left', 20) }), h('span', { text: label }));

  function saveState() {
    const el = h('span', { class: 'save-state', role: 'status' });
    el.update = () => {
      const st = S.d.save.status, idb = S.d.folder.mode === 'idb';
      el.dataset.s = st;
      el.hidden = st === 'idle' && !S.d.project;
      el.innerHTML = st === 'saving' ? U.icon('spin', 18) + '<span>Saving…</span>'
        : st === 'error' ? U.icon('warn', 18) + '<span>Couldn’t save</span>'
          : U.icon('check', 18) + '<span>' + (idb ? 'Saved in this browser' : 'Saved to your folder') + '</span>';
    };
    return el;
  }

  /* ---------- builders: each returns { nodes, update } ---------- */
  function forHome() {
    return { nodes: [h('div', { class: 'brand', 'aria-label': TITLE }, logo()), h('div', { class: 'topbar-right' }, sbtn())], update() {} };
  }
  function forInbox() {
    const save = saveState();
    return { nodes: [brandLink(), h('div', { class: 'topbar-right' }, save, sbtn())], update() { save.update(); } };
  }
  function forAdd() {
    const crumb = back('', () => U.nav.inbox());
    return { nodes: [brandLink(), crumb], update() { crumb.lastChild.textContent = S.d.project ? S.d.project.meta.name : ''; } };
  }

  function currentGroup(list) { return S.d.selectedId ? G.of(S.d.selectedId, list) : null; }
  const asList = (g) => g.pages.map((p, i) => Object.assign({}, p, { order: i }));

  function copyStudent() {
    const g = currentGroup();
    if (!g) return;
    const s = S.d.settings, E = OCR.export;
    const text = E.combined(asList(g), { mode: S.d.textVersion === 'original' ? 'raw' : 'corrected', sep: s.exportSeparator, fmt: 'txt' });
    E.copyText(text).then((ok) => OCR.toast(ok ? 'Copied ' + g.name + '’s text.' : 'Could not copy. Try an export instead.', { kind: ok ? 'info' : 'error' }));
  }

  function doneNext() {
    const list = G.list(), g = currentGroup(list);
    if (!g) return;
    g.pages.forEach((p) => { if (!p.reviewed) S.updatePage(p.id, { reviewed: true }); });
    const i = list.findIndex((x) => x.key === g.key), n = list[i + 1];
    if (n) S.select(n.pages[0].id);
    else { OCR.toast('That was the last student.'); U.nav.inbox(); }
  }

  function forReview() {
    const save = saveState();
    const crumb = back('', () => U.nav.inbox());
    const prev = ibtn('Previous student', 'left', () => step(-1)), next = ibtn('Next student', 'right', () => step(1));
    const nameEl = h('b'), posEl = h('span');
    const split = h('button', { type: 'button', text: 'Side by side', onclick: () => setLayout('split') });
    const text = h('button', { type: 'button', text: 'Text only', onclick: () => setLayout('text') });
    const doneBtn = h('button', { class: 'btn primary', type: 'button', text: 'Done, next student', onclick: doneNext });
    const copyBtn = h('button', { class: 'btn', type: 'button', text: 'Copy text', onclick: copyStudent });
    function step(d) {
      const list = G.list(), g = currentGroup(list);
      if (!g) return;
      const t = list[list.findIndex((x) => x.key === g.key) + d];
      if (t) S.select(t.pages[0].id);
    }
    function setLayout(v) {
      S.set({ layout: v });
      const m = S.d.project.meta;
      m.ui = Object.assign({}, m.ui, { layout: v });
      OCR.fs.saveMeta();
    }
    return {
      nodes: [brandLink(), crumb,
        h('div', { class: 'student-nav' }, prev, h('div', { class: 'who' }, nameEl, posEl), next),
        h('div', { class: 'topbar-right' }, save,
          h('div', { class: 'seg', role: 'group', 'aria-label': 'Layout' }, split, text), copyBtn, doneBtn, sbtn())],
      update() {
        save.update();
        crumb.lastChild.textContent = S.d.project ? S.d.project.meta.name : '';
        const list = G.list(), g = currentGroup(list), i = g ? list.findIndex((x) => x.key === g.key) : -1;
        nameEl.textContent = g ? g.name : 'No student';
        posEl.textContent = g ? (i + 1) + ' of ' + list.length : '';
        prev.disabled = i <= 0; next.disabled = i < 0 || i >= list.length - 1;
        split.setAttribute('aria-pressed', S.d.layout !== 'text' ? 'true' : 'false');
        text.setAttribute('aria-pressed', S.d.layout === 'text' ? 'true' : 'false');
        doneBtn.disabled = !g; copyBtn.disabled = !g || !g.pages.some((p) => p.raw);
      }
    };
  }

  const BUILD = { home: forHome, inbox: forInbox, add: forAdd, review: forReview };

  function rebuild() {
    const v = S.d.view;
    cur = (BUILD[v] || forHome)();
    cur.view = v;
    host.textContent = '';
    cur.nodes.forEach((n) => host.append(n));
    cur.update();
  }
  const schedule = () => { if (!raf) raf = requestAnimationFrame(() => { raf = 0; if (cur) cur.update(); }); };

  U.header = {
    init() {
      host = document.getElementById('topbar');
      S.on('change:view', rebuild);
      S.on('change:project', () => { if (cur) cur.update(); });
      ['change', 'page:update', 'pages:structure', 'stats', 'meta:update'].forEach((e) => S.on(e, schedule));
      rebuild();
    }
  };
})();
