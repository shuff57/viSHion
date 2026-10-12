/* Student name reader. Isolated on purpose: a separate, short request to the same
   server, never mixed into the transcription prompt. Returns { name: string|null }. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, ST = (OCR.student = OCR.student || {});
  const STRIP = 0.14;          /* top 14% of the page */
  const ASK = 'This is the top strip of a handwritten student page. If a student name is written there, read it. ' +
    'Answer with JSON only, like {"name": "Maya R."}. If no name is written, answer {"name": null}. Do not guess and do not add anything else.';
  const DEMO_NAMES = ['Maya R.', 'Devon T.', 'Sofia V.', 'Aaliyah M.', null, 'Caleb P.'];

  /* Turns the model's answer into a usable name, or null. */
  ST.clean = (v) => {
    if (typeof v !== 'string') return null;
    let n = v.replace(/^\s*(student\s*)?name\s*[:\-]\s*/i, '').replace(/["'`]/g, '').replace(/\s+/g, ' ').trim();
    n = n.replace(/[,;:]+$/, '');
    if (n.length < 2 || n.length > 40) return null;
    if (/^(null|none|n\/a|unknown|not found|no name)$/i.test(n)) return null;
    if (!/[A-Za-zÀ-ɏ]/.test(n)) return null;
    return n;
  };

  async function strip(blob) {
    const bmp = await createImageBitmap(blob);
    const h = Math.max(1, Math.round(bmp.height * STRIP));
    const c = document.createElement('canvas');
    c.width = bmp.width; c.height = h;
    c.getContext('2d').drawImage(bmp, 0, 0, bmp.width, h, 0, 0, bmp.width, h);
    if (bmp.close) bmp.close();
    const out = await new Promise((r) => c.toBlob(r, 'image/jpeg', 0.9));
    c.width = c.height = 0;
    return out;
  }

  /* opt: { hint: file name (demo mode only), signal } */
  ST.read = async (blob, opt) => {
    opt = opt || {};
    const s = S.d.settings;
    if (s.demo) {
      let h = 0;
      String(opt.hint || '').split('').forEach((ch) => { h = (h * 31 + ch.charCodeAt(0)) >>> 0; });
      await new Promise((r) => setTimeout(r, 120));
      return { name: DEMO_NAMES[h % DEMO_NAMES.length], raw: '' };
    }
    const origin = (s.ocrUrl || '').replace(/\/+$/, '');
    const headers = { 'Content-Type': 'application/json' };
    if (s.apiKey) headers.Authorization = 'Bearer ' + s.apiKey;
    const ctl = new AbortController(), tm = setTimeout(() => ctl.abort(), 45000);
    if (opt.signal) opt.signal.addEventListener('abort', () => ctl.abort(), { once: true });
    try {
      const body = {
        model: s.model || 'ocr', stream: false, temperature: 0.1, max_tokens: 60,
        messages: [{ role: 'user', content: [
          { type: 'image_url', image_url: { url: await OCR.image.toDataURL(await strip(blob)) } },
          { type: 'text', text: ASK }] }]
      };
      const r = await fetch(origin + '/v1/chat/completions', { method: 'POST', headers, body: JSON.stringify(body), signal: ctl.signal });
      if (!r.ok) return { name: null, raw: '', error: 'http' };
      const j = await r.json(), text = (j.choices && j.choices[0] && j.choices[0].message && j.choices[0].message.content) || '';
      const m = text.match(/\{[\s\S]*\}/);
      let name = null;
      try { name = ST.clean(JSON.parse(m ? m[0] : text).name); } catch (e) { name = null; }
      return { name, raw: text };
    } catch (e) {
      return { name: null, raw: '', error: e.name === 'AbortError' ? 'timeout' : 'network' };
    } finally { clearTimeout(tm); }
  };
})();
