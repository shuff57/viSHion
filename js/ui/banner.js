/* Alerts under the header: storage problems and transcription-server problems. Copy follows design/States.dc.html. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, h = OCR.h, U = OCR.ui;

  const item = (warn, title, text, label, fn) => h('div', { class: 'banner' + (warn ? ' warn' : ''), role: 'alert' },
    h('span', { html: U.icon('warn', 22) }),
    h('p', null, title ? h('b', { text: title + ' ' }) : null, text),
    h('button', { class: 'btn', type: 'button', text: label, onclick: fn }));

  function render() {
    const host = document.getElementById('banner');
    host.textContent = '';
    const b = S.d.banner, se = S.d.serverError, f = S.d.folder;
    if (b && b.kind === 'reconnect') host.append(item(true, 'Folder access was lost.', 'Your work is kept in this tab and will be saved once you reconnect.', 'Reconnect folder', () => OCR.fs.reconnect()));
    else if (b) host.append(item(false, 'Couldn’t save' + (f.name && f.mode === 'folder' ? ' to “' + f.name + '”.' : '.'),
      'Your changes are kept in this tab for now. Check that the folder still exists, then try again.', 'Try again', () => OCR.fs.retry()));
    if (se) {
      const url = (S.d.settings.ocrUrl || '').replace(/\/+$/, '');
      const text = /^Cannot reach OCR server/.test(se)
        ? 'Can’t reach the transcription server at ' + url + '. Start llama-server with CORS turned on, then retry.' : se;
      host.append(item(false, '', text, 'Retry', () => U.resumeAll()));
    }
  }

  /* Resume every page that still has no transcript, not only the last run's pages. */
  U.resumeAll = async () => {
    if (!(await OCR.ocr.ping())) { S.set({ serverError: OCR.ocr.unreachableMessage() }); return; }
    S.set({ serverError: null });
    OCR.queue.start();
  };

  U.banner = { init() { S.on('change:banner', render); S.on('change:serverError', render); S.on('change:folder', render); render(); } };
})();
