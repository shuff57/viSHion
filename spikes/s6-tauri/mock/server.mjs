// Mock OpenAI-style streaming server. NO CORS headers unless ?cors=1. Plain http, binds 0.0.0.0.
import http from 'node:http';
const PORT = +process.env.PORT || 8799;
const srv = http.createServer((req, res) => {
  const u = new URL(req.url, 'http://x');
  const cors = u.searchParams.get('cors') === '1';
  const base = { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' };
  if (cors) Object.assign(base, { 'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': '*' });
  console.log(new Date().toISOString(), req.method, req.url, 'origin=' + req.headers.origin);
  if (req.method === 'OPTIONS') { res.writeHead(cors ? 204 : 200, cors ? base : {}); return res.end(); }
  if (u.pathname === '/health') { res.writeHead(200); return res.end('ok'); }
  req.resume();
  const total = u.pathname === '/slow' ? 1e9 : 6;
  res.writeHead(200, base);
  let i = 0;
  const iv = setInterval(() => {
    if (i >= total) { clearInterval(iv); res.write('data: [DONE]\n\n'); return res.end(); }
    res.write('data: ' + JSON.stringify({ choices: [{ delta: { content: 'tok' + i } }] }) + '\n\n'); i++;
  }, 300);
  res.on('close', () => { clearInterval(iv); console.log(new Date().toISOString(), 'CLIENT-CLOSED after', i, 'chunks', u.pathname); });
});
srv.listen(PORT, '0.0.0.0', () => console.log('mock on', PORT));
