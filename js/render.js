/* Transcript rendering (markdown + KaTeX, one line per unit) and box overlay linking. */
(function () {
  'use strict';
  const OCR = window.OCR, R = OCR.render, esc = OCR.esc;
  let md = null;
  const getMd = () => {
    if (md !== null) return md;
    try { md = window.markdownit ? window.markdownit({ html: false, linkify: false, breaks: true }) : false; } catch (e) { md = false; }
    return md;
  };

  /* Splits a transcript into display units. A $$ block that spans lines stays one unit. */
  R.units = (text) => {
    const lines = String(text || '').replace(/\r/g, '').split('\n'), out = [];
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i];
      if (!l.trim()) continue;
      const t = l.trim();
      if (t.indexOf('$$') === 0 && !/\$\$\s*$/.test(t.slice(2))) {
        const buf = [l];
        while (++i < lines.length) { buf.push(lines[i]); if (/\$\$\s*$/.test(lines[i])) break; }
        out.push(buf.join('\n'));
      } else out.push(l);
    }
    return out;
  };

  const basic = (t) => esc(t).replace(/~~(.+?)~~/g, '<del>$1</del>');
  function mathHtml(m) {
    if (window.katex) {
      try { return window.katex.renderToString(m.tex, { displayMode: m.display, throwOnError: false }); } catch (e) { /* fall through */ }
    }
    return '<code class="math-fb">' + esc(m.tex) + '</code>';
  }

  R.unit = (src) => {
    const math = [], hold = (tex, display) => { math.push({ tex, display }); return '⟦' + (math.length - 1) + '⟧'; };
    let t = src.replace(/\$\$([\s\S]+?)\$\$/g, (m, a) => hold(a.trim(), true));
    t = t.replace(/\$([^$\n]+?)\$/g, (m, a) => (/^\s|\s$/.test(a) ? m : hold(a, false)));
    const mdi = getMd();
    let html = mdi ? mdi.renderInline(t) : basic(t);
    html = html.replace(/\[illegible\]/gi, '<span class="illegible">[illegible]</span>');
    return html.replace(/⟦(\d+)⟧/g, (m, i) => (math[+i] ? mathHtml(math[+i]) : ''));
  };

  R.renderFormatted = (host, text) => {
    const frag = document.createDocumentFragment();
    R.units(text).forEach((u, i) => {
      const d = document.createElement('div');
      d.className = 'ln' + (/^\s*Margin:/i.test(u) ? ' margin' : '');
      d.dataset.i = i;
      d.innerHTML = R.unit(u);
      frag.append(d);
    });
    host.textContent = '';
    host.append(frag);
  };

  /* Streaming appends a text node, so the transcript is never re-rendered per token. */
  R.appendStream = (pre, chunk) => { pre.append(document.createTextNode(chunk)); pre.scrollTop = pre.scrollHeight; };

  R.overlay = (host, boxes) => {
    host.textContent = '';
    (boxes || []).forEach((b, i) => {
      const d = document.createElement('div');
      d.className = 'box';
      d.dataset.i = i;
      d.style.cssText = 'left:' + b.x * 100 + '%;top:' + b.y * 100 + '%;width:' + b.w * 100 + '%;height:' + b.h * 100 + '%' + (b.r ? ';transform:rotate(' + b.r + 'deg)' : '');
      host.append(d);
    });
  };

  /* Hover a transcript line to light its box on the page, and the other way round. */
  R.hoverSync = (overlay, tx) => {
    const flip = (host, sel, i, on) => { const el = host.querySelector(sel + '[data-i="' + i + '"]'); if (el) el.classList.toggle('on', on); };
    const handle = (e, on) => {
      const t = e.target.closest && e.target.closest('.ln,.box');
      if (!t || t.dataset.i == null) return;
      flip(overlay, '.box', t.dataset.i, on);
      flip(tx, '.ln', t.dataset.i, on);
    };
    [overlay, tx].forEach((h) => { h.addEventListener('mouseover', (e) => handle(e, true)); h.addEventListener('mouseout', (e) => handle(e, false)); });
  };

  /* Fit detected ink bands to the number of transcript lines. */
  R.mapBands = (bands, n) => {
    const b = bands.map((x) => Object.assign({}, x));
    if (!n || !b.length) return [];
    while (b.length > n) {
      if (b.every((x) => typeof x.s === 'number')) { /* too many: the weakest band is the stray one (an edge, a smudge), not two lines to merge */
        let w = 0;
        for (let i = 1; i < b.length; i++) if (b[i].s < b[w].s) w = i;
        b.splice(w, 1);
        continue;
      }
      let k = 0, g = 1e9;
      for (let i = 0; i < b.length - 1; i++) { const gap = b[i + 1].y - (b[i].y + b[i].h); if (gap < g) { g = gap; k = i; } }
      const a = b[k], c = b[k + 1], x = Math.min(a.x, c.x), x2 = Math.max(a.x + a.w, c.x + c.w);
      b.splice(k, 2, { x, y: a.y, w: x2 - x, h: c.y + c.h - a.y, r: 0 });
    }
    while (b.length < n) {
      let k = 0;
      for (let i = 1; i < b.length; i++) if (b[i].h > b[k].h) k = i;
      const a = b[k], h = a.h / 2;
      b.splice(k, 1, Object.assign({}, a, { h }), Object.assign({}, a, { y: a.y + h, h }));
    }
    return b;
  };

  R.computeBoxes = async (blob, raw) => {
    const n = R.units(raw).length;
    if (!n) return [];
    const bands = await OCR.image.lineBands(blob);
    if (!bands.length || Math.abs(bands.length - n) > Math.max(2, Math.round(n * 0.35))) return []; /* unreliable: draw nothing */
    return R.mapBands(bands, n);
  };
})();
