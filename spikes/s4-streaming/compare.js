/* node compare.js  ->  drives js/ocr.js and the Rust client against every mock variant, prints diffs. */
const assert = require('assert');
const { start, variants, failures } = require('./mock-server');
const { jsClient, rustClient } = require('./clients');

const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const short = (s, n = 70) => { s = String(JSON.stringify(s)); return s.length > n ? s.slice(0, n) + '..' : s; };

async function once(client, srv, sc, opts = {}) {
  await fetch(`http://127.0.0.1:${srv.port}/_reset`, { method: 'POST' });
  const ctl = new AbortController();
  const origin = sc.origin || `http://127.0.0.1:${srv.port}/v/${sc.variant}`;
  let timer = null;
  if (sc.abort && sc.abort.pre) ctl.abort();
  if (sc.abort && sc.abort.afterMs) timer = setTimeout(() => ctl.abort(), sc.abort.afterMs);
  if (sc.abort && sc.abort.reason) setTimeout(() => ctl.abort(new Error('custom reason')), sc.abort.reasonAfterMs || 100);
  const t0 = Date.now();
  const r = await client({ origin, signal: ctl.signal, flushTail: opts.flushTail,
    onTextHook: sc.abort && sc.abort.afterTexts ? (n) => { if (n === sc.abort.afterTexts) ctl.abort(); } : null });
  if (timer) clearTimeout(timer);
  r.ms = Date.now() - t0;
  await new Promise((res) => setTimeout(res, 120)); /* let the server notice a hang-up */
  const lg = await (await fetch(`http://127.0.0.1:${srv.port}/_log`)).json();
  const post = lg.find((e) => e.path.endsWith('/chat/completions'));
  r.server = post ? { hungUpEarly: post.gone, serverEnded: post.ended } : null;
  r.slotsPolls = lg.filter((e) => e.path.endsWith('/slots')).length;
  return r;
}

(async () => {
  const srv = await start(0);
  const scenarios = [
    ...variants.filter((v) => !['slow', 'gated', 'slow-headers', 'big'].includes(v)).map((v) => ({ name: v, variant: v })),
    ...failures.map((v) => ({ name: 'http-' + v, variant: v })),
    { name: 'unreachable (port 1)', origin: 'http://127.0.0.1:1' },
    { name: 'abort: pre-aborted signal', variant: 'normal', abort: { pre: true } },
    { name: 'abort: after 2nd chunk (gated)', variant: 'gated', abort: { afterTexts: 2 } },
    { name: 'abort: during headers', variant: 'slow-headers', abort: { afterMs: 200 } },
    { name: 'abort: timer mid-stream (queue.js timeout path)', variant: 'slow', abort: { afterMs: 400 } },
    { name: 'abort: custom reason (name != AbortError)', variant: 'slow', abort: { reason: true } },
    { name: 'big (4000 chunks)', variant: 'big' },
  ];
  const rows = [];
  for (const sc of scenarios) {
    const j = await once(jsClient, srv, sc);
    const r = await once(rustClient, srv, sc);
    const sameEv = eq(j.events, r.events), sameOut = eq(j.outcome, r.outcome);
    const sameSrv = eq(j.server, r.server);
    rows.push({ sc, j, r, sameEv, sameOut, sameSrv });
    const tag = sameEv && sameOut && sameSrv ? 'SAME ' : 'DIFF ';
    const sum = (x) => (x.outcome.ok ? 'ok tokens=' + x.outcome.tokens : `${x.outcome.name}/${x.outcome.kind || '-'}`);
    console.log(tag + sc.name.padEnd(48) + ' js: ' + sum(j).padEnd(26) + ' rust: ' + sum(r).padEnd(26) + ` events ${j.events.length}/${r.events.length}`
      + (j.server ? ` hungUp ${j.server.hungUpEarly}/${r.server && r.server.hungUpEarly}` : ''));
    if (!(sameEv && sameOut)) {
      if (!sameOut) { console.log('     js  outcome:', short(j.outcome, 260)); console.log('     rust outcome:', short(r.outcome, 260)); }
      if (!sameEv) {
        const n = Math.max(j.events.length, r.events.length);
        for (let i = 0; i < n; i++) if (!eq(j.events[i], r.events[i])) { console.log(`     first event diff @${i}: js=${short(j.events[i], 120)} rust=${short(r.events[i], 120)}`); break; }
      }
    }
    if (!sameSrv) console.log('     server view js/rust:', JSON.stringify(j.server), JSON.stringify(r.server));
  }
  // The only knob: opt-in "fix" for unterminated final line.
  const fixed = await once((o) => rustClient({ ...o, flushTail: true }), srv, { variant: 'done-no-newline' });
  console.log('\nrust flush_tail=true on done-no-newline ->', short(fixed.outcome, 120));
  require('fs').writeFileSync(__dirname + '/results.json', JSON.stringify(rows.map((x) => ({ name: x.sc.name, js: x.j.outcome, rust: x.r.outcome, sameEvents: x.sameEv, sameOutcome: x.sameOut, sameServerView: x.sameSrv, jsMs: x.j.ms, rustMs: x.r.ms })), null, 1));
  srv.server.close();
  process.exit(0);
})().catch((e) => { console.error(e); process.exit(1); });
