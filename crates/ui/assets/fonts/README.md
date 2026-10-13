# Bundled fonts

Latin subsets of Instrument Sans (400, 500, 600) and Source Serif 4 (400, 600, 400 italic), the same files the
legacy app ships in `/fonts` as woff2, converted to plain TrueType (egui cannot read woff2). Both families are
licensed under the SIL Open Font License 1.1 (Instrument Sans (c) The Instrument Sans Project Authors,
Source Serif 4 (c) Adobe). Kalam and Caveat are not bundled (design-preview only).

`hack-latin-400-normal.ttf` is Hack Regular (MIT licence, Bitstream Vera licence for the base glyphs) cut down with
`fontTools.subset` to Basic Latin, Latin-1, Latin Extended-A, Greek, general punctuation, arrows and mathematical
operators. It is the monospace face (code spans, math source fallback, plain text view, wordmark) and the last
fallback for glyphs the two main families lack. egui's own default faces are not linked (they were 1.4 MB).

`hack-latin-700-normal.ttf` is Hack Bold (same licences) cut down to Basic Latin. It is only the weight of the
`viSHion` wordmark (header.js sets the logo in 600 weight monospace).

`fallback-cjk-emoji.ttf` (4.2 MB) is one merged face for scripts the Latin faces do not have, because student
writing and names are in any script: Noto Sans JP (kana, punctuation, the JIS X 0208 kanji), Noto Sans SC (the
GB 2312 hanzi JP lacks), Noto Sans KR (the 2,350 KS X 1001 syllables and the jamo) and Noto Emoji (monochrome,
all emoji code points). All are SIL Open Font License 1.1 (Copyright Google LLC / Adobe, obtained as the
`@fontsource` static 400 files, merged with fontTools, layout tables dropped because egui does not shape).
Native links it in; the web build does not (`ui::theme::bundled_fallback`), the host fetches
`fallback-cjk-emoji.ttf` the first time the open page has a character no bundled face can draw
(`Cmd::LoadFallbackFont`), the same way the math font is fetched.

`twemoji-colr.ttf` (1.4 MB) is Twemoji Mozilla 0.7.0 (COLR v0, 3,689 colour glyphs, ligatures for ZWJ sequences,
flags and keycaps), the Mozilla COLR build of Twitter's Twemoji. Graphics (c) 2020 Twitter, Inc and other
contributors, licensed CC-BY 4.0 (https://creativecommons.org/licenses/by/4.0/); the font tooling is Apache 2.0.
`ui::emoji` rasterises its layers into small bitmaps that the transcript draws between the words, so emoji are
in colour on native and on the web (egui itself only draws single-colour outlines). Native links the file in;
the web build fetches it the first time the open page has an emoji (`Cmd::LoadEmojiFont`).

`vishion-marks.ttf` (1 KB) is a face drawn for this app (generated with fontTools, no third-party outlines): the
check mark and cross (U+2713, U+2717) and the ballot boxes (U+2610 to U+2612), which none of the text faces
above have and which teachers write on student work.
