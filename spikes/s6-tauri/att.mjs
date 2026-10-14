import { chromium } from 'playwright';
try { const b = await chromium.connectOverCDP('http://127.0.0.1:9477'); console.log('ok', b.contexts().length, b.contexts().map(c=>c.pages().map(p=>p.url()))); await b.close(); } catch (e) { console.log('ERR', e.message.slice(0,800)); }
