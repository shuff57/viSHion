/* OCR client: streaming chat completion, /slots polling, abort. Also a demo mode. */
(function () {
  'use strict';
  const OCR = window.OCR, O = OCR.ocr, S = OCR.state;

  const DEMO = [
    'Lesson 4: Quadratics\nSolve x^2 - 5x + 6 = 0 by factoring\n$x^2 - 5x + 6 = (x-2)(x-3)$\nso $x = 2$ or $x = 3$\nMargin: check with the discriminant\n~~complete the square~~ not needed here\nVertex form:\n$$y = a(x-h)^2 + k$$\nStudents struggled w/ [illegible] on #7',
    'Pt seen 10/3 c/o headache x2 days\nBP 142/90, HR 88, afebrile\nHx of hypertension, on lisinopril 10mg qd\nMargin: recheck BP in 2 wks\nPlan: continue meds, low sodium diet\n~~start metformin~~ not indicated\nF/U prn',
    'Staff meeting notes\n1. Robotics budget due Friday\n2. Pathways grant report, draft by 10/20\n3. [illegible] to order sensors\nMargin: ask about the second cart\nMean of the quiz scores: $\\bar{x} = \\frac{\\sum x_i}{n}$'
  ];

  const origin = () => (S.d.settings.ocrUrl || '').replace(/\/+$/, '');
  const sleep = (ms, signal) => new Promise((res, rej) => {
    if (signal && signal.aborted) return rej(new DOMException('Aborted', 'AbortError'));
    const t = setTimeout(res, ms);
    if (signal) signal.addEventListener('abort', () => { clearTimeout(t); rej(new DOMException('Aborted', 'AbortError')); }, { once: true });
  });
  const fail = (kind, msg, extra) => Object.assign(new Error(msg), { kind }, extra);

  /* Small models sometimes wrap the answer: a code fence, "Here is the transcription:", a closing note.
     Remove only those wrappers, only at the very start or end, and leave the handwriting itself alone. */
  O.clean = (t) => {
    let s = String(t == null ? '' : t).replace(/\r/g, '').replace(/^﻿/, '');
    s = s.replace(/^\s*```[a-z]*\s*\n/i, '').replace(/\n\s*```\s*$/, '');
    const lines = s.split('\n');
    const lead = /^\s*(?:sure|okay|ok|certainly|of course)?[,!. ]*\s*(?:here(?:'s| is| are)\b.*|the (?:transcription|transcript|text)\b.*|transcription|transcript|handwritten text)\s*:?\s*$/i;
    while (lines.length && (lead.test(lines[0]) || !lines[0].trim()) && (lead.test(lines[0]) || lines.length > 1 && lead.test(lines[1] || ''))) lines.shift();
    /* A closing remark goes only when a blank line precedes it and it reads like the model talking about its own work. */
    const tail = /^\s*(?:\(?\s*notes?\b.*\b(?:transcri|illegible|handwrit|unclear|hard to read|difficult|image|the page)\b.*|let me know\b.*|i hope\b.*|please let me know\b.*|this (?:is|was) (?:a |the )?transcription\b.*)$/i;
    while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
    for (let k = lines.length - 1; k > 0; k--) {
      if (lines[k - 1].trim() === '' && tail.test(lines[k])) { lines.length = k; break; }
      if (lines[k].trim() && !tail.test(lines[k])) break;
    }
    while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
    return lines.join('\n').replace(/\n{3,}/g, '\n\n').replace(/[ \t]+$/gm, '').trim();
  };

  O.buildPrompt = () => {
    const meta = S.d.project.meta, base = (meta.prompt || '').trim() || S.d.settings.defaultPrompt;
    const topic = (S.d.settings.topics || {})[meta.topic];
    const hints = topic && topic.terms.length ? 'Spelling hints (use only when the handwriting matches): ' + topic.terms.join(', ') + '.' : '';
    const mine = OCR.words && OCR.words.referenceWords ? OCR.words.referenceWords(meta.topic, 30) : [];
    const earlier = mine.length ? 'Words this teacher has fixed before (use only when the handwriting matches): ' + mine.join(', ') + '.' : '';
    return [base, hints, earlier].filter(Boolean).join('\n');
  };

  O.ping = async () => {
    if (S.d.settings.demo) return true;
    if (OCR.models && OCR.models.isCloud()) return true; /* hosted endpoints have no /health; a bad key or address shows on the first page */
    try { const r = await fetch(origin() + '/health'); return r.ok; } catch (e) { return false; }
  };
  O.unreachableMessage = () => 'Cannot reach OCR server at ' + origin() + '. Is llama-server running with CORS enabled?';

  /* GET /slots every 400 ms while a request runs, to report prompt-reading progress. */
  function poll(opt, state) {
    if (OCR.models && OCR.models.isCloud()) return { stop() {} }; /* /slots is a llama.cpp thing */
    let on = true;
    const tick = async () => {
      if (!on) return;
      try {
        const r = await fetch(origin() + '/slots', { signal: opt.signal });
        if (r.ok && !state.writing) {
          const j = await r.json(), sl = Array.isArray(j) ? j[0] : j;
          if (sl && sl.is_processing && sl.n_prompt_tokens > 0) {
            const pct = Math.min(99, Math.round((100 * (sl.n_prompt_tokens_processed || 0)) / sl.n_prompt_tokens));
            opt.onStatus && opt.onStatus('reading document… ' + pct + '%');
          }
        }
      } catch (e) { /* /slots is optional */ }
      if (on) setTimeout(tick, 400);
    };
    tick();
    return { stop() { on = false; } };
  }

  async function demo(opt) {
    const text = DEMO[(opt.index || 0) % DEMO.length], t0 = performance.now();
    for (let p = 0; p <= 100; p += 20) { opt.onStatus && opt.onStatus('reading document… ' + p + '%'); await sleep(90, opt.signal); }
    let out = '', n = 0;
    for (const part of text.split(/(\s+)/)) {
      out += part;
      if (part.trim()) n++;
      opt.onText && opt.onText(part, out);
      opt.onStatus && opt.onStatus('writing… ' + n + ' tok');
      await sleep(30, opt.signal);
    }
    const ms = performance.now() - t0;
    return { text: O.clean(out), tokens: n, ms, tps: n / (ms / 1000) };
  }

  O.transcribe = async (blob, opt) => {
    if (S.d.settings.demo) return demo(opt);
    const s = S.d.settings, headers = { 'Content-Type': 'application/json' };
    if (s.apiKey) headers.Authorization = 'Bearer ' + s.apiKey;
    const body = {
      model: s.model || 'ocr', stream: true, temperature: 0, max_tokens: 2000, stream_options: { include_usage: true },
      messages: [{ role: 'user', content: [
        { type: 'image_url', image_url: { url: await OCR.image.toDataURL(blob) } },
        { type: 'text', text: O.buildPrompt() }] }]
    };
    const t0 = performance.now(), state = { writing: false }, slots = poll(opt, state);
    opt.onStatus && opt.onStatus('reading document…');
    let res;
    try { res = await fetch(origin() + '/v1/chat/completions', { method: 'POST', headers, body: JSON.stringify(body), signal: opt.signal }); }
    catch (e) { slots.stop(); if (e.name === 'AbortError') throw e; throw fail('network', O.unreachableMessage()); }
    if (!res.ok) {
      slots.stop();
      const detail = (await res.text().catch(() => '')).slice(0, 160);
      if (res.status === 503) throw fail('unavailable', 'The OCR model is not loaded yet. ' + detail);
      throw fail('http', 'OCR server returned ' + res.status + '. ' + detail);
    }

    const reader = res.body.getReader(), dec = new TextDecoder();
    let buf = '', text = '', usage = null, timings = null, finished = false, malformed = false, chunks = 0;
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += dec.decode(value, { stream: true });
        let i;
        while ((i = buf.indexOf('\n')) >= 0) {
          const line = buf.slice(0, i).trim();
          buf = buf.slice(i + 1);
          if (line.indexOf('data:') !== 0) continue;
          const payload = line.slice(5).trim();
          if (payload === '[DONE]') { finished = true; continue; }
          try {
            const j = JSON.parse(payload), t = j.choices && j.choices[0] && j.choices[0].delta && j.choices[0].delta.content;
            if (t) {
              text += t; chunks++; state.writing = true;
              opt.onText && opt.onText(t, text);
              opt.onStatus && opt.onStatus('writing… ' + chunks + ' tok');
            }
            if (j.usage) usage = j.usage;
            if (j.timings) timings = j.timings;
          } catch (e) { malformed = true; }
        }
      }
    } catch (e) {
      slots.stop();
      if (e.name === 'AbortError') throw e;
      throw fail('stream', 'The connection to the OCR server dropped mid-page.', { partial: text });
    }
    slots.stop();
    if (!finished || malformed) throw fail('stream', malformed ? 'The OCR server sent an unreadable response.' : 'The OCR server stopped before finishing this page.', { partial: text });
    const ms = performance.now() - t0;
    const tokens = (usage && usage.completion_tokens) || (timings && timings.predicted_n) || chunks;
    return { text: O.clean(text), tokens, ms, tps: timings && timings.predicted_per_second };
  };
})();
