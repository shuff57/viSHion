/* Run with: node tests/judge.test.js. Suggestions may only be spelling fixes toward a glossary term. */
const assert = require('assert'), fs = require('fs'), path = require('path');
global.window = { OCR: { judge: {}, state: { d: { settings: {} } } } };
new Function(fs.readFileSync(path.join(__dirname, '..', 'js', 'judge.js'), 'utf8'))();
const J = window.OCR.judge, list = ['confidence interval', 'standard deviation', 'population', 'sample'];
const text = 'the populaton mean and a standrd deviation, so the sample is good, we are sure';
assert.ok(J.accept({ word: 'populaton', term: 'population', conf: 0.9 }, list, text));          // misspelling of a term
assert.ok(!J.accept({ word: 'sure', term: 'confident', conf: 0.9 }, list, text));              // not a glossary term: rewording
assert.ok(!J.accept({ word: 'good', term: 'population', conf: 0.9 }, list, text));            // a different word, too far apart
assert.ok(!J.accept({ word: 'sample', term: 'population', conf: 0.9 }, list, text));          // already a glossary word
assert.ok(!J.accept({ word: 'missing', term: 'sample', conf: 0.9 }, list, text));             // word is not in the transcript
console.log('judge.test.js: all checks passed');
