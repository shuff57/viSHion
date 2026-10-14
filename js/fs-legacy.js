/* One-time import of sessions saved by the old single-file ocr.html (design/legacy-ocr.html).
   That app kept a JSON array under localStorage "ocr_sessions_v2" and each page image as a data URL
   in IndexedDB "ocr_images" / "images", keyed by session id. Both are only visible from the same
   browser origin, so this reads them in place. Adds one new assignment; nothing is deleted. */
(function () {
  'use strict';
  const OCR = window.OCR, fs = OCR.fs, S = OCR.state;
  const KEY = 'ocr_sessions_v2';

  function sessions() {
    try { const a = JSON.parse(localStorage.getItem(KEY) || '[]'); return Array.isArray(a) ? a.filter((s) => s && s.id && (s.lastRaw || s.preRaw || s.hasImage)) : []; }
    catch (e) { return []; }
  }
  fs.legacyAvailable = () => sessions().length > 0;
  fs.legacyCount = () => sessions().length;

  async function legacyImages() {
    const out = new Map();
    try {
      if (indexedDB.databases && !(await indexedDB.databases()).some((d) => d.name === 'ocr_images')) return out;
      const db = await new Promise((res, rej) => { const r = indexedDB.open('ocr_images'); r.onsuccess = () => res(r.result); r.onerror = () => rej(r.error); });
      if (!db.objectStoreNames.contains('images')) { db.close(); return out; }
      await new Promise((res) => {
        const store = db.transaction('images', 'readonly').objectStore('images'), req = store.openCursor();
        req.onsuccess = () => { const c = req.result; if (c) { out.set(String(c.key), c.value); c.continue(); } else res(); };
        req.onerror = () => res();
      });
      db.close();
    } catch (e) { /* no images: pages are imported as text only */ }
    return out;
  }

  /* Returns { slug, pages, withImage }. */
  fs.importLegacy = async (opt) => {
    opt = opt || {};
    const list = (opt.sessions || sessions()).slice().sort((a, b) => (a.created || 0) - (b.created || 0));
    if (!list.length) throw new Error('No sessions from the old app were found in this browser.');
    const images = opt.images ? new Map(Object.entries(opt.images)) : await legacyImages();

    const name = opt.name || 'Imported from the old app';
    const slug = await fs.createProject(name);
    const topics = S.d.settings.topics || {};
    const pick = list.map((s) => s.topic).filter((t) => t && topics[t])[0] || 'General';
    const prompt = (list.map((s) => (s.prompt || '').trim()).filter(Boolean)[0]) || '';
    const order = [];
    let withImage = 0, doneN = 0;

    for (const s of list) {
      const p = S.newPage((s.name || 'Imported page') + '');
      const raw = String(s.preRaw || s.lastRaw || ''), final = String(s.lastRaw || raw);
      Object.assign(p, {
        status: raw ? 'done' : 'pending', raw, corrected: final,
        corrections: (Array.isArray(s.appliedDiff) ? s.appliedDiff : []).filter((d) => d && d.orig && d.term)
          .map((d) => ({ word: String(d.orig), term: String(d.term), conf: 1, applied: true })),
        timings: { ms: 0, tokens: 0 }
      });
      p.corrections.forEach((c, i) => { p.resolved['c:' + i] = 'suggested'; }); /* the teacher already applied these */
      const url = images.get(String(s.id));
      let blob = null;
      if (typeof url === 'string' && url.indexOf('data:') === 0) {
        try {
          const raw0 = await (await fetch(url)).blob();
          blob = (await OCR.image.prepare(new File([raw0], p.name, { type: raw0.type || 'image/png' }))).blob;
        } catch (e) { blob = null; }
      }
      if (blob) {
        withImage++;
        await fs.write(slug + '/images/' + p.id + '.jpg', blob);
        if (raw) { try { p.boxes = await OCR.render.computeBoxes(blob, raw); } catch (e) { p.boxes = []; } }
      } else if (!raw) continue; /* nothing worth keeping */
      if (p.status === 'done') doneN++;
      p.order = order.length;
      order.push(p.id);
      await fs.write(slug + '/pages/' + p.id + '.json', JSON.stringify(p));
    }

    const now = Date.now();
    const meta = JSON.parse(await fs.read(slug + '/project.json'));
    Object.assign(meta, { order, topic: pick, prompt, updated: now, stats: { pages: order.length, done: doneN } });
    await fs.write(slug + '/project.json', JSON.stringify(meta, null, 2));
    return { slug, pages: order.length, withImage };
  };
})();
