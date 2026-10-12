/* Creates the single global namespace and small shared helpers. */
(function () {
  'use strict';
  const OCR = (window.OCR = window.OCR || {});
  ['api', 'ocr', 'queue', 'image', 'judge', 'render', 'export', 'fs', 'ui'].forEach((k) => { OCR[k] = OCR[k] || {}; });
  OCR.state = OCR.state || {};

  OCR.$ = (s, r) => (r || document).querySelector(s);
  OCR.$$ = (s, r) => Array.from((r || document).querySelectorAll(s));
  /* Short text stand-ins, shown only if the icon font fails to load (for example when offline). */
  const FB = {
    house: '⌂', 'sidebar-simple': '☰', gear: '⚙', 'folder-open': '▤', 'folder-simple': '▤', database: '▤', notebook: '▤',
    'plugs-connected': '⇄', plus: '+', 'upload-simple': '↥', 'file-arrow-up': '↥', 'pencil-simple': '✎', copy: 'Copy',
    trash: 'Del', 'file-zip': 'Zip', swap: '⇄', selection: '▭', 'arrow-clockwise': '↻', x: '×', 'caret-left': '‹',
    'caret-right': '›', play: '▶', stop: '■', check: '✓', warning: '!', 'arrow-right': '→', 'circle-notch': '…',
    'list-checks': '☰', 'arrow-counter-clockwise': '↺', export: '↗', 'download-simple': '↓', 'file-text': '≣',
    files: '≣', 'brackets-curly': '{}', table: '▦', 'text-aa': 'Aa'
  };
  OCR.icon = (n) => '<i class="ph ph-' + n + '" data-fb="' + (FB[n] || '•') + '" aria-hidden="true"></i>';

  const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  OCR.esc = (s) => String(s == null ? '' : s).replace(/[&<>"']/g, (c) => ESC[c]);

  /* h('div', {class:'x', onclick:fn, text:'hi'}, child, [children]) */
  OCR.h = function (tag, props, ...kids) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(props || {})) {
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'html') el.innerHTML = v;
      else if (k === 'text') el.textContent = v;
      else if (k === 'dataset') Object.assign(el.dataset, v);
      else if (k === 'value') el.value = v;
      else if (k.slice(0, 2) === 'on' && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
      else el.setAttribute(k, v === true ? '' : v);
    }
    const add = (c) => {
      if (c == null || c === false) return;
      if (Array.isArray(c)) c.forEach(add);
      else el.append(c.nodeType ? c : document.createTextNode(c));
    };
    kids.forEach(add);
    return el;
  };

  OCR.uid = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 7);
  OCR.slug = (s) => String(s).toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '')
    .replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 48);
  OCR.natCompare = (a, b) => String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: 'base' });
  OCR.debounce = (fn, ms) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; };

  OCR.fmtDur = (ms) => {
    const s = Math.max(0, Math.round(ms / 1000));
    if (s < 60) return s + 's';
    const m = Math.ceil(s / 60);
    return m < 60 ? m + ' min' : Math.floor(m / 60) + ' h ' + (m % 60) + ' min';
  };
  OCR.fmtAgo = (ts) => {
    if (!ts) return '';
    const s = (Date.now() - ts) / 1000;
    if (s < 60) return 'just now';
    if (s < 3600) return Math.floor(s / 60) + ' min ago';
    if (s < 86400) return Math.floor(s / 3600) + ' h ago';
    return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  };

  /* toast(message, {kind:'info'|'warn'|'error', ms, action:{label, fn}}) */
  OCR.toast = function (msg, o) {
    o = o || {};
    const host = document.getElementById('toasts');
    if (!host) return;
    const kind = o.kind || 'info';
    const el = OCR.h('div', { class: 'toast ' + kind, role: kind === 'error' ? 'alert' : 'status' }, OCR.h('span', { text: msg }));
    if (o.action) el.append(OCR.h('button', { class: 'btn sm ghost', type: 'button', text: o.action.label, onclick: () => { o.action.fn(); el.remove(); } }));
    host.append(el);
    setTimeout(() => el.remove(), o.ms || (kind === 'info' ? 4000 : 8000));
  };
})();
