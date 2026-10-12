/* Store + events. Single source of truth for the UI. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, L = {};
  const KEY = 'ocr_settings_v1';

  /* Written for small models: short rules, and what NOT to add. The default prompt has no editor
     (each assignment can set its own), so a saved copy is always replaced by the current one. */
  const DEFAULT_PROMPT = [
    'Transcribe the handwriting on this page. Output only the words written on the page, exactly as written, one line of handwriting per line of output.',
    'Rules:',
    '- Copy spelling, abbreviations, numbers, symbols and punctuation exactly. Do not fix spelling or grammar. Do not reword.',
    '- Keep the line breaks, and keep bullets and dashes as written. Write arrows and other symbols as the characters you see (for example →), never as LaTeX.',
    '- Do not add a title, summary, introduction, explanation, heading or code fence. Do not describe the page or the handwriting.',
    '- Do not add any word that is not on the page. Do not repeat a line.',
    '- If a word is truly unreadable, write [illegible]. Do not invent a word to fill the gap.',
    '- Write ~~word~~ for struck-through text.',
    '- Put a margin note on its own line starting with "Margin:".',
    '- Use LaTeX inside $...$ or $$...$$ only for mathematical formulas.',
    'Start with the first line of handwriting and stop after the last.'].join('\n');

  const DEFAULT_TOPICS = {
    General: { description: 'Everyday handwriting with no special vocabulary.', terms: [] },
    Medical: { description: 'Clinical notes, charts and medication names.', terms: ['hypertension', 'tachycardia', 'bradycardia', 'dyspnea', 'edema', 'acetaminophen', 'ibuprofen', 'metformin', 'lisinopril', 'atorvastatin', 'auscultation', 'prn', 'bid', 'tid', 'qid'] },
    'Math/Education': { description: 'Lesson plans, student work and mathematics.', terms: ['derivative', 'integral', 'quadratic', 'polynomial', 'coefficient', 'theorem', 'hypotenuse', 'logarithm', 'asymptote', 'regression', 'hypothesis', 'rubric', 'formative', 'summative', 'scaffold', 'differentiation'] }
  };

  const DEFAULTS = {
    theme: 'auto', ocrUrl: 'http://localhost:8188', judgeUrl: 'http://localhost:8189', judgeMode: 'auto',
    apiKey: '', demo: false, storage: 'auto', keepOriginals: false, timeoutSec: 180,
    exportFormat: 'md', exportSeparator: 'label', exportText: 'corrected',
    topics: DEFAULT_TOPICS, defaultPrompt: DEFAULT_PROMPT, noticeSeen: false, readNames: true, straighten: true, provider: 'local', model: '', profiles: {}, idleUnloadMin: 10, guessIllegible: true, rememberFixes: true, reference: []
  };
  const RUN0 = { active: false, paused: false, total: 0, done: 0, currentId: null, etaMs: 0, message: '' };

  const emit = (evt, payload) => (L[evt] || []).slice().forEach((fn) => {
    try { fn(payload); } catch (e) { console.error('[state] ' + evt, e); }
  });

  Object.assign(S, {
    DEFAULT_PROMPT, DEFAULT_TOPICS, RUN0, emit,
    d: {
      view: 'home', folder: { mode: 'none', name: '', permission: 'none' }, projects: [], project: null,
      selectedId: null, multi: new Set(), settings: {}, save: { status: 'idle', pending: 0 },
      run: Object.assign({}, RUN0), status: '', drawerOpen: true, textView: 'formatted',
      textVersion: 'corrected', showBoxes: true, banner: null, serverError: null, layout: 'split'
    }
  });

  S.on = (evt, fn) => { (L[evt] = L[evt] || []).push(fn); return () => { L[evt] = L[evt].filter((f) => f !== fn); }; };
  S.set = (patch) => {
    Object.assign(S.d, patch);
    const keys = Object.keys(patch);
    keys.forEach((k) => emit('change:' + k, S.d[k]));
    emit('change', keys);
  };

  /* ---- settings (localStorage holds the API key; it never goes to disk or exports) ---- */
  const applyTheme = () => { document.documentElement.dataset.theme = S.d.settings.theme || 'auto'; };
  S.loadSettings = () => {
    let s = {};
    try { s = JSON.parse(localStorage.getItem(KEY) || '{}'); } catch (e) { /* ignore */ }
    S.d.settings = Object.assign({}, DEFAULTS, s);
    S.d.settings.defaultPrompt = DEFAULT_PROMPT;
    applyTheme();
  };
  S.saveSettings = (patch) => {
    Object.assign(S.d.settings, patch || {});
    try { localStorage.setItem(KEY, JSON.stringify(S.d.settings)); } catch (e) { /* private mode */ }
    applyTheme();
    emit('settings', patch);
    if (OCR.fs.saveSettings) OCR.fs.saveSettings();
  };

  /* ---- project helpers ---- */
  S.pages = () => { const P = S.d.project; return P ? P.meta.order.map((id) => P.pages[id]).filter(Boolean) : []; };
  S.page = (id) => (S.d.project ? S.d.project.pages[id] || null : null);
  S.current = () => S.page(S.d.selectedId);
  S.indexOf = (id) => (S.d.project ? S.d.project.meta.order.indexOf(id) : -1);
  S.counts = () => {
    const c = { total: 0, done: 0, error: 0, pending: 0, transcribing: 0 };
    S.pages().forEach((p) => { c.total++; c[p.status] = (c[p.status] || 0) + 1; });
    return c;
  };
  S.newPage = (name) => ({
    id: OCR.uid(), name, order: 0, status: 'pending', raw: '', corrected: '', corrections: [],
    boxes: [], timings: { ms: 0, tokens: 0 }, error: '', rotation: 0,
    student: null, reviewed: false, resolved: {}, edits: [], guesses: {} /* optional on old projects; see OCR.words */
  });
  S.reindex = () => S.pages().forEach((p, i) => { p.order = i; });

  S.select = (id, opt) => {
    if (!S.page(id)) return;
    if (!(opt && opt.keepMulti) && S.d.multi.size) S.d.multi = new Set();
    S.set({ selectedId: id });
    const m = S.d.project.meta;
    m.ui = Object.assign({}, m.ui, { selectedId: id });
    OCR.fs.saveMeta();
  };

  S.updatePage = (id, patch, opt) => {
    const p = S.page(id);
    if (!p) return;
    const statusChanged = 'status' in patch && patch.status !== p.status;
    Object.assign(p, patch);
    emit('page:update', { id, keys: Object.keys(patch) });
    if (!opt || opt.save !== false) OCR.fs.savePage(id);
    if (statusChanged) S.syncStats();
  };

  S.updateMeta = (patch) => {
    if (!S.d.project) return;
    Object.assign(S.d.project.meta, patch);
    emit('meta:update', patch);
    OCR.fs.saveMeta();
  };

  S.syncStats = () => {
    const P = S.d.project;
    if (!P) return;
    const c = S.counts();
    P.meta.stats = { pages: c.total, done: c.done };
    emit('stats', c);
    OCR.fs.saveMeta();
  };

  S.addPages = (list) => {
    const P = S.d.project;
    list.forEach((p) => { P.pages[p.id] = p; P.meta.order.push(p.id); });
    S.reindex();
    emit('pages:structure');
    list.forEach((p) => OCR.fs.savePage(p.id));
    S.syncStats();
    if (!S.d.selectedId && list[0]) S.select(list[0].id);
  };

  S.removePage = (id) => {
    const P = S.d.project, i = S.indexOf(id);
    if (i < 0) return;
    P.meta.order.splice(i, 1);
    delete P.pages[id];
    S.d.multi.delete(id);
    OCR.fs.deletePage(id);
    S.reindex();
    if (S.d.selectedId === id) S.set({ selectedId: P.meta.order[Math.min(i, P.meta.order.length - 1)] || null });
    emit('pages:structure');
    S.syncStats();
  };

  S.movePage = (from, to) => {
    const o = S.d.project.meta.order;
    if (from === to || from < 0 || to < 0 || from >= o.length || to >= o.length) return;
    o.splice(to, 0, o.splice(from, 1)[0]);
    S.reindex();
    emit('pages:structure');
    OCR.fs.saveMeta();
  };
})();
