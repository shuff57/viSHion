/* Run with: node tests/words.test.js
   Checks the words-to-check model (js/words.js) without a browser: effective text, markers,
   unresolved counts, and that [illegible] inside math is ignored. */
const assert = require('assert');
const fs = require('fs');
const path = require('path');

const pages = {};
global.window = { OCR: { state: { d: { settings: {}, project: null }, saveSettings() {}, page: (id) => pages[id], updatePage: (id, patch) => Object.assign(pages[id], patch) }, judge: { toggle(id, i, applied) { pages[id].corrections[i].applied = applied; } } } };
new Function(fs.readFileSync(path.join(__dirname, '..', 'js', 'words.js'), 'utf8'))();
new Function(fs.readFileSync(path.join(__dirname, '..', 'js', 'guess.js'), 'utf8'))();
const W = window.OCR.words;
const S0 = '', S1 = '', S2 = '';

const mk = (o) => Object.assign({ id: 'p1', status: 'done', raw: '', corrected: '', corrections: [], resolved: {}, edits: [] }, o);

// 1. Nothing flagged: text passes through.
let p = mk({ raw: 'a plain line', corrected: 'a plain line' });
assert.strictEqual(W.effective(p), 'a plain line');
assert.strictEqual(W.items(p).length, 0);

// 2. A suggestion and an illegible word, both unresolved.
p = mk({ raw: 'the confidance interval is [illegible] here', corrected: 'the confidance interval is [illegible] here',
  corrections: [{ word: 'confidance', term: 'confidence', conf: 0.9, applied: false }] });
pages.p1 = p;
assert.deepStrictEqual(W.items(p).map((i) => i.key), ['c:0', 'i:0']);
assert.strictEqual(W.count(p), 2);
assert.strictEqual(W.marked(p, 'corrected'), 'the ' + S0 + 'c:0' + S1 + 'confidance' + S2 + ' interval is ' + S0 + 'i:0' + S1 + '[illegible]' + S2 + ' here');

// 3. Using the suggestion: corrected text is swapped in, the word is resolved.
W.choose('p1', 'c:0', 'suggested');
p.corrected = 'the confidence interval is [illegible] here'; // what judge.toggle's rebuild produces
assert.strictEqual(W.count(pages.p1), 1);
assert.ok(W.marked(pages.p1, 'corrected').indexOf(S0 + 'c:0' + S1 + 'confidence' + S2) > 0);

// 4. Typing over the illegible word shows up in the effective text, and in nothing else.
W.choose('p1', 'i:0', 'typed', ' the   mean $');
assert.strictEqual(W.effective(pages.p1), 'the confidence interval is the mean here');
assert.strictEqual(W.count(pages.p1), 0);
assert.strictEqual(pages.p1.raw, 'the confidance interval is [illegible] here');

// 5. The original view shows the raw words, unedited.
assert.ok(W.marked(pages.p1, 'original').indexOf(S0 + 'c:0' + S1 + 'confidance' + S2) > 0);

// 6. [illegible] inside math is not a word to check.
p = mk({ raw: '$x = [illegible]$ and [illegible]', corrected: '$x = [illegible]$ and [illegible]' });
assert.deepStrictEqual(W.items(p).map((i) => i.key), ['i:0']);
assert.strictEqual(W.marked(p, 'corrected'), '$x = [illegible]$ and ' + S0 + 'i:0' + S1 + '[illegible]' + S2);

// 7. Pages that are not done have no words to check.
assert.strictEqual(W.items(mk({ status: 'transcribing', raw: '[illegible]' })).length, 0);

console.log('words.test.js: all checks passed');

// 8. The model's guesses for unreadable words are cleaned: ranked, capped, bounded, de-duplicated.
{
  const g = window.OCR.words.parseGuesses('Sure!\n```json\n[{"n":1,"options":[{"text":"mean","percent":30},{"text":"median","percent":70},{"text":"Mean","percent":5},{"text":"[illegible]","percent":9},{"text":"mode","percent":400}]},{"n":9,"options":[{"text":"x","percent":50}]},{"n":2,"options":[]}]\n```', 2);
  assert.deepStrictEqual(Object.keys(g), ['i:0']);
  assert.deepStrictEqual(g['i:0'].map((o) => o.text), ['mode', 'median', 'mean']);
  assert.ok(g['i:0'].reduce((t, o) => t + o.percent, 0) <= 101);
  assert.deepStrictEqual(window.OCR.words.parseGuesses('no json here', 2), {});
}
console.log('words.test.js: guess parsing ok');

// 9. Remembered fixes: the words around an unreadable word are captured, and reused as hints.
{
  const store = { reference: [], rememberFixes: true };
  window.OCR.state.d = { settings: store, project: { meta: { topic: 'Stats' } } };
  window.OCR.state.saveSettings = (patch) => Object.assign(store, patch);
  const W2 = window.OCR.words, pg = mk({ raw: 'we use $x$ and the [illegible] deviation of the sample is small', corrected: 'we use $x$ and the [illegible] deviation of the sample is small' });
  assert.deepStrictEqual(W2.contextFor(pg, 'i:0'), { before: 'we use and the', after: 'deviation of the sample is' });
  W2.remember(pg, 'i:0', 'standard');
  W2.remember(pg, 'i:0', 'standard'); // same fix twice is kept once
  assert.strictEqual(store.reference.length, 1);
  assert.deepStrictEqual(W2.referenceWords('Stats'), ['standard']);
  assert.deepStrictEqual(W2.referenceWords('Calculus'), []); // other subjects' fixes stay out of the way
  assert.ok(W2.referenceExamples('Stats')[0].indexOf('standard') > 0);
  store.rememberFixes = false; W2.remember(pg, 'i:0', 'other');
  assert.strictEqual(store.reference.length, 1);
}
console.log('words.test.js: remembered fixes ok');
