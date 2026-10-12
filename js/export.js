/* Exports: combined txt/md/docx, per-page zip, json, csv, copy all, download all. */
(function () {
  'use strict';
  const OCR = window.OCR, E = OCR.export, S = OCR.state, enc = new TextEncoder();

  /* ---- zip: fflate when it loaded, otherwise a small store-only writer ---- */
  let table = null;
  function crc32(u8) {
    if (!table) { table = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; table[n] = c >>> 0; } }
    let c = 0xffffffff;
    for (let i = 0; i < u8.length; i++) c = table[(c ^ u8[i]) & 255] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  }
  function storeZip(map) {
    const parts = [], central = [];
    let off = 0, count = 0, cdSize = 0;
    Object.keys(map).forEach((path) => {
      const name = enc.encode(path), u8 = map[path], crc = crc32(u8);
      const lh = new DataView(new ArrayBuffer(30));
      lh.setUint32(0, 0x04034b50, true); lh.setUint16(4, 20, true); lh.setUint16(6, 0x0800, true);
      lh.setUint16(12, 0x21, true); lh.setUint32(14, crc, true); lh.setUint32(18, u8.length, true);
      lh.setUint32(22, u8.length, true); lh.setUint16(26, name.length, true);
      parts.push(lh.buffer, name, u8);
      const ch = new DataView(new ArrayBuffer(46));
      ch.setUint32(0, 0x02014b50, true); ch.setUint16(4, 20, true); ch.setUint16(6, 20, true); ch.setUint16(8, 0x0800, true);
      ch.setUint16(14, 0x21, true); ch.setUint32(16, crc, true); ch.setUint32(20, u8.length, true);
      ch.setUint32(24, u8.length, true); ch.setUint16(28, name.length, true); ch.setUint32(42, off, true);
      central.push(ch.buffer, name);
      cdSize += 46 + name.length; off += 30 + name.length + u8.length; count++;
    });
    const end = new DataView(new ArrayBuffer(22));
    end.setUint32(0, 0x06054b50, true); end.setUint16(8, count, true); end.setUint16(10, count, true);
    end.setUint32(12, cdSize, true); end.setUint32(16, off, true);
    return new Blob(parts.concat(central, [end.buffer]), { type: 'application/zip' });
  }
  E.makeZip = (files) => {
    const map = {};
    Object.keys(files).forEach((k) => { map[k] = typeof files[k] === 'string' ? enc.encode(files[k]) : files[k]; });
    return window.fflate ? new Blob([window.fflate.zipSync(map, { level: 6 })], { type: 'application/zip' }) : storeZip(map);
  };

  /* ---- text builders ---- */
  E.pageText = (p, mode) => {
    const raw = p.raw || '', cor = (OCR.words ? OCR.words.effective(p) : p.corrected) || raw; /* corrected + typed edits */
    if (mode === 'raw') return raw;
    if (mode === 'corrected') return cor;
    return '--- Raw ---\n' + raw + '\n\n--- Corrected ---\n' + cor;
  };
  E.label = (p) => '--- ' + (p.student && p.student.name ? p.student.name + ', ' : '') + 'Page ' + (p.order + 1) + ' ---';
  E.combined = (list, o) => list.map((p) => (o.sep === 'label' ? E.label(p) + '\n' : '') + E.pageText(p, o.mode))
    .join(o.sep === 'break' ? (o.fmt === 'md' ? '\n\n---\n\n' : '\n\f\n') : '\n\n');

  E.docx = (list, o) => {
    const x = OCR.esc, para = (t) => '<w:p><w:r><w:t xml:space="preserve">' + x(t) + '</w:t></w:r></w:p>';
    let body = '';
    list.forEach((p, i) => {
      if (i && o.sep === 'break') body += '<w:p><w:r><w:br w:type="page"/></w:r></w:p>';
      if (o.sep === 'label') body += para(E.label(p));
      E.pageText(p, o.mode).split('\n').forEach((l) => { body += para(l); });
      if (o.sep !== 'break') body += para('');
    });
    const head = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';
    return E.makeZip({
      '[Content_Types].xml': head + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>',
      '_rels/.rels': head + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>',
      'word/document.xml': head + '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>' + body + '</w:body></w:document>'
    });
  };

  const fileNames = (list) => {
    const used = new Set();
    return list.map((p) => {
      const base = (p.name || 'page').replace(/\.[a-z0-9]+$/i, '').replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '') || 'page';
      let n = base, i = 2;
      while (used.has(n)) n = base + '-' + i++;
      used.add(n);
      return n;
    });
  };
  const csvCell = (v) => '"' + String(v).replace(/"/g, '""') + '"';
  E.csv = (list) => 'order,name,status,chars,seconds\n' + list.map((p) =>
    [p.order + 1, csvCell(p.name), p.status, (p.raw || '').length, ((p.timings && p.timings.ms) / 1000 || 0).toFixed(1)].join(',')).join('\n') + '\n';
  E.json = (list) => JSON.stringify({ project: { name: S.d.project.meta.name, topic: S.d.project.meta.topic, prompt: S.d.project.meta.prompt, exported: new Date().toISOString() }, pages: list }, null, 2);
  E.perPage = (list, mode) => { const n = fileNames(list), f = {}; list.forEach((p, i) => { f[n[i] + '.txt'] = E.pageText(p, mode); }); return f; };

  const MIME = { txt: 'text/plain', md: 'text/markdown' };
  E.combinedBlob = (list, o) => (o.fmt === 'docx'
    ? E.docx(list, o)
    : new Blob([E.combined(list, o)], { type: (MIME[o.fmt] || 'text/plain') + ';charset=utf-8' }));

  E.download = (name, blob) => {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob); a.download = name;
    document.body.append(a); a.click();
    setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 4000);
  };
  E.deliver = async (name, blob) => {
    E.download(name, blob);
    const saved = await OCR.fs.writeExport(name, blob);
    OCR.toast(saved ? 'Exported ' + name + ' and saved a copy in the project exports folder.' : 'Exported ' + name + '.');
  };

  E.copyText = async (text) => {
    try { await navigator.clipboard.writeText(text); return true; }
    catch (e) {
      const t = document.createElement('textarea');
      t.value = text; t.style.position = 'fixed'; t.style.opacity = '0';
      document.body.append(t); t.select();
      let ok = false; try { ok = document.execCommand('copy'); } catch (x) { /* ignore */ }
      t.remove(); return ok;
    }
  };

  /* kind: combined | pages | json | csv | copy | all. o: {mode, sep, fmt} */
  E.run = async (kind, list, o) => {
    const base = OCR.slug(S.d.project.meta.name) || 'transcript';
    if (kind === 'combined') return E.deliver(base + '.' + o.fmt, E.combinedBlob(list, o));
    if (kind === 'pages') return E.deliver(base + '-pages.zip', E.makeZip(E.perPage(list, o.mode)));
    if (kind === 'json') return E.deliver(base + '-pages.json', new Blob([E.json(list)], { type: 'application/json' }));
    if (kind === 'csv') return E.deliver(base + '-index.csv', new Blob([E.csv(list)], { type: 'text/csv' }));
    if (kind === 'copy') {
      const ok = await E.copyText(E.combined(list, Object.assign({}, o, { fmt: 'txt' })));
      return OCR.toast(ok ? 'Copied ' + list.length + ' pages.' : 'Could not copy. Use a download instead.', { kind: ok ? 'info' : 'error' });
    }
    if (kind === 'all') {
      const files = {};
      files['combined.' + (o.fmt === 'docx' ? 'docx' : o.fmt)] = o.fmt === 'docx' ? new Uint8Array(await E.docx(list, o).arrayBuffer()) : E.combined(list, o);
      const per = E.perPage(list, o.mode);
      Object.keys(per).forEach((k) => { files['pages/' + k] = per[k]; });
      files['pages.json'] = E.json(list);
      files['index.csv'] = E.csv(list);
      return E.deliver(base + '-all.zip', E.makeZip(files));
    }
  };
})();
