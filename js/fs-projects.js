/* Project-level operations on top of the storage backend (folder or IndexedDB). */
(function () {
  'use strict';
  const OCR = window.OCR, fs = OCR.fs, S = OCR.state;
  const SCHEMA = 1, CACHE_MAX = 20;
  const imgCache = new Map(); /* id -> Blob, small LRU so full-size data is not all held at once */

  const normalize = (m) => Object.assign({
    schemaVersion: SCHEMA, name: 'Untitled', topic: 'General', prompt: '', order: [],
    created: Date.now(), updated: Date.now(), ui: {}, stats: null
  }, m || {});

  async function readMeta(slug, restore) {
    const txt = await fs.read(slug + '/project.json');
    if (txt == null) return null;
    try { return normalize(JSON.parse(txt)); } catch (e) { /* fall through to the backup */ }
    const bak = await fs.read(slug + '/project.json.bak');
    if (bak) {
      try {
        const m = normalize(JSON.parse(bak));
        if (restore) {
          await fs.write(slug + '/project.json', bak);
          OCR.toast('project.json was damaged, so it was restored from the last backup.', { kind: 'warn' });
        }
        return m;
      } catch (e2) { /* backup damaged too */ }
    }
    if (restore) OCR.toast('Could not read ' + slug + '/project.json and there is no usable backup.', { kind: 'error' });
    return null;
  }

  async function uniqueSlug(base) {
    const taken = new Set((await fs.list('')).map((e) => e.name));
    let s = base || 'project', i = 2;
    while (taken.has(s)) s = (base || 'project') + '-' + i++;
    return s;
  }

  fs.listProjects = async () => {
    const out = [];
    const dirs = (await fs.list('')).filter((e) => e.kind === 'directory' && e.name[0] !== '.');
    for (const e of dirs) {
      const m = await readMeta(e.name, false);
      if (m) out.push({ slug: e.name, name: m.name, pages: (m.stats && m.stats.pages) || m.order.length, done: (m.stats && m.stats.done) || 0, updated: m.updated });
    }
    return out.sort((a, b) => (b.updated || 0) - (a.updated || 0));
  };

  fs.refresh = async () => {
    if (!fs.hasBackend()) { S.set({ projects: [] }); return; }
    try { S.set({ projects: await fs.listProjects() }); }
    catch (e) { console.error(e); OCR.toast('Could not read the projects folder: ' + (e.message || e.name), { kind: 'error' }); }
  };

  fs.createProject = async (name) => {
    const slug = await uniqueSlug(OCR.slug(name) || 'project'), now = Date.now();
    const meta = normalize({ name, prompt: '', created: now, updated: now, stats: { pages: 0, done: 0 } });
    await fs.write(slug + '/project.json', JSON.stringify(meta, null, 2));
    return slug;
  };

  fs.openProject = async (slug) => {
    const meta = await readMeta(slug, true);
    if (!meta) return false;
    const pages = {};
    await Promise.all(meta.order.map(async (id) => {
      try { const t = await fs.read(slug + '/pages/' + id + '.json'); if (t) pages[id] = JSON.parse(t); }
      catch (e) { console.warn('[fs] page unreadable', id, e); }
    }));
    meta.order = meta.order.filter((id) => pages[id]);
    Object.values(pages).forEach((p) => { if (p.status === 'transcribing') p.status = 'pending'; }); /* crash recovery */
    imgCache.clear();
    const sel = meta.ui && pages[meta.ui.selectedId] ? meta.ui.selectedId : meta.order[0] || null;
    S.set({ project: { slug, meta, pages }, selectedId: sel, multi: new Set(), view: 'inbox', layout: (meta.ui && meta.ui.layout) || 'split',
      run: Object.assign({}, S.RUN0), serverError: null, status: '' });
    S.reindex();
    S.emit('pages:structure');
    return true;
  };

  fs.closeProject = async () => {
    OCR.queue.stop();
    await fs.flushAll();
    S.set({ view: 'home', project: null, selectedId: null, multi: new Set() });
    await fs.refresh();
  };

  fs.saveMeta = () => {
    const P = S.d.project;
    if (!P) return;
    fs.schedule('meta', P.slug + '/project.json', () => { P.meta.updated = Date.now(); return JSON.stringify(P.meta, null, 2); }, { json: true });
  };

  fs.savePage = (id) => {
    const P = S.d.project, p = P && P.pages[id];
    if (!p) return;
    fs.schedule('page:' + id, P.slug + '/pages/' + id + '.json', () => JSON.stringify(p));
  };

  fs.saveImage = (id, blob, original) => {
    const P = S.d.project;
    imgCache.set(id, blob);
    fs.schedule('img:' + id, P.slug + '/images/' + id + '.jpg', () => blob, { delay: 0 });
    if (original) fs.schedule('orig:' + id, P.slug + '/originals/' + id + '.' + original.ext, () => original.file, { delay: 0 });
    while (imgCache.size > CACHE_MAX) imgCache.delete(imgCache.keys().next().value);
  };

  fs.loadImage = async (id) => {
    if (imgCache.has(id)) { const b = imgCache.get(id); imgCache.delete(id); imgCache.set(id, b); return b; }
    const P = S.d.project;
    const b = await fs.readBlob(P.slug + '/images/' + id + '.jpg');
    if (b) { imgCache.set(id, b); while (imgCache.size > CACHE_MAX) imgCache.delete(imgCache.keys().next().value); }
    return b;
  };

  fs.deletePage = async (id) => {
    const P = S.d.project;
    ['page:' + id, 'img:' + id, 'orig:' + id].forEach(fs.cancel);
    imgCache.delete(id);
    try {
      await fs.remove(P.slug + '/pages/' + id + '.json');
      await fs.remove(P.slug + '/images/' + id + '.jpg');
      for (const e of await fs.list(P.slug + '/originals')) if (e.name.startsWith(id + '.')) await fs.remove(P.slug + '/originals/' + e.name);
    } catch (e) { console.warn('[fs] delete page', e); }
  };

  fs.renameProject = async (slug, name) => {
    const P = S.d.project;
    if (P && P.slug === slug) { S.updateMeta({ name }); return; }
    const m = await readMeta(slug, true);
    if (!m) return;
    m.name = name; m.updated = Date.now();
    await fs.write(slug + '/project.json', JSON.stringify(m, null, 2));
  };

  async function copyDir(src, dst) {
    for (const e of await fs.list(src)) {
      if (e.kind === 'directory') await copyDir(src + '/' + e.name, dst + '/' + e.name);
      else { const b = await fs.readBlob(src + '/' + e.name); if (b) await fs.write(dst + '/' + e.name, b); }
    }
  }
  fs.duplicateProject = async (slug) => {
    const m = await readMeta(slug, true);
    if (!m) return null;
    const dst = await uniqueSlug(slug + '-copy');
    await copyDir(slug, dst);
    m.name = m.name + ' copy'; m.created = m.updated = Date.now();
    await fs.write(dst + '/project.json', JSON.stringify(m, null, 2));
    return dst;
  };

  fs.deleteProject = (slug) => fs.remove(slug);

  fs.writeExport = async (name, blob) => {
    const P = S.d.project;
    if (!P || !fs.hasBackend()) return false;
    try { await fs.write(P.slug + '/exports/' + name, blob); return true; }
    catch (e) { console.warn('[fs] export copy failed', e); return false; }
  };

  /* ---- move a project between modes: export a project zip, import a zip or folder ---- */
  async function walk(dir, out, skip) {
    for (const e of await fs.list(dir)) {
      const p = dir + '/' + e.name;
      if (e.kind === 'directory') { if (p !== skip) await walk(p, out, skip); } else out.push(p);
    }
  }
  fs.exportProjectZip = async (slug) => {
    const paths = [], files = {};
    await walk(slug, paths, slug + '/exports');
    for (const p of paths) { const b = await fs.readBlob(p); if (b) files[p] = new Uint8Array(await b.arrayBuffer()); }
    return OCR.export.makeZip(files);
  };

  fs.importFiles = async (fileList) => {
    const files = Array.from(fileList);
    let entries = [];
    if (files.length === 1 && /\.zip$/i.test(files[0].name)) {
      if (!window.fflate) throw new Error('The zip library did not load, so zip import is unavailable. Pick a project folder instead.');
      const u = window.fflate.unzipSync(new Uint8Array(await files[0].arrayBuffer()));
      entries = Object.keys(u).filter((p) => !p.endsWith('/')).map((path) => ({ path, bytes: u[path] }));
    } else {
      for (const f of files) entries.push({ path: f.webkitRelativePath || f.name, bytes: new Uint8Array(await f.arrayBuffer()) });
    }
    const pj = entries.filter((e) => /(^|\/)project\.json$/.test(e.path)).sort((a, b) => a.path.length - b.path.length)[0];
    if (!pj) throw new Error('No project.json found. Pick a project folder or a project zip.');
    let meta;
    try { meta = JSON.parse(new TextDecoder().decode(pj.bytes)); } catch (e) { throw new Error('project.json in that selection is damaged.'); }
    const base = pj.path.slice(0, pj.path.length - 'project.json'.length);
    const slug = await uniqueSlug(OCR.slug(meta.name || 'imported') || 'imported');
    for (const e of entries) {
      if (!e.path.startsWith(base)) continue;
      const rel = e.path.slice(base.length);
      if (!rel || rel.indexOf('exports/') === 0) continue;
      await fs.write(slug + '/' + rel, e.bytes);
    }
    return slug;
  };
})();
