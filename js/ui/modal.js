/* Modal dialogs: closes on Escape and backdrop click, restores focus, keeps Tab inside. */
(function () {
  'use strict';
  const OCR = window.OCR, h = OCR.h, U = OCR.ui, stack = [];
  const FOCUSABLE = 'a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

  U.modal = (o) => {
    const prev = document.activeElement, titleId = 'm' + OCR.uid();
    const back = h('div', { class: 'backdrop' });
    const head = h('div', { class: 'modal-head' },
      h('h2', { id: titleId, text: o.title || '' }),
      h('button', { class: 'btn icon ghost', type: 'button', 'aria-label': 'Close', html: U.icon('x', 22), onclick: () => api.close() }));
    const body = h('div', { class: 'modal-body' }, o.body);
    const foot = o.footer && o.footer.length ? h('div', { class: 'modal-foot' }, o.footer) : null;
    const dlg = h('div', { class: 'modal' + (o.narrow ? ' narrow' : ''), role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': titleId }, head, body, foot);
    back.append(dlg);
    back.addEventListener('mousedown', (e) => { if (e.target === back) api.close(); });
    dlg.addEventListener('keydown', (e) => {
      if (e.key !== 'Tab') return;
      const f = Array.from(dlg.querySelectorAll(FOCUSABLE));
      if (!f.length) return;
      const first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    });
    const api = {
      el: dlg, body,
      close(v) {
        if (!back.isConnected) return;
        back.remove();
        stack.splice(stack.indexOf(api), 1);
        if (prev && prev.focus) prev.focus();
        if (o.onClose) o.onClose(v);
      }
    };
    stack.push(api);
    document.getElementById('modal-root').append(back);
    const first = dlg.querySelector('[autofocus],.modal-body input,.modal-body select,.modal-body textarea,.modal-body button') || dlg.querySelector('button');
    if (first) first.focus();
    return api;
  };

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && stack.length) { e.stopPropagation(); stack[stack.length - 1].close(); }
  });

  U.confirm = (msg, o) => new Promise((res) => {
    o = o || {};
    let yes = false;
    const m = U.modal({
      title: o.title || 'Please confirm', narrow: true, body: h('p', { text: msg }),
      footer: [
        h('button', { class: 'btn', type: 'button', text: 'Cancel', onclick: () => m.close() }),
        h('button', { class: 'btn primary', type: 'button', text: o.okLabel || 'OK', onclick: () => { yes = true; m.close(); } })],
      onClose: () => res(yes)
    });
  });

  U.prompt = (title, label, value, okLabel) => new Promise((res) => {
    let out = null;
    const input = h('input', { class: 'input', type: 'text', value: value || '', id: 'prompt-in', autofocus: true });
    const submit = () => { out = input.value.trim(); if (out) m.close(); else input.focus(); };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') submit(); });
    const m = U.modal({
      title, narrow: true, body: h('div', { class: 'field' }, h('label', { for: 'prompt-in', text: label }), input),
      footer: [
        h('button', { class: 'btn', type: 'button', text: 'Cancel', onclick: () => { out = null; m.close(); } }),
        h('button', { class: 'btn primary', type: 'button', text: okLabel || 'Save', onclick: submit })],
      onClose: () => res(out)
    });
    input.select();
  });
})();
