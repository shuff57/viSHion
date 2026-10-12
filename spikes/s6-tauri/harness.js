// Plain-JS test harness; logic under test lives in Tauri commands / plugins. Exposed as window.s6.
(function () {
  const T = window.__TAURI__;
  const inv = T && T.core ? T.core.invoke : null;
  const LOG = [];
  const out = () => document.getElementById('out');
  function log(tag, data) {
    const line = JSON.stringify({ t: Math.round(performance.now()), tag, data });
    LOG.push(line);
    const o = out(); if (o) o.textContent += line + '\n';
    if (inv) inv('log_line', { line }).catch(() => {});
  }
  const s6 = { log, LOG };
  window.s6 = s6;

  // ---------- 1. origin / storage ----------
  s6.env = async function () {
    const r = {
      origin: location.origin, protocol: location.protocol, href: location.href,
      isSecureContext: window.isSecureContext, ua: navigator.userAgent,
      showDirectoryPicker: typeof window.showDirectoryPicker,
      showOpenFilePicker: typeof window.showOpenFilePicker,
      showSaveFilePicker: typeof window.showSaveFilePicker,
      hasTauri: !!T, tauriKeys: T ? Object.keys(T) : [],
      localStorageLen: localStorage.length,
      localStorageKeys: Object.keys(localStorage),
    };
    try { r.idbDatabases = (await indexedDB.databases()).map(d => d.name); } catch (e) { r.idbErr = String(e); }
    try { const e = await navigator.storage.estimate(); r.quota = e.quota; r.usage = e.usage; } catch (e) {}
    try { r.persisted = await navigator.storage.persisted(); } catch (e) {}
    log('env', r); return r;
  };
  s6.seedStorage = function () {
    localStorage.setItem('ocr_settings_v1', '{"seeded":"in-tauri"}');
    return new Promise(res => {
      const q = indexedDB.open('vishion_test', 1);
      q.onupgradeneeded = () => q.result.createObjectStore('s');
      q.onsuccess = () => { const tx = q.result.transaction('s', 'readwrite'); tx.objectStore('s').put('x', 'k'); tx.oncomplete = () => res(true); };
    });
  };

  // ---------- 3. drag and drop ----------
  const dd = {};
  function dtInfo(e) {
    const dt = e.dataTransfer; if (!dt) return null;
    const items = [...(dt.items || [])].map(i => {
      let entry = null;
      try { const en = i.webkitGetAsEntry && i.webkitGetAsEntry(); entry = en ? { name: en.name, isFile: en.isFile, isDirectory: en.isDirectory } : null; } catch (x) {}
      return { kind: i.kind, type: i.type, entry };
    });
    return { types: [...dt.types], files: [...dt.files].map(f => ({ name: f.name, size: f.size })), items };
  }
  for (const ev of ['dragstart', 'dragenter', 'drop', 'dragend']) {
    document.addEventListener(ev, e => {
      log('html5-' + ev, { target: e.target.id || e.target.tagName, info: dtInfo(e) });
    }, true);
  }
  document.addEventListener('dragover', e => { e.preventDefault(); dd.over = (dd.over || 0) + 1; }, true);
  document.addEventListener('drop', e => { e.preventDefault(); }, true);
  // in-page reorder
  let dragged = null;
  const list = () => document.getElementById('list');
  document.addEventListener('dragstart', e => {
    const li = e.target.closest && e.target.closest('#list li');
    if (li) { dragged = li; e.dataTransfer.setData('text/plain', li.id); e.dataTransfer.effectAllowed = 'move'; }
  });
  document.addEventListener('drop', e => {
    const li = e.target.closest && e.target.closest('#list li');
    if (dragged && li && li !== dragged) { list().insertBefore(dragged, li); log('reorder', [...list().children].map(c => c.textContent)); }
    dragged = null;
  });
  if (T && T.event) {
    for (const n of ['tauri://drag-enter', 'tauri://drag-drop', 'tauri://drag-leave']) {
      T.event.listen(n, ev => log('tauri-' + n, ev.payload));
    }
  }

  // ---------- 4. HTTP ----------
  // impl: 'plugin' | 'window'. Reads the stream, records arrival time of each chunk.
  s6.sse = async function (url, impl, opts = {}) {
    const f = impl === 'plugin' ? T.http.fetch : window.fetch.bind(window);
    const ac = new AbortController();
    const t0 = performance.now();
    const res = { impl, url, chunks: [], aborted: false };
    if (opts.abortAfterMs) setTimeout(() => { res.abortCalledAt = Math.round(performance.now() - t0); ac.abort(); }, opts.abortAfterMs);
    try {
      const r = await f(url, {
        method: 'POST', headers: { 'content-type': 'application/json', authorization: 'Bearer test' },
        body: JSON.stringify({ stream: true, model: 'm' }), signal: ac.signal,
      });
      res.status = r.status; res.ct = r.headers.get('content-type'); res.headersAt = Math.round(performance.now() - t0);
      const rd = r.body.getReader(); const dec = new TextDecoder();
      for (;;) {
        const { done, value } = await rd.read();
        if (done) break;
        res.chunks.push({ at: Math.round(performance.now() - t0), n: value.length, head: dec.decode(value).slice(0, 40) });
      }
      res.done = true;
    } catch (e) { res.error = String((e && e.message) || e); res.errName = e && e.name; res.aborted = ac.signal.aborted; }
    res.totalMs = Math.round(performance.now() - t0);
    log('sse', res); return res;
  };

  // ---------- 2. native storage via plugin-fs (scope from dialog / persisted scope) ----------
  s6.pluginFs = async function (dir) {
    const fs = T.fs, r = {};
    try {
      const p = dir + '/plugin-fs.txt', tmp = dir + '/.plugin-fs.txt.tmp';
      await fs.writeTextFile(tmp, 'hello via plugin-fs');
      await fs.rename(tmp, p);
      r.read = await fs.readTextFile(p);
      r.list = (await fs.readDir(dir)).map(e => e.name);
      await fs.remove(p);
      r.existsAfterRemove = await fs.exists(p);
    } catch (e) { r.error = String((e && e.message) || e); }
    log('pluginFs', r); return r;
  };
  s6.jsDialogPick = async function () {
    const p = await T.dialog.open({ directory: true, title: 'S6 pick folder (JS dialog)' });
    log('jsDialogPick', p); return p;
  };

  // ---------- 5. downloads ----------
  s6.anchorDownload = function (name) {
    const blob = new Blob(['exported via anchor ' + name], { type: 'text/plain' });
    const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = name;
    document.body.appendChild(a); a.click(); a.remove(); log('anchorDownload', name);
  };

  document.addEventListener('DOMContentLoaded', () => log('ready', { origin: location.origin }));
  s6.inv = inv;
})();
