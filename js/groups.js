/* Students: pages grouped by page.student.name, and the status of each group.
   A page with no name is its own row, named after the file. Read-only over the store. */
(function () {
  'use strict';
  const OCR = window.OCR, S = OCR.state, G = (OCR.groups = OCR.groups || {});

  const stem = (n) => String(n || 'Untitled').replace(/\.[a-z0-9]+$/i, '');
  const norm = (n) => String(n).trim().toLowerCase().replace(/\s+/g, ' ');
  G.nameOf = (p) => (p.student && p.student.name && p.student.name.trim()) || stem(p.name);
  G.keyOf = (p) => (p.student && p.student.name && p.student.name.trim() ? 'n:' + norm(p.student.name) : 'p:' + p.id);

  const wordCount = (p) => (p.status === 'done' ? (OCR.words.effective(p).match(/\S+/g) || []).length : 0);

  /* failed > transcribing > waiting > check > ready */
  function statusOf(pages, open) {
    const Q = OCR.queue;
    if (pages.some((p) => p.status === 'error')) return 'failed';
    if (pages.some((p) => p.status === 'transcribing')) return 'processing';
    if (pages.some((p) => p.status === 'pending')) return pages.some((p) => Q.isQueued(p.id)) ? 'queued' : 'waiting';
    if (open > 0 && !pages.every((p) => p.reviewed)) return 'check';
    return 'ready';
  }

  G.list = () => {
    const by = new Map();
    S.pages().forEach((p) => {
      const k = G.keyOf(p);
      if (!by.has(k)) by.set(k, { key: k, name: G.nameOf(p), pages: [] });
      by.get(k).pages.push(p);
    });
    const out = Array.from(by.values()).map((g) => {
      g.open = g.pages.reduce((n, p) => n + OCR.words.count(p), 0);
      g.words = g.pages.reduce((n, p) => n + wordCount(p), 0);
      g.failed = g.pages.filter((p) => p.status === 'error').map((p) => p.id);
      g.status = statusOf(g.pages, g.open);
      g.source = (g.pages[0].student && g.pages[0].student.source) || null;
      return g;
    });
    return out.sort((a, b) => OCR.natCompare(a.name, b.name));
  };

  G.of = (pageId, list) => {
    const p = S.page(pageId);
    if (!p) return null;
    const k = G.keyOf(p);
    return (list || G.list()).find((g) => g.key === k) || null;
  };

  /* Counts for the segmented bar and the filter tabs. "processing" covers waiting pages too. */
  G.tally = (list) => {
    const t = { total: list.length, ready: 0, check: 0, failed: 0, processing: 0 };
    list.forEach((g) => {
      if (g.status === 'ready') t.ready++;
      else if (g.status === 'check') t.check++;
      else if (g.status === 'failed') t.failed++;
      else t.processing++;
    });
    return t;
  };
  G.matches = (g, f) => f === 'all' || (f === 'processing' ? g.status === 'processing' || g.status === 'queued' || g.status === 'waiting' : g.status === f);

  /* Names already used in this assignment, for the Add screen and the change-student dialog. */
  G.names = () => G.list().filter((g) => g.pages.some((p) => p.student && p.student.name)).map((g) => g.name);
})();
