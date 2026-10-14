//! One-time import of sessions saved by the old single-file app, as pure functions over the legacy
//! data. Mirrors js/fs-legacy.js: the caller reads `localStorage["ocr_sessions_v2"]` and the legacy
//! IndexedDB `ocr_images/images` store (data URLs by session id), and writes the returned plan through
//! the project layer (`vishion_store::projects::Projects::apply_legacy`).
//!
//! The two steps JS does with a canvas (re-encoding the picture to the stored JPEG, and
//! `render.computeBoxes`) are platform work: they are closures here, so core stays I/O free.
use crate::jsx;
use crate::model::{Assignment, Page, Project};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const SESSIONS_KEY: &str = "ocr_sessions_v2";
pub const DEFAULT_NAME: &str = "Imported from the old app";

/// `sessions()` of fs-legacy.js: the parsed array filtered to usable sessions; `[]` on any parse problem.
pub fn parse_sessions(stored: Option<&str>) -> Vec<Value> {
    let text = match stored {
        Some(t) if !t.is_empty() => t,
        _ => return vec![],
    };
    match crate::lenient::parse::<Value>(text) {
        Ok(Value::Array(a)) => a
            .into_iter()
            .filter(|s| {
                s.is_object()
                    && jsx::truthy(s.get("id"))
                    && (jsx::truthy(s.get("lastRaw")) || jsx::truthy(s.get("preRaw")) || jsx::truthy(s.get("hasImage")))
            })
            .collect(),
        _ => vec![],
    }
}

/// `fs.legacyAvailable()`.
pub fn available(stored: Option<&str>) -> bool {
    !parse_sessions(stored).is_empty()
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | u32::from(b64_val(c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Decode a `data:` URL into `(mime, bytes)`. `None` when it is not a data URL or is malformed.
pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (head, body) = rest.split_once(',')?;
    let mut parts = head.split(';');
    let mime = parts.next().unwrap_or("");
    let is_b64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    let mime = if mime.is_empty() { "text/plain".to_string() } else { mime.to_string() };
    let bytes = if is_b64 { base64_decode(body)? } else { percent_decode(body) };
    Some((mime, bytes))
}

/// Inputs of [`plan`].
pub struct Options<'a> {
    /// `opt.name`; falls back to [`DEFAULT_NAME`].
    pub name: Option<&'a str>,
    /// `settings.topics`; a session topic is only picked when it names one of these.
    pub topics: &'a Map<String, Value>,
    /// `Date.now()`.
    pub now: i64,
}

/// A page of the plan plus the stored JPEG bytes for `images/<id>.jpg`, when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedPage {
    pub page: Page,
    pub image: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    /// Final `project.json` (what `createProject` wrote, then `Object.assign`ed with the import result).
    pub meta: Project,
    pub pages: Vec<PlannedPage>,
    pub with_image: usize,
}

fn js_str(v: Option<&Value>) -> String {
    v.map(jsx::to_string).unwrap_or_else(|| "undefined".into())
}

/// `a || b` over JSON values, as a string (`String(a || b || '')`).
fn or_str(vals: &[Option<&Value>]) -> String {
    for v in vals {
        if jsx::truthy(*v) {
            return js_str(*v);
        }
    }
    String::new()
}

/// `importLegacy` minus I/O. `prepare_image(data_url, page_name)` returns the final JPEG bytes (or `None`
/// when the picture cannot be decoded); `boxes(image, raw)` is `render.computeBoxes`. `new_id` is `OCR.uid`.
pub fn plan(
    sessions: &[Value],
    images: &BTreeMap<String, String>,
    opt: &Options,
    new_id: &mut dyn FnMut() -> String,
    prepare_image: &mut dyn FnMut(&str, &str) -> Option<Vec<u8>>,
    boxes: &mut dyn FnMut(&[u8], &str) -> Vec<Value>,
) -> Result<Plan, String> {
    if sessions.is_empty() {
        return Err("No sessions from the old app were found in this browser.".into());
    }
    let mut list: Vec<&Value> = sessions.iter().collect();
    let created = |s: &Value| -> f64 {
        let c = s.get("created");
        if jsx::truthy(c) {
            let n = jsx::number(c.unwrap());
            if n.is_nan() {
                0.0
            } else {
                n
            }
        } else {
            0.0
        }
    };
    list.sort_by(|a, b| created(a).partial_cmp(&created(b)).unwrap_or(std::cmp::Ordering::Equal));

    let name = match opt.name {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => DEFAULT_NAME.to_string(),
    };
    let pick = list
        .iter()
        .filter_map(|s| s.get("topic"))
        .filter(|t| jsx::truthy(Some(t)))
        .map(jsx::to_string)
        .find(|t| jsx::truthy(opt.topics.get(t)))
        .unwrap_or_else(|| "General".into());
    let prompt = list
        .iter()
        .map(|s| {
            let p = s.get("prompt");
            if jsx::truthy(p) {
                jsx::trim(&js_str(p)).to_string()
            } else {
                String::new()
            }
        })
        .find(|p| !p.is_empty())
        .unwrap_or_default();

    let mut order: Vec<String> = vec![];
    let mut pages = vec![];
    let (mut with_image, mut done_n) = (0usize, 0usize);

    for s in list {
        let pname = or_str(&[s.get("name"), Some(&json!("Imported page"))]);
        let mut p = Assignment::new_page(&new_id(), &pname);
        let raw = or_str(&[s.get("preRaw"), s.get("lastRaw")]);
        let final_text = {
            let l = s.get("lastRaw");
            if jsx::truthy(l) {
                js_str(l)
            } else {
                raw.clone()
            }
        };
        let corrections: Vec<Value> = match s.get("appliedDiff") {
            Some(Value::Array(a)) => a
                .iter()
                .filter(|d| jsx::truthy(d.get("orig")) && jsx::truthy(d.get("term")))
                .map(|d| {
                    json!({
                        "word": js_str(d.get("orig")),
                        "term": js_str(d.get("term")),
                        "conf": 1,
                        "applied": true
                    })
                })
                .collect(),
            _ => vec![],
        };
        p.set_carried("status", json!(if raw.is_empty() { "pending" } else { "done" }));
        p.set_carried("raw", json!(raw));
        p.set_carried("corrected", json!(final_text));
        let n_corr = corrections.len();
        p.set_carried("corrections", Value::Array(corrections));
        p.set_carried("timings", json!({"ms": 0, "tokens": 0}));
        let mut resolved = Map::new();
        for i in 0..n_corr {
            resolved.insert(format!("c:{i}"), json!("suggested"));
        }
        p.set_carried("resolved", Value::Object(resolved));

        let image = s
            .get("id")
            .map(jsx::to_string)
            .and_then(|id| images.get(&id))
            .filter(|u| u.starts_with("data:"))
            .and_then(|u| prepare_image(u, &pname));
        match image {
            Some(bytes) => {
                with_image += 1;
                if !raw.is_empty() {
                    let b = boxes(&bytes, &raw);
                    p.set_carried("boxes", Value::Array(b));
                }
                finish(&mut p, &mut order, &mut done_n);
                pages.push(PlannedPage { page: p, image: Some(bytes) });
            }
            None if raw.is_empty() => continue, // nothing worth keeping
            None => {
                finish(&mut p, &mut order, &mut done_n);
                pages.push(PlannedPage { page: p, image: None });
            }
        }
    }

    // createProject: normalize({ name, prompt: '', created, updated, stats }), then the final assign.
    let mut m = Map::new();
    m.insert("name".into(), json!(name));
    m.insert("prompt".into(), json!(""));
    m.insert("created".into(), json!(opt.now));
    m.insert("updated".into(), json!(opt.now));
    m.insert("stats".into(), json!({"pages": 0, "done": 0}));
    let mut meta = Project::from_json(&serde_json::to_string(&Value::Object(m)).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?
        .normalized(opt.now);
    meta.set_order(&order);
    meta.set_carried("topic", json!(pick));
    meta.set_carried("prompt", json!(prompt));
    meta.set_carried("updated", json!(opt.now));
    meta.set_carried("stats", json!({"pages": order.len(), "done": done_n}));
    Ok(Plan { name, meta, pages, with_image })
}

fn finish(p: &mut Page, order: &mut Vec<String>, done_n: &mut usize) {
    if p.status() == "done" {
        *done_n += 1;
    }
    p.set_carried("order", Value::from(order.len() as i64));
    order.push(p.id().to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sessions: &str, images: &[(&str, &str)], name: Option<&str>) -> Result<Plan, String> {
        let list = parse_sessions(Some(sessions));
        let imgs: BTreeMap<String, String> = images.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let topics = match json!({"General": {}, "Medical": {}}) {
            Value::Object(m) => m,
            _ => unreachable!(),
        };
        let mut n = 0;
        plan(
            &list,
            &imgs,
            &Options { name, topics: &topics, now: 1760000000000 },
            &mut || {
                n += 1;
                format!("id{n}")
            },
            &mut |u, _| decode_data_url(u).map(|(_, b)| b).filter(|b| b.starts_with(&[0x89, b'P'])),
            &mut |_, raw| vec![json!({"raw": raw})],
        )
    }

    #[test]
    fn data_urls() {
        assert_eq!(decode_data_url("data:text/plain;base64,aGk="), Some(("text/plain".into(), b"hi".to_vec())));
        assert_eq!(decode_data_url("data:,a%20b"), Some(("text/plain".into(), b"a b".to_vec())));
        assert_eq!(decode_data_url("http://x"), None);
        assert_eq!(decode_data_url("data:image/png;base64,iVBORw0KGgo=").unwrap().1, vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10]);
    }

    #[test]
    fn session_filter() {
        let s = r#"[{"id":"a","lastRaw":"x"},{"id":"","lastRaw":"x"},{"id":"b"},null,{"id":"c","hasImage":true},5]"#;
        let ids: Vec<_> = parse_sessions(Some(s)).iter().map(|v| v["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids, vec!["a", "c"]);
        assert!(parse_sessions(Some("not json")).is_empty());
        assert!(parse_sessions(Some("{}")).is_empty());
        assert!(!available(None));
    }

    #[test]
    fn empty_is_an_error() {
        assert!(run("[]", &[], None).is_err());
    }

    #[test]
    fn mirrors_import_legacy() {
        let s = r#"[
          {"id":"s2","name":"Legacy é","created":1700000001000,"lastRaw":"two","hasImage":false},
          {"id":"s1","name":"Legacy one","created":1700000000000,"lastRaw":"final","preRaw":"raw","hasImage":true,
           "topic":"Medical","prompt":"  p  ","appliedDiff":[{"orig":"teh","term":"the"},{"orig":"","term":"x"}]},
          {"id":"s3","created":1700000002000,"hasImage":true}
        ]"#;
        let r = run(s, &[("s1", "data:image/png;base64,iVBORw0KGgo="), ("s3", "data:image/png;base64,zzz")], None).unwrap();
        // s3: no raw and no usable image -> dropped. s1 sorts first.
        assert_eq!(r.name, DEFAULT_NAME);
        assert_eq!(r.pages.len(), 2);
        assert_eq!(r.with_image, 1);
        let p0 = &r.pages[0];
        assert_eq!(p0.page.name(), "Legacy one");
        assert_eq!(p0.page.raw(), "raw");
        assert_eq!(p0.page.corrected(), "final");
        assert_eq!(p0.page.status(), "done");
        assert_eq!(p0.page.get("order"), Some(&json!(0)));
        assert_eq!(p0.page.get("corrections"), Some(&json!([{"word":"teh","term":"the","conf":1,"applied":true}])));
        assert_eq!(p0.page.get("resolved"), Some(&json!({"c:0":"suggested"})));
        assert_eq!(p0.page.get("boxes"), Some(&json!([{"raw":"raw"}])));
        assert!(p0.image.is_some());
        let p1 = &r.pages[1];
        assert_eq!(p1.page.raw(), "two");
        assert_eq!(p1.page.corrected(), "two");
        assert!(p1.image.is_none());
        assert_eq!(p1.page.get("boxes"), Some(&json!([])));
        assert_eq!(r.meta.topic(), "Medical");
        assert_eq!(r.meta.prompt(), "p");
        assert_eq!(r.meta.order(), vec!["id1", "id2"]);
        assert_eq!(r.meta.get("stats"), Some(&json!({"pages": 2, "done": 2})));
        let keys: Vec<_> = r.meta.doc().keys().cloned().collect();
        assert_eq!(keys, ["schemaVersion", "name", "topic", "prompt", "order", "created", "updated", "ui", "stats"]);
    }

    #[test]
    fn unknown_topic_falls_back() {
        let r = run(r#"[{"id":"a","lastRaw":"x","topic":"Nope"}]"#, &[], Some("Mine")).unwrap();
        assert_eq!(r.meta.topic(), "General");
        assert_eq!(r.name, "Mine");
    }
}
