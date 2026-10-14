/* Add pages: a staging step in front of the add path. Files are checked, shrunk, shown with the
   student name read from the top of each page, and only then saved and queued. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, Q = OCR.queue, G = OCR.groups;
  let root, staged = [], skipped = [], preset = null, nameDown = false, chain = Promise.resolve(), gen = 0, seq = 0;

  const plural = (n, one, many) => n + ' ' + (n === 1 ? one : many);
  const stem = (n) => String(n || '').replace(/\.[a-z0-9]+$/i, '');

  /* ---------- staging ---------- */
  function reset(opt) {
    gen++;
    staged.forEach((s) => { if (s.thumb) URL.revokeObjectURL(s.thumb); });
    staged = []; skipped = []; nameDown = false;
    preset = opt && opt.student ? opt.student : null;
    if (S.d.view === 'add') render();
  }

  /* Applies this photo's own choices (turn, straighten) to the prepared original, then finds the lines on it. */
  async function fit(s) {
    let b = s.orig;
    if (s.turn) b = (await OCR.image.rotate(b, s.turn)).blob;
    s.angle = 0;
    if (s.straighten) { const r = await OCR.image.deskew(b); b = r.blob; s.angle = r.angle; }
    s.blob = b;
    try { s.bands = await OCR.image.lineBands(b); } catch (e) { s.bands = []; }
    try { if (s.thumb) URL.revokeObjectURL(s.thumb); s.thumb = await OCR.image.thumb(b, 104); } catch (e) { s.thumb = null; }
  }

  /* Full-size look at one photo with the lines the scan found, before anything is sent to the model. */
  function preview(s) {
    const img = h('img', { alt: 'The photo, after straightening' }), ov = h('div', { class: 'overlay', 'aria-hidden': 'true' });
    const sheet = h('div', { class: 'scan-sheet', style: 'max-width:520px' }, img, ov);
    const summary = h('p', { class: 'hint', role: 'status' });
    let url = null;
    const draw = () => {
      if (url) URL.revokeObjectURL(url);
      url = URL.createObjectURL(s.blob); img.src = url;
      ov.textContent = '';
      const strengths = (s.bands || []).map((b) => b.s || 0).sort((a, c) => a - c), median = strengths.length ? strengths[Math.floor(strengths.length / 2)] : 0;
      const isWeak = (b) => (s.bands || []).length >= 4 && (b.s || 0) < median * 0.3; /* much fainter than the rest: probably an edge, not writing */
      (s.bands || []).forEach((b, i) => {
        const d = document.createElement('div');
        d.className = 'box preview-box' + (isWeak(b) ? ' weak' : ''); d.dataset.n = String(i + 1);
        d.style.cssText = 'left:' + b.x * 100 + '%;top:' + b.y * 100 + '%;width:' + b.w * 100 + '%;height:' + b.h * 100 + '%';
        ov.append(d);
      });
      const n = (s.bands || []).length, weak = (s.bands || []).filter(isWeak).length;
      summary.textContent = (s.angle ? 'Straightened by ' + Math.abs(s.angle) + '°. ' : s.straighten ? 'The photo is already level. ' : 'Straightening is off for this photo. ')
        + (n ? n + (n === 1 ? ' line' : ' lines') + ' found' + (weak ? ' (' + weak + ' faint, shown dashed, and probably not writing)' : '') + '. Each numbered box is a line of writing.' : 'No lines were found, so line boxes will be left off for this page.');
    };
    const redo = async () => { summary.textContent = 'Working…'; await fit(s); draw(); render(); };
    const level = h('input', { type: 'checkbox', id: 'pv-level', checked: !!s.straighten });
    level.addEventListener('change', () => { s.straighten = level.checked; redo(); });
    const m = U.modal({ title: s.fileName,
      body: [h('div', { style: 'display:flex;justify-content:center;background:var(--scanbg);padding:16px;border-radius:8px' }, sheet),
        h('label', { class: 'checkrow', for: 'pv-level' }, level, h('span', null, h('b', { text: 'Straighten this photo' }), h('span', { text: 'Turns the photo so the writing runs level.' }))),
        h('div', { class: 'row' }, h('button', { class: 'btn', type: 'button', onclick: () => { s.turn = ((s.turn || 0) + 90) % 360; redo(); } },
          h('span', { html: U.icon('rotate', 20) }), 'Turn 90°')),
        summary],
      footer: [h('button', { class: 'btn primary', type: 'button', text: 'Done', onclick: () => m.close() })],
      onClose: () => { if (url) URL.revokeObjectURL(url); } });
    draw();
  }

  async function process(s, mine) {
    try {
      s.orig = (await OCR.image.prepare(s.file)).blob;
      if (mine !== gen) return;
      await fit(s);
      if (mine !== gen) return;
      const blob = s.blob;
      if (preset) { s.name = preset; s.source = 'chosen'; }
      else if (S.d.settings.readNames !== false) {
        s.state = 'reading'; render();
        const r = await OCR.student.read(blob, { hint: s.file.name });
        if (mine !== gen) return;
        if (r.error) nameDown = true;
        if (r.name) { s.name = r.name; s.source = 'read'; }
      }
      s.state = 'ready';
    } catch (e) {
      s.state = 'error'; s.problem = 'Could not read this image.';
    }
    if (mine === gen) render();
  }

  function addFiles(files) {
    if (!S.d.project) return;
    const good = [];
    files.forEach((f) => {
      const why = OCR.image.check(f);
      if (why) skipped.push({ name: f.name || 'Pasted image', why }); else good.push(f);
    });
    good.sort((a, b) => OCR.natCompare(a._path || a.name, b._path || b.name)); /* page2 before page10 */
    const mine = gen;
    good.forEach((f) => {
      const name = f.name && f.name !== 'image.png' ? f.name : 'pasted-' + Date.now().toString(36) + '-' + seq++ + '.png';
      const s = { id: OCR.uid(), file: f, fileName: name, state: 'prep', name: null, source: null, blob: null, thumb: null, orig: null, bands: [], angle: 0, turn: 0, straighten: S.d.settings.straighten !== false };
      staged.push(s);
      chain = chain.then(() => (mine === gen ? process(s, mine) : null));
    });
    render();
  }

  function setName(s, name) {
    s.name = name || null;
    s.source = name ? 'chosen' : null;
    render();
  }

  /* ---------- commit ---------- */
  function startWhenFree(ids) {
    if (!Q.isRunning()) { Q.start(ids); return; }
    OCR.toast('Added ' + plural(ids.length, 'page', 'pages') + '. They will be transcribed when the current run finishes.');
    const off = S.on('change:run', () => { if (!S.d.run.active) { off(); Q.start(ids); } });
  }

  function commit() {
    const ready = staged.filter((s) => s.state === 'ready');
    if (!ready.length) return;
    const keep = S.d.settings.keepOriginals;
    const list = ready.map((s) => {
      const p = S.newPage(s.fileName);
      p.student = s.name ? { name: s.name, source: s.source, confirmed: s.source !== 'read' }
        : { name: stem(s.fileName), source: 'file', confirmed: false };
      OCR.fs.saveImage(p.id, s.blob, keep ? { file: s.file, ext: ((s.file.name || '').split('.').pop() || 'img').toLowerCase() } : null);
      return p;
    });
    S.addPages(list);
    const ids = list.map((p) => p.id);
    gen++; staged.forEach((s) => { if (s.thumb) URL.revokeObjectURL(s.thumb); }); staged = []; skipped = [];
    U.nav.inbox();
    startWhenFree(ids);
  }

  /* ---------- view ---------- */
  function studentOptions(current) {
    const names = new Set(G.names());
    staged.forEach((s) => { if (s.name) names.add(s.name); });
    if (current) names.add(current);
    return Array.from(names).sort(OCR.natCompare);
  }

  function row(s) {
    const busy = s.state === 'prep' || s.state === 'reading';
    const img = h('img', { alt: '', src: s.thumb || null });
    const found = s.state === 'ready' || s.blob ? (s.bands || []).length : 0;
    const sub = s.blob ? (found ? found + (found === 1 ? ' line found' : ' lines found') : 'No lines found') + (s.angle ? ', straightened ' + Math.abs(s.angle) + '°' : '') : '';
    const look = s.blob ? h('button', { class: 'sthumb', type: 'button', 'aria-label': 'Look at ' + s.fileName + ' and its lines', title: 'Look at this photo and its lines', onclick: () => preview(s) }, s.thumb ? img : null)
      : h('span', { class: 'sthumb', 'aria-hidden': 'true' }, s.thumb ? img : null);
    const parts = [look, h('div', { class: 'file' }, h('span', { text: s.fileName }), sub ? h('span', { style: 'display:block;font-weight:400;font-size:13px;color:var(--muted)', text: sub }) : null)];
    if (s.state === 'error') parts.push(h('div', { class: 'nm' }, h('b', { text: 'Can’t use this file' }), h('span', { text: s.problem })));
    else if (busy) parts.push(h('div', { class: 'nm' }, h('span', { text: s.state === 'reading' ? 'Reading the name…' : 'Getting the page ready…' })));
    else if (s.name) {
      parts.push(h('div', { class: 'nm' }, h('b', { text: s.name }),
        h('span', { text: s.source === 'read' ? 'Read from the top of the page' : 'Chosen by you' })));
      parts.push(h('button', { class: 'btn ghost', type: 'button', text: 'Change', 'aria-label': 'Change the student for ' + s.fileName, onclick: () => setName(s, null) }));
    } else {
      const sel = h('select', { class: 'select', 'aria-label': 'Choose a student for ' + s.fileName },
        h('option', { value: '', text: 'Choose a student' }),
        studentOptions(null).map((n) => h('option', { value: n, text: n })),
        h('option', { value: '__new', text: 'A new student…' }));
      sel.addEventListener('change', async () => {
        if (sel.value === '__new') {
          const n = await U.prompt('New student', 'Student name', '', 'Add student');
          if (n) setName(s, n); else sel.value = '';
        } else if (sel.value) setName(s, sel.value);
      });
      parts.push(h('div', { class: 'pick' }, h('span', { class: 'pill check', text: 'Name not found' }), sel));
    }
    parts.push(h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': 'Remove ' + s.fileName, html: U.icon('trash', 20),
      onclick: () => { if (s.thumb) URL.revokeObjectURL(s.thumb); staged = staged.filter((x) => x !== s); render(); } }));
    return h('div', { class: 'staged-row' }, parts);
  }

  function render() {
    if (S.d.view !== 'add' || !S.d.project) return;
    const P = S.d.project, topics = S.d.settings.topics || {};
    const ae = document.activeElement, fk = ae && root.contains(ae) && ae.id ? ae.id : null;
    root.textContent = '';
    const inner = h('div', { class: 'page-inner' }, h('h1', { class: 'page-title', text: preset ? 'Add pages for ' + preset : 'Add pages' }));
    const main = h('div', { class: 'add-main' });

    main.append(h('div', { class: 'dropzone' },
      h('span', { html: U.icon('upload', 40) }),
      h('div', null, h('div', { class: 'big', text: 'Drop photos or scans here' }),
        h('p', { text: 'PNG, JPG, or WebP, up to 40 MB each. Folders work too, and you can paste from the clipboard.' })),
      h('button', { class: 'btn primary', type: 'button', text: 'Choose files', onclick: () => U.upload.pick() }),
      h('p', { class: 'tip', text: 'iPhone photos: in Settings, Camera, Formats, choose Most Compatible so pages save as JPG.' })));

    if (staged.length || skipped.length) {
      main.append(h('div', { class: 'section-head' }, h('h2', { class: 'section-title', text: 'Ready to transcribe' }),
        h('span', { style: 'color:var(--muted)', text: plural(staged.length, 'page', 'pages') })));
    }
    if (skipped.length) {
      const names = skipped.slice(0, 3).map((x) => x.name);
      const more = skipped.length > 3 ? ' and ' + (skipped.length - 3) + ' more' : '';
      const why = Array.from(new Set(skipped.map((x) => x.why)));
      main.append(h('div', { class: 'notice', role: 'status', style: 'margin-top:14px' }, h('span', { html: U.icon('warn', 22) }),
        h('div', null, h('b', { text: plural(skipped.length, 'file was', 'files were') + ' skipped' }),
          h('span', { text: names.join(', ') + more + ' can’t be added. ' + why.join(' ') }))));
    }
    if (nameDown) {
      main.append(h('div', { class: 'notice', role: 'status', style: 'margin-top:14px' }, h('span', { html: U.icon('warn', 22) }),
        h('div', null, h('b', { text: 'Names could not be read' }),
          h('span', { text: 'The transcription server did not answer, so choose a student for each page, or start the server and add the pages again.' }))));
    }
    if (staged.length) main.append(h('div', { class: 'staged' }, staged.map(row)));

    const ready = staged.filter((s) => s.state === 'ready'), working = staged.some((s) => s.state === 'prep' || s.state === 'reading');
    const missing = ready.filter((s) => !s.name).length;
    const sub = h('select', { class: 'select', id: 'add-topic' }, Object.keys(topics).map((k) => h('option', { value: k, text: k, selected: k === P.meta.topic })));
    sub.addEventListener('change', () => S.updateMeta({ topic: sub.value }));
    const rn = h('input', { type: 'checkbox', id: 'read-names', checked: S.d.settings.readNames !== false });
    rn.addEventListener('change', () => S.saveSettings({ readNames: rn.checked }));
    const cta = h('button', { class: 'btn primary wide', type: 'button', disabled: !ready.length || working || null, onclick: commit,
      text: working ? 'Getting pages ready…' : 'Transcribe ' + plural(ready.length, 'page', 'pages') });

    root.append(inner);
    inner.append(h('div', { class: 'add-grid' }, main,
      h('div', { class: 'add-side' },
        h('div', { class: 'field' }, h('span', { class: 'lbl', text: 'Adding to' }), h('span', { class: 'big', text: P.meta.name })),
        h('div', { class: 'field' }, h('label', { class: 'lbl', for: 'add-topic', text: 'Subject' }), sub,
          h('p', { class: 'hint', text: 'Used to catch course words that get misread. Math written on the page is kept as math either way.' })),
        h('label', { class: 'checkrow', for: 'read-names' }, rn,
          h('span', null, h('b', { text: 'Read the name at the top of each page' }), h('span', { text: 'You’ll pick the student for any page where a name can’t be found.' }))),
        h('div', { class: 'field', style: 'gap:10px' }, cta,
          h('p', { class: 'hint', role: 'status', text: !ready.length ? 'Add pages to get started.' : missing === 0 ? 'Every page has a student.'
            : plural(missing, 'page needs', 'pages need') + ' a student. You can choose one after transcribing.' })))));
    if (fk) { const el = document.getElementById(fk); if (el) el.focus(); }
  }

  U.add = {
    reset, addFiles,
    init() {
      root = document.getElementById('add');
      S.on('change:view', () => { if (S.d.view === 'add') render(); });
      S.on('settings', () => { if (S.d.view === 'add') render(); });
      S.on('change:project', () => reset());
    }
  };
})();
