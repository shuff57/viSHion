/* Sequential batch queue (single GPU slot): stop, retry, resume. */
(function () {
  'use strict';
  const OCR = window.OCR, Q = OCR.queue, S = OCR.state;
  let ctrl = null, stopFlag = false, running = false, timedOut = false, lastIds = [];
  const durations = [];

  const setRun = (p) => S.set({ run: Object.assign({}, S.d.run, p) });
  Q.isRunning = () => running;
  Q.isQueued = (id) => running && lastIds.indexOf(id) >= 0; /* read-only: is this page part of the active run */

  const eta = (remaining) => {
    if (!durations.length) return 0;
    return (durations.reduce((a, b) => a + b, 0) / durations.length) * remaining;
  };

  async function finish(id, r, blob) {
    const raw = r.text.replace(/\s+$/, '');
    let boxes = [];
    try { boxes = await OCR.render.computeBoxes(blob, raw); } catch (e) { console.warn('[queue] boxes', e); }
    S.updatePage(id, { status: 'done', raw, corrected: raw, corrections: [], boxes, error: '', timings: { ms: Math.round(r.ms), tokens: r.tokens } });
    setRun({ message: 'done in ' + (r.ms / 1000).toFixed(1) + 's · ' + r.tokens + ' tok' });
    OCR.judge.run(id).catch(() => { /* a failing judge never blocks a result */ });
  }

  /* Returns 'ok' | 'error' | 'abort' | 'unreachable'. */
  Q.runOne = async (id, index) => {
    const page = S.page(id);
    if (!page) return 'error';
    const blob = await OCR.fs.loadImage(id).catch(() => null);
    if (!blob) { S.updatePage(id, { status: 'error', error: 'The image for this page is missing from the project.' }); return 'error'; }
    ctrl = new AbortController(); timedOut = false;
    const tm = setTimeout(() => { timedOut = true; ctrl.abort(); }, (S.d.settings.timeoutSec || 180) * 1000);
    S.select(id);
    S.updatePage(id, { status: 'transcribing', raw: '', corrected: '', corrections: [], boxes: [], error: '' }, { save: false });
    setRun({ currentId: id, message: 'reading document…' });
    try {
      const r = await OCR.ocr.transcribe(blob, {
        signal: ctrl.signal, index,
        onStatus: (m) => setRun({ message: m }),
        onText: (chunk, full) => { page.raw = full; S.emit('page:stream', { id, chunk }); }
      });
      clearTimeout(tm);
      await finish(id, r, blob);
      return 'ok';
    } catch (e) {
      clearTimeout(tm);
      if (e.name === 'AbortError' && !timedOut) { S.updatePage(id, { status: 'pending', raw: '', corrected: '' }); return 'abort'; }
      if (timedOut) {
        S.updatePage(id, { status: 'error', error: 'Timed out after ' + (S.d.settings.timeoutSec || 180) + ' seconds. Retry, or raise the timeout in Settings.' });
        return 'error';
      }
      if (e.kind === 'network' || e.kind === 'unavailable') {
        S.updatePage(id, { status: 'pending', raw: '' });
        S.set({ serverError: e.message });
        return 'unreachable';
      }
      const partial = e.partial || page.raw || '';
      S.updatePage(id, { status: 'error', raw: partial, corrected: partial, error: e.message || 'Transcription failed.' });
      return 'error';
    } finally { ctrl = null; }
  };

  /* ids: pages to run. force re-runs pages that already have a transcript. */
  Q.start = async (ids, opt) => {
    if (running || !S.d.project) return;
    ids = ids || S.pages().filter((p) => p.status === 'pending').map((p) => p.id);
    if (!ids.length) { OCR.toast('Every page already has a transcript.'); return; }
    running = true; stopFlag = false; durations.length = 0; lastIds = ids.slice();
    S.set({ serverError: null });
    setRun({ active: true, paused: false, total: ids.length, done: 0, currentId: null, etaMs: 0, message: '' });
    let done = 0, outcome = 'ok';
    for (let i = 0; i < ids.length; i++) {
      if (stopFlag) { outcome = 'abort'; break; }
      const p = S.page(ids[i]);
      if (!p || (p.status === 'done' && !(opt && opt.force))) { done++; continue; }
      const t0 = performance.now();
      const r = await Q.runOne(ids[i], i);
      if (r === 'abort') { outcome = 'abort'; break; }
      if (r === 'unreachable') { outcome = 'paused'; break; }
      durations.push(performance.now() - t0);
      done++;
      setRun({ done, etaMs: eta(ids.length - done) });
    }
    running = false;
    const c = S.counts();
    const msg = outcome === 'abort' ? 'Stopped. ' + c.pending + ' pages still need a transcript.'
      : outcome === 'paused' ? 'Paused. Fix the server, then press Retry.'
        : c.error ? 'Finished with ' + c.error + ' failed ' + (c.error === 1 ? 'page' : 'pages') + '.' : 'Finished. ' + c.done + ' of ' + c.total + ' pages transcribed.';
    setRun({ active: false, paused: outcome === 'paused', currentId: null, etaMs: 0, done, message: '' });
    S.set({ status: msg });
  };

  Q.stop = () => {
    stopFlag = true;
    if (ctrl) ctrl.abort();
  };
  Q.retryFailed = () => Q.start(S.pages().filter((p) => p.status === 'error').map((p) => p.id));
  Q.resume = async () => {
    if (!(await OCR.ocr.ping())) { S.set({ serverError: OCR.ocr.unreachableMessage() }); return; }
    S.set({ serverError: null });
    const left = lastIds.filter((id) => { const p = S.page(id); return p && p.status === 'pending'; });
    Q.start(left.length ? left : undefined);
  };
})();
