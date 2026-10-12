'use strict';
/* Loads the REAL js/image.js and js/deskew.js unmodified (new Function, like tests/words.test.js),
   with a minimal canvas/createImageBitmap mock. The mock's drawImage+getImageData is a nearest-neighbour
   resample (browser resampling is not bit-reproducible and is out of scope for the port); every
   getImageData result is recorded so the exact same pixels can be fed to WASM. */
const fs = require('fs');
const path = require('path');
const ROOT = path.join(__dirname, '..', '..', '..', 'js');

const recorded = [];
function getImageDataFor(bmp, w, h) {
  const key = w + 'x' + h;
  bmp._cache = bmp._cache || {};
  let d = bmp._cache[key];
  if (!d) {
    d = new Uint8ClampedArray(w * h * 4);
    for (let y = 0; y < h; y++) {
      const sy = Math.min(bmp.height - 1, Math.floor(((y + 0.5) * bmp.height) / h));
      for (let x = 0; x < w; x++) {
        const sx = Math.min(bmp.width - 1, Math.floor(((x + 0.5) * bmp.width) / w));
        const si = (sy * bmp.width + sx) * 4, di = (y * w + x) * 4;
        d[di] = bmp._d[si]; d[di + 1] = bmp._d[si + 1]; d[di + 2] = bmp._d[si + 2]; d[di + 3] = bmp._d[si + 3];
      }
    }
    bmp._cache[key] = d;
  }
  return d;
}

class Canvas {
  constructor() { this.width = 300; this.height = 150; }
  getContext() {
    const c = this;
    return {
      fillStyle: '', fillRect() {}, translate() {}, rotate() {},
      drawImage(bmp) { c._bmp = bmp; },
      getImageData(x, y, w, h) { const data = getImageDataFor(c._bmp, w, h); recorded.push({ w, h, data }); return { data }; },
    };
  }
}

global.document = { createElement: () => new Canvas() };
global.createImageBitmap = async (blob) => ({ width: blob.w, height: blob.h, _d: blob.data, close() {} });
global.window = { OCR: { image: {} } };
new Function(fs.readFileSync(path.join(ROOT, 'image.js'), 'utf8'))();
new Function(fs.readFileSync(path.join(ROOT, 'deskew.js'), 'utf8'))();
const I = window.OCR.image;

/* edgeColor is private to the deskew.js IIFE; slice its source out verbatim and eval it. */
const src = fs.readFileSync(path.join(ROOT, 'deskew.js'), 'utf8');
const m = src.match(/async function edgeColor\(bmp\) \{[\s\S]*?\n  \}\n/);
if (!m) throw new Error('edgeColor not found in deskew.js');
const edgeColor = new Function('document', m[0] + '\nreturn edgeColor;')(global.document);

module.exports = { I, edgeColor, recorded, Canvas, getImageDataFor };
