/* Local relay. Browsers cannot call https://ollama.com directly (it sends no CORS headers), so viSHion
   talks to this relay and the relay forwards to Ollama Cloud with your API key as sent by the page.
     node tools/relay.js                 listens on http://localhost:8189
   Page requests look like  http://localhost:8189/u/ollama.com/v1/chat/completions  and are forwarded to
   https://ollama.com/v1/chat/completions. Only ollama.com is allowed. Nothing is stored or logged.
   Environment: PORT (default 8189), UPSTREAM_KEY (used when the page sends no key). */
const http = require('http'), https = require('https');
const PORT = +(process.env.PORT || 8189), KEY = process.env.UPSTREAM_KEY || '';
const ALLOW = /^([a-z0-9-]+\.)*ollama\.com$/i;

http.createServer((req, res) => {
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Headers', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET,POST,OPTIONS');
  if (req.method === 'OPTIONS') { res.writeHead(204); return res.end(); }
  if (req.url === '/health') { res.writeHead(200, { 'Content-Type': 'application/json' }); return res.end('{"status":"ok","relay":true}'); }
  const m = req.url.match(/^\/u\/([^/]+)(\/.*)?$/);
  if (!m || !ALLOW.test(m[1])) { res.writeHead(403, { 'Content-Type': 'application/json' }); return res.end('{"error":"This relay only forwards to ollama.com."}'); }
  const host = m[1], path = m[2] || '/';
  const chunks = [];
  req.on('data', (c) => chunks.push(c));
  req.on('end', () => {
    const body = Buffer.concat(chunks);
    const headers = { 'content-type': req.headers['content-type'] || 'application/json', accept: req.headers.accept || '*/*' };
    if (body.length) headers['content-length'] = body.length;
    const auth = req.headers.authorization || (KEY ? 'Bearer ' + KEY : '');
    if (auth) headers.authorization = auth;
    const up = https.request({ host, port: 443, path, method: req.method, headers }, (u) => {
      res.writeHead(u.statusCode, { 'Content-Type': u.headers['content-type'] || 'application/json' });
      u.pipe(res);
    });
    up.on('error', (e) => { res.writeHead(502, { 'Content-Type': 'application/json' }); res.end(JSON.stringify({ error: String(e.message || e) })); });
    up.end(body);
  });
}).listen(PORT, '127.0.0.1', () => console.log('relay on http://localhost:' + PORT + '  ->  https://ollama.com (via /u/ollama.com/...)'));
