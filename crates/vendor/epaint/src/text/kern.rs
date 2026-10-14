//! viSHion patch: OpenType pair kerning (GPOS `kern`). epaint only reads the legacy `kern` table, which
//! the Instrument Sans and Source Serif 4 files do not have, so text came out a little wider than a browser
//! sets it (a browser applies GPOS kerning by default). This reads the pair adjustments of the `kern`
//! feature and answers them in font design units.
use std::collections::HashMap;
use std::sync::Mutex;
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use owned_ttf_parser::AsFaceRef as _;
use ttf_parser::{GlyphId, Tag};

pub struct Kern {
    face: owned_ttf_parser::OwnedFace,
    /// Lookup indices of every `kern` feature, once.
    lookups: Vec<u16>,
    cache: Mutex<HashMap<(u16, u16), i16>>,
}

impl Kern {
    /// `None` when the font has no GPOS `kern` feature (the legacy table, if any, is used then).
    pub fn new(bytes: &[u8], index: u32) -> Option<Self> {
        let face = owned_ttf_parser::OwnedFace::from_vec(bytes.to_vec(), index).ok()?;
        let mut lookups = vec![];
        {
            let f = face.as_face_ref();
            let gpos = f.tables().gpos?;
            for feat in gpos.features {
                if feat.tag == Tag::from_bytes(b"kern") {
                    for i in 0..feat.lookup_indices.len() {
                        if let Some(l) = feat.lookup_indices.get(i) {
                            if !lookups.contains(&l) {
                                lookups.push(l);
                            }
                        }
                    }
                }
            }
        }
        if lookups.is_empty() {
            return None;
        }
        Some(Self { face, lookups, cache: Mutex::new(HashMap::new()) })
    }

    /// The advance adjustment of the first glyph of the pair, in font design units (negative tightens).
    pub fn units(&self, a: u16, b: u16) -> i16 {
        if let Some(v) = self.cache.lock().ok().and_then(|c| c.get(&(a, b)).copied()) {
            return v;
        }
        let v = self.compute(GlyphId(a), GlyphId(b));
        if let Ok(mut c) = self.cache.lock() {
            c.insert((a, b), v);
        }
        v
    }

    fn compute(&self, a: GlyphId, b: GlyphId) -> i16 {
        let f = self.face.as_face_ref();
        let Some(gpos) = f.tables().gpos else { return 0 };
        let mut total: i32 = 0;
        for &li in &self.lookups {
            let Some(lookup) = gpos.lookups.get(li) else { continue };
            for sub in lookup.subtables.into_iter::<PositioningSubtable<'_>>() {
                let PositioningSubtable::Pair(pair) = sub else { continue };
                // the first subtable that has the pair decides this lookup
                let found = match pair {
                    PairAdjustment::Format1 { coverage, sets } => coverage
                        .get(a)
                        .and_then(|i| sets.get(i))
                        .and_then(|set| set.get(b))
                        .map(|(v1, _)| v1.x_advance),
                    PairAdjustment::Format2 { coverage, classes, matrix } => {
                        if coverage.get(a).is_some() {
                            matrix.get((classes.0.get(a), classes.1.get(b))).map(|(v1, _)| v1.x_advance)
                        } else {
                            None
                        }
                    }
                };
                if let Some(v) = found {
                    total += v as i32;
                    break;
                }
            }
        }
        total.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }
}
