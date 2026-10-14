/* Builds the test corpus: (a) strings from tests/words.test.js and tests/clean.test.js, run through the real
   js/words.js + O.clean; (b) the DEMO transcripts from js/ocr.js; (c) 30 synthetic transcripts. */
const fs = require('fs'), path = require('path');
const REPO = path.resolve(__dirname, '..', '..');
const src = (f) => fs.readFileSync(path.join(REPO, f), 'utf8');

/* ---- real code: words.js (marked/effective) and ocr.js (clean) loaded with a stub window ---- */
const pages = {};
const win = { OCR: { state: { d: { settings: {}, project: null }, saveSettings() {}, page: (id) => pages[id], updatePage: (id, p) => Object.assign(pages[id], p) },
  judge: { toggle(id, i, a) { pages[id].corrections[i].applied = a; } }, ocr: {} } };
new Function('window', src('js/words.js'))(win);
new Function('window', src('js/ocr.js'))(win);
const W = win.OCR.words, clean = win.OCR.ocr.clean;
const mk = (o) => Object.assign({ id: 'p1', status: 'done', raw: '', corrected: '', corrections: [], resolved: {}, edits: [] }, o);

const C = []; /* { name, text } */
const add = (name, text) => C.push({ name, text });

// words.test.js inputs, as the transcript view sees them (W.marked adds U+E000..U+E002 markers)
{
  const p2 = mk({ raw: 'the confidance interval is [illegible] here', corrected: 'the confidance interval is [illegible] here', corrections: [{ word: 'confidance', term: 'confidence', conf: 0.9, applied: false }] });
  add('words#2 raw', p2.raw); add('words#2 marked/corrected', W.marked(p2, 'corrected')); add('words#2 marked/original', W.marked(p2, 'original'));
  const p6 = mk({ raw: '$x = [illegible]$ and [illegible]', corrected: '$x = [illegible]$ and [illegible]' });
  add('words#6 raw', p6.raw); add('words#6 marked', W.marked(p6, 'corrected'));
  const p4 = mk({ raw: 'the confidance interval is [illegible] here', corrected: 'the confidence interval is [illegible] here', edits: [{ key: 'i:0', text: 'the mean' }], corrections: [{ word: 'confidance', term: 'confidence', conf: 0.9, applied: true }], resolved: { 'c:0': 'suggested', 'i:0': 'typed' } });
  add('words#4 effective', W.effective(p4)); add('words#4 marked', W.marked(p4, 'corrected'));
  add('words#9 raw', 'we use $x$ and the [illegible] deviation of the sample is small');
  add('words#9 marked', W.marked(mk({ raw: 'we use $x$ and the [illegible] deviation of the sample is small', corrected: 'we use $x$ and the [illegible] deviation of the sample is small' }), 'corrected'));
  add('words#1 plain', 'a plain line');
}
// clean.test.js inputs, raw and cleaned
['Sylvia plath\n-1932 born', '```\nline one\nline two\n```', 'Here is the transcription:\n\nline one\nline two', "Sure! Here's the text:\nline one",
  'line one\nline two\n\nNote: some words were hard to read.', 'line one\n\nLet me know if you need anything else!', 'Transcript of the meeting\nitem 1',
  'a\n\n\n\nb', 'note to self: buy milk'].forEach((s, i) => { add('clean#' + i + ' raw', s); add('clean#' + i + ' cleaned', clean(s)); });
// js/ocr.js DEMO transcripts (the app's own sample data)
{
  const m = src('js/ocr.js').match(/const DEMO = (\[[\s\S]*?\n  \]);/);
  const demo = new Function('return ' + m[1])();
  demo.forEach((d, i) => { add('DEMO#' + i, d); add('DEMO#' + i + ' windows-nl', d.replace(/\n/g, '\r\n')); });
}

/* ---- 30 synthetic transcripts ---- */
const S = [
  '$x^2 + y^2 = z^2$ is Pythagoras',
  'Euler: $$e^{i\\pi} + 1 = 0$$',
  '$$\n\\int_0^1 x\\,dx\n= \\frac12\n$$\nthen continue',
  '$$ unterminated display\nstill going\nand going',
  '~~struck~~ and ~~two~~ words ~~ not struck ~~ ok',
  '# Heading looking line\n## second\n#hashtag',
  '- bullet one\n- bullet two\n  - nested\n* star\n+ plus',
  '1. first\n2. second\n3) paren\n10. ten',
  '> quoted line\n>> deeper',
  '    four-space indented code\n\ttab indented',
  '```\nfence in the middle\n```\n``inline`` `tick` ``` triple',
  'odd dollars: costs $5 and $10 total, ok? $ alone',
  'three dollars $a$ $b $c$',
  '$ padded $ and $x $ and $ y$ end',
  'price $$ 5 $$ weird and $$$ triple and $$$$ quad',
  'windows\r\nnewlines\r\n$$\r\nx+y\r\n$$\r\nend\r\n',
  'trailing spaces   \nhard  \nbreak\\\nbackslash break',
  'unicode: caf\u00e9 \u00fcber \u65e5\u672c\u8a9e \ud83d\ude00 \u03b1\u03b2\u03b3 $\\alpha + \\beta$ \u2014 \u201cquotes\u201d',
  'html <b>bold</b> <script>alert(1)</script> & &amp; &lt; &copy; &#35; &#xD7;',
  'links [text](http://example.com) [bad](javascript:alert(1)) <http://auto.link> http://bare.link www.bare.link',
  '*em* **strong** ***both*** _under_ __dunder__ snake_case_word 2*3*4',
  '[illegible] at start, [Illegible] mid, [ILLEGIBLE] caps, [illegible]x[illegible]',
  'Margin: check the $\\bar{x}$ value [illegible]',
  '$\\frac{1}{$ broken tex and $\\undefined$ and $\\\\$ and $}$',
  '$\\def\\a{\\a}\\a$ expansion bomb and $\\text{caf\u00e9 \u65e5\u672c}$',
  'literal \u27e60\u27e7 placeholder in source and $x$ and \u27e699\u27e7 and \u27e6\u27e7',
  'html entity trap: $a<b$ and $x&y$ and "quotes" \'apos\'',
  '\ue000c:0\ue001confidance\ue002 \ue000i:0\ue001[illegible]\ue002 with $x$',
  '= = = =\n***\n---\n___\n===',
  'Mixed: **bold $x^2$ bold** and ~~strike $y$~~ and `code $z$` and [link $w$](http://x.y)',
];
S.forEach((t, i) => add('synthetic#' + String(i + 1).padStart(2, '0'), t));

module.exports = { corpus: C, synthetic: S };
