/* Words to check. Works on the data the engine really produces: page.corrections
   (suggestions) and each [illegible] marker in page.raw. Typed fixes and "kept"
   decisions live in two optional page fields, so old projects open unchanged:
     page.resolved = { 'c:<index>' | 'i:<occurrence>': 'suggested' | 'written' | 'typed' | 'marked' }
     page.edits    = [ { key, text } ]   applied on top of page.corrected */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, W = (OCR.words = OCR.words || {});
  const ILL = /\[illegible\]/gi;
  const MATH = /\$\$[\s\S]+?\$\$|\$[^$\n]+?\$/g;
  const S0 = '\uE000', S1 = '\uE001', S2 = '\uE002'; /* private-use markers around a flagged word */
  const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const cur = (c) => (c.applied ? c.term : c.word);
  const editOf = (p, key) => (p.edits || []).find((e) => e.key === key);

  /* Runs fn on the text outside $...$ math, so markers never land inside KaTeX source. */
  function mapText(t, fn) {
    let out = '', last = 0, m;
    MATH.lastIndex = 0;
    while ((m = MATH.exec(t))) { out += fn(t.slice(last, m.index)) + m[0]; last = m.index + m[0].length; }
    return out + fn(t.slice(last));
  }

  /* Suggestions whose current text repeats an earlier one are dropped so every item has a unique place. */
  function suggestItems(p, plain) {
    const seen = new Set(), out = [];
    (p.corrections || []).forEach((c, i) => {
      const shownBase = plain ? c.word : cur(c);
      if (!shownBase || seen.has(shownBase)) return;
      seen.add(shownBase);
      out.push({ key: 'c:' + i, kind: 'suggest', index: i, word: c.word, term: c.term, conf: c.conf, base: shownBase });
    });
    return out;
  }

  /* One pass over the base text: apply typed edits, and (when mark) wrap each flagged word in markers. */
  function build(p, base, o) {
    const items = suggestItems(p, o.plain), map = new Map();
    items.forEach((it) => map.set(it.base, it));
    const alts = items.map((it) => esc(it.base)).sort((a, b) => b.length - a.length);
    const re = new RegExp('\\[[Ii][Ll][Ll][Ee][Gg][Ii][Bb][Ll][Ee]\\]' + (alts.length ? '|\\b(?:' + alts.join('|') + ')\\b' : ''), 'g');
    let n = 0;
    return mapText(base, (seg) => seg.replace(re, (m) => {
      let key, text = m;
      if (m.toLowerCase() === '[illegible]') key = 'i:' + n++;
      else { const it = map.get(m); if (!it) return m; key = it.key; }
      const ed = o.plain ? null : editOf(p, key);
      if (ed) text = ed.text;
      return o.mark ? S0 + key + S1 + text + S2 : text;
    }));
  }

  /* Text with typed fixes applied. This is what the review screen and every export show. */
  W.effective = (p) => {
    const base = (p && (p.corrected || p.raw)) || '';
    return p && p.edits && p.edits.length ? build(p, base, { mark: false }) : base;
  };
  /* Same text with markers, for the transcript view. version: 'corrected' | 'original'. */
  W.marked = (p, version) => (version === 'original'
    ? build(p, p.raw || '', { plain: true, mark: true })
    : build(p, p.corrected || p.raw || '', { mark: true }));

  const memo = new WeakMap();
  /* Every flagged word on a page, in reading order, with its decision (via) when there is one. */
  W.items = (p) => {
    if (!p || p.status !== 'done' || !p.raw) return [];
    const hit = memo.get(p);
    if (hit && hit.raw === p.raw && hit.c === p.corrections && hit.r === p.resolved && hit.e === p.edits && hit.cor === p.corrected) return hit.v;
    const res = p.resolved || {}, out = [];
    suggestItems(p, false).forEach((it) => {
      const ed = editOf(p, it.key);
      out.push(Object.assign(it, { via: res[it.key] || null, shown: ed ? ed.text : it.base }));
    });
    let n = 0;
    mapText(p.corrected || p.raw, (seg) => { seg.replace(ILL, () => { const key = 'i:' + n++, ed = editOf(p, key); out.push({ key, kind: 'illegible', index: n - 1, word: '[illegible]', via: res[key] || null, shown: ed ? ed.text : '[illegible]' }); return ''; }); return seg; });
    memo.set(p, { raw: p.raw, c: p.corrections, r: p.resolved, e: p.edits, cor: p.corrected, v: out });
    return out;
  };
  W.open = (p) => W.items(p).filter((it) => !it.via);
  W.count = (p) => W.open(p).length;

  const clean = (s) => String(s || '').replace(/[\uE000-\uE002$\n\r]/g, ' ').replace(/\[illegible\]/gi, '').replace(/\s+/g, ' ').trim();

  /* Up to five words either side of the n-th unreadable word (outside math), for the reference library. */
  W.contextFor = (p, key) => {
    const base = p.corrected || p.raw || '', want = +String(key).slice(2);
    const masked = base.replace(MATH, (m) => ' '.repeat(m.length)), re = /\[illegible\]/gi;
    let n = 0, m, at = -1;
    while ((m = re.exec(masked))) { if (n++ === want) { at = m.index; break; } }
    if (at < 0) return null;
    const words = (t) => t.replace(/\[illegible\]/gi, ' … ').replace(/[~$]/g, ' ').split(/\s+/).filter(Boolean);
    const plain = (t) => t.replace(MATH, ' ');
    return { before: words(plain(base.slice(0, at))).slice(-5).join(' '), after: words(plain(base.slice(at + 11))).slice(0, 5).join(' ') };
  };

  /* The reference library: fixes the teacher typed for unreadable handwriting. It is prompt context
     for the model (not training), kept in settings.reference, newest last. */
  const REF_MAX = 300;
  W.remember = (p, key, text) => {
    const st = S.d.settings;
    if (st.rememberFixes === false || !text) return;
    const ctx = W.contextFor(p, key);
    if (!ctx) return;
    const topic = (S.d.project && S.d.project.meta.topic) || '';
    const list = (st.reference || []).filter((r) => !(r.word === text && r.before === ctx.before && r.after === ctx.after));
    list.push({ word: text, before: ctx.before, after: ctx.after, topic, at: Date.now() });
    S.saveSettings({ reference: list.slice(-REF_MAX) });
  };
  W.referenceWords = (topic, max) => {
    const seen = new Set(), out = [];
    (S.d.settings.reference || []).slice().reverse().forEach((r) => {
      const k = r.word.toLowerCase();
      if ((topic && r.topic && r.topic !== topic) || seen.has(k) || out.length >= (max || 30) || /\s/.test(r.word) && r.word.length > 30) return;
      seen.add(k); out.push(r.word);
    });
    return out;
  };
  W.referenceExamples = (topic, max) => (S.d.settings.reference || []).filter((r) => !topic || !r.topic || r.topic === topic)
    .slice(-(max || 10)).map((r) => '“' + [r.before, '___', r.after].filter(Boolean).join(' ') + '” was ' + r.word);

  /* Records a decision. via: suggested | written | typed | marked. text only for typed. */
  W.choose = (id, key, via, text) => {
    const p = S.page(id);
    if (!p) return false;
    const resolved = Object.assign({}, p.resolved), edits = (p.edits || []).filter((e) => e.key !== key);
    if (via === 'typed') {
      const t = clean(text);
      if (!t) return false;
      edits.push({ key, text: t });
    }
    resolved[key] = via;
    if (via === 'typed' && key[0] === 'i') W.remember(p, key, edits[edits.length - 1].text);
    S.updatePage(id, { resolved, edits, reviewed: false });
    if (key[0] === 'c') OCR.judge.toggle(id, +key.slice(2), via === 'suggested'); /* keeps `corrected` in step */
    return true;
  };
  /* Takes back a decision on one word. */
  W.reopen = (id, key) => {
    const p = S.page(id);
    if (!p) return;
    const resolved = Object.assign({}, p.resolved);
    delete resolved[key];
    S.updatePage(id, { resolved, edits: (p.edits || []).filter((e) => e.key !== key), reviewed: false });
  };
  /* Use every suggestion that has no decision yet, on the given pages. */
  W.useAll = (pageIds) => {
    pageIds.forEach((id) => {
      const p = S.page(id);
      if (p) W.items(p).filter((it) => it.kind === 'suggest' && !it.via).forEach((it) => W.choose(id, it.key, 'suggested'));
    });
  };

  /* Turns the marker characters in a rendered transcript into <button> elements.
     make(key) returns the button; KaTeX output is never touched. */
  W.wrap = (host, make) => {
    const accept = { acceptNode: (n) => {
      if (n.nodeValue.indexOf(S0) < 0 && n.nodeValue.indexOf(S2) < 0) return NodeFilter.FILTER_SKIP;
      return n.parentNode && n.parentNode.closest && n.parentNode.closest('.katex') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
    } };
    for (let guard = 0; guard < 500; guard++) {
      const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT, accept);
      let start = null;
      for (let n = walker.nextNode(); n; n = walker.nextNode()) { if (n.nodeValue.indexOf(S0) >= 0) { start = n; break; } }
      if (!start) break;
      const v = start.nodeValue, i = v.indexOf(S0), j = v.indexOf(S1, i);
      if (j < 0) { start.nodeValue = v.replace(/[\uE000-\uE002]/g, ''); continue; }
      const key = v.slice(i + 1, j);
      start.nodeValue = v.slice(0, i) + v.slice(j + 1);
      let end = null, k = -1;
      const w2 = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
      w2.currentNode = start;
      for (let n = start; n; n = w2.nextNode()) {
        const at = n.nodeValue.indexOf(S2, n === start ? i : 0);
        if (at >= 0) { end = n; k = at; break; }
      }
      if (!end) continue;
      end.nodeValue = end.nodeValue.slice(0, k) + end.nodeValue.slice(k + 1);
      try {
        const r = document.createRange();
        r.setStart(start, i); r.setEnd(end, k);
        if (r.collapsed) continue;
        r.surroundContents(make(key));
      } catch (e) { /* markers were removed already; the word stays as plain text */ }
    }
    return host;
  };

})();
