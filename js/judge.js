/* Glossary checks. A failing judge never blocks a transcript. */
(function () {
  'use strict';
  const OCR = window.OCR, J = OCR.judge, S = OCR.state;

  const terms = () => { const t = (S.d.settings.topics || {})[S.d.project.meta.topic]; return t ? t.terms : []; };

  function lev(a, b) {
    const m = a.length, n = b.length;
    let prev = Array.from({ length: n + 1 }, (_, j) => j);
    for (let i = 1; i <= m; i++) {
      const cur = [i];
      for (let j = 1; j <= n; j++) cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      prev = cur;
    }
    return prev[n];
  }

  /* Offline mode: flag words that are one or two edits away from a glossary term. */
  J.heuristic = (text, list) => {
    const single = list.filter((t) => !/\s/.test(t)), known = new Set(list.map((t) => t.toLowerCase())), seen = new Set(), out = [];
    (text.match(/[A-Za-z][A-Za-z'-]{3,}/g) || []).forEach((w) => {
      const lw = w.toLowerCase();
      if (seen.has(lw) || known.has(lw)) return;
      seen.add(lw);
      let best = null, bd = 99;
      single.forEach((t) => {
        const d = lev(lw, t.toLowerCase()), lim = lw.length >= 8 ? 2 : 1;
        if (d <= lim && d < bd && Math.abs(lw.length - t.length) <= 2) { best = t; bd = d; }
      });
      if (best) out.push({ word: w, term: best, conf: Math.max(0.3, 1 - bd / Math.max(w.length, 3)) });
    });
    return out;
  };

  /* A suggestion is only a spelling fix: the replacement must be a glossary term, close to the
     written word by edit distance, and the written word must not already be a glossary term.
     Anything else (rewording, grammar, different words) is dropped, so the student's own wording is never changed. */
  J.accept = (x, list, text) => {
    if (!x || typeof x.word !== 'string' || typeof x.term !== 'string' || text.indexOf(x.word) < 0) return false;
    const w = x.word.toLowerCase(), t = x.term.toLowerCase(), known = list.map((s) => s.toLowerCase());
    if (w === t || known.indexOf(t) < 0 || known.indexOf(w) >= 0) return false;
    return lev(w, t) <= Math.max(1, Math.min(3, Math.floor(Math.max(w.length, t.length) / 4)));
  };

  /* Local judge model. In typesafe mode any malformed entry rejects the whole answer. */
  J.remote = async (text, list, strict) => {
    const url = (S.d.settings.ocrUrl || '').replace(/\/+$/, '') + '/v1/chat/completions'; /* the one model does both jobs */
    const prompt = 'Check a handwriting transcript against a glossary. Return only a JSON array of {"word","term","conf"} for words in the transcript that are probably misspellings of a glossary term. Fix spelling only: never reword, never change grammar, numbers or meaning, and only use a term exactly as it appears in the glossary. conf is 0 to 1. Return [] when nothing needs fixing.\nGlossary: ' +
      list.join(', ') + '\nTranscript:\n' + text;
    const ctl = new AbortController(), tm = setTimeout(() => ctl.abort(), 20000);
    try {
      const r = await fetch(url, { method: 'POST', headers: Object.assign({ 'Content-Type': 'application/json' }, S.d.settings.apiKey ? { Authorization: 'Bearer ' + S.d.settings.apiKey } : {}), signal: ctl.signal,
        body: JSON.stringify({ model: S.d.settings.model || 'judge', temperature: 0, max_tokens: 800, messages: [{ role: 'user', content: [{ type: 'text', text: prompt }] }] }) });
      if (!r.ok) throw new Error('judge ' + r.status);
      const j = await r.json(), t = j.choices[0].message.content, m = t.match(/\[[^\[\]]*\]/), arr = JSON.parse(m ? m[0] : t); /* first array only: models add notes and a second answer */
      if (!Array.isArray(arr)) throw new Error('judge returned no array');
      const ok = arr.filter((x) => J.accept(x, list, text))
        .map((x) => ({ word: x.word, term: x.term, conf: Math.min(1, Math.max(0, Number(x.conf) || 0.5)) }));
      if (strict && ok.length !== arr.length) throw new Error('judge output failed validation');
      return ok;
    } finally { clearTimeout(tm); }
  };

  J.run = async (id) => {
    const p = S.page(id), list = S.d.project ? terms() : [];
    if (!p || !p.raw || !list.length) return;
    const mode = S.d.settings.judgeMode;
    let found = null;
    if (mode !== 'heuristic' && !S.d.settings.demo) { try { found = await J.remote(p.raw, list, mode === 'typesafe'); } catch (e) { found = null; } }
    if (!found) found = J.heuristic(p.raw, list);
    S.updatePage(id, { corrections: found.map((c) => Object.assign({ applied: false }, c)) });
  };

  /* corrected = raw with every applied suggestion swapped in, so undo is exact. */
  const rebuild = (p) => {
    let t = p.raw;
    p.corrections.forEach((c) => {
      if (!c.applied) return;
      const re = new RegExp('\\b' + c.word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '\\b', 'g');
      t = t.replace(re, c.term);
    });
    return t;
  };
  J.toggle = (id, idx, applied) => {
    const p = S.page(id);
    if (!p || !p.corrections[idx]) return;
    const next = p.corrections.map((c, i) => (i === idx ? Object.assign({}, c, { applied }) : c));
    const tmp = { raw: p.raw, corrections: next };
    S.updatePage(id, { corrections: next, corrected: rebuild(tmp) });
  };
  J.applyAll = (id) => {
    const p = S.page(id);
    if (!p) return;
    const next = p.corrections.map((c) => Object.assign({}, c, { applied: true }));
    S.updatePage(id, { corrections: next, corrected: rebuild({ raw: p.raw, corrections: next }) });
  };
})();
