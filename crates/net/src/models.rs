//! What a server says it serves: the rows of `GET /v1/models` (OpenAI shape, llama.cpp router and OpenRouter
//! extras included), as `M.list` of js/models.js reads them. Pure; the request is the provider's.
use serde_json::Value;

/// One model of a listing.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelRow {
    pub id: String,
    /// `name` of an OpenRouter row ("Vendor: Model").
    pub api_name: String,
    /// `Some(true)` when the row says it takes images, `Some(false)` when it lists modalities without one,
    /// `None` when it does not say.
    pub vision: Option<bool>,
    /// A llama.cpp router row (it has a `status` and no `/api/show`).
    pub router: bool,
}

/// `(await r.json()).data`, each row with an `id`.
pub fn parse_rows(body: &str) -> Vec<ModelRow> {
    let Ok(v) = serde_json::from_str::<Value>(body) else { return vec![] };
    let Some(data) = v.get("data").and_then(Value::as_array) else { return vec![] };
    data.iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            if id.is_empty() {
                return None;
            }
            let vision = m
                .get("architecture")
                .and_then(|a| a.get("input_modalities"))
                .and_then(Value::as_array)
                .map(|mods| mods.iter().any(|x| x.as_str() == Some("image")));
            Some(ModelRow {
                id,
                api_name: m.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                vision,
                router: m.get("status").is_some_and(|s| !s.is_null()),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_openrouter_and_router_rows() {
        let body = r#"{"data":[
            {"id":"gemma4:31b-cloud"},
            {"id":"vendor/m","name":"Vendor: M","architecture":{"input_modalities":["text","image"]}},
            {"id":"text-only","architecture":{"input_modalities":["text"]}},
            {"id":"q.gguf","status":{"value":"loaded"}},
            {"nope":1},{"id":""}]}"#;
        let rows = parse_rows(body);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].vision, None);
        assert_eq!((rows[1].vision, rows[1].api_name.as_str()), (Some(true), "Vendor: M"));
        assert_eq!(rows[2].vision, Some(false));
        assert!(rows[3].router && !rows[0].router);
        assert!(parse_rows("not json").is_empty() && parse_rows("{}").is_empty());
    }
}

// ---------------------------------------------------------------------------------------------------------
// Names, filtering and order of the picker (`M.pretty` and the tail of `M.list` of js/models.js)
// ---------------------------------------------------------------------------------------------------------

/// A model as the Settings picker shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    pub vision: Option<bool>,
}

const KNOWN: [(&str, &str); 5] = [
    ("qwen3vl", "Qwen3-VL 8B Instruct"),
    ("qwen25vl", "Qwen2.5-VL 7B Instruct"),
    ("lumichats", "LumiChats v1.2"),
    ("nanonets", "Nanonets-OCR2 3B"),
    ("paddle", "PaddleOCR-VL 1.6"),
];

const BRAND: [(&str, &str); 20] = [
    ("gpt", "GPT"),
    ("oss", "OSS"),
    ("glm", "GLM"),
    ("vl", "VL"),
    ("ocr", "OCR"),
    ("lfm", "LFM"),
    ("llama", "Llama"),
    ("qwen", "Qwen"),
    ("gemma", "Gemma"),
    ("kimi", "Kimi"),
    ("deepseek", "DeepSeek"),
    ("minimax", "MiniMax"),
    ("nemotron", "Nemotron"),
    ("mistral", "Mistral"),
    ("phi", "Phi"),
    ("nomic", "Nomic"),
    ("vibethinker", "VibeThinker"),
    ("paddleocr", "PaddleOCR"),
    ("nanonets", "Nanonets"),
    ("lumichats", "LumiChats"),
];

fn brand(w: &str) -> Option<&'static str> {
    let l = w.to_lowercase();
    BRAND.iter().find(|(k, _)| *k == l).map(|(_, v)| *v)
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) if f.is_ascii_alphabetic() => f.to_ascii_uppercase().to_string() + c.as_str(),
        _ => w.to_string(),
    }
}

/// `^q\d+(?:_[a-z0-9]+)*$` (case-insensitive).
fn is_quant(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    let mut parts = l.split('_');
    let first = parts.next().unwrap_or("");
    let ok_first = first.len() > 1 && first.starts_with('q') && first[1..].bytes().all(|b| b.is_ascii_digit());
    ok_first && parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// `^\d+(?:\.\d+)?[bm]$`
fn is_size(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    let Some(body) = l.strip_suffix('b').or_else(|| l.strip_suffix('m')) else { return false };
    let mut it = body.splitn(2, '.');
    let int = it.next().unwrap_or("");
    let frac = it.next();
    !int.is_empty() && int.bytes().all(|b| b.is_ascii_digit()) && frac.map_or(true, |f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// `^([a-z]{2,})(\d[\d.]*)$`
fn letters_then_number(w: &str) -> Option<(&str, &str)> {
    let at = w.find(|c: char| c.is_ascii_digit())?;
    let (a, b) = w.split_at(at);
    (a.len() >= 2 && a.bytes().all(|c| c.is_ascii_alphabetic()) && b.bytes().all(|c| c.is_ascii_digit() || c == b'.')).then_some((a, b))
}

/// `M.pretty`: a readable name for a model id ("gemma4:31b-cloud" is "Gemma 4 31B").
pub fn pretty(id: &str, api_name: &str) -> String {
    let raw = id;
    let low = raw.to_lowercase();
    if let Some((_, v)) = KNOWN.iter().find(|(k, _)| *k == low) {
        return (*v).to_string();
    }
    if !api_name.is_empty() && api_name != raw {
        // OpenRouter: "Vendor: Model"
        return match api_name.split_once(':') {
            Some((_, rest)) if !rest.is_empty() && rest.starts_with(char::is_whitespace) => rest.trim_start().to_string(),
            _ => api_name.to_string(),
        };
    }
    let s = if raw.len() >= 6 && raw[..6].eq_ignore_ascii_case("hf.co/") { &raw[6..] } else { raw };
    let s = match s.split_once('/') {
        Some((_, rest)) => rest,
        None => s,
    };
    let (base, tag) = match s.find(':') {
        Some(at) => (&s[..at], &s[at + 1..]),
        None => (s, ""),
    };
    // hosted and latest are not part of the name
    let mut tag = tag.to_string();
    let lt = tag.to_ascii_lowercase();
    if lt == "cloud" {
        tag.clear();
    } else if lt.ends_with("-cloud") {
        tag.truncate(tag.len() - "-cloud".len());
    }
    if tag.eq_ignore_ascii_case("latest") {
        tag.clear();
    }
    let quant = is_quant(&tag).then(|| tag.clone());
    let base = if base.len() >= 5 && base[base.len() - 5..].eq_ignore_ascii_case("-gguf") { &base[..base.len() - 5] } else { base };
    let split = |t: &str| -> Vec<String> { t.split(|c: char| c == '-' || c == '_' || c.is_whitespace()).filter(|w| !w.is_empty()).map(String::from).collect() };
    let mut words = split(base);
    if quant.is_none() {
        words.extend(split(&tag));
    }
    let mut out: Vec<String> = vec![];
    for w in words {
        if let Some(b) = brand(&w) {
            out.push(b.to_string());
        } else if let Some((a, n)) = letters_then_number(&w) {
            out.push(brand(a).map(String::from).unwrap_or_else(|| capitalize(&a.to_lowercase())));
            out.push(n.to_string());
        } else if is_size(&w) || (w.len() > 1 && w.starts_with(['v', 'V']) && w.as_bytes()[1].is_ascii_digit()) {
            out.push(w.to_uppercase());
        } else {
            out.push(capitalize(&w));
        }
    }
    let mut name = out.join(" ");
    if let Some(q) = quant {
        name.push_str(&format!(" ({})", q.split('_').next().unwrap_or("").to_uppercase()));
    }
    name
}

/// The tail of `M.list`: hosted models out of a local picker, text-only models out, `working` (cloud, after a
/// check) applied unless `all`, vision first and then by name.
pub fn assemble(rows: Vec<(ModelRow, Option<bool>)>, provider_id: &str, cloud: bool, working: Option<&[String]>, all: bool) -> Vec<ModelChoice> {
    let hosted = |id: &str| {
        let l = id.to_ascii_lowercase();
        l.ends_with(":cloud") || l.ends_with("-cloud")
    };
    let mut out: Vec<ModelChoice> = rows
        .into_iter()
        .filter(|(m, _)| !(provider_id == "local" && hosted(&m.id)))
        .map(|(m, shown)| {
            let vision = shown.or(m.vision);
            ModelChoice { label: pretty(&m.id, &m.api_name), id: m.id, vision }
        })
        .filter(|m| m.vision != Some(false))
        .collect();
    if cloud && !all {
        if let Some(w) = working {
            out.retain(|m| w.iter().any(|x| *x == m.id));
        }
    }
    out.sort_by(|a, b| {
        (b.vision == Some(true))
            .cmp(&(a.vision == Some(true)))
            .then_with(|| vishion_core::jsx::nat_compare(&a.label, &b.label))
    });
    out
}

#[cfg(test)]
mod pretty_tests {
    use super::*;

    #[test]
    fn names_are_tidied_like_the_js_picker() {
        assert_eq!(pretty("gemma4:31b-cloud", ""), "Gemma 4 31B");
        assert_eq!(pretty("qwen3-vl:8b", ""), "Qwen 3 VL 8B");
        assert_eq!(pretty("qwen3vl", ""), "Qwen3-VL 8B Instruct");
        assert_eq!(pretty("hf.co/unsloth/Qwen3-VL-8B-Instruct-GGUF:Q4_K_M", ""), "Qwen 3 VL 8B Instruct (Q4)");
        assert_eq!(pretty("vendor/model-x", "Vendor: Model X"), "Model X");
        assert_eq!(pretty("llama3.2:latest", ""), "Llama 3.2");
        assert_eq!(pretty("gpt-oss:20b", ""), "GPT OSS 20B");
    }

    #[test]
    fn the_picker_drops_hosted_local_models_and_text_only_ones_and_puts_vision_first() {
        let row = |id: &str, vision: Option<bool>| ModelRow { id: id.into(), api_name: String::new(), vision, router: false };
        let rows = vec![
            (row("zeta", None), None),
            (row("alpha-vl", None), Some(true)),
            (row("gemma4:31b-cloud", None), Some(true)),
            (row("coder", None), Some(false)),
        ];
        let local = assemble(rows.clone(), "local", false, None, false);
        assert_eq!(local.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["alpha-vl", "zeta"]);
        // another provider keeps hosted models; a working list narrows a cloud picker unless asked for all
        let custom = assemble(rows.clone(), "custom", true, Some(&["zeta".to_string()]), false);
        assert_eq!(custom.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["zeta"]);
        let every = assemble(rows, "custom", true, Some(&["zeta".to_string()]), true);
        assert_eq!(every.len(), 3);
    }
}
