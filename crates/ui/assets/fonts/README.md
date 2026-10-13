# Bundled fonts

Latin subsets of Instrument Sans (400, 500, 600) and Source Serif 4 (400, 600, 400 italic), the same files the
legacy app ships in `/fonts` as woff2, converted to plain TrueType (egui cannot read woff2). Both families are
licensed under the SIL Open Font License 1.1 (Instrument Sans (c) The Instrument Sans Project Authors,
Source Serif 4 (c) Adobe). Kalam and Caveat are not bundled (design-preview only).

`hack-latin-400-normal.ttf` is Hack Regular (MIT licence, Bitstream Vera licence for the base glyphs) cut down with
`fontTools.subset` to Basic Latin, Latin-1, Latin Extended-A, Greek, general punctuation, arrows and mathematical
operators. It is the monospace face (code spans, math source fallback, plain text view, wordmark) and the last
fallback for glyphs the two main families lack. egui's own default faces are not linked (they were 1.4 MB).
