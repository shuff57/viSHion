/* Likely words for handwriting the model could not read. After a page is transcribed, the same
   model looks at the page again and, for each [illegible], offers up to three candidates with a
   percentage. Stored in the optional page field  page.guesses = { 'i:<n>': [ { text, percent } ] }.
   The percentage is the model's own estimate, not a calibrated probability. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, W = (OCR.words = OCR.words || {});
  const MATH = /\$\$[\s\S]+?\$\$|\$[^$\n]+?\$/g;
  const busy = new Set();
  let chain = Promise.resolve();

  W.isGuessing = (id) => busy.has(id);

  /* The transcript with each unreadable word numbered, in the same order words.js counts them. */
  function numbered(raw) {
    let n = 0, out = '', last = 0, m;
    const num = (seg) => seg.replace(/\[illegible\]/gi, () => '[illegible #' + (++n) + ']');
    MATH.lastIndex = 0;
    while ((m = MATH.exec(raw))) { out += num(raw.slice(last, m.index)) + m[0]; last = m.index + m[0].length; }
    return { text: out + num(raw.slice(last)), count: n };
  }

  /* First JSON array in a reply that may also contain notes or code fences. */
  function firstArray(t) {
    const s = t.indexOf('[');
    if (s < 0) return null;
    let depth = 0;
    for (let i = s; i < t.length; i++) {
      if (t[i] === '[') depth++;
      else if (t[i] === ']' && --depth === 0) { try { return JSON.parse(t.slice(s, i + 1)); } catch (e) { return null; } }
    }
    return null;
  }

  /* Cleans the model's answer into { 'i:<n-1>': [{ text, percent }] }. */
  W.parseGuesses = (reply, count) => {
    const arr = firstArray(String(reply || '')), out = {};
    if (!Array.isArray(arr)) return out;
    arr.forEach((g) => {
      const n = Number(g && g.n);
      if (!(n >= 1 && n <= count) || !Array.isArray(g.options)) return;
      const seen = new Set();
      let opts = g.options.map((o) => ({ text: String((o && o.text) == null ? '' : o.text).replace(/\s+/g, ' ').trim(), percent: Math.max(0, Math.min(100, Math.round(Number(o && (o.percent != null ? o.percent : o.certainty)) || 0))) }))
        .filter((o) => o.text && o.text.length <= 40 && !/^\[?illegible/i.test(o.text) && o.percent > 0 && !seen.has(o.text.toLowerCase()) && seen.add(o.text.toLowerCase()))
        .sort((a, b) => b.percent - a.percent).slice(0, 3);
      const sum = opts.reduce((t, o) => t + o.percent, 0);
      if (sum > 100) opts = opts.map((o) => ({ text: o.text, percent: Math.max(1, Math.round((o.percent * 100) / sum)) }));
      if (opts.length) out['i:' + (n - 1)] = opts;
    });
    return out;
  };

  async function ask(p) {
    const n = numbered(p.raw);
    if (!n.count) return {};
    if (S.d.settings.demo) {
      const d = {};
      for (let i = 0; i < n.count; i++) d['i:' + i] = [{ text: 'the discriminant', percent: 55 }, { text: 'discriminant', percent: 25 }, { text: 'a derivative', percent: 10 }];
      return d;
    }
    const blob = await OCR.fs.loadImage(p.id);
    if (!blob) throw new Error('The image is missing.');
    const s = S.d.settings, headers = { 'Content-Type': 'application/json' };
    if (s.apiKey) headers.Authorization = 'Bearer ' + s.apiKey;
    const ex = W.referenceExamples((S.d.project && S.d.project.meta.topic) || '', 10);
    const prompt = 'Below is the transcript of the handwritten page in the image. Each [illegible #k] marks a word that could not be read. ' +
      'For each one, look at the handwriting and the sentence around it and give up to 3 of the most likely words, each with your certainty as a percentage from 0 to 100. ' +
      'Certainties for one word must not add up to more than 100. Give an empty list if you cannot tell. ' +
      (ex.length ? 'For reference, this teacher resolved unreadable words in earlier pages like this: ' + ex.join('; ') + '. Use them as a guide to vocabulary and style, not as answers to copy.\n' : '') +
      'Answer with JSON only, like [{"n":1,"options":[{"text":"discriminant","percent":60},{"text":"determinant","percent":25}]}].\n\nTranscript:\n' + n.text;
    const ctl = new AbortController(), tm = setTimeout(() => ctl.abort(), (s.timeoutSec || 180) * 1000);
    try {
      const r = await fetch((s.ocrUrl || '').replace(/\/+$/, '') + '/v1/chat/completions', { method: 'POST', headers, signal: ctl.signal,
        body: JSON.stringify({ model: s.model || 'ocr', stream: false, temperature: 0.2, max_tokens: 500,
          messages: [{ role: 'user', content: [{ type: 'image_url', image_url: { url: await OCR.image.toDataURL(blob) } }, { type: 'text', text: prompt }] }] }) });
      if (!r.ok) throw new Error('The server answered with ' + r.status + '.');
      const j = await r.json();
      return W.parseGuesses((j.choices && j.choices[0] && j.choices[0].message && j.choices[0].message.content) || '', n.count);
    } finally { clearTimeout(tm); }
  }

  /* Asks for guesses for one page (one at a time, so a batch does not pile onto the server). */
  W.guess = (id) => {
    const p = S.page(id);
    if (!p || p.status !== 'done' || busy.has(id) || !/\[illegible\]/i.test(p.raw || '')) return Promise.resolve(false);
    busy.add(id);
    S.emit('page:update', { id, keys: ['guessing'] });
    chain = chain.then(async () => {
      let found = null;
      try { found = await ask(S.page(id) || p); } catch (e) { console.warn('[guess]', e); }
      busy.delete(id);
      if (S.page(id)) S.updatePage(id, { guesses: found || {}, guessed: true });
      return !!found;
    });
    return chain;
  };

  /* Automatic: when a page finishes and has unreadable words. Re-reading a page clears old guesses. */
  W.autoGuess = () => {
    S.on('page:update', (e) => {
      if (e.keys.indexOf('status') < 0) return;
      const p = S.page(e.id);
      if (!p) return;
      if (p.status === 'transcribing' && (p.guessed || (p.guesses && Object.keys(p.guesses).length))) S.updatePage(e.id, { guesses: {}, guessed: false }, { save: false });
      else if (p.status === 'done' && !p.guessed && S.d.settings.guessIllegible !== false) W.guess(e.id);
    });
  };
})();
