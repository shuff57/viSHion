/* Settings dialog. Same fields as before, in plain words. Closes on Escape and backdrop click (see modal.js). */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui, fs = OCR.fs;

  const field = (label, control, hint, id) => h('div', { class: 'field' }, h('label', { for: id, text: label }), control, hint ? h('p', { class: 'hint', text: hint }) : null);
  const section = (title, ...kids) => h('section', { class: 'set-sec' }, h('h3', { text: title }), kids);
  const text = (id, key, type) => {
    const i = h('input', { class: 'input', id, type: type || 'text', value: S.d.settings[key] == null ? '' : S.d.settings[key], autocomplete: 'off', spellcheck: 'false', min: type === 'number' ? '10' : null });
    i.addEventListener('change', () => S.saveSettings({ [key]: type === 'number' ? Math.max(10, Number(i.value) || 180) : i.value.trim() }));
    return i;
  };
  const select = (id, key, opts, current) => {
    const cur = current == null ? S.d.settings[key] : current;
    const s = h('select', { class: 'select', id }, opts.map(([v, l]) => h('option', { value: v, text: l, selected: v === cur })));
    s.addEventListener('change', () => S.saveSettings({ [key]: s.value }));
    return s;
  };
  const check = (id, key, label, hint) => {
    const c = h('input', { type: 'checkbox', id, checked: !!S.d.settings[key] });
    c.addEventListener('change', () => S.saveSettings({ [key]: c.checked }));
    return h('label', { class: 'checkrow', for: id }, c, h('span', null, h('b', { text: label }), h('span', { text: hint })));
  };

  U.openSettings = () => {
    const s = S.d.settings;
    const theme = h('div', { class: 'seg', role: 'group', 'aria-labelledby': 'theme-label' }, [['auto', 'System'], ['light', 'Light'], ['dark', 'Dark']].map(([v, l]) =>
      h('button', { type: 'button', text: l, 'aria-pressed': s.theme === v ? 'true' : 'false', onclick: (e) => {
        S.saveSettings({ theme: v });
        e.target.parentNode.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', b === e.target ? 'true' : 'false'));
      } })));

    /* ---- which model: a local server, or an optional hosted endpoint ---- */
    const M = OCR.models;
    const put = (patch) => {
      const map = { ocrUrl: 'url', model: 'model', apiKey: 'key' }, mem = {};
      Object.keys(patch).forEach((k) => { if (map[k]) mem[map[k]] = k === 'ocrUrl' ? M.shownUrl(M.provider(), patch[k]) : patch[k]; });
      S.saveSettings(Object.assign({}, patch, { profiles: M.remember(mem) }));
    };
    const provSel = h('select', { class: 'select', id: 'st-provider' }, M.PROVIDERS.map((p) => h('option', { value: p.id, text: p.label })));
    const url = h('input', { class: 'input', id: 'st-ocr', type: 'text', autocomplete: 'off', spellcheck: 'false' });
    const key = h('input', { class: 'input', id: 'st-key', type: 'password', autocomplete: 'off' });
    const model = h('select', { class: 'select', id: 'st-model' });
    const custom = h('input', { class: 'input', id: 'st-model-custom', type: 'text', placeholder: 'Model name', autocomplete: 'off', spellcheck: 'false', hidden: true });
    let known = []; /* [{ id, label }] from Find models */
    const fillModels = (list) => {
      if (list) known = list;
      const cur = S.d.settings.model || '';
      model.textContent = '';
      model.append(h('option', { value: '', text: known.length ? 'Choose a model' : 'Whatever the server has loaded' }));
      known.forEach((m) => model.append(h('option', { value: m.id, text: m.label })));
      if (cur && !known.some((m) => m.id === cur)) model.append(h('option', { value: cur, text: M.pretty(cur) }));
      model.append(h('option', { value: '__other', text: 'Another model name…' }));
      model.value = cur;
      custom.hidden = true;
    };
    const result = h('span', { role: 'status' }), modelNote = h('span', { class: 'hint', role: 'status' });
    const cloudNote = h('p', { class: 'notice', style: 'display:block;margin:0' });
    const keyHint = h('p', { class: 'hint' });
    const refresh = () => {
      const st = S.d.settings, prov = M.provider();
      provSel.value = prov.id;
      url.value = M.shownUrl(prov, st.ocrUrl || ''); key.value = st.apiKey || ''; fillModels();
      url.placeholder = prov.id === 'custom' ? 'https://example.com' : '';
      cloudNote.hidden = !prov.cloud;
      const ub = document.getElementById('st-unload-box'); if (ub) ub.hidden = prov.cloud;
      const cb = document.getElementById('st-check-box'); if (cb) cb.hidden = !prov.cloud;
      cloudNote.textContent = 'Page images, and the student names read from them, are sent to ' + prov.name + ' over the internet. Use this only if that is allowed for your students’ work.';
      keyHint.textContent = prov.id === 'ollama-cloud' ? 'Create one at ollama.com/settings/keys. Requests go through tools/relay.js on this computer (run it once: node tools/relay.js). Kept in this browser only.' : prov.cloud ? 'Needed for this endpoint. Kept in this browser only, never written to a project folder or an export.'
        : 'Not needed for a server on this computer. Kept in this browser only, never written to a project folder or an export.';
      result.textContent = ''; modelNote.textContent = '';
    };
    provSel.addEventListener('change', async () => {
      const to = M.PROVIDERS.find((p) => p.id === provSel.value);
      if (to.cloud && !(await U.confirm('Pages and student names will be sent to ' + to.name + ' to be read. Switch anyway?', { okLabel: 'Use ' + to.name, title: 'Send pages to the internet?' }))) { refresh(); return; }
      known = []; M.switchTo(to.id); refresh();
    });
    url.addEventListener('change', () => put({ ocrUrl: M.engineUrl(M.provider(), url.value.trim()) }));
    key.addEventListener('change', () => put({ apiKey: key.value.trim() }));
    model.addEventListener('change', () => {
      if (model.value === '__other') { custom.hidden = false; custom.focus(); return; }
      custom.hidden = true; put({ model: model.value });
    });
    custom.addEventListener('change', () => { const v = custom.value.trim(); if (v) { put({ model: v }); fillModels(); } });
    const findModels = h('button', { class: 'btn', type: 'button', text: 'Find models', onclick: async () => {
      put({ ocrUrl: M.engineUrl(M.provider(), url.value.trim()), apiKey: key.value.trim() });
      modelNote.textContent = 'Looking…';
      try {
        const list = await M.list();
        fillModels(list);
        const seeing = list.filter((m) => m.vision === true).length;
        modelNote.textContent = list.length + (list.length === 1 ? ' model' : ' models') + ' found' + (seeing ? ', ' + seeing + ' that can see images.' : '.') + ' Pick one from the list.';
        if (!model.value && list.length === 1) { put({ model: list[0].id }); fillModels(); }
      } catch (e) { modelNote.textContent = e.message || 'Could not list models. You can still type a model name.'; }
    } });
    const checkNote = h('p', { class: 'hint', role: 'status' });
    const checkBtn = h('button', { class: 'btn', type: 'button', text: 'Check which models work', onclick: async () => {
      put({ ocrUrl: M.engineUrl(M.provider(), url.value.trim()), apiKey: key.value.trim() });
      checkBtn.disabled = true; checkNote.textContent = 'Listing models…';
      try {
        const all = await M.list({ all: true });
        if (!all.length) { checkNote.textContent = 'The endpoint listed no models that take images.'; return; }
        const res = await M.probe(all.map((m) => m.id), (d, n) => { checkNote.textContent = 'Checking ' + d + ' of ' + n + '…'; });
        if (!res.ok.length) { /* usually a missing or wrong key: keep the full list rather than an empty picker */
          checkNote.textContent = 'None of the ' + all.length + ' models answered' + (res.failed.some((f) => /401|403/.test(f.why)) ? ', and the endpoint refused the key. Check the API key above.' : '. Check the key and the connection.');
          return;
        }
        M.setWorking(res.ok);
        fillModels(await M.list());
        const bad = res.failed.slice(0, 4).map((f) => M.pretty(f.id) + ' (' + f.why + ')').join(', ');
        checkNote.textContent = res.ok.length + ' of ' + all.length + ' models answered with your key' + (res.ok.length === all.length ? '. All are listed.' : '. Only those are listed.') + (bad ? ' Left out: ' + bad + (res.failed.length > 4 ? ' and ' + (res.failed.length - 4) + ' more' : '') + '.' : '');
      } catch (e) { checkNote.textContent = e.message || 'The check could not run.'; }
      finally { checkBtn.disabled = false; }
    } });
    const showAll = h('button', { class: 'btn ghost', type: 'button', text: 'Show every model again', onclick: async () => {
      M.setWorking(null); fillModels(await M.list().catch(() => known)); checkNote.textContent = 'Showing every model that takes images.';
    } });
    const test = h('button', { class: 'btn', type: 'button', text: 'Test connection', onclick: async () => {
      result.style.cssText = ''; result.textContent = 'Checking…';
      put({ ocrUrl: M.engineUrl(M.provider(), url.value.trim()), apiKey: key.value.trim() });
      if (S.d.settings.demo) { result.textContent = 'Demo mode is on, so no server is needed.'; return; }
      try {
        const list = await M.list();
        result.textContent = 'Connected. ' + list.length + (list.length === 1 ? ' model' : ' models') + ' available.';
        result.style.cssText = 'color:var(--ok);font-weight:600';
      } catch (e) {
        try {
          const r = await fetch(url.value.trim().replace(/\/+$/, '') + '/health');
          result.textContent = r.ok ? 'Connected.' : 'The server answered with ' + r.status + '. The model may still be loading.';
          if (r.ok) result.style.cssText = 'color:var(--ok);font-weight:600';
        } catch (x) { result.textContent = e.message || 'Can’t reach the server.'; }
      }
    } });
    refresh();
    if (!M.isCloud()) M.list().then(fillModels).catch(() => { /* the button reports it */ }); /* a local server is cheap to ask */

    const idle = h('input', { class: 'input', id: 'st-idle', type: 'number', min: '0', style: 'width:140px', value: String(S.d.settings.idleUnloadMin == null ? 10 : S.d.settings.idleUnloadMin) });
    idle.addEventListener('change', () => S.saveSettings({ idleUnloadMin: Math.max(0, Math.round(Number(idle.value) || 0)) }));
    const unloadNote = h('span', { class: 'hint', role: 'status' });
    const unloadBtn = h('button', { class: 'btn', type: 'button', text: 'Unload now', onclick: async () => {
      unloadNote.textContent = 'Unloading…';
      try { const r = await M.unload(); unloadNote.textContent = r.count ? 'Unloaded ' + r.names.join(', ') + '.' : 'Nothing was loaded.'; }
      catch (e) { unloadNote.textContent = 'Could not unload: ' + (e.message || e); }
    } });
    const nRef = (S.d.settings.reference || []).length;
    const refCount = h('span', { style: 'color:var(--muted)', text: nRef + (nRef === 1 ? ' fix remembered.' : ' fixes remembered.') });
    const f = S.d.folder;
    const storage = h('div', { class: 'row' },
      h('span', { style: 'color:var(--muted)', text: f.mode === 'folder' ? 'Folder: ' + f.name : f.mode === 'idb' ? 'This browser only' : 'Not chosen yet' }),
      fs.supported ? h('button', { class: 'btn', type: 'button', text: f.mode === 'folder' ? 'Change folder' : 'Choose a folder', onclick: () => fs.chooseFolder() }) : null,
      f.mode === 'folder' && f.permission === 'prompt' ? h('button', { class: 'btn', type: 'button', text: 'Reconnect folder', onclick: () => fs.reconnect() }) : null,
      f.mode !== 'idb' ? h('button', { class: 'btn ghost', type: 'button', text: 'Use browser storage', onclick: () => { fs.useBrowserStorage(); } }) : null);

    const m = U.modal({
      title: 'Settings',
      body: [
        section('Appearance', h('div', { class: 'field' }, h('span', { class: 'label', id: 'theme-label', text: 'Theme' }), theme)),
        section('Model',
          field('Where the model runs', provSel, 'One model reads the pages, finds the student’s name and checks spelling.', 'st-provider'),
          cloudNote,
          field('Server address', url, 'A local llama.cpp or Ollama server, or the endpoint’s base address.', 'st-ocr'),
          field('API key', key, null, 'st-key'), keyHint,
          field('Model', model, 'Choose which model reads the pages. A local server swaps it in when you pick another.', 'st-model'),
          custom,
          h('div', { class: 'row' }, findModels, modelNote),
          h('div', { class: 'field', id: 'st-check-box', hidden: true },
            h('div', { class: 'row' }, checkBtn, showAll), checkNote,
            h('p', { class: 'hint', text: 'Sends one tiny picture to each model with your key and keeps only the ones that answer. Text-only models are never listed.' })),
          h('div', { class: 'row' }, test, result),
          h('div', { class: 'field', id: 'st-unload-box' },
            h('label', { for: 'st-idle', text: 'Unload the model after it has been idle for (minutes)' }), idle,
            h('p', { class: 'hint', text: 'Frees your graphics memory when you stop. 0 keeps it loaded. This only runs while this page is open.' }),
            h('div', { class: 'row' }, unloadBtn, unloadNote)),
          field('Give up on a page after (seconds)', text('st-timeout', 'timeoutSec', 'number'), 'A page that takes longer is marked as failed and the batch moves on.', 'st-timeout'),
          check('st-straighten', 'straighten', 'Straighten tilted pages', 'Turns a page that was photographed at a slant so the text runs level. This helps the line boxes.'),
          check('st-demo', 'demo', 'Demo mode', 'Uses sample text instead of a model, so you can try the app without a server.')),
        section('Spelling check',
          field('How to suggest', select('st-jmode', 'judgeMode', [['auto', 'Ask the model'], ['heuristic', 'Built-in matching only']], s.judgeMode === 'heuristic' ? 'heuristic' : 'auto'),
            'The same model that reads the page also checks spelling against the subject’s words. If it fails, built-in matching is used. A failure never blocks a transcript.', 'st-jmode')),
        section('Unreadable words', check('st-guess', 'guessIllegible', 'Suggest likely words', 'After a page is read, the model looks again at each word it could not read and offers its best guesses with a percentage. Those percentages are its own estimate. This adds one more request per page that has unreadable words.')),
        section('Remembered fixes',
          check('st-remember', 'rememberFixes', 'Remember my fixes for unreadable words', 'When you type a word for handwriting the model could not read, it is kept with the words around it. The model sees recent ones as hints when it reads or guesses later pages. This guides the model; it does not retrain it.'),
          h('div', { class: 'row' }, refCount, h('button', { class: 'btn', type: 'button', text: 'Clear remembered fixes', onclick: async () => {
            if (!(S.d.settings.reference || []).length) return;
            if (await U.confirm('Forget all remembered fixes? Transcripts and your choices on pages are not affected.', { okLabel: 'Forget them', title: 'Clear remembered fixes' })) { S.saveSettings({ reference: [] }); refCount.textContent = '0 fixes remembered.'; }
          } }))),
        section('Export defaults', h('div', { class: 'cols' },
          field('Combined file', select('st-fmt', 'exportFormat', [['docx', 'Word (.docx)'], ['md', 'Markdown (.md)'], ['txt', 'Plain text (.txt)']]), null, 'st-fmt'),
          field('Between pages', select('st-sep', 'exportSeparator', [['label', 'A “Page N” line'], ['break', 'A page break'], ['none', 'Nothing']]), null, 'st-sep'),
          field('Text to export', select('st-txt', 'exportText', [['corrected', 'Corrected'], ['raw', 'Original'], ['both', 'Both']]), null, 'st-txt'))),
        section('Storage', storage,
          check('st-orig', 'keepOriginals', 'Keep untouched originals', 'Saves each upload as it came, next to the resized copy. Uses more disk space.'))],
      footer: [h('button', { class: 'btn primary', type: 'button', text: 'Done', onclick: () => m.close() })]
    });
  };
})();
