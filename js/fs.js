/* Storage: folder handle (File System Access API) or IndexedDB fallback, with
   debounced per-key saves, atomic writes and never-silently-drop retry. */
(function () {
  'use strict';
  const OCR = window.OCR, fs = OCR.fs, S = OCR.state;
  fs.supported = typeof window.showDirectoryPicker === 'function';
  let backend = null, rootHandle = null, dbp = null;
  const pending = new Map();

  const noStorage = () => Object.assign(new Error('No storage is connected.'), { name: 'NotAllowedError' });

  /* ---------- IndexedDB (handle persistence + fallback file store) ---------- */
  const db = () => dbp || (dbp = new Promise((res, rej) => {
    const r = indexedDB.open('ocr_fs', 1);
    r.onupgradeneeded = () => { r.result.createObjectStore('kv'); r.result.createObjectStore('files'); };
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  }));
  const idb = async (store, mode, fn) => {
    const d = await db();
    return new Promise((res, rej) => {
      const t = d.transaction(store, mode);
      const r = fn(t.objectStore(store));
      t.oncomplete = () => res(r && r.result);
      t.onerror = t.onabort = () => rej(t.error);
    });
  };
  const kvGet = (k) => idb('kv', 'readonly', (s) => s.get(k));
  const kvSet = (k, v) => idb('kv', 'readwrite', (s) => s.put(v, k));

  const idbBackend = {
    kind: 'idb',
    async read(p) {
      const v = await idb('files', 'readonly', (s) => s.get(p));
      if (v == null) return null;
      if (typeof v === 'string') return v;
      return v instanceof Blob ? v.text() : new TextDecoder().decode(v);
    },
    async readBlob(p) {
      const v = await idb('files', 'readonly', (s) => s.get(p));
      if (v == null) return null;
      return v instanceof Blob ? v : new Blob([v]);
    },
    async write(p, data) { await idb('files', 'readwrite', (s) => s.put(data, p)); },
    async list(p) {
      const pre = p ? p.replace(/\/+$/, '') + '/' : '';
      const keys = await idb('files', 'readonly', (s) => s.getAllKeys());
      const seen = new Map();
      keys.forEach((k) => {
        if (!k.startsWith(pre)) return;
        const rest = k.slice(pre.length), i = rest.indexOf('/');
        const name = i < 0 ? rest : rest.slice(0, i);
        if (name && !seen.has(name)) seen.set(name, i < 0 ? 'file' : 'directory');
      });
      return [...seen].map(([name, kind]) => ({ name, kind }));
    },
    async remove(p) {
      const keys = await idb('files', 'readonly', (s) => s.getAllKeys());
      const del = keys.filter((k) => k === p || k.startsWith(p + '/'));
      if (del.length) await idb('files', 'readwrite', (s) => { del.forEach((k) => s.delete(k)); });
    }
  };

  /* ---------- Folder backend ---------- */
  function folderBackend(root) {
    const split = (p) => { const a = p.split('/').filter(Boolean); return { dirs: a.slice(0, -1), name: a[a.length - 1] }; };
    const dir = async (parts, create) => { let d = root; for (const x of parts) d = await d.getDirectoryHandle(x, { create }); return d; };
    const missing = (e) => e && (e.name === 'NotFoundError' || e.name === 'TypeMismatchError');
    return {
      kind: 'folder',
      async readBlob(p) {
        try { const { dirs, name } = split(p); return await (await (await dir(dirs, false)).getFileHandle(name)).getFile(); }
        catch (e) { if (missing(e)) return null; throw e; }
      },
      async read(p) { const f = await this.readBlob(p); return f ? f.text() : null; },
      async write(p, data) {
        const { dirs, name } = split(p);
        const fh = await (await dir(dirs, true)).getFileHandle(name, { create: true });
        const w = await fh.createWritable(); /* writes to a swap file, committed on close */
        try { await w.write(data); await w.close(); } catch (e) { try { await w.abort(); } catch (x) { /* ignore */ } throw e; }
      },
      async list(p) {
        let d;
        try { d = await dir(p.split('/').filter(Boolean), false); } catch (e) { if (missing(e)) return []; throw e; }
        const out = [];
        for await (const [name, h] of d.entries()) out.push({ name, kind: h.kind });
        return out;
      },
      async remove(p) {
        const { dirs, name } = split(p);
        try { await (await dir(dirs, false)).removeEntry(name, { recursive: true }); } catch (e) { if (!missing(e)) throw e; }
      }
    };
  }

  ['read', 'readBlob', 'write', 'list', 'remove'].forEach((m) => {
    fs[m] = (...a) => (backend ? backend[m](...a) : Promise.reject(noStorage()));
  });
  fs.kind = () => (backend ? backend.kind : null);
  fs.hasBackend = () => !!backend;

  /* ---------- Debounced, versioned save pipeline ---------- */
  const setSave = (status) => S.set({ save: { status, pending: pending.size } });

  fs.schedule = (key, path, produce, o) => {
    o = o || {};
    const e = pending.get(key) || { key, v: 0 };
    clearTimeout(e.timer);
    Object.assign(e, { path, produce, json: !!o.json, v: e.v + 1 });
    e.timer = setTimeout(() => flush(key), o.delay == null ? 500 : o.delay);
    pending.set(key, e);
    if (S.d.save.status !== 'saving') setSave('saving');
  };

  async function safeJson(path, text) {
    if (/(^|\/)project\.json$/.test(path)) {
      const old = await backend.read(path);
      if (old) { try { JSON.parse(old); await backend.write(path + '.bak', old); } catch (e) { /* old copy already damaged: keep existing .bak */ } }
    }
    await backend.write(path, text);
  }

  async function flush(key) {
    const e = pending.get(key);
    if (!e || e.busy) return;
    clearTimeout(e.timer);
    e.busy = true;
    const v = e.v;
    try {
      if (!backend) throw noStorage();
      const data = await e.produce();
      if (e.json) await safeJson(e.path, data); else await backend.write(e.path, data);
      e.busy = false;
      if (e.v === v) pending.delete(key); else e.timer = setTimeout(() => flush(key), 0);
      if (!pending.size) { setSave('saved'); if (S.d.banner) S.set({ banner: null }); }
    } catch (err) {
      e.busy = false;
      onWriteError(err);
    }
  }

  async function onWriteError(err) {
    console.error('[fs] write failed', err);
    let perm = 'granted';
    if (rootHandle && backend && backend.kind === 'folder') {
      try { perm = await rootHandle.queryPermission({ mode: 'readwrite' }); } catch (e) { /* ignore */ }
    }
    if (perm !== 'granted') {
      S.set({ folder: Object.assign({}, S.d.folder, { permission: 'prompt' }),
        banner: { kind: 'reconnect', text: 'Folder access was lost. Your work is kept in memory and will be saved once you reconnect.', action: 'Reconnect folder' } });
    } else {
      S.set({ banner: { kind: 'write', text: 'Could not write to disk (' + ((err && (err.message || err.name)) || 'unknown error') + '). Your work is kept in memory.', action: 'Retry' } });
    }
    setSave('error');
  }

  fs.flushAll = () => Promise.all([...pending.keys()].map(flush));
  fs.retry = () => { S.set({ banner: null }); return fs.flushAll(); };
  fs.cancel = (key) => { const e = pending.get(key); if (e) { clearTimeout(e.timer); pending.delete(key); } };
  fs.pendingCount = () => pending.size;

  /* ---------- Init, folder choice, reconnect ---------- */
  function notice() {
    if (S.d.settings.noticeSeen) return;
    OCR.toast('Saves live in this browser only. Use Export to keep a copy.', { ms: 9000 });
    S.saveSettings({ noticeSeen: true });
  }
  fs.useBrowserStorage = () => {
    backend = idbBackend;
    S.saveSettings({ storage: 'idb' });
    S.set({ folder: { mode: 'idb', name: 'Browser storage', permission: 'granted' }, banner: null });
    notice();
    return fs.refresh();
  };

  async function loadFolderSettings() {
    try {
      const t = await backend.read('settings.json');
      if (!t) return;
      const j = JSON.parse(t);
      ['apiKey', 'profiles', 'storage', 'noticeSeen'].forEach((k) => delete j[k]);
      Object.assign(S.d.settings, j);
      document.documentElement.dataset.theme = S.d.settings.theme || 'auto';
      S.emit('settings', j);
    } catch (e) { console.warn('[fs] settings.json unreadable', e); }
  }

  fs.saveSettings = () => {
    if (!backend) return;
    fs.schedule('settings', 'settings.json', () => {
      const s = Object.assign({}, S.d.settings);
      ['apiKey', 'profiles', 'storage', 'noticeSeen'].forEach((k) => delete s[k]); /* never the API keys */
      return JSON.stringify(s, null, 2);
    }, { delay: 400, json: false });
  };

  fs.init = async () => {
    const st = S.d.settings.storage;
    if (st === 'idb' || !fs.supported) {
      backend = idbBackend;
      S.set({ folder: { mode: 'idb', name: 'Browser storage', permission: 'granted' } });
      notice();
      return;
    }
    try { rootHandle = await kvGet('root'); } catch (e) { rootHandle = null; }
    if (!rootHandle) { S.set({ folder: { mode: 'none', name: '', permission: 'none' } }); return; }
    let perm = 'prompt';
    try { perm = await rootHandle.queryPermission({ mode: 'readwrite' }); } catch (e) { /* ignore */ }
    if (perm === 'granted') {
      backend = folderBackend(rootHandle);
      S.set({ folder: { mode: 'folder', name: rootHandle.name, permission: 'granted' } });
      await loadFolderSettings();
    } else {
      S.set({ folder: { mode: 'folder', name: rootHandle.name, permission: 'prompt' } });
    }
  };

  fs.chooseFolder = async () => {
    if (!fs.supported) return false;
    let h;
    try { h = await window.showDirectoryPicker({ id: 'ocr-projects', mode: 'readwrite' }); }
    catch (e) { if (e.name !== 'AbortError') OCR.toast('Could not open the folder picker: ' + e.message, { kind: 'error' }); return false; }
    rootHandle = h;
    await kvSet('root', h);
    backend = folderBackend(h);
    S.saveSettings({ storage: 'auto' });
    S.set({ folder: { mode: 'folder', name: h.name, permission: 'granted' }, banner: null });
    await loadFolderSettings();
    await fs.refresh();
    return true;
  };

  fs.reconnect = async () => {
    if (!rootHandle) return fs.chooseFolder();
    let p = 'denied';
    try { p = await rootHandle.requestPermission({ mode: 'readwrite' }); } catch (e) { /* needs a user gesture */ }
    if (p !== 'granted') { OCR.toast('Folder access was not granted. Pick the folder again or use browser storage.', { kind: 'warn' }); return false; }
    backend = folderBackend(rootHandle);
    S.set({ folder: { mode: 'folder', name: rootHandle.name, permission: 'granted' }, banner: null });
    await loadFolderSettings();
    await fs.flushAll();
    await fs.refresh();
    return true;
  };

  document.addEventListener('visibilitychange', () => { if (document.hidden) fs.flushAll(); });
  window.addEventListener('pagehide', () => { fs.flushAll(); });
  window.addEventListener('beforeunload', (e) => { if (pending.size && S.d.save.status === 'error') { e.preventDefault(); e.returnValue = ''; } });
})();
