//! Could pulldown-cmark replace markdown-it for the inline-only path? Feed it every string markdown-it's
//! renderInline saw (inline-cases.json, produced by run.js) through the best adapter we can write, and
//! count/print what differs. `cargo test --test pulldown -- --nocapture`.
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

fn pd(t: &str) -> String {
    let opts = Options::ENABLE_STRIKETHROUGH;
    let ev = Parser::new_ext(t, opts).map(|e| match e {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s), // html:false -> escape as text
        Event::SoftBreak => Event::HardBreak,                     // breaks:true
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, ev);
    let _ = (Tag::Paragraph, TagEnd::Paragraph);
    // adapter: <br />\n -> <br>\n, <del> -> <s>, drop block wrappers markdown-it never emits
    out.replace("<br />", "<br>").replace("<del>", "<s>").replace("</del>", "</s>")
}
fn strip_p(s: &str) -> String {
    let s = s.strip_prefix("<p>").unwrap_or(s);
    let s = s.strip_suffix("</p>\n").or_else(|| s.strip_suffix("</p>")).unwrap_or(s);
    s.to_string()
}

fn category(t: &str, mi: &str, p: &str) -> &'static str {
    let tt = t.trim_start_matches(' ');
    if p.starts_with("<hr") { return "thematic break eaten (--- *** ___)"; }
    if p.starts_with("<h") && tt.starts_with('#') { return "ATX heading eaten (# )"; }
    if p.starts_with("<ul>") || p.starts_with("<ol") { return "list eaten (- / * / + / 1.)"; }
    if p.starts_with("<blockquote>") { return "blockquote eaten (>)"; }
    if p.starts_with("<pre>") || p.contains("<pre>") { return "code block eaten (4-space/tab/fence)"; }
    if p.contains("<h1>") || p.contains("<h2>") { return "setext heading (=== / --- under text)"; }
    if t.starts_with(' ') || t.ends_with(' ') { return "leading/trailing space trimmed"; }
    if mi.contains("&quot;") { return "double-quote escaping (\" -> &quot;)"; }
    if mi.contains('�') { return "NUL -> U+FFFD normalisation"; }
    if mi.contains("<a ") || p.contains("<a ") { return "link/autolink/URL validation"; }
    "other"
}

#[test]
fn pulldown_vs_markdown_it() {
    let raw = std::fs::read_to_string("inline-cases.json").expect("run `node run.js` first");
    let cases: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
    let mut by_cat: std::collections::BTreeMap<&str, Vec<(String, String, String)>> = Default::default();
    let (mut same, mut total) = (0, 0);
    for c in &cases {
        let t = c["t"].as_str().unwrap();
        let mi = c["html"].as_str().unwrap();
        // markdown-it renderInline output has no wrapper; pulldown wraps in <p>
        let p = strip_p(&pd(t));
        total += 1;
        if p.trim_end_matches('\n') == mi.trim_end_matches('\n') && p.starts_with(mi.split('<').next().unwrap_or("")) { same += 1; continue; }
        by_cat.entry(category(t, mi, &p)).or_default().push((t.to_string(), mi.to_string(), p));
    }
    println!("\npulldown-cmark 0.13 (+strikethrough, adapter) vs markdown-it renderInline: {same}/{total} identical");
    for (cat, v) in &by_cat {
        println!("\n[{cat}] {} cases", v.len());
        for (t, mi, p) in v.iter().take(15) {
            println!("   in : {:?}\n   mdi: {:?}\n   pd : {:?}", t, mi, p);
        }
    }
}

/// Block constructs that markdown-it's inline-only entry point leaves literal. Each must be neutralised by hand.
#[test]
fn extra_block_probes() {
    for t in ["[foo]: /url \"title\"", "<div>*not em*</div>", "~~~", "Term
===", "a
---", "|a|b|
|-|-|", "* * *", "1) paren", "+ plus", "  - two-space bullet", "- [ ] task", "text
> quote after break", r"\# escaped hash", "&nbsp; entity", "<!-- c --> x"] {
        println!("{:<32} -> {:?}", format!("{:?}", t), strip_p(&pd(t)));
    }
}
