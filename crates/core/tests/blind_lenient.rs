//! Blind critic (phase 2 store). `lenient` carries a lone surrogate as U+10FFFE + 4 hex digits and turns
//! that back into a `\udXXX` escape on output, doubling a RAW U+10FFFE in the input so the mapping is
//! unambiguous. A U+10FFFE that reaches memory any other way (the escaped pair, which serde_json decodes
//! to the single char, or text typed/pasted after load) is not doubled, so on output the next four
//! characters are eaten as "hex" and the page is written as invalid JSON.
use serde_json::Value;
use vishion_core::model::Page;

/// `{"id":"p","raw":"<ESC>udbff<ESC>udffe xyz"}` with real backslashes, built piecewise so no tool in
/// between can decode the escapes.
fn escaped_pair_doc() -> String {
    let bs = "\\";
    ["{\"id\":\"p\",\"raw\":\"", bs, "udbff", bs, "udffe xyz\"}"].concat()
}

#[test]
fn escaped_pair_for_u10fffe_survives_a_rewrite() {
    // a valid surrogate pair for U+10FFFE (python json.dumps default style); JS reads it as one char
    let src = escaped_pair_doc();
    assert!(src.contains("\\udbff"), "test source must hold the literal escape: {src}");
    let out = Page::from_json(&src).unwrap().to_json().unwrap();
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("wrote invalid JSON {out:?}: {e}"));
    let orig: Value = serde_json::from_str(&src).unwrap();
    assert_eq!(v, orig, "value changed: {out:?}");
}

#[test]
fn u10fffe_set_after_load_survives_a_save() {
    let mut p = Page::from_json("{\"id\":\"p\"}").unwrap();
    let typed = ["hi ", &char::from_u32(0x10FFFE).unwrap().to_string(), "zz and more"].concat();
    p.set("corrected", serde_json::json!(typed));
    let out = p.to_json().unwrap();
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("wrote invalid JSON {out:?}: {e}"));
    assert_eq!(v["corrected"], typed.as_str());
}
