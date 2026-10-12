/* Getting files in: file picker, drag-drop (files and folders), clipboard paste, replace page.
   Files are never added straight to the assignment: they go to the Add pages screen (U.add) first. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, U = OCR.ui, up = (U.upload = U.upload || {});
  let replaceId = null;
  const input = () => document.getElementById('file-input');

  up.pick = () => { replaceId = null; input().value = ''; input().click(); };
  up.replace = (id) => { replaceId = id; input().value = ''; input().click(); };

  const readAll = async (reader) => {
    const all = [];
    for (;;) {
      const batch = await new Promise((res, rej) => reader.readEntries(res, rej));
      if (!batch.length) return all;
      all.push(...batch);
    }
  };
  async function walk(entry, out, prefix) {
    if (entry.isFile) {
      const f = await new Promise((res, rej) => entry.file(res, rej));
      f._path = prefix + f.name;
      out.push(f);
    } else if (entry.isDirectory) {
      for (const k of await readAll(entry.createReader())) await walk(k, out, prefix + entry.name + '/');
    }
  }
  /* dataTransfer must be read synchronously, so entries are grabbed before the first await. */
  up.collect = async (dt) => {
    const entries = [], plain = Array.from(dt.files || []);
    Array.from(dt.items || []).forEach((it) => {
      const en = it.kind === 'file' && it.webkitGetAsEntry ? it.webkitGetAsEntry() : null;
      if (en) entries.push(en);
    });
    if (!entries.length) return plain;
    const out = [];
    for (const en of entries) await walk(en, out, '');
    return out;
  };

  /* Hands files to the Add pages screen, opening it first when needed. */
  up.stage = (files) => {
    if (!S.d.project || !files.length) return;
    if (S.d.view !== 'add') U.nav.add();
    U.add.addFiles(files);
  };

  async function doReplace(id, f) {
    const why = OCR.image.check(f);
    if (why) { OCR.toast(f.name + ': ' + why, { kind: 'warn' }); return; }
    try {
      let { blob } = await OCR.image.prepare(f);
      if (S.d.settings.straighten !== false) blob = (await OCR.image.deskew(blob)).blob;
      OCR.fs.saveImage(id, blob, S.d.settings.keepOriginals ? { file: f, ext: (f.name.split('.').pop() || 'img').toLowerCase() } : null);
      S.updatePage(id, { name: f.name, status: 'pending', raw: '', corrected: '', corrections: [], boxes: [], timings: { ms: 0, tokens: 0 }, error: '', rotation: 0, reviewed: false, resolved: {}, edits: [] });
      S.emit('image:changed', { id });
      S.set({ status: 'Replaced ' + f.name + '. The other pages are unchanged.' });
      OCR.toast('Page replaced. Transcribe it again from this screen.');
    } catch (e) { OCR.toast('Could not read that image.', { kind: 'warn' }); }
  }

  up.bindDrop = (el) => {
    const hasFiles = (e) => e.dataTransfer && Array.from(e.dataTransfer.types || []).indexOf('Files') >= 0;
    const live = () => !!S.d.project && S.d.view !== 'home';
    let depth = 0;
    el.addEventListener('dragenter', (e) => { if (live() && hasFiles(e)) { depth++; el.dataset.drag = '1'; } });
    el.addEventListener('dragover', (e) => { if (live() && hasFiles(e)) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy'; } });
    el.addEventListener('dragleave', (e) => { if (hasFiles(e) && --depth <= 0) { depth = 0; delete el.dataset.drag; } });
    el.addEventListener('drop', async (e) => {
      if (!live() || !hasFiles(e)) return;
      e.preventDefault(); depth = 0; delete el.dataset.drag;
      up.stage(await up.collect(e.dataTransfer));
    });
  };

  up.init = () => {
    up.bindDrop(document.getElementById('main'));
    input().addEventListener('change', () => {
      const files = Array.from(input().files);
      if (!files.length) return;
      if (replaceId) { const id = replaceId; replaceId = null; doReplace(id, files[0]); } else up.stage(files);
    });
    window.addEventListener('paste', (e) => {
      if (!S.d.project || S.d.view === 'home') return;
      const t = e.target;
      if (t && /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)) return;
      const files = Array.from((e.clipboardData && e.clipboardData.files) || []);
      if (files.length) { e.preventDefault(); up.stage(files); }
    });
  };
})();
