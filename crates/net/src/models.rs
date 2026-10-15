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
