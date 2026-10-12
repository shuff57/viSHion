/* Run with: node tests/clean.test.js. O.clean strips model wrappers and leaves handwriting alone. */
const assert = require('assert'), fs = require('fs'), path = require('path');
global.window = { OCR: { ocr: {}, state: { d: { settings: {} } } } };
new Function(fs.readFileSync(path.join(__dirname, '..', 'js', 'ocr.js'), 'utf8'))();
const clean = window.OCR.ocr.clean;
assert.strictEqual(clean('Sylvia plath\n-1932 born'), 'Sylvia plath\n-1932 born');                       // plain: untouched
assert.strictEqual(clean('```\nline one\nline two\n```'), 'line one\nline two');                            // code fence
assert.strictEqual(clean("Here is the transcription:\n\nline one\nline two"), 'line one\nline two');       // preface
assert.strictEqual(clean('Sure! Here\'s the text:\nline one'), 'line one');
assert.strictEqual(clean('line one\nline two\n\nNote: some words were hard to read.'), 'line one\nline two'); // closing note
assert.strictEqual(clean('line one\n\nLet me know if you need anything else!'), 'line one');
assert.strictEqual(clean('Transcript of the meeting\nitem 1'), 'Transcript of the meeting\nitem 1');      // a real title is kept
assert.strictEqual(clean('a\n\n\n\nb'), 'a\n\nb');
assert.strictEqual(clean('note to self: buy milk'), 'note to self: buy milk');                           // note at the START is handwriting
console.log('clean.test.js: all checks passed');
