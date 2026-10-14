/* Inline stroke icons (24px grid), so the UI has no dependency on an icon font. U.icon(name, size) returns an SVG string. */
(function () {
  'use strict';
  const OCR = window.OCR, U = OCR.ui;
  const P = {
    gear: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/>',
    left: '<path d="M15 18l-6-6 6-6"/>',
    right: '<path d="M9 18l6-6-6-6"/>',
    check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
    x: '<path d="M6 6l12 12M18 6L6 18"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    pencil: '<path d="M4 20h4L19 9l-4-4L4 16v4zM13.5 6.5l4 4"/>',
    copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 012-2h9"/>',
    download: '<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>',
    trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 002 2h6a2 2 0 002-2l1-12M9 7V4h6v3"/>',
    folder: '<path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2V7z"/>',
    db: '<ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/>',
    warn: '<path d="M12 3l10 18H2L12 3zM12 10v5M12 18v.01"/>',
    upload: '<path d="M12 16V4M7 9l5-5 5 5M5 20h14"/>',
    rotate: '<path d="M20 12a8 8 0 11-2.6-5.9M20 4v5h-5"/>',
    swap: '<path d="M4 8h13l-3-3M20 16H7l3 3"/>',
    boxes: '<rect x="4" y="4" width="16" height="16" rx="2" stroke-dasharray="3 3"/>',
    file: '<path d="M14 3H7a2 2 0 00-2 2v14a2 2 0 002 2h10a2 2 0 002-2V8l-5-5zM14 3v5h5M9 13h6M9 17h6"/>',
    files: '<path d="M8 4h8l4 4v10a2 2 0 01-2 2H8a2 2 0 01-2-2V6a2 2 0 012-2zM16 4v4h4M3 8v11a2 2 0 002 2h9"/>',
    braces: '<path d="M8 4c-2 0-3 1-3 3v2c0 1.5-1 3-2 3 1 0 2 1.500 2 3v2c0 2 1 3 3 3M16 4c2 0 3 1 3 3v2c0 1.500 1 3 2 3-1 0-2 1.500-2 3v2c0 2-1 3-3 3"/>',
    table: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 10h18M9 5v14"/>',
    spin: '<path d="M12 3a9 9 0 109 9"/>',
    page: '<path d="M7 3h7l5 5v13H7zM14 3v5h5"/>',
    level: '<path d="M4 9l16-3M4 14l16-3M4 19l16-3"/>',
    zoomIn: '<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4M11 8v6M8 11h6"/>',
    zoomOut: '<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4M8 11h6"/>',
    play: '<path d="M7 5l12 7-12 7V5z"/>'
  };
  U.icon = (name, size) => {
    const s = size || 20;
    return '<svg class="ico" width="' + s + '" height="' + s + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' + (P[name] || '') + '</svg>';
  };
})();
