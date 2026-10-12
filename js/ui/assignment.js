/* Assignment settings (name, subject, instructions) and the subjects-and-words editor. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui;
  const clone = (o) => JSON.parse(JSON.stringify(o));

  U.openTopics = () => {
    const topics = clone(S.d.settings.topics || S.DEFAULT_TOPICS);
    let cur = S.d.project ? S.d.project.meta.topic : Object.keys(topics)[0];
    if (!topics[cur]) cur = Object.keys(topics)[0];
    const sel = h('select', { class: 'select', id: 'gl-topic' });
    const desc = h('input', { class: 'input', id: 'gl-desc', type: 'text', autocomplete: 'off' });
    const terms = h('textarea', { class: 'textarea', id: 'gl-terms', rows: 9 });
    const save = () => S.saveSettings({ topics });
    const load = () => {
      sel.textContent = '';
      Object.keys(topics).forEach((k) => sel.append(h('option', { value: k, text: k, selected: k === cur })));
      desc.value = topics[cur].description || '';
      terms.value = topics[cur].terms.join('\n');
    };
    sel.addEventListener('change', () => { cur = sel.value; load(); });
    desc.addEventListener('change', () => { topics[cur].description = desc.value; save(); });
    terms.addEventListener('change', () => { topics[cur].terms = terms.value.split(/[\n,]+/).map((t) => t.trim()).filter(Boolean); save(); });
    const m = U.modal({
      title: 'Subjects and their words',
      body: [
        h('div', { class: 'field' }, h('label', { for: 'gl-topic', text: 'Subject' }), sel),
        h('div', { class: 'field' }, h('label', { for: 'gl-desc', text: 'Description' }), desc),
        h('div', { class: 'field' }, h('label', { for: 'gl-terms', text: 'Words students use in this subject' }), terms,
          h('p', { class: 'hint', text: 'One word or phrase per line, or separated by commas. These are added to the model’s instructions, and used to suggest corrections after a page is read.' }))],
      footer: [
        h('button', { class: 'btn ghost', type: 'button', text: 'Restore defaults', onclick: async () => {
          if (!(await U.confirm('Replace all subjects with the built-in set? Your edits will be lost.', { okLabel: 'Restore defaults' }))) return;
          Object.keys(topics).forEach((k) => delete topics[k]);
          Object.assign(topics, clone(S.DEFAULT_TOPICS)); cur = 'General'; save(); load();
        } }),
        h('span', { class: 'grow' }),
        h('button', { class: 'btn', type: 'button', text: 'Delete subject', onclick: async () => {
          if (cur === 'General') { OCR.toast('The General subject stays.', { kind: 'warn' }); return; }
          if (!(await U.confirm('Delete the subject “' + cur + '”?', { danger: true, okLabel: 'Delete subject' }))) return;
          delete topics[cur]; cur = 'General'; save(); load();
        } }),
        h('button', { class: 'btn', type: 'button', text: 'New subject', onclick: async () => {
          const n = await U.prompt('New subject', 'Subject name', '', 'Add subject');
          if (!n) return;
          if (topics[n]) { OCR.toast('A subject with that name already exists.', { kind: 'warn' }); return; }
          topics[n] = { description: '', terms: [] }; cur = n; save(); load();
        } }),
        h('button', { class: 'btn primary', type: 'button', text: 'Done', onclick: () => m.close() })],
      onClose: () => {
        const P = S.d.project;
        if (P && !topics[P.meta.topic]) S.updateMeta({ topic: 'General' });
        S.emit('settings', { topics });
      }
    });
    load();
  };

  U.openAssignmentSettings = () => {
    const P = S.d.project;
    if (!P) return;
    const m0 = P.meta;
    const name = h('input', { class: 'input', id: 'as-name', type: 'text', value: m0.name, autocomplete: 'off' });
    name.addEventListener('change', () => { const v = name.value.trim(); if (v) S.updateMeta({ name: v }); else name.value = S.d.project.meta.name; });
    const sel = h('select', { class: 'select', id: 'as-topic', style: 'flex:1 1 auto;min-width:0' });
    const fill = () => {
      sel.textContent = '';
      Object.keys(S.d.settings.topics || {}).forEach((k) => sel.append(h('option', { value: k, text: k, selected: k === S.d.project.meta.topic })));
    };
    fill();
    sel.addEventListener('change', () => S.updateMeta({ topic: sel.value }));
    const prompt = h('textarea', { class: 'textarea', id: 'as-prompt', rows: 9, placeholder: 'Leave this empty to use the standard instructions.', value: m0.prompt || '' });
    const state = h('p', { class: 'hint', text: m0.prompt ? 'Using your instructions.' : 'Using the standard instructions.' });
    prompt.addEventListener('input', OCR.debounce(() => { S.updateMeta({ prompt: prompt.value }); state.textContent = prompt.value.trim() ? 'Using your instructions.' : 'Using the standard instructions.'; }, 400));
    const off = S.on('settings', fill);
    const m = U.modal({
      title: 'Assignment settings',
      body: [
        h('div', { class: 'field' }, h('label', { for: 'as-name', text: 'Assignment name' }), name),
        h('div', { class: 'field' }, h('label', { for: 'as-topic', text: 'Subject' }),
          h('div', { class: 'row', style: 'flex-wrap:nowrap' }, sel,
            h('button', { class: 'btn icon', type: 'button', 'aria-label': 'Edit subjects and their words', title: 'Edit subjects and their words', html: U.icon('pencil'), onclick: () => U.openTopics() })),
          h('p', { class: 'hint', text: 'The subject’s words are added to the instructions, and used to suggest corrections after a page is read.' })),
        h('div', { class: 'field' }, h('label', { for: 'as-prompt', text: 'Instructions for the model' }), prompt,
          h('div', { class: 'row', style: 'justify-content:space-between' }, state,
            h('button', { class: 'btn ghost', type: 'button', text: 'Reset to standard', onclick: () => { prompt.value = ''; S.updateMeta({ prompt: '' }); state.textContent = 'Using the standard instructions.'; } })))],
      footer: [h('button', { class: 'btn primary', type: 'button', text: 'Done', onclick: () => m.close() })],
      onClose: off
    });
  };
})();
