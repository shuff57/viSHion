# Spike S1: imaging loops in Rust/WASM, bit-exact vs JS?

Verdict: **GO-WITH-CAVEATS**. The port is bit-exact on every test (details below). The speed-up is real
in the micro-benchmark but is a few tens of milliseconds per page, which does not matter next to tens of
seconds of model inference. Port it for code-unification reasons (one Rust codebase), not for speed.

## Layout
- `core/` `imaging-core`: pure Rust, zero dependencies. `line_bands`, `skew_angle`, `edge_color`, `js_round`.
- `wasm/` `imaging-wasm`: thin wasm-bindgen wrapper over `&[u8]` RGBA + width + height (wasm-bindgen =0.2.129,
  wasm-bindgen-cli 0.2.129 installed, no wasm-opt). Output wasm is 41 KB.
- `harness/jsenv.js` loads the REAL `js/image.js` and `js/deskew.js` unmodified via `new Function`, with a
  small canvas / `createImageBitmap` mock. `edgeColor` is private to the deskew IIFE, so its source is sliced
  out of `deskew.js` verbatim and eval'd.
- `harness/fixtures.js` deterministic generator (mulberry32): ruled/plain text-like pages, rotation -8..+8,
  noise, shadow gradient, white and coloured borders, blank, sparse, dense; each at 420x560 and 600x800.
- `harness/run.js` exactness check, `harness/bench.js` timings, `harness/ulp-risk.js` trig-risk estimate.
- `build.sh` builds wasm and runs wasm-bindgen (`--target nodejs`) into `pkg/`.

Run: `sh build.sh && node harness/run.js && node harness/bench.js`

## What is and is not covered (canvas dependence)
All three functions start with canvas `drawImage` (resample) + `getImageData`. That step is browser code
(bilinear/area filtering, not reproducible bit-for-bit anywhere else) and is **out of scope**: the port starts
from the RGBA that `getImageData` returned. In the harness the mock does nearest-neighbour resampling and every
`getImageData` result is recorded, so JS and WASM receive byte-identical pixels. Pixels are assumed opaque
(JS composites onto white first; fixtures are opaque). Everything after the pixels (luma, integral image, run
scans, band logic, scoring, angle search, rounding) is ported and compared on its real output.

## Exact-match results
Compared with `Object.is` on every number (bands: x,y,w,h,r,s as f64; angle; edge colour string).
All 40 fixtures: bands EXACT, angle EXACT, edgeColor EXACT.

| fixture (each at w420 and w600) | nominal deg | bands (w420/w600) | JS angle = WASM angle | result |
|---|---|---|---|---|
| plain_0 | 0 | 1/1 | 0 | EXACT |
| plain_-8 | -8 | 10/18 | -8 | EXACT |
| plain_+8 | +8 | 13/13 | 8 | EXACT |
| plain_-3 | -3 | 21/21 | -3 | EXACT |
| plain_+1.5 | +1.5 | 15/17 | 1.5 | EXACT |
| plain_+0.3 | +0.3 | 3/2 | 0 (below MIN) | EXACT |
| ruled_0 | 0 | 12/11 | 0 | EXACT |
| ruled_-5 | -5 | 15/14 | -5 | EXACT |
| ruled_+4 | +4 | 17/19 | 4 | EXACT |
| ruled_+7.5 | +7.5 | 11/15 | 7.5 | EXACT |
| plain_+2_noise | +2 | 17/20 | 2 | EXACT |
| ruled_-2_noise | -2 | 21/21 | -2 | EXACT |
| plain_-6_noise_shadow | -6 | 15/13 | -6 | EXACT |
| plain_+3_whiteborder | +3 | 21/21 | 3 (edge rgb 255,255,255) | EXACT |
| plain_-4_colorborder | -4 | 16/17 | -4 (edge rgb 118,82,56) | EXACT |
| ruled_+5_colorborder_noise | +5 | 15/16 | 5 (edge ~rgb 120,83,58) | EXACT |
| blank | 0 | 0/0 | 0 | EXACT |
| blank_noise | 0 | 0/0 | 0 | EXACT |
| sparse_3lines | -1 | 2/2 | -1 | EXACT |
| dense_-1 | -1 | 16/8 | -1 | EXACT |

Plus: 150 random fuzz pages (random size, angle -12..12, noise, border, shadow, pitch; 1620 bands) 0
mismatches; 5000 random 16x16 edgeColor cases (covers every x.5 rounding residue) 0 mismatches. The detected
angle equals the nominal angle in all fixtures with |angle| >= 0.4, with the same sign convention
(positive = clockwise).

## Divergences found
1. **cos() is not bit-identical.** V8's `Math.cos` and Rust's wasm `f64::cos` (libm port) differ by 1 ulp on
   3 of 492 angles the search visits (e.g. -6.7, -5.7, +8.6 degrees; sin matched on all). This is the only
   numeric divergence. It never changed an output: the value only feeds `Math.round(-x*sin + y*cos)`, and a
   1-ulp change flips a bin only if the value lands within ~1e-13 of a .5 boundary. `harness/ulp-risk.js`:
   0 flips in 769 M point/angle evaluations (angle 0 is exact on both sides). If a hard guarantee is wanted,
   port fdlibm `__kernel_cos` / `__ieee754_cos` (V8's `ieee754.cc`) into core; about 60 lines.
2. Nothing else diverged. Items that needed care and are handled: `Math.round` half toward +inf (`js_round`);
   Uint8 luma truncation; f64 integral image; Float32Array `row`/`bins` (f32); `sm` is a plain Array of doubles
   (f64, so band score `s` is f64 not f32); `row[-1] || 0`; `paper = sorted[..] || 255` (a 0 value becomes
   255); `bands.map` reading neighbours from the OLD array; the refine loop `a <= best + 0.5` re-reading a
   mutating `best` with `a += 0.1` accumulation; `Math.round(best*10)/10`; `score` products in f64.

## Timing (Node 25 / V8, single thread, same machine, wasm release+LTO, no wasm-opt)
Median of 20 warm runs after 5 warm-ups; WASM time includes copying pixels into wasm memory and the result
out. Pixel resampling is excluded on both sides (cached). `lineBands` always works at width 420 (a 600 px
source is resampled to 420); `skewAngle` works at min(600, source width).

| page | fn | source W | work size | JS cold ms | WASM cold ms | JS median ms | WASM median ms | speed-up |
|---|---|---|---|---|---|---|---|---|
| plain 22 lines | lineBands | 420 | 420x560 | 49.5 | 4.9 | 38.4 | 1.6 | 23.8x |
| plain 22 lines | skewAngle | 420 | 420x560 | 23.5 | 11.3 | 16.9 | 9.3 | 1.8x |
| plain 22 lines | lineBands | 600 | 420x560 | 47.5 | 1.9 | 40.7 | 1.8 | 23.1x |
| plain 22 lines | skewAngle | 600 | 600x800 | 27.7 | 16.0 | 27.8 | 14.1 | 2.0x |
| ruled + noise | lineBands | 420 | 420x560 | 44.7 | 1.9 | 42.4 | 1.9 | 22.4x |
| ruled + noise | skewAngle | 420 | 420x560 | 18.2 | 11.1 | 16.5 | 12.7 | 1.3x |
| ruled + noise | lineBands | 600 | 420x560 | 47.8 | 2.0 | 40.8 | 1.7 | 24.6x |
| ruled + noise | skewAngle | 600 | 600x800 | 25.3 | 11.7 | 22.4 | 12.4 | 1.8x |

Reading it: `lineBands` is ~23x faster (the JS calls a closure with 4 bound-checked integral-image reads per
pixel, twice per ink pixel). `skewAngle` is only 1.3-2x: it is bound by the ~60 angle passes over up to 20 k
points, which V8 already compiles well. Per page, total analysis goes from about 60-70 ms to about 12-16 ms.
Cold first-call numbers matter most (one call per page): ~45 ms JS vs ~2 ms WASM for lineBands.

## Verdict: GO-WITH-CAVEATS
- Feasibility: GO. Bit-exact on all fixtures, fuzz, and rounding edge cases; no web deps in core; 41 KB wasm.
- Speed: not a reason. Saving ~50 ms per page against model inference of tens of seconds is under 0.5 percent
  of wall time and is invisible. Do not justify the port on performance.
- Caveats: (1) the canvas resample/decode stays outside the port; in a non-browser Rust app a different
  resizer will give different pixels, so end-to-end parity with the old JS must be re-baselined (the analysis
  stage itself is exact given identical pixels). (2) cos 1-ulp difference (harmless in practice, fixable by
  porting fdlibm cos). (3) opaque-pixel assumption (alpha composite onto white not ported). (4) timings are
  Node/V8 only; no browser run.
- Recommendation: port it when the rewrite wants one Rust code path, keep `harness/run.js` as the regression
  suite (it runs the real JS), and treat any performance gain as a bonus.
