/* Bootstrap. Loaded last. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, U = OCR.ui, $ = OCR.$;

  async function start() {
    S.loadSettings();
    OCR.words.autoGuess();
    OCR.models.migrate();
    ['banner', 'header', 'home', 'inbox', 'add', 'upload', 'review'].forEach((k) => U[k].init());

    const app = $('#app'), project = $('#project');
    const layout = () => {
      const P = S.d.project;
      app.dataset.view = S.d.view;
      project.dataset.layout = S.d.layout === 'text' ? 'text' : 'split';
      document.title = S.d.view !== 'home' && P ? P.meta.name + ' | viSHion' : 'viSHion';
    };
    ['view', 'project', 'layout'].forEach((k) => S.on('change:' + k, layout));
    S.on('meta:update', layout);
    layout();

    try { await OCR.fs.init(); await OCR.fs.refresh(); }
    catch (e) { console.error(e); OCR.toast('Storage could not start: ' + (e.message || e.name), { kind: 'error' }); }
    document.documentElement.dataset.ready = '1';
  }

  start();
})();
