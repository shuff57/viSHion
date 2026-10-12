/* Export dialog: everyone, or only students who are ready to read. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, G = OCR.groups;

  /* Page copies named after the student, so files and labels say who wrote them. Order is within the student. */
  const flatten = (groups) => [].concat(...groups.map((g) => g.pages.map((p, i) => Object.assign({}, p, {
    order: i,
    name: g.pages.length > 1 ? g.name + ' page ' + (i + 1) : g.name,
    student: Object.assign({}, p.student, { name: g.name })
  }))));

  U.openExport = (opt) => {
    const groups = G.list();
    if (!groups.length) { OCR.toast('Add pages first, then export them.', { kind: 'warn' }); return; }
    const s = S.d.settings;
    const chosenIds = S.d.multi, hasSel = chosenIds.size > 0;
    const o = { scope: opt && opt.scope === 'selected' && hasSel ? 'selected' : 'all', mode: s.exportText, sep: s.exportSeparator, fmt: s.exportFormat };
    const ready = groups.filter((g) => g.status === 'ready');
    const nPages = groups.reduce((n, g) => n + g.pages.length, 0);
    const only = (g) => Object.assign({}, g, { pages: g.pages.filter((p) => chosenIds.has(p.id)) });
    const chosen = () => flatten(o.scope === 'ready' ? ready : o.scope === 'selected' ? groups.map(only).filter((g) => g.pages.length) : groups);
    const summary = h('p', { class: 'sum', role: 'status' });

    const radio = (value, label, disabled) => h('label', null,
      h('input', { type: 'radio', name: 'exp-scope', value, checked: o.scope === value, disabled: disabled || null, onchange: () => { o.scope = value; refresh(); } }), h('span', { text: label }));
    const sel = (key, label, opts) => {
      const id = 'ex-' + key;
      const el = h('select', { class: 'select', id }, opts.map(([v, l]) => h('option', { value: v, text: l, selected: o[key] === v })));
      el.addEventListener('change', () => { o[key] = el.value; S.saveSettings({ [{ mode: 'exportText', sep: 'exportSeparator', fmt: 'exportFormat' }[key]]: el.value }); });
      return h('div', { class: 'field' }, h('label', { for: id, text: label }), el);
    };

    function refresh() {
      const list = chosen(), gaps = list.filter((p) => p.status !== 'done').length;
      summary.className = 'sum' + (gaps ? ' warn' : '');
      summary.textContent = !list.length ? 'No students are ready to read yet.'
        : gaps ? gaps + ' of ' + list.length + (list.length === 1 ? ' page has' : ' pages have') + ' no transcript yet. They will be empty in the export.'
          : 'All ' + list.length + (list.length === 1 ? ' page is' : ' pages are') + ' transcribed.';
    }

    async function run(kind) {
      const list = chosen(), gaps = list.filter((p) => p.status !== 'done').length;
      if (!list.length) { OCR.toast('Nothing to export for that choice.', { kind: 'warn' }); return; }
      if (gaps && !(await U.confirm(gaps + ' of ' + list.length + ' pages have no transcript. Export anyway?', { okLabel: 'Export anyway', title: 'Some pages are empty' }))) return;
      try { await OCR.export.run(kind, list, o); } catch (e) { OCR.toast('Export failed: ' + (e.message || e.name), { kind: 'error' }); }
    }
    const act = (kind, ic, title, sub) => h('button', { class: 'act', type: 'button', onclick: () => run(kind) },
      h('span', { html: U.icon(ic, 22) }), h('span', null, h('b', { text: title }), h('span', { text: sub })));

    const m = U.modal({
      title: 'Export transcripts',
      body: [
        h('fieldset', { style: 'border:0;padding:0;min-width:0' }, h('legend', { class: 'label', style: 'margin-bottom:8px;padding:0', text: 'Who to include' }),
          h('div', { class: 'scope' },
            radio('all', 'Everyone (' + groups.length + (groups.length === 1 ? ' student, ' : ' students, ') + nPages + (nPages === 1 ? ' page)' : ' pages)')),
            radio('ready', 'Only students who are ready to read (' + ready.length + ')', !ready.length),
            radio('selected', 'Only the pages I selected (' + chosenIds.size + ')', !hasSel))),
        h('div', { class: 'cols' },
          sel('mode', 'Text', [['corrected', 'Corrected'], ['raw', 'Original'], ['both', 'Both']]),
          sel('sep', 'Between pages', [['label', 'A “Page N” line'], ['break', 'A page break'], ['none', 'Nothing']]),
          sel('fmt', 'Combined file', [['docx', 'Word (.docx)'], ['md', 'Markdown (.md)'], ['txt', 'Plain text (.txt)']])),
        summary,
        h('div', { class: 'acts' }, h('span', { class: 'label', text: 'Or export just one kind of file' }),
          act('combined', 'file', 'Combined file', 'Every student in one document'),
          act('pages', 'files', 'One file per page (.zip)', 'A separate file for each page, named by student'),
          act('csv', 'table', 'CSV index', 'A spreadsheet of every page and its status'),
          act('json', 'braces', 'JSON', 'All the data, for other tools'),
          act('copy', 'copy', 'Copy all text', 'Puts the text on your clipboard')),
        h('p', { class: 'hint', text: 'When a folder is connected, each export is also saved in the assignment’s exports folder.' })],
      footer: [
        h('button', { class: 'btn', type: 'button', text: 'Cancel', onclick: () => m.close() }),
        h('button', { class: 'btn primary', type: 'button', onclick: () => run('all') },
          h('span', { html: U.icon('download', 20) }), 'Download all (.zip)')]
    });
    refresh();
  };
})();
