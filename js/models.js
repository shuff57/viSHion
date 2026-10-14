/* Where the one model lives: a local server, or an optional cloud endpoint.
   Everything speaks the OpenAI-style /v1 API, so a provider is just a base address,
   an optional key and a model name. Per-provider choices are kept in settings.profiles
   (never written to a project folder, because they hold API keys). */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, M = (OCR.models = OCR.models || {});

  M.PROVIDERS = [
    { id: 'local', label: 'Local server (llama.cpp or Ollama)', url: 'http://localhost:8188', cloud: false, show: true, name: 'this computer' },
    /* ollama.com sends no CORS headers, so a page cannot call it. tools/relay.js (run once) forwards for the page. */
    { id: 'ollama-cloud', label: 'Ollama Cloud', url: 'https://ollama.com', cloud: true, relay: true, name: 'Ollama Cloud' },
    { id: 'openrouter', label: 'OpenRouter', url: 'https://openrouter.ai/api', cloud: true, name: 'OpenRouter' },
    { id: 'custom', label: 'Another OpenAI-compatible endpoint', url: '', cloud: true, name: 'the server you entered' }
  ];
  const find = (id) => M.PROVIDERS.find((p) => p.id === id) || M.PROVIDERS[0];
  M.provider = () => find(S.d.settings.provider);
  M.isCloud = () => M.provider().cloud;
  M.name = () => M.provider().name;

  const trim = (u) => String(u || '').replace(/\/+$/, '');
  const RELAY_DEFAULT = 'http://localhost:8189';
  const relayBase = () => trim(S.d.settings.relayUrl) || RELAY_DEFAULT;
  /* The address the page really calls. For a relayed provider that is the relay, with the host in the path. */
  M.engineUrl = (provider, shown) => {
    const p = typeof provider === 'string' ? find(provider) : provider, u = trim(shown);
    if (!p.relay || !u) return u;
    const host = u.replace(/^https?:\/\//i, '').replace(/\/.*$/, '');
    return relayBase() + '/u/' + host;
  };
  /* The address a person recognises (https://ollama.com), recovered from what the engine uses. */
  M.shownUrl = (provider, engine) => {
    const p = typeof provider === 'string' ? find(provider) : provider, m = trim(engine).match(/\/u\/([^/]+)$/);
    return p.relay && m ? 'https://' + m[1] : trim(engine);
  };
  const headers = () => (S.d.settings.apiKey ? { Authorization: 'Bearer ' + S.d.settings.apiKey } : {});

  /* Earlier builds pointed Ollama Cloud at https://ollama.com, which browsers cannot reach. Move it to the local app. */
  M.migrate = () => {
    const st = S.d.settings, fix = find('ollama-cloud').url, profiles = Object.assign({}, st.profiles);
    let changed = false;
    const pr = profiles['ollama-cloud'];
    if (pr && pr.url !== fix) { profiles['ollama-cloud'] = Object.assign({}, pr, { url: fix }); changed = true; }
    const want = M.engineUrl('ollama-cloud', fix);
    if (st.provider === 'ollama-cloud' && st.ocrUrl !== want) S.saveSettings({ ocrUrl: want, profiles });
    else if (changed) S.saveSettings({ profiles });
  };

  /* The values that go in the flat settings the engine reads (ocrUrl, model, apiKey). */
  M.profile = (id) => {
    const p = find(id), saved = (S.d.settings.profiles || {})[p.id] || {};
    return { url: saved.url != null ? saved.url : p.url, model: saved.model || '', key: saved.key || '', working: Array.isArray(saved.working) ? saved.working : null };
  };
  /* Remember the active values under the active provider, then (optionally) switch. */
  M.remember = (patch) => {
    const id = S.d.settings.provider || 'local', profiles = Object.assign({}, S.d.settings.profiles);
    profiles[id] = Object.assign({}, M.profile(id), patch);
    return profiles;
  };
  M.switchTo = (id) => {
    const profiles = M.remember({ url: M.shownUrl(S.d.settings.provider, S.d.settings.ocrUrl), model: S.d.settings.model || '', key: S.d.settings.apiKey || '' });
    const next = Object.assign({}, profiles[id] || {});
    const base = find(id), prof = { url: next.url != null ? next.url : base.url, model: next.model || '', key: next.key || '' };
    S.saveSettings({ provider: base.id, profiles, ocrUrl: M.engineUrl(base, prof.url), model: prof.model, apiKey: prof.key });
  };

  /* Names for the models this app is commonly run with. Anything else is tidied from its id. */
  const KNOWN = {
    qwen3vl: 'Qwen3-VL 8B Instruct', qwen25vl: 'Qwen2.5-VL 7B Instruct', lumichats: 'LumiChats v1.2',
    nanonets: 'Nanonets-OCR2 3B', paddle: 'PaddleOCR-VL 1.6'
  };
  const BRAND = { gpt: 'GPT', oss: 'OSS', glm: 'GLM', vl: 'VL', ocr: 'OCR', lfm: 'LFM', llama: 'Llama', qwen: 'Qwen', gemma: 'Gemma', kimi: 'Kimi', deepseek: 'DeepSeek',
    minimax: 'MiniMax', nemotron: 'Nemotron', mistral: 'Mistral', phi: 'Phi', nomic: 'Nomic', vibethinker: 'VibeThinker', paddleocr: 'PaddleOCR', nanonets: 'Nanonets', lumichats: 'LumiChats' };
  M.pretty = (id, apiName) => {
    const raw = String(id || '');
    if (KNOWN[raw.toLowerCase()]) return KNOWN[raw.toLowerCase()];
    if (apiName && apiName !== raw) return String(apiName).replace(/^[^:]+:\s+/, ''); /* OpenRouter: "Vendor: Model" */
    const s = raw.replace(/^hf\.co\//i, '').replace(/^[^/]+\//, ''); /* registry and owner */
    const at = s.indexOf(':'), base = at < 0 ? s : s.slice(0, at);
    let tag = at < 0 ? '' : s.slice(at + 1);
    tag = tag.replace(/(^|-)cloud$/i, '').replace(/^latest$/i, ''); /* hosted / latest are not part of the name */
    const quant = (tag.match(/^q\d+(?:_[a-z0-9]+)*$/i) || [])[0];
    const words = base.replace(/-GGUF$/i, '').split(/[-_\s]+/).filter(Boolean).concat(quant ? [] : tag.split(/[-_\s]+/).filter(Boolean));
    const out = [];
    words.forEach((w) => {
      const m = w.match(/^([a-z]{2,})(\d[\d.]*)$/i); /* gemma4 -> Gemma 4, qwen3.5 -> Qwen 3.5 */
      if (BRAND[w.toLowerCase()]) out.push(BRAND[w.toLowerCase()]);
      else if (m) out.push(BRAND[m[1].toLowerCase()] || m[1][0].toUpperCase() + m[1].slice(1).toLowerCase(), m[2]);
      else if (/^\d+(?:\.\d+)?[bm]$/i.test(w) || /^v\d/i.test(w)) out.push(w.toUpperCase()); /* 12b -> 12B, v4 -> V4 */
      else out.push(/^[a-z]/.test(w) ? w[0].toUpperCase() + w.slice(1) : w);
    });
    return out.join(' ') + (quant ? ' (' + quant.split('_')[0].toUpperCase() + ')' : '');
  };

  /* Sends one tiny image request to each model with the saved key. A model that answers is usable with
     this key and accepts images; one that is refused (plan, size, no image support) is left out. */
  const TINY = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACAAAAAgCAIAAAD8GO2jAAAAN0lEQVR4nO3RwQ0AMAjDwJT9d05HMB9+vgGCZF7bXJrT9XhgwR8gEyETIRMhEyETIRMhEyEThXzH8QM9OMM6fAAAAABJRU5ErkJggg==';
  M.probe = async (ids, onProgress) => {
    const base = trim(S.d.settings.ocrUrl), ok = [], failed = [];
    let next = 0, done = 0;
    async function one(id) {
      const ctl = new AbortController(), tm = setTimeout(() => ctl.abort(), 60000);
      try {
        const r = await fetch(base + '/v1/chat/completions', { method: 'POST', signal: ctl.signal,
          headers: Object.assign({ 'Content-Type': 'application/json' }, headers()),
          body: JSON.stringify({ model: id, stream: false, max_tokens: 8, temperature: 0, messages: [{ role: 'user', content: [
            { type: 'image_url', image_url: { url: TINY } }, { type: 'text', text: 'Reply with the single word OK.' }] }] }) });
        if (r.ok) {
          const j = await r.json(), c = j.choices && j.choices[0] && j.choices[0].message;
          if (c && typeof c.content === 'string') ok.push(id); else failed.push({ id, why: 'empty answer' });
        } else failed.push({ id, why: 'HTTP ' + r.status });
      } catch (e) { failed.push({ id, why: e.name === 'AbortError' ? 'timed out' : 'no connection' }); }
      finally { clearTimeout(tm); done++; if (onProgress) onProgress(done, ids.length); }
    }
    const worker = async () => { while (next < ids.length) await one(ids[next++]); };
    await Promise.all([worker(), worker(), worker()]);
    return { ok, failed };
  };
  M.setWorking = (ids) => S.saveSettings({ profiles: M.remember({ working: ids }) });

  /* ---- unloading: free the GPU when the model is not needed ----
     llama.cpp in router mode:  POST /models/unload { model }
     Ollama:                    POST /api/generate { model, keep_alive: 0 }                     */
  M.unload = async () => {
    const base = trim(S.d.settings.ocrUrl);
    if (!base || M.isCloud()) return { count: 0, names: [] }; /* nothing of yours is loaded when it runs elsewhere */
    const names = [];
    try { /* router mode: models report a status */
      const r = await fetch(base + '/v1/models', { headers: headers() });
      if (r.ok) {
        const rows = (await r.json()).data || [];
        for (const m of rows.filter((x) => x.status && x.status.value === 'loaded')) {
          const u = await fetch(base + '/models/unload', { method: 'POST', headers: Object.assign({ 'Content-Type': 'application/json' }, headers()), body: JSON.stringify({ model: m.id }) });
          if (u.ok) names.push(M.pretty(m.id));
        }
        if (names.length) return { count: names.length, names };
      }
    } catch (e) { /* not a router: try Ollama */ }
    try {
      const r = await fetch(base + '/api/ps', { headers: headers() });
      if (r.ok) {
        for (const m of ((await r.json()).models || [])) {
          const u = await fetch(base + '/api/generate', { method: 'POST', headers: Object.assign({ 'Content-Type': 'application/json' }, headers()), body: JSON.stringify({ model: m.name || m.model, keep_alive: 0 }) });
          if (u.ok) names.push(M.pretty(m.name || m.model));
        }
      }
    } catch (e) { /* nothing to unload here */ }
    return { count: names.length, names };
  };

  /* Idle unload. Any request to the model server marks it used; a run in progress counts as busy.
     Works only while this page is open, which is when the app is the thing using the model. */
  let lastUse = Date.now(), asleep = false, inflight = 0;
  const realFetch = window.fetch.bind(window);
  const isModelCall = (u) => {
    const base = trim(S.d.settings.ocrUrl);
    return base && String(u && u.url ? u.url : u).indexOf(base + '/v1/chat/completions') === 0;
  };
  window.fetch = function (u, o) {
    if (!isModelCall(u)) return realFetch(u, o);
    lastUse = Date.now(); asleep = false; inflight++;
    return realFetch(u, o).finally(() => { inflight--; lastUse = Date.now(); });
  };
  M.idleCheck = async () => {
    const mins = Number(S.d.settings.idleUnloadMin);
    if (!(mins > 0) || asleep || inflight || S.d.run.active || (OCR.queue && OCR.queue.isRunning && OCR.queue.isRunning())) return false;
    if (Date.now() - lastUse < mins * 60000) return false;
    asleep = true;
    const r = await M.unload();
    if (r.count) OCR.toast('Unloaded ' + r.names.join(', ') + ' after ' + mins + (mins === 1 ? ' minute' : ' minutes') + ' idle.');
    return r.count > 0;
  };
  setInterval(() => { M.idleCheck().catch(() => {}); }, 20000);

  /* Models the endpoint offers. Returns [{ id, label, vision }] with vision models first.
     OpenRouter says which models take images; a local Ollama says it per model via /api/show. */
  M.list = async (opt) => {
    const base = trim(S.d.settings.ocrUrl);
    if (!base) throw new Error('Enter the server address first.');
    const ctl = new AbortController(), tm = setTimeout(() => ctl.abort(), 15000);
    let rows;
    try {
      const r = await fetch(base + '/v1/models', { headers: headers(), signal: ctl.signal });
      if (r.status === 401 || r.status === 403) throw new Error('The server did not accept the API key.');
      if (!r.ok) throw new Error('The server answered with ' + r.status + '.');
      rows = ((await r.json()).data || []);
    } catch (e) {
      if (e.name === 'AbortError') throw new Error('The server did not answer in time.');
      if (e instanceof TypeError) {
        if (M.provider().relay) throw new Error('The relay is not running. Start it once with: node tools/relay.js');
        throw new Error('Can’t reach ' + base + '. Is it running, and does it allow this page (CORS)?');
      }
      throw e;
    } finally { clearTimeout(tm); }

    let out = rows.filter((m) => m && m.id).map((m) => {
      const mods = m.architecture && m.architecture.input_modalities;
      return { id: String(m.id), apiName: typeof m.name === 'string' ? m.name : '', vision: Array.isArray(mods) ? mods.indexOf('image') >= 0 : null };
    });
    /* Ollama lists its hosted models next to the ones on disk, tagged :cloud or -cloud. Keep each picker to its own kind. */
    const hosted = (id) => /[:-]cloud$/i.test(id), pid = M.provider().id;
    if (pid === 'local') out = out.filter((m) => !hosted(m.id));
    const router = rows.some((m) => m && m.status); /* llama.cpp router rows carry a status and have no /api/show */
    if (M.provider().show && !router) { /* a local Ollama can tell us which of its models see images */
      await Promise.all(out.map(async (m) => {
        try {
          const r = await fetch(base + '/api/show', { method: 'POST', headers: Object.assign({ 'Content-Type': 'application/json' }, headers()), body: JSON.stringify({ model: m.id }) });
          if (r.ok) { const c = (await r.json()).capabilities; if (Array.isArray(c)) m.vision = c.indexOf('vision') >= 0; }
        } catch (e) { /* llama.cpp has no /api/show: leave it unknown */ }
      }));
    }
    out.forEach((m) => { m.label = M.pretty(m.id, m.apiName); });
    out = out.filter((m) => m.vision !== false); /* this app reads images: text-only models are not offered */
    const working = M.profile(M.provider().id).working;
    if (M.isCloud() && working && !(opt && opt.all)) out = out.filter((m) => working.indexOf(m.id) >= 0);
    out = out.sort((a, b) => (b.vision === true) - (a.vision === true) || OCR.natCompare(a.label, b.label));
    return out;
  };
})();
