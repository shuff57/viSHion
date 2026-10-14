// Dumps node's ICU order of every Han-script code point (groups of base-equal chars) to
// tools/parity/han_order.json. `cargo test -p vishion-core --test gen_han -- --ignored` turns it
// into crates/core/src/hanfold.rs (ICU4X orders Han by code point; node/ICU 78 by radical-stroke).
const fs = require('fs'), path = require('path');
const col = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
const han = /\p{Script=Han}|\p{Unified_Ideograph}/u;
const arr = [];
for (let cp = 0x2e80; cp <= 0x3ffff; cp++) {
  if (cp >= 0xd800 && cp < 0xe000) continue;
  const ch = String.fromCodePoint(cp);
  if (han.test(ch)) arr.push(ch);
}
arr.sort((a, b) => col.compare(a, b) || a.codePointAt(0) - b.codePointAt(0));
const groups = [];
for (const ch of arr) {
  const g = groups[groups.length - 1];
  if (g && col.compare(String.fromCodePoint(g[0]), ch) === 0) g.push(ch.codePointAt(0)); else groups.push([ch.codePointAt(0)]);
}
fs.writeFileSync(path.join(__dirname, 'han_order.json'), JSON.stringify(groups));
console.log('han chars', arr.length, 'groups', groups.length);
