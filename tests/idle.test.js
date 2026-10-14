/* Run with: node tests/idle.test.js. Idle unload: only when idle long enough, nothing running, and enabled. */
const assert = require('assert'), fs = require('fs'), path = require('path');
const calls = [];
let now = 1000000;
const realNow = Date.now;
Date.now = () => now;
const settings = { ocrUrl: 'http://localhost:8188', provider: 'local', idleUnloadMin: 1 };
global.window = {
  fetch: async (u, o) => {
    calls.push((o && o.method || 'GET') + ' ' + u);
    if (/\/v1\/models$/.test(u)) return { ok: true, json: async () => ({ data: [{ id: 'qwen3vl', status: { value: 'loaded' } }, { id: 'paddle', status: { value: 'unloaded' } }] }) };
    return { ok: true, json: async () => ({}) };
  },
  OCR: { state: { d: { settings, run: { active: false } } }, natCompare: (a, b) => a.localeCompare(b), queue: { isRunning: () => false }, toast() {} }
};
global.fetch = window.fetch; /* models.js calls the global fetch: keep it off the real network */
new Function(fs.readFileSync(path.join(__dirname, '..', 'js', 'models.js'), 'utf8'))();
const M = window.OCR.models;

(async () => {
  assert.strictEqual(await M.idleCheck(), false);                     // just started: not idle yet
  now += 30000; assert.strictEqual(await M.idleCheck(), false);       // 30 s
  window.OCR.state.d.run.active = true; now += 120000;
  assert.strictEqual(await M.idleCheck(), false);                     // a run is active: never unload
  window.OCR.state.d.run.active = false;
  settings.idleUnloadMin = 0;
  assert.strictEqual(await M.idleCheck(), false);                     // 0 means never
  settings.idleUnloadMin = 1;
  assert.strictEqual(await M.idleCheck(), true);                      // idle past the limit: unloads
  assert.ok(calls.some((c) => c === 'POST http://localhost:8188/models/unload'));
  const n = calls.length;
  assert.strictEqual(await M.idleCheck(), false);                     // already unloaded: not repeated
  assert.strictEqual(calls.length, n);
  Date.now = realNow;
  console.log('idle.test.js: all checks passed');
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
