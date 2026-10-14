//! Ports of tests/*.test.js with the same inputs and expectations, in the same order.
use serde_json::{json, Value};
use vishion_core::idle::Idle;
use vishion_core::model::{Page, Settings};
use vishion_core::{clean, guess, judge, words};

const S0: &str = "\u{E000}";
const S1: &str = "\u{E001}";
const S2: &str = "\u{E002}";

fn mk(v: Value) -> Page {
    let mut base = json!({"id": "p1", "status": "done", "raw": "", "corrected": "", "corrections": [], "resolved": {}, "edits": []});
    for (k, x) in v.as_object().unwrap() {
        base[k] = x.clone();
    }
    Page::from_value(base).unwrap()
}
fn keys(p: &Page) -> Vec<String> {
    words::items(p).into_iter().map(|i| i.key).collect()
}
fn st() -> Settings {
    Settings(serde_json::Map::new())
}

/// tests/words.test.js, steps 1 to 7.
#[test]
fn words_model() {
    let mut s = st();
    // 1. Nothing flagged: text passes through.
    let p = mk(json!({"raw": "a plain line", "corrected": "a plain line"}));
    assert_eq!(words::effective(&p), "a plain line");
    assert_eq!(words::items(&p).len(), 0);

    // 2. A suggestion and an illegible word, both unresolved.
    let t = "the confidance interval is [illegible] here";
    let mut p = mk(json!({"raw": t, "corrected": t,
        "corrections": [{"word": "confidance", "term": "confidence", "conf": 0.9, "applied": false}]}));
    assert_eq!(keys(&p), vec!["c:0", "i:0"]);
    assert_eq!(words::count(&p), 2);
    assert_eq!(
        words::marked(&p, false),
        format!("the {S0}c:0{S1}confidance{S2} interval is {S0}i:0{S1}[illegible]{S2} here")
    );

    // 3. Using the suggestion: corrected text is swapped in, the word is resolved.
    words::choose(&mut p, "c:0", "suggested", "", &mut s, "", 0);
    // the JS test overwrites `corrected` by hand; judge.toggle's rebuild already produced the same text
    p.set("corrected", json!("the confidence interval is [illegible] here"));
    assert_eq!(words::count(&p), 1);
    assert!(words::marked(&p, false).contains(&format!("{S0}c:0{S1}confidence{S2}")));

    // 4. Typing over the illegible word shows up in the effective text, and in nothing else.
    words::choose(&mut p, "i:0", "typed", " the   mean $", &mut s, "", 0);
    assert_eq!(words::effective(&p), "the confidence interval is the mean here");
    assert_eq!(words::count(&p), 0);
    assert_eq!(p.raw(), "the confidance interval is [illegible] here");

    // 5. The original view shows the raw words, unedited.
    assert!(words::marked(&p, true).contains(&format!("{S0}c:0{S1}confidance{S2}")));

    // 6. [illegible] inside math is not a word to check.
    let p = mk(json!({"raw": "$x = [illegible]$ and [illegible]", "corrected": "$x = [illegible]$ and [illegible]"}));
    assert_eq!(keys(&p), vec!["i:0"]);
    assert_eq!(words::marked(&p, false), format!("$x = [illegible]$ and {S0}i:0{S1}[illegible]{S2}"));

    // 7. Pages that are not done have no words to check.
    assert_eq!(words::items(&mk(json!({"status": "transcribing", "raw": "[illegible]"}))).len(), 0);
}

/// tests/words.test.js step 8: the model's guesses are cleaned: ranked, capped, bounded, de-duplicated.
#[test]
fn guess_parsing() {
    let reply = "Sure!\n```json\n[{\"n\":1,\"options\":[{\"text\":\"mean\",\"percent\":30},{\"text\":\"median\",\"percent\":70},{\"text\":\"Mean\",\"percent\":5},{\"text\":\"[illegible]\",\"percent\":9},{\"text\":\"mode\",\"percent\":400}]},{\"n\":9,\"options\":[{\"text\":\"x\",\"percent\":50}]},{\"n\":2,\"options\":[]}]\n```";
    let g = guess::parse_guesses(reply, 2);
    assert_eq!(g.keys().collect::<Vec<_>>(), vec!["i:0"]);
    let opts = g["i:0"].as_array().unwrap();
    let texts: Vec<&str> = opts.iter().map(|o| o["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["mode", "median", "mean"]);
    let sum: i64 = opts.iter().map(|o| o["percent"].as_i64().unwrap()).sum();
    assert!(sum <= 101);
    assert!(guess::parse_guesses("no json here", 2).is_empty());
}

/// tests/words.test.js step 9: remembered fixes.
#[test]
fn remembered_fixes() {
    let mut store = Settings(serde_json::Map::new());
    store.set("reference", json!([]));
    store.set("rememberFixes", json!(true));
    let t = "we use $x$ and the [illegible] deviation of the sample is small";
    let pg = mk(json!({"raw": t, "corrected": t}));
    assert_eq!(
        words::context_for(&pg, "i:0"),
        Some(("we use and the".to_string(), "deviation of the sample is".to_string()))
    );
    words::remember(&pg, "i:0", "standard", &mut store, "Stats", 1);
    words::remember(&pg, "i:0", "standard", &mut store, "Stats", 2); // same fix twice is kept once
    assert_eq!(store.reference().len(), 1);
    assert_eq!(words::reference_words(&store, "Stats", None), vec!["standard"]);
    assert!(words::reference_words(&store, "Calculus", None).is_empty()); // other subjects stay out of the way
    assert!(words::reference_examples(&store, "Stats", None)[0].contains("standard"));
    store.set("rememberFixes", json!(false));
    words::remember(&pg, "i:0", "other", &mut store, "Stats", 3);
    assert_eq!(store.reference().len(), 1);
}

/// tests/judge.test.js: suggestions may only be spelling fixes toward a glossary term.
#[test]
fn judge_accepts_only_spelling_fixes() {
    let list: Vec<String> = ["confidence interval", "standard deviation", "population", "sample"].map(String::from).to_vec();
    let text = "the populaton mean and a standrd deviation, so the sample is good, we are sure";
    assert!(judge::accept(&json!({"word": "populaton", "term": "population", "conf": 0.9}), &list, text)); // misspelling of a term
    assert!(!judge::accept(&json!({"word": "sure", "term": "confident", "conf": 0.9}), &list, text)); // rewording
    assert!(!judge::accept(&json!({"word": "good", "term": "population", "conf": 0.9}), &list, text)); // too far apart
    assert!(!judge::accept(&json!({"word": "sample", "term": "population", "conf": 0.9}), &list, text)); // already a glossary word
    assert!(!judge::accept(&json!({"word": "missing", "term": "sample", "conf": 0.9}), &list, text)); // not in the transcript
}

/// tests/clean.test.js: `O.clean` strips model wrappers and leaves handwriting alone.
#[test]
fn clean_strips_wrappers_only() {
    assert_eq!(clean::clean("Sylvia plath\n-1932 born"), "Sylvia plath\n-1932 born"); // plain: untouched
    assert_eq!(clean::clean("```\nline one\nline two\n```"), "line one\nline two"); // code fence
    assert_eq!(clean::clean("Here is the transcription:\n\nline one\nline two"), "line one\nline two"); // preface
    assert_eq!(clean::clean("Sure! Here's the text:\nline one"), "line one");
    assert_eq!(clean::clean("line one\nline two\n\nNote: some words were hard to read."), "line one\nline two"); // closing note
    assert_eq!(clean::clean("line one\n\nLet me know if you need anything else!"), "line one");
    assert_eq!(clean::clean("Transcript of the meeting\nitem 1"), "Transcript of the meeting\nitem 1"); // a real title is kept
    assert_eq!(clean::clean("a\n\n\n\nb"), "a\n\nb");
    assert_eq!(clean::clean("note to self: buy milk"), "note to self: buy milk"); // a note at the START is handwriting
}

/// tests/idle.test.js: unload only when idle long enough, nothing running, and enabled.
/// The JS drives `M.idleCheck` with a fake clock and fetch; the decision rule is `Idle::check`.
#[test]
fn idle_unload_rule() {
    let mut now: i64 = 1_000_000;
    let mut idle = Idle::new(now);
    let mins = Idle::minutes(Some(&json!(1)));
    assert!(!idle.check(now, mins, false, false)); // just started: not idle yet
    now += 30_000;
    assert!(!idle.check(now, mins, false, false)); // 30 s
    now += 120_000;
    assert!(!idle.check(now, mins, true, false)); // a run is active: never unload
    assert!(!idle.check(now, Idle::minutes(Some(&json!(0))), false, false)); // 0 means never
    assert!(idle.check(now, mins, false, false)); // idle past the limit: unloads
    assert!(!idle.check(now, mins, false, false)); // already unloaded: not repeated
    // a model request wakes it again and restarts the clock
    idle.request_started(now);
    idle.request_finished(now);
    assert!(!idle.check(now + 30_000, mins, false, false));
    assert!(idle.check(now + 61_000, mins, false, false));
}
