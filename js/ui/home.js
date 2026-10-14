/* Home view: where work is saved, and the list of assignments (engine word: projects). */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, fs = OCR.fs, ic = U.icon;
  let root;

  const btn = (cls, label, fn) => h('button', { class: 'btn ' + cls, type: 'button', text: label, onclick: fn });

  async function open(slug) {
    if (!(await fs.openProject(slug))) await fs.refresh();
  }

  async function create() {
    const name = await U.prompt('New assignment', 'Assignment name', '', 'Create assignment');
    if (!name) return;
    try { open(await fs.createProject(name)); } catch (e) { OCR.toast('Could not create the assignment: ' + (e.message || e.name), { kind: 'error' }); }
  }

  function importer(folder) {
    const inp = h('input', { type: 'file', hidden: true });
    if (folder) inp.setAttribute('webkitdirectory', ''); else inp.accept = '.zip,application/zip';
    inp.addEventListener('change', async () => {
      if (!inp.files.length) return;
      try { const slug = await fs.importFiles(inp.files); await fs.refresh(); OCR.toast('Assignment imported.'); open(slug); }
      catch (e) { OCR.toast(e.message || 'Import failed.', { kind: 'error' }); }
    });
    document.body.append(inp);
    return () => { inp.value = ''; inp.click(); };
  }
  const pickZip = importer(false), pickFolder = importer(true);

  async function importLegacy() {
    const n = fs.legacyCount();
    if (!(await U.confirm('Bring ' + n + (n === 1 ? ' page' : ' pages') + ' saved by the old single-file app into a new assignment? The old data stays where it is.', { okLabel: 'Import', title: 'Import old sessions' }))) return;
    try {
      const r = await fs.importLegacy();
      await fs.refresh();
      OCR.toast('Imported ' + r.pages + (r.pages === 1 ? ' page' : ' pages') + (r.withImage < r.pages ? ' (' + (r.pages - r.withImage) + ' without an image)' : '') + '.');
      open(r.slug);
    } catch (e) { OCR.toast(e.message || 'Import failed.', { kind: 'error' }); }
  }

  function saveCard() {
    const f = S.d.folder;
    const card = (cls, icon, title, text, buttons) => h('section', { class: 'save-card ' + cls },
      h('span', { html: ic(icon, 28) }), h('div', { class: 'txt' }, h('h2', { text: title }), h('p', { text })), h('div', { class: 'row' }, buttons));
    if (f.mode === 'none') {
      return card('', 'folder', 'Choose where assignments are saved', 'Pick a folder on this computer. Each assignment gets its own subfolder with pages, images and exports.',
        [btn('primary', 'Choose a folder', () => fs.chooseFolder()), btn('', 'Use browser storage instead', () => fs.useBrowserStorage())]);
    }
    if (f.permission === 'prompt') {
      return card('warn', 'folder', 'Reconnect “' + f.name + '”', 'The browser needs your permission again before it can open this folder. Nothing was lost.',
        [btn('primary', 'Reconnect folder', () => fs.reconnect()), btn('', 'Choose a different folder', () => fs.chooseFolder())]);
    }
    if (f.mode === 'idb') {
      return card('', 'db', 'Saving in this browser',
        fs.supported ? 'Work lives in this browser only. Use Export to keep a copy, or choose a folder to save to disk.'
          : 'This browser cannot pick folders, so work lives in this browser only. Use Export to keep a copy. Chrome and Edge can save to a folder.',
        fs.supported ? [btn('', 'Choose a folder instead', () => fs.chooseFolder())] : []);
    }
    return card('', 'folder', 'Saving to ' + f.name, 'Changes are saved automatically a moment after you make them.',
      [btn('', 'Change folder', () => fs.chooseFolder())]);
  }

  function card(p, i) {
    const pct = p.pages ? Math.round((100 * p.done) / p.pages) : 0;
    const act = (icon, title, fn, extra) => h('button', { class: 'btn ghost icon', type: 'button', 'aria-label': title + ' ' + p.name, title, html: ic(icon), onclick: fn, style: extra || null });
    return h('article', { class: 'proj', style: '--i:' + i },
      h('button', { class: 'proj-open', type: 'button', onclick: () => open(p.slug) },
        h('span', { class: 'proj-name', text: p.name }),
        h('span', { class: 'proj-meta', text: p.pages + (p.pages === 1 ? ' page' : ' pages') + ', ' + p.done + ' transcribed' }),
        h('span', { class: 'bar', 'aria-hidden': 'true' }, h('i', { style: 'width:' + pct + '%' })),
        h('span', { class: 'proj-when', text: 'Updated ' + OCR.fmtAgo(p.updated) })),
      h('div', { class: 'proj-actions' },
        act('pencil', 'Rename', async () => { const n = await U.prompt('Rename assignment', 'Assignment name', p.name); if (n) { await fs.renameProject(p.slug, n); fs.refresh(); } }),
        act('copy', 'Duplicate', async () => { try { await fs.duplicateProject(p.slug); await fs.refresh(); OCR.toast('Assignment duplicated.'); } catch (e) { OCR.toast('Could not duplicate: ' + e.message, { kind: 'error' }); } }),
        act('download', 'Download zip of', async () => { try { OCR.export.download(p.slug + '.zip', await fs.exportProjectZip(p.slug)); } catch (e) { OCR.toast('Could not build the zip: ' + e.message, { kind: 'error' }); } }),
        act('trash', 'Delete', async () => {
          if (await U.confirm('Delete “' + p.name + '” and all of its pages? This cannot be undone.', { danger: true, okLabel: 'Delete assignment', title: 'Delete assignment' })) { await fs.deleteProject(p.slug); fs.refresh(); }
        }, 'color:var(--bad)')));
  }

  function render() {
    root.textContent = '';
    const f = S.d.folder, ready = f.permission === 'granted' && f.mode !== 'none';
    const inner = h('div', { class: 'page-inner' },
      h('h1', { class: 'page-title', text: TITLE }),
      h('p', { class: 'page-sub', text: 'Turn stacks of handwritten pages into typed text. ' + (OCR.models.isCloud() ? 'Pages are sent to ' + OCR.models.name() + ' to be read.' : 'Everything runs on this computer.') }),
      saveCard());
    if (ready) {
      inner.append(h('div', { class: 'section-head' }, h('h2', { class: 'section-title', text: 'Assignments' }),
        h('div', { class: 'row' }, fs.legacyAvailable() ? btn('ghost', 'Import old sessions', importLegacy) : null,
          btn('ghost', 'Import zip', pickZip), btn('ghost', 'Import folder', pickFolder), btn('primary', 'New assignment', create))));
      const list = S.d.projects;
      if (list.length) inner.append(h('div', { class: 'proj-grid' }, list.map(card)));
      else inner.append(h('div', { class: 'empty', style: 'margin-top:16px' }, h('span', { html: ic('page', 40) }),
        h('h3', { text: 'No assignments yet' }), h('p', { text: 'An assignment holds one stack of student pages. Create one, then drop in your scans.' }),
        btn('primary', 'Create your first assignment', create)));
    }
    root.append(inner);
  }

  const TITLE = 'viSHion';
  U.home = { init() { root = document.getElementById('home'); ['folder', 'projects'].forEach((k) => S.on('change:' + k, render)); S.on('settings', render); render(); } };
})();
