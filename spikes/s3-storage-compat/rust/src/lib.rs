//! Spike S3: can Rust/WASM at the same origin as the JS app read, rewrite and create the data
//! the JS app persists (IndexedDB `ocr_fs`, localStorage, legacy `ocr_images`, folder handle)?
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Map, Number, Value};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, Event, IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

// ---------------------------------------------------------------- handwritten FSA externs
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = FileSystemDirectoryHandle)]
    type DirHandle;
    #[wasm_bindgen(method, js_name = queryPermission)]
    fn query_permission(this: &DirHandle, d: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = getDirectoryHandle)]
    fn get_directory_handle(this: &DirHandle, name: &str, o: &JsValue) -> Promise;
    #[wasm_bindgen(method, js_name = getFileHandle)]
    fn get_file_handle(this: &DirHandle, name: &str, o: &JsValue) -> Promise;
    #[wasm_bindgen(method)]
    fn values(this: &DirHandle) -> JsValue;
    #[wasm_bindgen(method, getter)]
    fn name(this: &DirHandle) -> String;

    #[wasm_bindgen(js_name = FileSystemFileHandle)]
    type FileHandle;
    #[wasm_bindgen(method, js_name = getFile)]
    fn get_file(this: &FileHandle) -> Promise;
    #[wasm_bindgen(method, js_name = createWritable)]
    fn create_writable(this: &FileHandle) -> Promise;

    #[wasm_bindgen(js_name = FileSystemWritableFileStream)]
    type Writable;
    #[wasm_bindgen(method)]
    fn write(this: &Writable, d: &JsValue) -> Promise;
    #[wasm_bindgen(method)]
    fn close(this: &Writable) -> Promise;
}

fn err<E: ToString>(e: E) -> JsValue {
    JsValue::from_str(&e.to_string())
}
fn win() -> web_sys::Window {
    web_sys::window().expect("window")
}
fn pj(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}
fn obj(pairs: &[(&str, JsValue)]) -> JsValue {
    let o = js_sys::Object::new();
    for (k, v) in pairs {
        let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o.into()
}

// ---------------------------------------------------------------- IndexedDB plumbing
fn req_promise(r: &IdbRequest) -> Promise {
    let (a, b) = (r.clone(), r.clone());
    Promise::new(&mut move |res: Function, rej: Function| {
        let (a2, b2) = (a.clone(), b.clone());
        let ok = Closure::once_into_js(move |_e: Event| {
            let _ = res.call1(&JsValue::NULL, &a2.result().unwrap_or(JsValue::UNDEFINED));
        });
        let er = Closure::once_into_js(move |_e: Event| {
            let e = b2.error().ok().flatten().map(JsValue::from).unwrap_or_else(|| JsValue::from_str("idb request error"));
            let _ = rej.call1(&JsValue::NULL, &e);
        });
        a.set_onsuccess(Some(ok.unchecked_ref()));
        a.set_onerror(Some(er.unchecked_ref()));
    })
}

fn tx_done(t: &IdbTransaction) -> Promise {
    let t = t.clone();
    Promise::new(&mut move |res: Function, rej: Function| {
        let ok = Closure::once_into_js(move |_e: Event| {
            let _ = res.call0(&JsValue::NULL);
        });
        let rej2 = rej.clone();
        let er = Closure::once_into_js(move |_e: Event| {
            let _ = rej2.call1(&JsValue::NULL, &JsValue::from_str("transaction error"));
        });
        let ab = Closure::once_into_js(move |_e: Event| {
            let _ = rej.call1(&JsValue::NULL, &JsValue::from_str("transaction aborted"));
        });
        t.set_oncomplete(Some(ok.unchecked_ref()));
        t.set_onerror(Some(er.unchecked_ref()));
        t.set_onabort(Some(ab.unchecked_ref()));
    })
}

/// Opens an existing database. If it does not exist the upgrade is aborted so we never create one.
async fn open_db(name: &str, version: Option<u32>) -> Result<IdbDatabase, JsValue> {
    let f = win().indexed_db()?.ok_or_else(|| err("no indexedDB"))?;
    let r = match version {
        Some(v) => f.open_with_u32(name, v)?,
        None => f.open(name)?,
    };
    let rr = r.clone();
    let up = Closure::once_into_js(move |_e: Event| {
        if let Some(t) = rr.transaction() {
            let _ = t.abort();
        }
    });
    r.set_onupgradeneeded(Some(up.unchecked_ref()));
    let v = JsFuture::from(req_promise(&r)).await.map_err(|_| err(format!("database '{}' does not exist (or could not open)", name)))?;
    v.dyn_into::<IdbDatabase>().map_err(|_| err("not an IdbDatabase"))
}

async fn idb_keys(db: &IdbDatabase, store: &str) -> Result<Vec<String>, JsValue> {
    let t = db.transaction_with_str(store)?;
    let r = t.object_store(store)?.get_all_keys()?;
    let v = JsFuture::from(req_promise(&r)).await?;
    Ok(Array::from(&v).iter().filter_map(|k| k.as_string()).collect())
}
async fn idb_get(db: &IdbDatabase, store: &str, key: &str) -> Result<JsValue, JsValue> {
    let t = db.transaction_with_str(store)?;
    let r = t.object_store(store)?.get(&JsValue::from_str(key))?;
    JsFuture::from(req_promise(&r)).await
}
async fn idb_put(db: &IdbDatabase, store: &str, key: &str, val: &JsValue) -> Result<(), JsValue> {
    let t = db.transaction_with_str_and_mode(store, IdbTransactionMode::Readwrite)?;
    t.object_store(store)?.put_with_key(val, &JsValue::from_str(key))?;
    JsFuture::from(tx_done(&t)).await?;
    Ok(())
}

// ---------------------------------------------------------------- value classification
fn classify(v: &JsValue) -> String {
    if v.is_undefined() {
        return "undefined".into();
    }
    if v.is_string() {
        return "string".into();
    }
    if v.is_null() {
        return "null".into();
    }
    Reflect::get(v, &"constructor".into())
        .ok()
        .and_then(|c| Reflect::get(&c, &"name".into()).ok())
        .and_then(|n| n.as_string())
        .unwrap_or_else(|| "?".into())
}
fn strip_bom(s: String) -> String {
    s.strip_prefix('\u{feff}').map(|x| x.to_string()).unwrap_or(s)
}
fn bytes_of(v: &JsValue) -> Option<Vec<u8>> {
    if let Some(a) = v.dyn_ref::<js_sys::ArrayBuffer>() {
        return Some(Uint8Array::new(a).to_vec());
    }
    if let Some(a) = v.dyn_ref::<Uint8Array>() {
        return Some(a.to_vec());
    }
    None
}
/// Mirrors the JS idbBackend.read(): string as is, Blob via text(), buffers via TextDecoder.
async fn stored_text(v: &JsValue) -> Result<Option<String>, JsValue> {
    if let Some(s) = v.as_string() {
        return Ok(Some(s));
    }
    if let Some(b) = v.dyn_ref::<Blob>() {
        return Ok(JsFuture::from(b.text()).await?.as_string());
    }
    if let Some(b) = bytes_of(v) {
        return Ok(Some(strip_bom(String::from_utf8_lossy(&b).into_owned())));
    }
    Ok(None)
}
fn byte_len(v: &JsValue) -> usize {
    if let Some(s) = v.as_string() {
        return s.len();
    }
    if let Some(b) = v.dyn_ref::<Blob>() {
        return b.size() as usize;
    }
    bytes_of(v).map(|b| b.len()).unwrap_or(0)
}
/// Make a value of the same JS type as `like` carrying `text`.
fn like_value(like: &JsValue, text: &str) -> Result<JsValue, JsValue> {
    if like.is_string() || like.is_undefined() {
        return Ok(JsValue::from_str(text));
    }
    if like.dyn_ref::<Blob>().is_some() {
        let parts = Array::of1(&JsValue::from_str(text));
        let o = BlobPropertyBag::new();
        o.set_type(&like.dyn_ref::<Blob>().unwrap().type_());
        return Ok(Blob::new_with_str_sequence_and_options(&parts, &o)?.into());
    }
    let enc = js_sys::Uint8Array::from(text.as_bytes());
    if like.dyn_ref::<js_sys::ArrayBuffer>().is_some() {
        return Ok(enc.buffer().into());
    }
    Ok(enc.into())
}

// ---------------------------------------------------------------- storage backends
enum Backend {
    Idb(IdbDatabase),
    Dir(DirHandle),
}

fn is_missing(e: &JsValue) -> bool {
    let n = Reflect::get(e, &"name".into()).ok().and_then(|n| n.as_string()).unwrap_or_default();
    n == "NotFoundError" || n == "TypeMismatchError"
}

async fn root_handle() -> Result<DirHandle, JsValue> {
    let db = open_db("ocr_fs", Some(1)).await?;
    let v = idb_get(&db, "kv", "root").await?;
    db.close();
    if v.is_undefined() || v.is_null() {
        return Err(err("kv/root is empty"));
    }
    v.dyn_into::<DirHandle>().map_err(|_| err("kv/root is not a FileSystemDirectoryHandle"))
}

async fn open_backend(kind: &str) -> Result<Backend, JsValue> {
    match kind {
        "idb" => Ok(Backend::Idb(open_db("ocr_fs", Some(1)).await?)),
        "folder" => Ok(Backend::Dir(root_handle().await?)),
        k => Err(err(format!("unknown backend {}", k))),
    }
}

async fn dir_entries(d: &DirHandle) -> Result<Vec<(String, String, JsValue)>, JsValue> {
    let it = d.values();
    let next: Function = Reflect::get(&it, &"next".into())?.dyn_into()?;
    let mut out = vec![];
    loop {
        let r = JsFuture::from(next.call0(&it)?.dyn_into::<Promise>()?).await?;
        if Reflect::get(&r, &"done".into())?.as_bool().unwrap_or(true) {
            break;
        }
        let h = Reflect::get(&r, &"value".into())?;
        let name = Reflect::get(&h, &"name".into())?.as_string().unwrap_or_default();
        let kind = Reflect::get(&h, &"kind".into())?.as_string().unwrap_or_default();
        out.push((name, kind, h));
    }
    Ok(out)
}

fn walk<'a>(d: &'a DirHandle, prefix: String, out: &'a mut Vec<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), JsValue>> + 'a>> {
    Box::pin(async move {
        for (name, kind, h) in dir_entries(d).await? {
            let p = format!("{}{}", prefix, name);
            if kind == "directory" {
                let sub: DirHandle = h.unchecked_into();
                walk(&sub, format!("{}/", p), out).await?;
            } else {
                out.push(p);
            }
        }
        Ok(())
    })
}

impl Backend {
    async fn list(&self) -> Result<Vec<String>, JsValue> {
        match self {
            Backend::Idb(db) => idb_keys(db, "files").await,
            Backend::Dir(d) => {
                let mut v = vec![];
                walk(d, String::new(), &mut v).await?;
                Ok(v)
            }
        }
    }
    async fn dir_at(&self, parts: &[&str], create: bool) -> Result<DirHandle, JsValue> {
        let Backend::Dir(root) = self else { return Err(err("not a dir backend")) };
        let rv: &JsValue = root.as_ref();
        let mut cur: DirHandle = rv.clone().unchecked_into();
        for p in parts {
            let o = obj(&[("create", create.into())]);
            let h = JsFuture::from(cur.get_directory_handle(p, &o)).await?;
            cur = h.unchecked_into();
        }
        Ok(cur)
    }
    /// None when the entry does not exist.
    async fn read(&self, path: &str) -> Result<Option<JsValue>, JsValue> {
        match self {
            Backend::Idb(db) => {
                let v = idb_get(db, "files", path).await?;
                Ok(if v.is_undefined() || v.is_null() { None } else { Some(v) })
            }
            Backend::Dir(_) => {
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                let (name, dirs) = parts.split_last().ok_or_else(|| err("empty path"))?;
                let r: Result<JsValue, JsValue> = async {
                    let d = self.dir_at(dirs, false).await?;
                    let fh = JsFuture::from(d.get_file_handle(name, &obj(&[]))).await?;
                    let fh: FileHandle = fh.unchecked_into();
                    JsFuture::from(fh.get_file()).await
                }
                .await;
                match r {
                    Ok(f) => Ok(Some(f)),
                    Err(e) if is_missing(&e) => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }
    async fn write(&self, path: &str, data: &JsValue) -> Result<(), JsValue> {
        match self {
            Backend::Idb(db) => idb_put(db, "files", path, data).await,
            Backend::Dir(_) => {
                let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                let (name, dirs) = parts.split_last().ok_or_else(|| err("empty path"))?;
                let d = self.dir_at(dirs, true).await?;
                let fh = JsFuture::from(d.get_file_handle(name, &obj(&[("create", true.into())]))).await?;
                let fh: FileHandle = fh.unchecked_into();
                let w: Writable = JsFuture::from(fh.create_writable()).await?.unchecked_into();
                JsFuture::from(w.write(data)).await?;
                JsFuture::from(w.close()).await?;
                Ok(())
            }
        }
    }
    async fn text(&self, path: &str) -> Result<Option<String>, JsValue> {
        match self.read(path).await? {
            Some(v) => stored_text(&v).await,
            None => Ok(None),
        }
    }
    fn close(&self) {
        if let Backend::Idb(db) = self {
            db.close();
        }
    }
}

// ---------------------------------------------------------------- typed models
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(d)?))
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = "N: Deserialize<'de>", serialize = "N: Serialize"))]
struct Meta<N> {
    #[serde(rename = "schemaVersion", default, skip_serializing_if = "Option::is_none")]
    schema_version: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<Map<String, Value>>,
    /// `null` in the file must stay `null`; absent must stay absent.
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    stats: Option<Option<Map<String, Value>>>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = "N: Deserialize<'de>", serialize = "N: Serialize"))]
struct BoxT<N> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    x: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    y: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    w: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    h: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    r: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    s: Option<N>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = "N: Deserialize<'de>", serialize = "N: Serialize"))]
struct Timings<N> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ms: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tokens: Option<N>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = "N: Deserialize<'de>", serialize = "N: Serialize"))]
struct Correction<N> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    word: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    term: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    conf: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(bound(deserialize = "N: Deserialize<'de>, B: Deserialize<'de>", serialize = "N: Serialize, B: Serialize"))]
struct Page<N, B> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    order: Option<N>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corrected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    corrections: Option<Vec<Correction<N>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boxes: Option<Vec<B>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timings: Option<Timings<N>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotation: Option<N>,
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    student: Option<Option<Map<String, Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reviewed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolved: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edits: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guesses: Option<Map<String, Value>>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

type MetaS = Meta<Number>;
type PageS = Page<Number, BoxT<Number>>;

fn ser<T: Serialize>(v: &T, pretty: bool) -> Result<String, String> {
    (if pretty { serde_json::to_string_pretty(v) } else { serde_json::to_string(v) }).map_err(|e| e.to_string())
}
fn rt<T: Serialize + DeserializeOwned>(text: &str, pretty: bool) -> Result<String, String> {
    let v: T = serde_json::from_str(text).map_err(|e| e.to_string())?;
    ser(&v, pretty)
}

#[derive(Clone, Copy, PartialEq)]
enum DocKind {
    Meta,
    Page,
    Settings,
}
fn doc_kind(path: &str) -> Option<DocKind> {
    if path.ends_with("/project.json") || path.ends_with("/project.json.bak") {
        Some(DocKind::Meta)
    } else if path == "settings.json" {
        Some(DocKind::Settings)
    } else if path.contains("/pages/") && path.ends_with(".json") {
        Some(DocKind::Page)
    } else {
        None
    }
}
fn kind_pretty(k: DocKind) -> bool {
    k != DocKind::Page
}

const SENT: char = '\u{10FFFE}';
fn hex4(s: Option<&str>) -> Option<u32> {
    let s = s?;
    if s.len() == 4 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    }
}
/// JSON.stringify emits lone surrogates as `\udXXX` escapes; serde_json rejects them. Swap each lone
/// escape for SENT+hex so the document parses, and swap back after serialising (lossless).
fn lenient_in(text: &str) -> String {
    let b = text.as_bytes();
    let (mut out, mut i, mut last) = (String::with_capacity(text.len()), 0usize, 0usize);
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            if b[i + 1] == b'u' {
                if let Some(cp) = hex4(text.get(i + 2..i + 6)) {
                    let high = (0xD800..0xDC00).contains(&cp);
                    let low = (0xDC00..0xE000).contains(&cp);
                    if high
                        && b.get(i + 6) == Some(&b'\\')
                        && b.get(i + 7) == Some(&b'u')
                        && hex4(text.get(i + 8..i + 12)).map(|c| (0xDC00..0xE000).contains(&c)).unwrap_or(false)
                    {
                        i += 12; // a valid pair: leave for serde_json
                        continue;
                    }
                    if high || low {
                        out.push_str(&text[last..i]);
                        out.push(SENT);
                        out.push_str(&text[i + 2..i + 6]);
                        i += 6;
                        last = i;
                        continue;
                    }
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out.push_str(&text[last..]);
    out
}
fn lenient_out(s: String) -> String {
    if !s.contains(SENT) {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == SENT {
            out.push_str("\\u");
            for _ in 0..4 {
                if let Some(h) = it.next() {
                    out.push(h);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn reserialize_kind(text: &str, kind: DocKind, variant: &str, pretty: bool) -> Result<String, String> {
    if let Some(v) = variant.strip_suffix("+sur") {
        let r = reserialize_core(&lenient_in(text), kind, v, pretty)?;
        return Ok(lenient_out(r));
    }
    reserialize_core(text, kind, variant, pretty)
}

fn reserialize_core(text: &str, kind: DocKind, variant: &str, pretty: bool) -> Result<String, String> {
    match (variant, kind) {
        ("value", _) => rt::<Value>(text, pretty),
        ("typed", DocKind::Meta) => rt::<MetaS>(text, pretty),
        ("typed", DocKind::Page) => rt::<PageS>(text, pretty),
        ("typed", DocKind::Settings) => rt::<Map<String, Value>>(text, pretty),
        ("naive", DocKind::Meta) => rt::<Meta<f64>>(text, pretty),
        ("naive", DocKind::Page) => rt::<Page<f64, BoxT<f64>>>(text, pretty),
        ("naive", DocKind::Settings) => rt::<Map<String, Value>>(text, pretty),
        (v, _) => Err(format!("unknown variant {}", v)),
    }
}

fn first_diff(a: &str, b: &str) -> Value {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let n = ab.iter().zip(bb.iter()).take_while(|(x, y)| x == y).count();
    let snip = |s: &str| -> String {
        let st = s.char_indices().map(|(i, _)| i).filter(|i| *i + 20 >= n).next().unwrap_or(0);
        s[st..].chars().take(50).collect()
    };
    json!({"at": n, "js": snip(a), "rust": snip(b), "len_js": a.len(), "len_rust": b.len()})
}

fn analyze(path: &str, text: &str) -> Value {
    let kind = doc_kind(path);
    let mut o = Map::new();
    match serde_json::from_str::<Value>(text) {
        Ok(_) => o.insert("parse_ok".into(), json!(true)),
        Err(e) => {
            o.insert("parse_ok".into(), json!(false));
            o.insert("error".into(), json!(e.to_string()));
            let k = kind.unwrap_or(DocKind::Settings);
            let pretty = text.starts_with("{\n") || text.starts_with("[\n");
            for v in ["value+sur", "typed+sur"] {
                let d = match reserialize_kind(text, k, v, pretty) {
                    Ok(s) if s == text => json!({"identical": true}),
                    Ok(s) => json!({"identical": false, "diff": first_diff(text, &s)}),
                    Err(e) => json!({"error": e}),
                };
                o.insert(v.into(), d);
            }
            return Value::Object(o);
        }
    };
    let kind = kind.unwrap_or(DocKind::Settings);
    let pretty = text.starts_with("{\n") || text.starts_with("[\n");
    o.insert("style".into(), json!(if pretty { "pretty" } else { "compact" }));
    o.insert("style_matches_convention".into(), json!(pretty == kind_pretty(kind)));
    for v in ["value", "typed", "naive", "value+sur", "typed+sur"] {
        let r = reserialize_kind(text, kind, v, pretty);
        o.insert(
            v.into(),
            match r {
                Ok(s) if s == text => json!({"identical": true}),
                Ok(s) => json!({"identical": false, "diff": first_diff(text, &s)}),
                Err(e) => json!({"error": e}),
            },
        );
    }
    Value::Object(o)
}

// ---------------------------------------------------------------- exports
/// Pure helper for the format battery: reserialize `text` as `kind` ("value"|"meta"|"page"|"settings").
#[wasm_bindgen]
pub fn reserialize(text: &str, kind: &str, variant: &str, pretty: bool) -> Result<String, JsValue> {
    let k = match kind {
        "meta" => DocKind::Meta,
        "page" => DocKind::Page,
        _ => DocKind::Settings,
    };
    let variant = if kind == "value" { if variant.ends_with("+sur") { "value+sur" } else { "value" } } else { variant };
    reserialize_kind(text, k, variant, pretty).map_err(err)
}

#[wasm_bindgen]
pub fn serde_json_features() -> String {
    format!("arbitrary_precision={} float_roundtrip={}", cfg!(feature = "arbprec"), cfg!(feature = "frt"))
}

/// Reads every entry of the `files` store (or the folder) and reports per-document round-trip status.
#[wasm_bindgen]
pub async fn inspect(backend: &str) -> Result<String, JsValue> {
    let b = open_backend(backend).await?;
    let mut files = vec![];
    for p in b.list().await? {
        let raw = match b.read(&p).await? {
            Some(r) => r,
            None => continue,
        };
        let mut e = json!({"path": p, "type": classify(&raw), "bytes": byte_len(&raw)});
        if doc_kind(&p).is_some() {
            match stored_text(&raw).await? {
                Some(t) => e["doc"] = analyze(&p, &t),
                None => e["doc"] = json!({"error": "unreadable"}),
            }
        }
        files.push(e);
    }
    b.close();
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(pj(&json!({"backend": backend, "count": files.len(), "files": files})))
}

/// Parse and re-serialise every JSON document with `variant` and write it back to the same key,
/// as the same JS value type.
#[wasm_bindgen]
pub async fn roundtrip_write(backend: &str, variant: &str) -> Result<String, JsValue> {
    let b = open_backend(backend).await?;
    let (mut written, mut changed, mut errors) = (0, vec![], vec![]);
    for p in b.list().await? {
        let Some(kind) = doc_kind(&p) else { continue };
        let Some(raw) = b.read(&p).await? else { continue };
        let Some(text) = stored_text(&raw).await? else { continue };
        let pretty = text.starts_with("{\n") || text.starts_with("[\n");
        match reserialize_kind(&text, kind, variant, pretty) {
            Ok(out) => {
                let val = like_value(&raw, &out)?;
                b.write(&p, &val).await?;
                written += 1;
                if out != text {
                    changed.push(p.clone());
                }
            }
            Err(e) => errors.push(json!({"path": p, "error": e})),
        }
    }
    b.close();
    Ok(pj(&json!({"written": written, "changed": changed, "errors": errors})))
}

fn now() -> i64 {
    js_sys::Date::now() as i64
}
fn b36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut s = vec![];
    while n > 0 {
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}
fn uid() -> String {
    let r: String = (0..5).map(|_| b36((js_sys::Math::random() * 36.0) as u64)).collect();
    format!("{}{}", b36(now() as u64), r)
}
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}
fn num(n: i64) -> Number {
    Number::from(n)
}

/// Same algorithm as the JS safeJson(): copy the previous project.json to .bak if it still parses.
async fn safe_write_json(b: &Backend, path: &str, text: &str) -> Result<(), JsValue> {
    if path.ends_with("project.json") {
        if let Some(old) = b.text(path).await? {
            if serde_json::from_str::<Value>(&old).is_ok() {
                b.write(&format!("{}.bak", path), &JsValue::from_str(&old)).await?;
            }
        }
    }
    b.write(path, &JsValue::from_str(text)).await
}

/// Creates a brand-new assignment exactly the way the JS app would (createProject + addPages).
/// Returns {"slug","id"}.
#[wasm_bindgen]
pub async fn make_project(backend: &str, name: &str, jpeg: Vec<u8>, transcript: &str) -> Result<String, JsValue> {
    let b = open_backend(backend).await?;
    let taken: std::collections::HashSet<String> = b.list().await?.iter().filter_map(|p| p.split('/').next().map(|s| s.to_string())).collect();
    let base = { let s = slugify(name); if s.is_empty() { "project".to_string() } else { s } };
    let (mut slug, mut i) = (base.clone(), 2);
    while taken.contains(&slug) {
        slug = format!("{}-{}", base, i);
        i += 1;
    }
    let t = now();
    let mut meta = MetaS {
        schema_version: Some(num(1)),
        name: Some(name.into()),
        topic: Some("General".into()),
        prompt: Some("".into()),
        order: Some(vec![]),
        created: Some(num(t)),
        updated: Some(num(t)),
        ui: Some(Map::new()),
        stats: Some(Some(json!({"pages": 0, "done": 0}).as_object().unwrap().clone())),
        extra: Map::new(),
    };
    let pj_path = format!("{}/project.json", slug);
    b.write(&pj_path, &JsValue::from_str(&ser(&meta, true).map_err(err)?)).await?; // createProject: plain write

    let id = uid();
    let page = PageS {
        id: Some(id.clone()),
        name: Some("rust-made-page.jpg".into()),
        order: Some(num(0)),
        status: Some("done".into()),
        raw: Some(transcript.into()),
        corrected: Some(transcript.into()),
        corrections: Some(vec![]),
        boxes: Some(vec![]),
        timings: Some(Timings { ms: Some(num(0)), tokens: Some(num(0)), extra: Map::new() }),
        error: Some("".into()),
        rotation: Some(num(0)),
        student: Some(None),
        reviewed: Some(false),
        resolved: Some(Map::new()),
        edits: Some(vec![]),
        guesses: Some(Map::new()),
        extra: Map::new(),
    };
    // image as a Blob (what canvas.toBlob gave the JS app), or Uint8Array in folder mode is also fine
    let arr = Uint8Array::from(jpeg.as_slice());
    let bag = BlobPropertyBag::new();
    bag.set_type("image/jpeg");
    let blob = Blob::new_with_u8_array_sequence_and_options(&Array::of1(&arr), &bag)?;
    b.write(&format!("{}/images/{}.jpg", slug, id), &blob.into()).await?;
    b.write(&format!("{}/pages/{}.json", slug, id), &JsValue::from_str(&ser(&page, false).map_err(err)?)).await?;
    meta.order = Some(vec![id.clone()]);
    meta.stats = Some(Some(json!({"pages": 1, "done": 1}).as_object().unwrap().clone()));
    meta.updated = Some(num(now()));
    safe_write_json(&b, &pj_path, &ser(&meta, true).map_err(err)?).await?;
    b.close();
    Ok(pj(&json!({"slug": slug, "id": id})))
}

/// Edit one existing page the way a Rust UI would: change `corrected`, set `reviewed`, bump meta.updated.
#[wasm_bindgen]
pub async fn edit_page(backend: &str, slug: &str, id: &str, suffix: &str) -> Result<String, JsValue> {
    let b = open_backend(backend).await?;
    let pp = format!("{}/pages/{}.json", slug, id);
    let text = b.text(&pp).await?.ok_or_else(|| err("page missing"))?;
    let mut page: PageS = serde_json::from_str(&text).map_err(err)?;
    page.corrected = Some(format!("{}{}", page.corrected.clone().unwrap_or_default(), suffix));
    page.reviewed = Some(true);
    b.write(&pp, &JsValue::from_str(&ser(&page, false).map_err(err)?)).await?;
    let mp = format!("{}/project.json", slug);
    let mtext = b.text(&mp).await?.ok_or_else(|| err("meta missing"))?;
    let mut meta: MetaS = serde_json::from_str(&mtext).map_err(err)?;
    meta.updated = Some(num(now()));
    safe_write_json(&b, &mp, &ser(&meta, true).map_err(err)?).await?;
    b.close();
    Ok("ok".into())
}

// ---------------------------------------------------------------- localStorage + legacy
fn ls_get(k: &str) -> Option<String> {
    win().local_storage().ok().flatten().and_then(|s| s.get_item(k).ok().flatten())
}

#[wasm_bindgen]
pub fn ls_report() -> String {
    let mut out = Map::new();
    for key in ["ocr_settings_v1", "ocr_sessions_v2"] {
        let Some(t) = ls_get(key) else {
            out.insert(key.into(), json!({"present": false}));
            continue;
        };
        let v = serde_json::from_str::<Value>(&t);
        let mut e = json!({"present": true, "bytes": t.len()});
        match v {
            Ok(v) => {
                let back = serde_json::to_string(&v).unwrap();
                e["parse_ok"] = json!(true);
                e["value_identical"] = json!(back == t);
                if back != t {
                    e["diff"] = first_diff(&t, &back);
                }
                if let Some(m) = v.as_object() {
                    e["keys"] = json!(m.keys().collect::<Vec<_>>());
                    e["has_apiKey_value"] = json!(m.get("apiKey").and_then(|x| x.as_str()).map(|s| !s.is_empty()).unwrap_or(false));
                    // typed settings round trip with extra map is the same as Value; also test naive f64 on whole doc
                }
                if let Some(a) = v.as_array() {
                    e["sessions"] = json!(a.len());
                    e["session_keys"] = json!(a.iter().map(|s| s.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()).collect::<Vec<_>>());
                }
            }
            Err(er) => {
                e["parse_ok"] = json!(false);
                e["error"] = json!(er.to_string());
            }
        }
        out.insert(key.into(), e);
    }
    pj(&Value::Object(out))
}

/// Folder mode: settings.json on disk overrides localStorage for non-secret keys at startup.
#[wasm_bindgen]
pub async fn settings_json_set_theme(backend: &str, theme: &str) -> Result<String, JsValue> {
    let b = open_backend(backend).await?;
    let t = b.text("settings.json").await?.ok_or_else(|| err("no settings.json"))?;
    let mut m: Map<String, Value> = serde_json::from_str(&t).map_err(err)?;
    m.insert("theme".into(), json!(theme));
    let s = serde_json::to_string_pretty(&m).map_err(err)?;
    b.write("settings.json", &JsValue::from_str(&s)).await?;
    b.close();
    Ok("ok".into())
}

#[wasm_bindgen]
pub fn ls_set_theme(theme: &str) -> Result<String, JsValue> {
    let t = ls_get("ocr_settings_v1").ok_or_else(|| err("no settings"))?;
    let mut m: Map<String, Value> = serde_json::from_str(&t).map_err(err)?;
    m.insert("theme".into(), json!(theme)); // existing key: position preserved
    m.insert("rustWrote".into(), json!(true)); // unknown key: JS must tolerate/keep it
    let s = serde_json::to_string(&m).map_err(err)?;
    win().local_storage()?.ok_or_else(|| err("no ls"))?.set_item("ocr_settings_v1", &s)?;
    Ok(s.len().to_string())
}

#[wasm_bindgen]
pub async fn legacy_report() -> Result<String, JsValue> {
    let db = match open_db("ocr_images", None).await {
        Ok(d) => d,
        Err(e) => return Ok(pj(&json!({"present": false, "why": e.as_string()}))),
    };
    let keys = idb_keys(&db, "images").await?;
    let mut items = vec![];
    for k in &keys {
        let v = idb_get(&db, "images", k).await?;
        let s = v.as_string();
        items.push(json!({"key": k, "type": classify(&v), "len": s.as_ref().map(|s| s.len()), "data_url": s.map(|s| s.starts_with("data:")).unwrap_or(false)}));
    }
    db.close();
    Ok(pj(&json!({"present": true, "count": keys.len(), "items": items})))
}

/// Folder mode: what does a structured-cloned handle look like from Rust, and can we use it with no gesture?
#[wasm_bindgen]
pub async fn probe_root() -> Result<String, JsValue> {
    let db = open_db("ocr_fs", Some(1)).await?;
    let v = idb_get(&db, "kv", "root").await?;
    db.close();
    let mut o = json!({"kv_root_class": classify(&v), "is_dir_handle": v.is_instance_of::<DirHandle>()});
    if let Ok(h) = v.dyn_into::<DirHandle>() {
        o["name"] = json!(h.name());
        let opts = obj(&[("mode", "readwrite".into())]);
        o["queryPermission_readwrite"] = json!(JsFuture::from(h.query_permission(&opts)).await?.as_string());
        let ro = obj(&[("mode", "read".into())]);
        o["queryPermission_read"] = json!(JsFuture::from(h.query_permission(&ro)).await?.as_string());
        o["top_level"] = json!(dir_entries(&h).await?.into_iter().map(|(n, k, _)| format!("{}:{}", k, n)).collect::<Vec<_>>());
    }
    Ok(pj(&o))
}
