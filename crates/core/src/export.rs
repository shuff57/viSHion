//! Port of js/export.js: combined txt/md/docx, per-page zip, json, csv. Every builder returns bytes (or a
//! string) that match the JS output byte for byte; delivery (download, copy, folder write) is not here.
//! The JS uses fflate when it loaded; this port is the store-only zip writer the JS falls back to, which
//! is what the oracle harness exercises (fflate is not loaded there).
use crate::jsx::{self, num_str, u16len};
use crate::model::{Page, Project};
use crate::render::esc;
use crate::words;
use serde_json::{json, Map, Value};
use unicode_normalization::UnicodeNormalization;

/// `{mode, sep, fmt}` from the export dialog. Kept as strings: the JS falls through on unknown values.
#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    /// raw | corrected | (anything else: both)
    pub mode: String,
    /// label | break | (anything else: none)
    pub sep: String,
    /// txt | md | docx
    pub fmt: String,
}

impl Opts {
    pub fn new(mode: &str, sep: &str, fmt: &str) -> Self {
        Opts { mode: mode.into(), sep: sep.into(), fmt: fmt.into() }
    }
}

/// `OCR.slug`.
pub fn slug(s: &str) -> String {
    let folded: String = s
        .to_lowercase()
        .nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect();
    let mut out = String::new();
    let mut dash = false;
    for c in folded.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
    }
    // a leading run of separators is dropped, a trailing run is never emitted
    out.chars().take(48).collect()
}

/// `p.order + 1` as JS prints it.
fn order_plus_one(p: &Page) -> String {
    match p.get("order") {
        None => "NaN".into(),
        Some(Value::Number(n)) => num_str(n.as_f64().unwrap_or(f64::NAN) + 1.0),
        Some(Value::Null) => "1".into(),
        Some(Value::Bool(b)) => num_str(*b as u8 as f64 + 1.0),
        Some(v) => format!("{}1", jsx::to_string(v)),
    }
}

/// `E.pageText`.
pub fn page_text(p: &Page, mode: &str) -> String {
    let raw = p.raw();
    let eff = words::effective(p);
    let cor = if eff.is_empty() { raw } else { &eff };
    match mode {
        "raw" => raw.to_string(),
        "corrected" => cor.to_string(),
        _ => format!("--- Raw ---\n{}\n\n--- Corrected ---\n{}", raw, cor),
    }
}

/// `E.label`.
pub fn label(p: &Page) -> String {
    let name = p.get("student").and_then(|s| s.get("name"));
    let who = if jsx::truthy(name) { format!("{}, ", jsx::to_string(name.unwrap())) } else { String::new() };
    format!("--- {}Page {} ---", who, order_plus_one(p))
}

/// `E.combined`.
pub fn combined(list: &[&Page], o: &Opts) -> String {
    let sep = if o.sep == "break" {
        if o.fmt == "md" {
            "\n\n---\n\n"
        } else {
            "\n\u{c}\n"
        }
    } else {
        "\n\n"
    };
    list.iter()
        .map(|p| {
            let head = if o.sep == "label" { format!("{}\n", label(p)) } else { String::new() };
            format!("{}{}", head, page_text(p, &o.mode))
        })
        .collect::<Vec<_>>()
        .join(sep)
}

// ---- zip (store only) ----

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xffff_ffff
}

fn p16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn p32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// `storeZip`: a store-only zip with the exact header layout of the JS writer.
pub fn store_zip(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut central: Vec<u8> = vec![];
    let mut off: u32 = 0;
    for (path, data) in files {
        let name = path.as_bytes();
        let crc = crc32(data);
        let len = data.len() as u32;
        // local header (30 bytes)
        p32(&mut out, 0x0403_4b50);
        p16(&mut out, 20);
        p16(&mut out, 0x0800);
        p16(&mut out, 0); // method
        p16(&mut out, 0); // time
        p16(&mut out, 0x21); // date
        p32(&mut out, crc);
        p32(&mut out, len);
        p32(&mut out, len);
        p16(&mut out, name.len() as u16);
        p16(&mut out, 0);
        out.extend_from_slice(name);
        out.extend_from_slice(data);
        // central header (46 bytes)
        p32(&mut central, 0x0201_4b50);
        p16(&mut central, 20);
        p16(&mut central, 20);
        p16(&mut central, 0x0800);
        p16(&mut central, 0);
        p16(&mut central, 0);
        p16(&mut central, 0x21);
        p32(&mut central, crc);
        p32(&mut central, len);
        p32(&mut central, len);
        p16(&mut central, name.len() as u16);
        p16(&mut central, 0); // extra
        p16(&mut central, 0); // comment
        p16(&mut central, 0); // disk
        p16(&mut central, 0); // internal attrs
        p32(&mut central, 0); // external attrs
        p32(&mut central, off);
        central.extend_from_slice(name);
        off += 30 + name.len() as u32 + len;
    }
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    p32(&mut out, 0x0605_4b50);
    p16(&mut out, 0);
    p16(&mut out, 0);
    p16(&mut out, files.len() as u16);
    p16(&mut out, files.len() as u16);
    p32(&mut out, cd_size);
    p32(&mut out, off);
    p16(&mut out, 0);
    out
}

/// `E.makeZip` for text files.
pub fn make_zip(files: &[(String, String)]) -> Vec<u8> {
    let f: Vec<(String, Vec<u8>)> = files.iter().map(|(k, v)| (k.clone(), v.clone().into_bytes())).collect();
    store_zip(&f)
}

/// `E.docx`.
pub fn docx(list: &[&Page], o: &Opts) -> Vec<u8> {
    let para = |t: &str| format!("<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>", esc(t));
    let mut body = String::new();
    for (i, p) in list.iter().enumerate() {
        if i > 0 && o.sep == "break" {
            body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        if o.sep == "label" {
            body.push_str(&para(&label(p)));
        }
        for l in page_text(p, &o.mode).split('\n') {
            body.push_str(&para(l));
        }
        if o.sep != "break" {
            body.push_str(&para(""));
        }
    }
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
    make_zip(&[
        ("[Content_Types].xml".into(), format!("{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>", head)),
        ("_rels/.rels".into(), format!("{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>", head)),
        ("word/document.xml".into(), format!("{}<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{}</w:body></w:document>", head, body)),
    ])
}

/// `fileNames`: unique, filesystem-safe base names for the per-page files.
pub fn file_names(list: &[&Page]) -> Vec<String> {
    let mut used: Vec<String> = vec![];
    list.iter()
        .map(|p| {
            let raw = if jsx::truthy(p.get("name")) { jsx::to_string(p.get("name").unwrap()) } else { "page".into() };
            // .replace(/\.[a-z0-9]+$/i, '')
            let stem = match raw.rfind('.') {
                Some(d) if d + 1 < raw.len() && raw[d + 1..].bytes().all(|b| b.is_ascii_alphanumeric()) => &raw[..d],
                _ => raw.as_str(),
            };
            // .replace(/[^\w.-]+/g, '-')
            let mut s = String::new();
            let mut dash = false;
            for c in stem.chars() {
                if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
                    dash = false;
                    s.push(c);
                } else if !dash {
                    dash = true;
                    s.push('-');
                }
            }
            let s = s.trim_matches('-');
            let base = if s.is_empty() { "page".to_string() } else { s.to_string() };
            let (mut n, mut i) = (base.clone(), 2);
            while used.contains(&n) {
                n = format!("{}-{}", base, i);
                i += 1;
            }
            used.push(n.clone());
            n
        })
        .collect()
}

fn csv_cell(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `E.csv`.
pub fn csv(list: &[&Page]) -> String {
    let rows: Vec<String> = list
        .iter()
        .map(|p| {
            let name = if let Some(v) = p.get("name") { jsx::to_string(v) } else { "undefined".into() };
            let status = p.get("status").map_or(String::new(), |v| if v.is_null() { String::new() } else { jsx::to_string(v) });
            let ms = p
                .get("timings")
                .filter(|t| jsx::truthy(Some(t)))
                .and_then(|t| t.get("ms"))
                .map_or(f64::NAN, jsx::number);
            let secs = ms / 1000.0;
            let secs = if secs == 0.0 || secs.is_nan() { 0.0 } else { secs };
            [
                order_plus_one(p),
                csv_cell(&name),
                status,
                u16len(p.raw()).to_string(),
                jsx::to_fixed1(secs),
            ]
            .join(",")
        })
        .collect();
    format!("order,name,status,chars,seconds\n{}\n", rows.join("\n"))
}

/// `E.json`: `exported` is the ISO timestamp the caller read from its clock.
pub fn json(list: &[&Page], meta: &Project, exported: &str) -> String {
    let mut proj = Map::new();
    for k in ["name", "topic", "prompt"] {
        if let Some(v) = meta.get(k) {
            proj.insert(k.into(), v.clone());
        }
    }
    proj.insert("exported".into(), json!(exported));
    let pages: Vec<Value> = list.iter().map(|p| p.to_value()).collect();
    let v = json!({"project": Value::Object(proj), "pages": pages});
    serde_json::to_string_pretty(&v).unwrap()
}

/// `E.perPage`: `(file name, text)` pairs, in page order.
pub fn per_page(list: &[&Page], mode: &str) -> Vec<(String, String)> {
    let names = file_names(list);
    list.iter().zip(names).map(|(p, n)| (format!("{}.txt", n), page_text(p, mode))).collect()
}

/// What `E.run` hands to delivery.
#[derive(Debug, Clone, PartialEq)]
pub enum Export {
    File { name: String, mime: String, bytes: Vec<u8> },
    /// `copy`: text for the clipboard.
    Copy(String),
}

/// `E.run(kind, list, o)` without delivery. `kind`: combined | pages | json | csv | copy | all.
/// `meta` is the open project's meta; `exported` the ISO timestamp for the json parts.
pub fn run(kind: &str, list: &[&Page], o: &Opts, meta: &Project, exported: &str) -> Option<Export> {
    let name_v = match meta.get("name") {
        Some(v) => jsx::to_string(v),
        None => "undefined".into(),
    };
    let s = slug(&name_v);
    let base = if s.is_empty() { "transcript".to_string() } else { s };
    let file = |name: String, mime: &str, bytes: Vec<u8>| Some(Export::File { name, mime: mime.into(), bytes });
    match kind {
        "combined" => {
            let name = format!("{}.{}", base, o.fmt);
            if o.fmt == "docx" {
                file(name, "application/zip", docx(list, o))
            } else {
                let mime = match o.fmt.as_str() {
                    "md" => "text/markdown",
                    _ => "text/plain",
                };
                file(name, &format!("{};charset=utf-8", mime), combined(list, o).into_bytes())
            }
        }
        "pages" => file(format!("{}-pages.zip", base), "application/zip", make_zip(&per_page(list, &o.mode))),
        "json" => file(format!("{}-pages.json", base), "application/json", json(list, meta, exported).into_bytes()),
        "csv" => file(format!("{}-index.csv", base), "text/csv", csv(list).into_bytes()),
        "copy" => {
            let mut c = o.clone();
            c.fmt = "txt".into();
            Some(Export::Copy(combined(list, &c)))
        }
        "all" => {
            let mut files: Vec<(String, Vec<u8>)> = vec![];
            let cname = format!("combined.{}", o.fmt);
            if o.fmt == "docx" {
                files.push((cname, docx(list, o)));
            } else {
                files.push((cname, combined(list, o).into_bytes()));
            }
            for (k, v) in per_page(list, &o.mode) {
                files.push((format!("pages/{}", k), v.into_bytes()));
            }
            files.push(("pages.json".into(), json(list, meta, exported).into_bytes()));
            files.push(("index.csv".into(), csv(list).into_bytes()));
            file(format!("{}-all.zip", base), "application/zip", store_zip(&files))
        }
        _ => None,
    }
}
