//! Accessibility structure of the twin as pure data (plan rule 3, phase 4): which groups and landmarks the
//! hidden DOM has, and what a formula sounds like. `web` turns these into elements; native tests check
//! them directly, and a future UI Automation tree can take the same description.
use crate::twin::{Node, DIALOG, HEADER, SCAN, TEXT, TRAY};

/// The groups of related controls the web twin wraps in one element, so a screen reader announces "Layout,
/// group" or "Pages, list 2 items": (key, role, label). Controls are grouped when consecutive in reading
/// order. The legacy app names the same groups (`Layout`, `Which text to show`, `Pages`, `Which pages`).
pub fn group_of(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match id {
        "layout-split" | "layout-text" => Some(("layout", "group", "Layout")),
        "ver-corrected" | "ver-original" => Some(("version", "group", "Which text to show")),
        "zoom-out" | "zoom-fit" | "zoom-in" => Some(("zoom", "group", "Zoom")),
        "cs-scope-page" | "cs-scope-all" => Some(("scope", "group", "Which pages")),
        "cs-new" => Some(("students", "list", "Students")),
        _ if id.starts_with("theme-") => Some(("theme", "group", "Theme")),
        _ if id.starts_with("st-jmode-") => Some(("jmode", "group", "How to suggest")),
        _ if id.starts_with("st-fmt-") => Some(("fmt", "group", "Combined file")),
        _ if id.starts_with("st-sep-") => Some(("sep", "group", "Between pages")),
        _ if id.starts_with("st-txt-") => Some(("txt", "group", "Text to export")),
        _ if id.starts_with("tab-") => Some(("filter", "group", "Show")),
        _ if id.starts_with("page-") => Some(("pages", "list", "Pages")),
        _ if id.starts_with("cs-student-") => Some(("students", "list", "Students")),
        _ if id.starts_with("alt-") => Some(("choices", "list", "Choices for this word")),
        _ => None,
    }
}

/// Why a control is unavailable, where the JS app says so in a `title` (js/ui/review.js: the `Which text to
/// show` group when both versions read the same). A dimmed button with no reason is a puzzle to someone who
/// cannot see the page; the web twin gives it as `aria-description`, the native tree in the name.
pub fn disabled_reason(id: &str) -> Option<&'static str> {
    match id {
        "ver-original" => Some("Nothing has been corrected on this page yet, so the original and corrected text are the same."),
        "copy" => Some("Nothing to copy yet: this student has no transcript."),
        "prev-student" => Some("This is the first student."),
        "next-student" => Some("This is the last student."),
        "done" => Some("Select a student first."),
        "zoom-out" => Some("Already at the smallest size, or no page is selected."),
        "zoom-in" => Some("Already at the largest size, or no page is selected."),
        "zoom-fit" | "boxes" | "replace" | "remove" => Some("Select a page first."),
        "level" | "rotate" => Some("Select a page first, and wait until it has been read."),
        "st-clear-fixes" => Some("There are no remembered fixes."),
        _ => None,
    }
}

/// The description of a group of controls (the reason one of its members is unavailable).
pub fn group_description(key: &str, members: &[&Node]) -> Option<&'static str> {
    let off = |id: &str| members.iter().any(|n| matches!(n, Node::Control(r) if r.id == id && r.disabled));
    match key {
        "version" if off("ver-original") => disabled_reason("ver-original"),
        _ => None,
    }
}

/// The landmark a region of the screen becomes: (role, accessible name). `None`: a plain container.
/// The page image and the transcript are the two sections of `<main>`, as in index.html of the JS app.
pub fn landmark(region: u8, nodes: &[&Node]) -> Option<(&'static str, String)> {
    let heading = |id: &str| {
        nodes.iter().find_map(|n| match n {
            Node::Heading { id: i, text, .. } if i == id => Some(text.clone()),
            _ => None,
        })
    };
    match region {
        HEADER => Some(("banner", String::new())),
        SCAN => Some(("region", "Page image".into())),
        TEXT if heading("student-name").is_some() => Some(("region", "Transcript".into())),
        TEXT => heading("inbox-title").map(|t| ("region", t)),
        TRAY => Some(("complementary", "Word panel".into())),
        DIALOG => Some(("dialog", heading("dialog-title").unwrap_or_else(|| "Dialog".into()))),
        _ => None,
    }
}

/// Regions that sit inside `<main>`.
pub fn in_main(region: u8) -> bool {
    region == SCAN || region == TEXT
}

thread_local! {
    /// How many fractions or binomials enclose the one being spoken: an inner one is always closed with its
    /// own "end fraction", or "1 over 2 over 3" would not say which division comes first.
    static NEST: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Inside the subscript of a `\lim`: only there does an arrow "approach" something.
    static IN_LIM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What a screen reader should say for a formula written in TeX: `\frac{s}{\sqrt{n}}` is "the fraction s
/// over square root of n end fraction". The TeX itself stays in the element's text, so it can still be read
/// letter by letter.
pub fn tex_speech(tex: &str) -> String {
    NEST.with(|n| n.set(0));
    // `\vert` is a bar like `|`
    let norm = tex.replace("\\vert", "|");
    let cs: Vec<char> = norm.chars().collect();
    let mut p = 0;
    let out = seq(&cs, &mut p, false);
    let mut s = out.split_whitespace().collect::<Vec<_>>().join(" ");
    while s.contains(" ,") {
        s = s.replace(" ,", ",");
    }
    s
}

fn word_for(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "pm" => "plus or minus",
        "mp" => "minus or plus",
        "cdot" | "times" | "ast" => "times",
        "div" => "divided by",
        "le" | "leq" => "is less than or equal to",
        "ge" | "geq" => "is greater than or equal to",
        "ne" | "neq" => "is not equal to",
        "approx" => "is approximately",
        "sim" => "is distributed as",
        "infty" => "infinity",
        "to" | "rightarrow" | "longrightarrow" => "to",
        "mapsto" | "longmapsto" => "maps to",
        "leftarrow" => "from",
        "Rightarrow" | "implies" | "Longrightarrow" => "implies",
        "Leftarrow" => "is implied by",
        "Leftrightarrow" | "iff" | "Longleftrightarrow" => "if and only if",
        "leftrightarrow" => "to and from",
        "emptyset" | "varnothing" => "the empty set",
        "forall" => "for all",
        "exists" => "there is some",
        "nexists" => "there is no",
        "lfloor" => "floor of",
        "rfloor" => "end floor",
        "lceil" => "ceiling of",
        "rceil" => "end ceiling",
        "lvert" => "absolute value of",
        "rvert" => "end absolute value",
        "lVert" => "norm of",
        "rVert" => "end norm",
        "|" | "Vert" => "norm",
        "cong" => "is congruent to",
        "perp" => "is perpendicular to",
        "parallel" => "is parallel to",
        "mid" => "divides",
        "notin" => "is not in",
        "ni" => "contains",
        "subset" => "is a subset of",
        "subseteq" => "is a subset of or equal to",
        "supset" => "is a superset of",
        "supseteq" => "is a superset of or equal to",
        "setminus" => "set minus",
        "equiv" => "is equivalent to",
        "propto" => "is proportional to",
        "wedge" | "land" => "and",
        "vee" | "lor" => "or",
        "neg" | "lnot" => "not",
        "bmod" | "mod" => "mod",
        "angle" => "angle",
        "triangle" | "bigtriangleup" => "triangle",
        "square" | "Box" => "square",
        "measuredangle" => "the measure of angle",
        "oplus" => "direct sum",
        "otimes" => "tensor product",
        "vdots" => "vertical dots",
        "ominus" => "circled minus",
        "oslash" => "circled slash",
        "ddots" => "diagonal dots",
        "prime" => "prime",
        "simeq" => "is similar or equal to",
        "textdegree" => "degrees",
        "nsim" => "is not similar to",
        "ncong" => "is not congruent to",
        "nparallel" => "is not parallel to",
        "nmid" => "does not divide",
        "gg" => "is much greater than",
        "ll" => "is much less than",
        "therefore" => "therefore",
        "because" => "because",
        "partial" => "partial",
        "nabla" => "del",
        "ell" => "script l",
        "cdotp" => "times",
        "lt" => "is less than",
        "gt" => "is greater than",
        "langle" => "open angle bracket",
        "rangle" => "close angle bracket",
        "ldots" | "cdots" | "dots" => "dot dot dot",
        "circ" => "composed with",
        "degree" => "degrees",
        "frown" | "overarc" => "arc",
        "top" => "transpose",
        "sum" => "the sum of",
        "prod" => "the product of",
        "int" => "the integral of",
        "in" => "is in",
        "cup" => "union",
        "cap" => "intersection",
        "quad" | "qquad" | "," | ";" | "!" | ":" | " " => " ",
        "%" => "percent",
        "{" => "open brace",
        "}" => "end set",
        "\\" => ",",
        "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "displaystyle" | "limits" => "",
        _ => return None,
    })
}

/// One argument: a `{group}`, a `\command` with its own arguments, or a single character.
fn atom(cs: &[char], p: &mut usize) -> String {
    while *p < cs.len() && cs[*p].is_whitespace() {
        *p += 1;
    }
    match cs.get(*p) {
        None => String::new(),
        Some('{') => {
            *p += 1;
            let s = seq(cs, p, true);
            if cs.get(*p) == Some(&'}') {
                *p += 1;
            }
            s
        }
        Some('\\') => command(cs, p),
        Some(c) => {
            *p += 1;
            symbol(*c)
        }
    }
}

/// The text of a `{group}` as written, for the text command: words stay words.
fn raw_group(cs: &[char], p: &mut usize) -> String {
    while *p < cs.len() && cs[*p].is_whitespace() {
        *p += 1;
    }
    if cs.get(*p) != Some(&'{') {
        return atom(cs, p);
    }
    *p += 1;
    let mut depth = 1;
    let mut out = String::new();
    while *p < cs.len() {
        let c = cs[*p];
        *p += 1;
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn symbol(c: char) -> String {
    match c {
        '+' => "plus".into(),
        '-' | '\u{2212}' => "minus".into(),
        '=' => "equals".into(),
        '<' => "is less than".into(),
        '>' => "is greater than".into(),
        '/' => "divided by".into(),
        '*' => "times".into(),
        '(' => "open paren".into(),
        ')' => "close paren".into(),
        '[' => "open bracket".into(),
        ']' => "close bracket".into(),
        '|' => "bar".into(),
        '!' => "factorial".into(),
        '\'' => "prime".into(),
        '&' | '~' | '$' => " ".into(),
        c => c.to_string(),
    }
}

/// Does a script argument need an explicit end? `x^{a+b} + c` must not read as `x^(a+b+c)`; a bare sign and
/// one symbol (`e^{-x}`) is short enough to hear in one breath.
fn needs_end(arg: &str) -> bool {
    let a = arg.trim();
    let a = a.strip_prefix("minus ").unwrap_or(a);
    a.split_whitespace().count() > 1
}

fn power(arg: &str) -> String {
    let arg = arg.split_whitespace().collect::<Vec<_>>().join(" ");
    let arg = arg.as_str();
    let simple_frac = arg.split_once(" divided by ").is_some_and(|(n, d)| {
        !n.is_empty() && !d.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && d.chars().all(|c| c.is_ascii_digit())
    });
    let frame = arg.strip_prefix("the fraction ").and_then(|r| r.strip_suffix(" end fraction"));
    if let Some((n, d)) = frame.and_then(|r| r.split_once(" over ")) {
        if !n.is_empty() && !d.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && d.chars().all(|c| c.is_ascii_digit()) {
            return format!(" to the power of {n} over {d} ");
        }
    }
    match arg {
        // `^\circ` is a degree sign; `^{\top}` and `^T` a transpose, `^{-1}` an inverse
        "composed with" => " degrees ".into(),
        "T" | "transpose" => " transpose ".into(),
        "minus 1" => " inverse ".into(),
        a if simple_frac => format!(" to the power of {} ", a.replace(" divided by ", " over ")),
        "2" => " squared ".into(),
        "3" => " cubed ".into(),
        "degrees" => " degrees ".into(),
        "prime" => " prime ".into(),
        "prime prime" => " double prime ".into(),
        "prime prime prime" => " triple prime ".into(),
        a if needs_end(a) => format!(" to the power of {a} end of power "),
        a => format!(" to the power of {a} "),
    }
}

fn sub(arg: &str) -> String {
    if needs_end(arg) {
        format!(" sub {arg} end sub ")
    } else {
        format!(" sub {arg} ")
    }
}

/// Limits written after a big operator: `\sum_{i=1}^{n}` is "the sum from i equals 1 to n of".
fn limits(cs: &[char], p: &mut usize, head: &str) -> String {
    let (mut lo, mut hi) = (None, None);
    for _ in 0..2 {
        let mut q = *p;
        while q < cs.len() && cs[q].is_whitespace() {
            q += 1;
        }
        match cs.get(q) {
            Some('_') if lo.is_none() => {
                *p = q + 1;
                let was = IN_LIM.with(|l| l.replace(head.starts_with("the limit")));
                lo = Some(atom(cs, p).trim().to_string());
                IN_LIM.with(|l| l.set(was));
            }
            Some('^') if hi.is_none() => {
                *p = q + 1;
                hi = Some(atom(cs, p).trim().to_string());
            }
            _ => break,
        }
    }
    let mut out = format!(" {head}");
    if let Some(l) = lo {
        // `\lim_{x \to 0}` is "the limit as x approaches 0 of"
        if head.starts_with("the limit") {
            out.push_str(&format!(" as {l}"));
        } else if head.contains("contour") {
            out.push_str(&format!(" around {l}"));
        } else if head.contains("double") || head.contains("triple") || (head == "the integral" && hi.is_none() && is_region(&l)) {
            out.push_str(&format!(" over {l}"));
        } else {
            out.push_str(&format!(" from {l}"));
        }
    }
    if let Some(h) = hi {
        out.push_str(&format!(" to {h}"));
    }
    out.push_str(" of ");
    out
}

/// A region or curve under an integral sign: one capital letter (`\int_C`, `\int_D`) or a Greek curve name.
fn is_region(l: &str) -> bool {
    let w: Vec<&str> = l.split_whitespace().collect();
    match w.as_slice() {
        [t] => (t.len() == 1 && t.chars().all(|c| c.is_ascii_uppercase())) || matches!(*t, "gamma" | "sigma" | "omega"),
        ["capital", _] => true,
        _ => false,
    }
}

/// The two arguments of `\frac` or `\binom`; the flag says the fraction sits inside another one.
fn two_args(cs: &[char], p: &mut usize) -> (String, String, bool) {
    let outer = NEST.with(|n| n.get());
    NEST.with(|n| n.set(outer + 1));
    let (a, b) = (atom(cs, p), atom(cs, p));
    NEST.with(|n| n.set(outer));
    (a, b, outer > 0)
}

/// Is what follows a closed fraction an operator, a closing bracket or the end (so "1 over 2" cannot be
/// taken for the start of something longer)?
fn ends_here(cs: &[char], p: usize) -> bool {
    let mut q = p;
    while q < cs.len() && cs[q].is_whitespace() {
        q += 1;
    }
    match cs.get(q) {
        None => true,
        Some('\\') => {
            let w: String = cs[q + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
            w.is_empty()
                || matches!(w.as_str(), "pm" | "mp" | "cdot" | "times" | "div" | "le" | "leq" | "ge" | "geq" | "ne" | "neq" | "approx" | "sim" | "quad" | "qquad" | "right" | "to" | "in" | "end")
        }
        Some(c) => "+-=<>,;.)]}&|/!\u{2212}".contains(*c),
    }
}

/// `\begin{pmatrix} a & b \\ c & d \end{pmatrix}`: rows and columns are said, not spelled. `p` is just
/// after `\begin`.
fn environment(cs: &[char], p: &mut usize) -> String {
    let name = raw_group(cs, p);
    // the body runs to the `\end{name}` that matches, nested environments of the same name included
    let begin: Vec<char> = format!("\\begin{{{name}}}").chars().collect();
    let end: Vec<char> = format!("\\end{{{name}}}").chars().collect();
    let at = |q: usize, pat: &[char]| cs.len() >= q + pat.len() && cs[q..q + pat.len()] == *pat;
    let (start, mut q, mut depth) = (*p, *p, 1);
    let mut stop = cs.len();
    while q < cs.len() {
        if at(q, &begin) {
            depth += 1;
            q += begin.len();
        } else if at(q, &end) {
            depth -= 1;
            if depth == 0 {
                stop = q;
                break;
            }
            q += end.len();
        } else {
            q += 1;
        }
    }
    *p = (stop + end.len()).min(cs.len());
    let mut body: &[char] = &cs[start..stop];
    // `array` takes a column spec first: `{cc}`
    if matches!(name.as_str(), "array" | "subarray" | "alignedat") {
        let mut r = 0;
        let _ = raw_group(body, &mut r);
        body = &body[r..];
    }
    let say = |cell: &[char]| {
        let mut r = 0;
        seq(cell, &mut r, false).split_whitespace().collect::<Vec<_>>().join(" ")
    };
    // rows split at a top level `\\`, cells at a top level `&`
    let (mut rows, mut cells, mut cur): (Vec<Vec<String>>, Vec<String>, Vec<char>) = (vec![], vec![], vec![]);
    let (mut brace, mut env, mut k) = (0i32, 0i32, 0usize);
    let starts = |k: usize, s: &str| {
        let n = s.chars().count();
        body.len() >= k + n && body[k..k + n].iter().copied().eq(s.chars())
    };
    while k < body.len() {
        let c = body[k];
        if starts(k, "\\begin{") {
            env += 1;
        } else if starts(k, "\\end{") {
            env -= 1;
        }
        match c {
            '{' => brace += 1,
            '}' => brace -= 1,
            _ => {}
        }
        if brace == 0 && env == 0 && c == '&' {
            cells.push(say(&cur));
            cur.clear();
            k += 1;
        } else if brace == 0 && env == 0 && starts(k, "\\\\") {
            cells.push(say(&cur));
            cur.clear();
            rows.push(std::mem::take(&mut cells));
            k += 2;
        } else {
            cur.push(c);
            k += 1;
        }
    }
    if !cur.iter().all(|c| c.is_whitespace()) || !cells.is_empty() {
        cells.push(say(&cur));
        rows.push(cells);
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let list = |label: &str| rows.iter().enumerate().map(|(i, r)| format!("{label} {}: {}", i + 1, r.join(", "))).collect::<Vec<_>>().join("; ");
    let n = rows.len();
    let out = match name.trim_end_matches('*') {
        "pmatrix" | "bmatrix" | "Bmatrix" | "matrix" | "smallmatrix" | "array" => format!("the {n} by {cols} matrix, {}; end matrix", list("row")),
        "vmatrix" | "Vmatrix" => format!("the {n} by {cols} determinant, {}; end determinant", list("row")),
        "cases" => format!("cases, {}; end cases", list("case")),
        _ => list("line"),
    };
    format!(" {out} ")
}

/// `\name` with its arguments, `p` at the backslash.
fn command(cs: &[char], p: &mut usize) -> String {
    *p += 1;
    let start = *p;
    while *p < cs.len() && cs[*p].is_ascii_alphabetic() {
        *p += 1;
    }
    let name: String = if *p == start {
        // `\,` `\%` `\\`: one non-letter character
        match cs.get(*p) {
            Some(c) => {
                *p += 1;
                c.to_string()
            }
            None => return String::new(),
        }
    } else {
        cs[start..*p].iter().collect()
    };
    let multi = |s: &str| s.trim().contains(' ');
    match name.as_str() {
        "frac" | "dfrac" | "tfrac" | "cfrac" => {
            let (a, b, nested) = two_args(cs, p);
            // always closed: "1 over 2 plus 1 over 3" cannot be told from 1 over (2 plus 1) over 3
            let _ = nested;
            if a.is_empty() && b.is_empty() {
                return " over ".into();
            }
            format!(" the fraction {a} over {b} end fraction ")
        }
        "binom" | "dbinom" | "tbinom" => {
            let (a, b, nested) = two_args(cs, p);
            if nested || multi(&a) || multi(&b) || !ends_here(cs, *p) {
                format!(" the binomial {a} choose {b} end binomial ")
            } else {
                format!(" {a} choose {b} ")
            }
        }
        "begin" => environment(cs, p),
        "end" => {
            // a stray end of an environment: its name is not said
            let _ = raw_group(cs, p);
            String::new()
        }
        "sqrt" => {
            let mut idx = String::new();
            if cs.get(*p) == Some(&'[') {
                *p += 1;
                let st = *p;
                while *p < cs.len() && cs[*p] != ']' {
                    *p += 1;
                }
                idx = cs[st..*p].iter().collect();
                *p += 1;
            }
            let a = atom(cs, p);
            let head = match idx.as_str() {
                "" | "2" => "square root of".to_string(),
                "3" => "cube root of".to_string(),
                n => format!("root {n} of"),
            };
            if multi(&a) {
                format!(" {head} {a} end root ")
            } else {
                format!(" {head} {a} ")
            }
        }
        "bar" | "overline" => {
            let a = atom(cs, p);
            // a bar over several things needs a scope, or only the last one is heard as barred
            if multi(&a) {
                format!(" bar over {} end bar ", a.trim())
            } else {
                format!(" {a} bar ")
            }
        }
        "hat" => format!(" {} hat ", atom(cs, p)),
        "widehat" => {
            // an arc over vertex letters is an arc, not a hat
            let a = atom(cs, p);
            let w: Vec<&str> = a.split_whitespace().collect();
            if w.len() >= 2 && w.iter().all(|t| t.len() == 1 && t.chars().all(|c| c.is_ascii_uppercase())) {
                format!(" arc {a} ")
            } else {
                format!(" {a} hat ")
            }
        }
        "underbrace" | "overbrace" => {
            let a = atom(cs, p);
            let mut q = *p;
            while q < cs.len() && cs[q].is_whitespace() {
                q += 1;
            }
            if matches!(cs.get(q), Some('_') | Some('^')) {
                *p = q + 1;
                let l = atom(cs, p);
                format!(" {a} , labeled {l} end label ")
            } else {
                format!(" {a} ")
            }
        }
        "overset" | "stackrel" | "underset" => {
            let (top, base) = (atom(cs, p), atom(cs, p));
            let (t, b) = (top.trim(), base.trim());
            if t == "arc" {
                format!(" arc {b} ")
            } else if name == "underset" {
                format!(" {b} with {t} below ")
            } else {
                format!(" {b} with {t} above ")
            }
        }
        "log" if cs.get(*p) == Some(&'_') => {
            *p += 1;
            let b = atom(cs, p);
            format!(" log base {} of ", b.trim())
        }
        "left" | "right" => {
            // `\left.` and `\right.` are invisible delimiters
            if cs.get(*p) == Some(&'.') {
                *p += 1;
            }
            String::new()
        }
        "tilde" | "widetilde" => format!(" {} tilde ", atom(cs, p)),
        "dot" => format!(" {} dot ", atom(cs, p)),
        "vec" => format!(" vector {} ", atom(cs, p)),
        "color" => {
            // `\color{red}{x}` speaks just x; the colour is not part of the maths
            let _ = raw_group(cs, p);
            format!(" {} ", atom(cs, p))
        }
        "textcolor" => {
            let _ = raw_group(cs, p);
            format!(" {} ", atom(cs, p))
        }
        "mathbb" | "Bbb" => {
            let g = raw_group(cs, p);
            let w = match g.trim() {
                "R" => "real numbers",
                "Z" => "integers",
                "Q" => "rationals",
                "N" => "natural numbers",
                "C" => "complex numbers",
                "P" => "probability",
                "E" => "expected value",
                "H" => "quaternions",
                other => other,
            };
            format!(" {w} ")
        }
        "mathcal" | "mathscr" => {
            let g = raw_group(cs, p);
            let g = g.trim();
            if g == "P" && cs.get(*p) == Some(&'(') {
                " the power set of ".into()
            } else if g.chars().count() == 1 && g.chars().all(|c| c.is_ascii_alphabetic()) {
                format!(" script {g} ")
            } else {
                format!(" {g} ")
            }
        }
        "mathbf" | "mathit" | "boldsymbol" | "bm" => {
            // a short run of letters is a product of variables, not a word
            let g = raw_group(cs, p);
            let g = g.trim();
            if g.chars().count() <= 3 && g.chars().all(|c| c.is_ascii_alphabetic()) {
                format!(" {} ", g.chars().map(String::from).collect::<Vec<_>>().join(" "))
            } else {
                format!(" {g} ")
            }
        }
        "text" | "textbf" | "textit" | "mathrm" | "operatorname" | "mbox" => {
            format!(" {} ", raw_group(cs, p))
        }
        "sum" => limits(cs, p, "the sum"),
        "prod" => limits(cs, p, "the product"),
        "int" => limits(cs, p, "the integral"),
        "iint" => limits(cs, p, "the double integral"),
        "iiint" => limits(cs, p, "the triple integral"),
        "oint" => limits(cs, p, "the contour integral"),
        "lim" => limits(cs, p, "the limit"),
        "bigcup" | "bigsqcup" => limits(cs, p, "the union"),
        "bigcap" => limits(cs, p, "the intersection"),
        "bigvee" => limits(cs, p, "the logical or"),
        "bigwedge" => limits(cs, p, "the logical and"),
        "bigoplus" => limits(cs, p, "the direct sum"),
        "bigotimes" => limits(cs, p, "the tensor product"),
        "coprod" => limits(cs, p, "the coproduct"),
        "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "min" | "max" | "sup" | "inf" => format!(" {name} "),
        "Sigma" | "Delta" | "Omega" | "Gamma" | "Theta" | "Lambda" | "Pi" | "Phi" | "Psi" | "Xi" => {
            format!(" capital {} ", name.to_lowercase())
        }
        "to" | "rightarrow" | "longrightarrow" if IN_LIM.with(|l| l.get()) => " approaches ".into(),
        "xrightarrow" | "xleftarrow" => {
            if cs.get(*p) == Some(&'[') {
                while *p < cs.len() && cs[*p] != ']' {
                    *p += 1;
                }
                *p += 1;
            }
            let a = atom(cs, p);
            let dir = if name == "xrightarrow" { "arrow" } else { "arrow back" };
            format!(" {dir} labeled {a} end arrow ")
        }
        "exists" if cs.get(*p) == Some(&'!') => {
            *p += 1;
            " there exists exactly one ".into()
        }
        "overleftrightarrow" => format!(" line {} ", atom(cs, p)),
        "ddot" => format!(" {} double dot ", atom(cs, p)),
        "dddot" => format!(" {} triple dot ", atom(cs, p)),
        "underline" | "underbar" | "mathsf" | "mathtt" | "mathfrak" | "mathbbm" => {
            format!(" {} ", atom(cs, p))
        }
        "overrightarrow" | "overleftarrow" | "overrightarrow*" => format!(" vector {} ", atom(cs, p)),
        "equiv" if { let rest: String = cs[*p..].iter().collect(); rest.contains("\\pmod") || rest.contains("\\pod") } => " is congruent to ".into(),
        "pmod" | "pod" => format!(" mod {} ", atom(cs, p)),
        "overrightharpoon" => format!(" vector {} ", atom(cs, p)),
        "varepsilon" => " epsilon ".into(),
        "varphi" => " phi ".into(),
        "vartheta" => " theta ".into(),
        n => match word_for(n) {
            Some(s) => format!(" {s} "),
            None => format!(" {n} "),
        },
    }
}

/// Bars left in the current group after `from` (a `\|` or `\{` is not one).
fn bars_ahead(cs: &[char], from: usize, in_group: bool) -> usize {
    let (mut n, mut depth, mut q) = (0, 0i32, from);
    while q < cs.len() {
        match cs[q] {
            '\\' => q += 1,
            '{' => depth += 1,
            '}' => {
                if depth == 0 && in_group {
                    break;
                }
                depth -= 1;
            }
            '|' if depth == 0 => n += 1,
            _ => {}
        }
        q += 1;
    }
    n
}

/// `|_{x=2}` and `|_0^1` after a bar with nothing open: the bar says where an expression is evaluated.
/// `p` is just past the bar; it moves past the scripts only when the bar is one.
fn evaluation_bar(cs: &[char], p: &mut usize) -> Option<String> {
    evaluation_after(cs, p, false)
}

/// `need_sub`: after `\right)` or `\right]` a lone `^` is a power ("cubed"); only a subscript evaluates.
fn evaluation_after(cs: &[char], p: &mut usize, need_sub: bool) -> Option<String> {
    let mut q = *p;
    while q < cs.len() && cs[q].is_whitespace() {
        q += 1;
    }
    if !(cs.get(q) == Some(&'_') || (!need_sub && cs.get(q) == Some(&'^'))) {
        return None;
    }
    let (mut lo, mut hi) = (None, None);
    for _ in 0..2 {
        match cs.get(q) {
            Some('_') if lo.is_none() => {
                q += 1;
                lo = Some(atom(cs, &mut q).trim().to_string());
            }
            Some('^') if hi.is_none() => {
                q += 1;
                hi = Some(atom(cs, &mut q).trim().to_string());
            }
            _ => break,
        }
    }
    *p = q;
    Some(match (lo, hi) {
        (Some(l), Some(h)) => format!(" evaluated from {l} to {h} "),
        (Some(l), None) => format!(" evaluated at {l} end evaluation "),
        (None, Some(h)) => format!(" evaluated at {h} end evaluation "),
        (None, None) => String::new(),
    })
}

/// `\|` / `\Vert` left after `from`.
fn norms_ahead(cs: &[char], from: usize) -> usize {
    let (mut n, mut q) = (0, from);
    while q + 1 < cs.len() {
        if cs[q] == '\\' {
            if cs[q + 1] == '|' {
                n += 1;
                q += 1;
            } else if cs[q + 1..].starts_with(&['V', 'e', 'r', 't']) {
                n += 1;
                q += 4;
            }
        }
        q += 1;
    }
    n
}

/// The words just spoken end in a figure (a triangle, an angle, a run of capital letters `ABC`), so a
/// tilde after them is similarity.
fn figure_before(out: &str) -> bool {
    let w: Vec<&str> = out.split_whitespace().collect();
    let last = w.iter().rev().take(4).copied().collect::<Vec<_>>();
    if last.iter().any(|t| matches!(*t, "triangle" | "angle" | "square" | "polygon" | "quadrilateral" | "measured")) {
        return true;
    }
    // single capital letters in a row, as a vertex list
    let caps = w.iter().rev().take_while(|t| t.len() == 1 && t.chars().all(|c| c.is_ascii_uppercase())).count();
    caps >= 3
}

/// Is the delimiter at `at` the one a `\right` (or `\left`, with `word`) just before it names?
fn after_word(cs: &[char], at: usize, word: &str) -> bool {
    let mut q = at;
    while q > 0 && cs[q - 1].is_whitespace() {
        q -= 1;
    }
    let w: Vec<char> = word.chars().collect();
    q >= w.len() && cs[q - w.len()..q] == w[..]
}

/// `\to`, `\rightarrow` or `\longrightarrow` at `p`: its length.
fn arrow_len(cs: &[char], p: usize) -> Option<usize> {
    for w in ["\\longrightarrow", "\\rightarrow", "\\to"] {
        let n = w.chars().count();
        if cs.len() >= p + n && cs[p..p + n].iter().copied().eq(w.chars()) && !cs.get(p + n).is_some_and(|c| c.is_ascii_alphabetic()) {
            return Some(n);
        }
    }
    None
}

/// After an arrow: a value with a `^+` or `^-` (`0^+`), a one-sided limit.
fn one_sided_ahead(cs: &[char], from: usize) -> bool {
    let mut q = from;
    let skip = |q: &mut usize| {
        while *q < cs.len() && cs[*q].is_whitespace() {
            *q += 1;
        }
    };
    skip(&mut q);
    if cs.get(q) == Some(&'-') {
        q += 1;
    }
    match cs.get(q) {
        Some('{') => {
            let mut depth = 0;
            while q < cs.len() {
                match cs[q] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            q += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                q += 1;
            }
        }
        Some('\\') => {
            q += 1;
            while q < cs.len() && cs[q].is_ascii_alphabetic() {
                q += 1;
            }
        }
        Some(c) if c.is_ascii_digit() => {
            while q < cs.len() && (cs[q].is_ascii_digit() || cs[q] == '.') {
                q += 1;
            }
        }
        Some(c) if c.is_alphabetic() => q += 1,
        _ => return false,
    }
    skip(&mut q);
    if cs.get(q) != Some(&'^') {
        return false;
    }
    q += 1;
    skip(&mut q);
    let braced = cs.get(q) == Some(&'{');
    if braced {
        q += 1;
        skip(&mut q);
    }
    matches!(cs.get(q), Some('+') | Some('-'))
}

/// A vector written in TeX: `\vec{u}`, `\overrightarrow{AB}`, `\hat{i}`, and (as a dot product only, since
/// a bold capital is a matrix) `\mathbf{u}`. `cross`: only what can be crossed.
fn vector_tex(head: &str, arg: &str, cross: bool) -> bool {
    match head {
        "vec" | "overrightarrow" | "overleftarrow" | "hat" => true,
        "mathbf" | "boldsymbol" => !cross || (arg.chars().count() == 1 && arg.chars().all(|c| c.is_ascii_lowercase())),
        _ => false,
    }
}

/// Is there a vector right before `at` (the backslash of the operator) or right after `after`?
fn vector_near(cs: &[char], at: usize, after: usize, cross: bool) -> bool {
    // before: `\vec{u}` ends in a `}`
    let mut q = at;
    while q > 0 && cs[q - 1].is_whitespace() {
        q -= 1;
    }
    if q > 0 && cs[q - 1] == '}' {
        let (mut depth, mut m) = (0i32, q - 1);
        loop {
            match cs[m] {
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            if m == 0 {
                break;
            }
            m -= 1;
        }
        let head: String = cs[..m].iter().rev().take_while(|c| c.is_ascii_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
        let arg: String = cs[m + 1..q - 1].iter().collect();
        if m > head.len() && cs[m - head.len() - 1] == '\\' && vector_tex(&head, arg.trim(), cross) {
            return true;
        }
    }
    // after
    let mut r = after;
    while r < cs.len() && cs[r].is_whitespace() {
        r += 1;
    }
    if cs.get(r) == Some(&'\\') {
        let head: String = cs[r + 1..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        let k = r + 1 + head.len();
        let arg: String = if cs.get(k) == Some(&'{') { cs[k + 1..].iter().take_while(|c| **c != '}').collect() } else { String::new() };
        return vector_tex(&head, arg.trim(), cross) || (!cross && head == "nabla");
    }
    false
}

/// `[a,b)` and `(a,b]`: an interval with one end in and one out. `p` is at the opening bracket.
fn interval(cs: &[char], p: usize) -> Option<(String, usize)> {
    let open = cs[p];
    let (mut depth, mut q, mut comma) = (0i32, p + 1, None);
    while q < cs.len() {
        match cs[q] {
            '\\' => q += 1,
            '(' | '[' | '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                if comma.is_some() {
                    return None;
                }
                comma = Some(q);
            }
            ')' | ']' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
        q += 1;
    }
    let close = *cs.get(q)?;
    let comma = comma?;
    if !((open == '[' && close == ')') || (open == '(' && close == ']')) {
        return None;
    }
    let mut hi_end = q;
    // `\left[a,b\right)`
    let right: Vec<char> = "\\right".chars().collect();
    if hi_end >= right.len() && cs[hi_end - right.len()..hi_end] == right[..] {
        hi_end -= right.len();
    }
    let say = |s: &[char]| {
        let mut r = 0;
        seq(s, &mut r, false).split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let (a, b) = (say(&cs[p + 1..comma]), say(&cs[comma + 1..hi_end]));
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let text = if open == '[' {
        format!(" the interval from {a}, including {a}, to {b}, not including {b} ")
    } else {
        format!(" the interval from {a}, not including {a}, to {b}, including {b} ")
    };
    Some((text, q + 1))
}

/// Between two bars: a set (`A \cap B`, `\mathcal{A}`), whose size is the cardinality.
fn bar_holds_set(cs: &[char], from: usize) -> bool {
    let (mut depth, mut q) = (0i32, from);
    while q < cs.len() {
        match cs[q] {
            '\\' => q += 1,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            '|' if depth == 0 => {
                let inner: String = cs[from..q].iter().collect();
                return ["\\cap", "\\cup", "\\setminus", "\\mathcal", "\\emptyset", "\\varnothing"].iter().any(|w| inner.contains(w));
            }
            _ => {}
        }
        q += 1;
    }
    false
}

fn ordinal(n: &str) -> String {
    match n {
        "1" => "first".into(),
        "2" => "second".into(),
        "3" => "third".into(),
        "4" => "fourth".into(),
        "5" => "fifth".into(),
        n if n.chars().all(|c| c.is_ascii_digit()) => format!("{n}th"),
        n => format!("{n}th"),
    }
}

fn divider(paren: i32, in_set: bool) -> &'static str {
    if in_set {
        " such that "
    } else if paren > 0 {
        " given "
    } else {
        " divides "
    }
}

/// Is there a finished clause before a quantifier (`\forall \epsilon>0 \exists \delta`), so a pause keeps the
/// two quantifiers from running into one sentence?
fn quantifier_follows_clause(out: &str) -> bool {
    let t = out.trim_end();
    if t.is_empty() || t.ends_with(',') {
        return false;
    }
    let last = t.split_whitespace().next_back().unwrap_or("");
    !matches!(last, "not" | "and" | "or" | "implies" | "if" | "that" | "some" | "all" | "of" | "open" | "paren" | "bracket" | "set" | "to" | "equals" | "plus" | "minus" | "times" | "divides" | "given")
}

fn seq(cs: &[char], p: &mut usize, in_group: bool) -> String {
    let mut out = String::new();
    // `|x - 3|`: the first bar opens an absolute value and the next closes it
    let mut bar_open = false;
    // the closing words of an open `|...|`: a single capital letter between bars is a set's size
    let mut bar_close = " end absolute value ";
    let mut norm_open = false;
    // after an arrow that heads for a one-sided value (x to 0^+)
    let mut approach = false;
    // `P(A|B)` and `\{x | x > 0\}`: a bar with no partner is a divider, not an absolute value
    let (mut paren, mut in_set) = (0i32, false);
    while *p < cs.len() {
        let c = cs[*p];
        match c {
            '|' => {
                *p += 1;
                if bar_open {
                    out.push_str(bar_close);
                    bar_open = false;
                } else if let Some(s) = evaluation_bar(cs, p) {
                    out.push_str(&s);
                } else if bars_ahead(cs, *p, in_group) % 2 == 0 {
                    out.push_str(divider(paren, in_set));
                } else {
                    let mut q = *p;
                    while q < cs.len() && cs[q].is_whitespace() {
                        q += 1;
                    }
                    let mut r = q + 1;
                    while r < cs.len() && cs[r].is_whitespace() {
                        r += 1;
                    }
                    let set = (cs.get(q).is_some_and(|c| c.is_ascii_uppercase()) && cs.get(r) == Some(&'|')) || bar_holds_set(cs, *p);
                    out.push_str(if set { " the size of " } else { " absolute value of " });
                    bar_close = if set { " end size " } else { " end absolute value " };
                    bar_open = true;
                }
            }
            ':' if in_set && paren == 0 => {
                *p += 1;
                out.push_str(" such that ");
            }
            '\\' if cs[*p + 1..].starts_with(&['c', 'd', 'o', 't']) && !cs.get(*p + 5).is_some_and(|c| c.is_ascii_alphabetic()) => {
                *p += 5;
                let w: Vec<&str> = out.split_whitespace().collect();
                let n = w.len();
                let vector_before = n >= 1 && (w[n - 1] == "del" || (n >= 2 && w[n - 2] == "vector"));
                let mut q = *p;
                while q < cs.len() && cs[q].is_whitespace() {
                    q += 1;
                }
                let rest: String = cs[q..].iter().take(12).collect();
                let vector_after = ["\\vec", "\\mathbf", "\\overrightarrow", "\\nabla", "\\boldsymbol", "\\hat"].iter().any(|v| rest.starts_with(v));
                let vector_tex = vector_near(cs, *p - 5, *p, false);
                out.push_str(if vector_before || vector_after || vector_tex { " dot " } else { " times " });
            }
            '\\' if cs[*p + 1..].starts_with(&['t', 'i', 'm', 'e', 's']) && !cs.get(*p + 6).is_some_and(|c| c.is_ascii_alphabetic()) => {
                let at = *p;
                *p += 6;
                out.push_str(if vector_near(cs, at, *p, true) { " cross " } else { " times " });
            }
            '\\' if arrow_len(cs, *p).is_some_and(|n| one_sided_ahead(cs, *p + n)) => {
                *p += arrow_len(cs, *p).unwrap_or(0);
                approach = true;
                out.push_str(" approaches ");
            }
            '(' | '[' if interval(cs, *p).is_some() => {
                let (text, end) = interval(cs, *p).unwrap_or_default();
                out.push_str(&text);
                *p = end;
            }
            ']' if after_word(cs, *p, "\\right") => {
                *p += 1;
                out.push_str(" close bracket ");
                if let Some(s) = evaluation_after(cs, p, true) {
                    out.push_str(&s);
                }
            }
            '(' => {
                paren += 1;
                *p += 1;
                out.push_str(" open paren ");
            }
            ')' => {
                let right = after_word(cs, *p, "\\right");
                paren -= 1;
                *p += 1;
                out.push_str(" close paren ");
                if right {
                    if let Some(s) = evaluation_after(cs, p, true) {
                        out.push_str(&s);
                    }
                }
            }
            '\\' if cs[*p + 1..].starts_with(&['o', 'd', 'o', 't']) && !cs.get(*p + 5).is_some_and(|c| c.is_ascii_alphabetic()) => {
                *p += 5;
                // a circle with its centre (`\odot O`) at the start; between two things the Hadamard product
                out.push_str(if out.trim().is_empty() { " circle " } else { " circle dot " });
            }
            '\\' if cs[*p + 1..].starts_with(&['s', 'i', 'm']) && !cs.get(*p + 4).is_some_and(|c| c.is_ascii_alphabetic()) => {
                *p += 4;
                out.push_str(if figure_before(&out) { " is similar to " } else { " is distributed as " });
            }
            '\\' if cs.get(*p + 1) == Some(&'|') || (cs[*p + 1..].starts_with(&['V', 'e', 'r', 't']) && !cs.get(*p + 5).is_some_and(|c| c.is_ascii_alphabetic())) => {
                *p += if cs[*p + 1] == '|' { 2 } else { 5 };
                if norm_open {
                    out.push_str(" end norm ");
                    norm_open = false;
                } else if norms_ahead(cs, *p) > 0 {
                    out.push_str(" norm of ");
                    norm_open = true;
                } else {
                    out.push_str(" norm ");
                }
            }
            '\\' if cs.get(*p + 1) == Some(&'{') => {
                // a left brace before an array is a system of equations, not a set
                let left = after_word(cs, *p, "\\left");
                *p += 2;
                let mut q = *p;
                while q < cs.len() && cs[q].is_whitespace() {
                    q += 1;
                }
                if !(left && cs[q..].starts_with(&['\\', 'b', 'e', 'g', 'i', 'n'])) {
                    in_set = true;
                    out.push_str(" the set ");
                }
            }
            '\\' if cs[*p + 1..].starts_with(&['m', 'i', 'd']) && !cs.get(*p + 4).is_some_and(|c| c.is_ascii_alphabetic()) => {
                *p += 4;
                out.push_str(divider(paren, in_set));
            }
            '\\' if ["forall", "exists", "nexists"].iter().any(|w| {
                let n = w.chars().count();
                cs[*p + 1..].len() >= n && cs[*p + 1..*p + 1 + n].iter().copied().eq(w.chars()) && !cs.get(*p + 1 + n).is_some_and(|c| c.is_ascii_alphabetic())
            }) && quantifier_follows_clause(&out) =>
            {
                out.push_str(" , ");
                out.push_str(&command(cs, p));
            }
            '}' if in_group => break,
            '}' => *p += 1,
            '{' => {
                out.push(' ');
                out.push_str(&atom(cs, p));
                out.push(' ');
            }
            '^' if out.trim().is_empty() => {
                // a leading superscript is a mass number: `{}^{14}C` is "14 C"
                *p += 1;
                let a = atom(cs, p);
                out.push_str(&format!(" {} ", a.trim()));
            }
            '^' => {
                *p += 1;
                let a = atom(cs, p);
                let was_approach = std::mem::replace(&mut approach, false);
                let lone_capital = out.split_whitespace().next_back().is_some_and(|w| w.len() == 1 && w.chars().all(|c| c.is_ascii_uppercase()));
                let tail = out.trim_end().to_string();
                let set = [("real numbers", ""), ("complex numbers", ""), ("rationals", ""), ("integers", ""), ("natural numbers", "")]
                    .into_iter()
                    .find(|(w, _)| tail.ends_with(w));
                let one = a.trim();
                let derivative = one.strip_prefix("open paren ").and_then(|r| r.strip_suffix(" close paren")).filter(|r| !r.is_empty() && !r.contains("paren") && r.split_whitespace().count() == 1);
                if was_approach && one == "plus" {
                    out.push_str(" from the right ");
                } else if was_approach && one == "minus" {
                    out.push_str(" from the left ");
                } else if let Some(n) = derivative {
                    out.push_str(&format!(" {} derivative ", ordinal(n.trim())));
                } else if let (Some((w, _)), true) = (set, one == "plus" || one == "minus" || (one.chars().count() == 1 && one.chars().all(|c| c.is_ascii_alphanumeric()))) {
                    out.truncate(tail.len() - w.len());
                    match one {
                        "plus" => out.push_str(&format!(" positive {w} ")),
                        "minus" => out.push_str(&format!(" negative {w} ")),
                        n => out.push_str(&format!(" {w} in {n} dimensions ")),
                    }
                } else if one == "times" {
                    // z^* (a critical value): the asterisk is a star, not a product
                    out.push_str(" star ");
                } else if a.trim() == "c" && lone_capital {
                    out.push_str(" complement ");
                } else {
                    out.push_str(&power(a.trim()));
                }
            }
            '_' => {
                *p += 1;
                let a = atom(cs, p);
                // `{}_nC_r` (or `_nC_r`): a count of choices, "n choose r"
                let mut q = *p;
                while q < cs.len() && cs[q].is_whitespace() {
                    q += 1;
                }
                if out.trim().is_empty() && matches!(cs.get(q), Some('C') | Some('P')) && cs.get(q + 1) == Some(&'_') {
                    let letter = cs[q];
                    let mut r = q + 2;
                    let b = atom(cs, &mut r);
                    *p = r;
                    if letter == 'C' {
                        out.push_str(&format!(" {} choose {} ", a.trim(), b.trim()));
                    } else {
                        out.push_str(&format!(" {} permute {} ", a.trim(), b.trim()));
                    }
                } else {
                    out.push_str(&sub(a.trim()));
                }
            }
            '\\' => out.push_str(&command(cs, p)),
            c if c.is_whitespace() => {
                *p += 1;
                out.push(' ');
            }
            c if c.is_ascii_digit() => {
                // a number stays one token: 21.25
                while *p < cs.len() && (cs[*p].is_ascii_digit() || (cs[*p] == '.' && cs.get(*p + 1).is_some_and(|d| d.is_ascii_digit()))) {
                    out.push(cs[*p]);
                    *p += 1;
                }
                out.push(' ');
                // `1\frac{1}{2}` is a mixed number
                if cs[*p..].starts_with(&['\\', 'f', 'r', 'a', 'c']) || cs[*p..].starts_with(&['\\', 'd', 'f', 'r', 'a', 'c']) {
                    out.push_str(" and ");
                }
            }
            c => {
                *p += 1;
                out.push(' ');
                out.push_str(&symbol(c));
                out.push(' ');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speaks_the_demo_formulas() {
        assert_eq!(
            tex_speech(r"\bar{x} \pm 1.96 \cdot \frac{s}{\sqrt{n}} = 21.25"),
            "x bar plus or minus 1.96 times the fraction s over square root of n end fraction equals 21.25"
        );
        assert_eq!(
            tex_speech(r"\bar{x} \pm 1.96\cdot\frac{s}{\sqrt{n}} = (18.4,\ 24.1)"),
            "x bar plus or minus 1.96 times the fraction s over square root of n end fraction equals open paren 18.4, 24.1 close paren"
        );
    }

    #[test]
    fn speaks_powers_scripts_and_fractions() {
        assert_eq!(tex_speech("x^2 + y_1"), "x squared plus y sub 1");
        assert_eq!(tex_speech("e^{-x}"), "e to the power of minus x");
        assert_eq!(tex_speech(r"\frac{a+b}{2}"), "the fraction a plus b over 2 end fraction");
        assert_eq!(tex_speech(r"90^\circ"), "90 degrees");
        assert_eq!(tex_speech(r"\sqrt[3]{x}"), "cube root of x");
        assert_eq!(tex_speech(r"\text{mean} \le \mu"), "mean is less than or equal to mu");
        assert_eq!(tex_speech(r"50\%"), "50 percent");
        // limits, the end of long exponents and subscripts, absolute value
        assert_eq!(tex_speech(r"\sum_{i=1}^{n} x_i"), "the sum from i equals 1 to n of x sub i");
        assert_eq!(tex_speech(r"\int_0^1 x\,dx"), "the integral from 0 to 1 of x d x");
        assert_eq!(tex_speech(r"x^{a+b} + c"), "x to the power of a plus b end of power plus c");
        assert_eq!(tex_speech(r"a_{n+1} = 2"), "a sub n plus 1 end sub equals 2");
        assert_eq!(tex_speech(r"|x-3|<2"), "absolute value of x minus 3 end absolute value is less than 2");
        // never panics on junk or a truncated formula
        assert_eq!(tex_speech(r"\frac{"), "over");
        let _ = tex_speech(r"\{^_}\\");
    }

    /// Blind critic round 3: formulas where a listener cannot tell which part belongs to which. Failing by design.
    #[test]
    fn nested_fractions_keep_their_scope() {
        // "1 over 2 over 3" is (1/2)/3 or 1/(2/3): the inner fraction must be closed
        let s = tex_speech(r"\frac{\frac{1}{2}}{3}");
        assert_eq!(s.matches("end fraction").count(), 2, "{s}");
        // "1 over 2 x" reads as 1/(2x): the fraction must be closed before the factor
        let s = tex_speech(r"\frac{1}{2}x");
        assert!(s.contains("end fraction"), "{s}");
    }

    #[test]
    fn matrices_and_binomials_are_not_spelled_out() {
        let s = tex_speech(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
        assert!(!s.contains("p m a t r i x"), "{s}");
        assert!(s.contains("matrix") && s.contains("row"), "{s}");
        let s = tex_speech(r"\binom{n}{k}");
        assert!(s.contains("choose"), "{s}");
    }

    /// Blind critic round 5: a bar between two things is not an absolute value. A statistics teacher writes
    /// `P(A|B)` and `\{x \mid x > 0\}`; the listener must not hear "absolute value". Failing by design.
    #[test]
    fn a_bar_between_things_is_not_an_absolute_value() {
        let s = tex_speech(r"P(A|B)");
        assert!(!s.contains("absolute value"), "conditional probability: {s}");
        assert!(s.contains("given"), "conditional probability reads as 'given': {s}");
        let s = tex_speech(r"\{x | x > 0\}");
        assert!(!s.contains("absolute value"), "set builder: {s}");
        // a real one still is
        assert_eq!(tex_speech(r"\lvert x \rvert"), "absolute value of x end absolute value");
    }

    /// Blind critic round 5: a command with no spoken form is read as its TeX name ("Rightarrow", "lfloor",
    /// "emptyset"), which is noise to someone who cannot see the symbol. Failing by design.
    #[test]
    fn symbols_are_not_read_as_their_tex_names() {
        let names = [
            "emptyset", "forall", "exists", "Rightarrow", "Leftrightarrow", "lfloor", "rfloor", "lceil", "rceil", "cong",
            "perp", "mid", "notin", "subseteq", "supseteq", "mapsto", "equiv", "propto", "lvert", "rvert",
            "iint", "wedge", "vee", "neg", "overrightarrow", "xrightarrow", "bmod",
        ];
        let bad: Vec<String> = names
            .iter()
            .filter(|n| {
                let s = tex_speech(&format!("a \\{n} b"));
                s.split_whitespace().any(|w| w.eq_ignore_ascii_case(n))
            })
            .map(|n| n.to_string())
            .collect();
        assert!(bad.is_empty(), "spoken as the TeX name: {bad:?}");
    }

    /// Blind critic round 5: `\to` in a function arrow is "to", not "approaches" (only a limit approaches).
    #[test]
    fn a_function_arrow_is_not_a_limit() {
        let s = tex_speech(r"f: A \to B");
        assert!(!s.contains("approaches"), "{s}");
        // inside a limit it still approaches
        assert!(tex_speech(r"\lim_{x \to 0} f(x)").contains("approaches"));
    }

    /// Round 5: a mixed number is "1 and 1 over 2", a set-builder bar is "such that", a pair of bars is one value.
    #[test]
    fn mixed_numbers_and_set_builder_bars() {
        let s = tex_speech(r"1\frac{1}{2}");
        assert!(s.starts_with("1 and "), "{s}");
        assert!(tex_speech(r"\{x | x > 0\}").contains("such that"));
        assert!(tex_speech(r"P(A|B)").contains("given"));
        assert_eq!(tex_speech(r"|a| + |b|").matches("absolute value of").count(), 2);
        assert!(tex_speech(r"a \mid b").contains("divides"));
        assert!(!tex_speech(r"x \to 3").contains("approaches"));
    }

    /// Round 7 (blind critic): \ between two figures is similarity, not "is distributed as" (only a random
    /// variable before a distribution is). \ is the real numbers, not the letter R. Failing by design.
    #[test]
    fn similar_figures_and_number_sets_are_not_misread() {
        let s = tex_speech(r"\triangle ABC \sim \triangle DEF");
        assert!(!s.contains("distributed"), "similar triangles: {s}");
        assert!(s.contains("similar"), "similar triangles: {s}");
        // a random variable still is distributed
        assert!(tex_speech(r"X \sim N(0, 1)").contains("distributed"));
        let s = tex_speech(r"x \in \mathbb{R}");
        assert!(s.contains("real numbers"), "R of the reals: {s}");
        let s = tex_speech(r"n \in \mathbb{Z}");
        assert!(s.contains("integers"), "Z of the integers: {s}");
        for (tex, want) in [
            (r"q \in \mathbb{Q}", "rationals"),
            (r"n \in \mathbb{N}", "natural numbers"),
            (r"z \in \mathbb{C}", "complex numbers"),
            (r"\mathbb{R}^2", "real numbers"),
        ] {
            let s = tex_speech(tex);
            assert!(s.contains(want), "{tex}: {s}");
        }
        assert_eq!(tex_speech(r"\| v \|"), "norm of v end norm");
        assert_eq!(tex_speech(r"\|x\| + \|y\|").matches("end norm").count(), 2);
        let s = tex_speech(r"\color{red}{x} + 1");
        assert!(!s.contains("red") && !s.contains("color") && s.contains("x"), "{s}");
        assert!(!tex_speech(r"\textcolor{blue}{y}").contains("blue"));
        let s = tex_speech(r"\angle A \cong \angle B");
        assert!(s.contains("congruent") && !s.contains("distributed"), "{s}");
        let s = tex_speech(r"ABCD \sim EFGH");
        assert!(s.contains("similar"), "{s}");
        assert!(tex_speech(r"x \sim \text{Binomial}(n, p)").contains("distributed"));
        // more shapes from the S5 corpus
        assert!(tex_speech(r"\triangle ABC \cong \triangle DEF").contains("congruent"));
        assert!(tex_speech(r"\mathbb{R}^n").contains("real numbers"));
        assert_eq!(tex_speech(r"\|\vec{v}\|"), "norm of vector v end norm");
        assert!(!tex_speech(r"\textcolor{blue}{x^2}").contains("blue"));
        assert!(tex_speech(r"f: \mathbb{R} \to \mathbb{R}").contains("real numbers to real numbers"));
        assert!(tex_speech(r"\mathbb{Z}^+").contains("positive integers"));
    }

    /// Critic round 11: critical value star, script letters, bar scope, quantifier pause, bold letters.
    #[test]
    fn round11_star_script_bar_quantifiers_bold() {
        let s = tex_speech(r"\hat{p}\pm z^*\sqrt{\frac{\hat p(1-\hat p)}{n}}");
        assert!(s.contains("z star square root of") && !s.contains("to the power of"), "{s}");
        assert_eq!(tex_speech(r"z^{*}"), "z star");
        assert_eq!(tex_speech(r"\mathcal{P}(A)"), "the power set of open paren A close paren");
        assert_eq!(tex_speech(r"\mathcal{L}"), "script L");
        assert_eq!(tex_speech(r"\overline{A\cup B}=\overline{A}\cap\overline{B}"), "bar over A union B end bar equals A bar intersection B bar");
        assert_eq!(tex_speech(r"\overline{AB}"), "bar over A B end bar");
        assert_eq!(tex_speech(r"\overline{x}"), "x bar");
        assert_eq!(
            tex_speech(r"\forall \epsilon>0\,\exists\delta>0"),
            "for all epsilon is greater than 0, there is some delta is greater than 0"
        );
        assert_eq!(tex_speech(r"\forall x \exists y"), "for all x, there is some y");
        assert_eq!(tex_speech(r"\mathbf{Ax}=\lambda\mathbf{x}"), "A x equals lambda x");
    }

    /// Blind critic round 8: formulas a listener mishears. `#[ignore]`d so the workspace stays green; run with
    /// `cargo test -p vishion-ui -- --ignored round8`. Failing by design.
    #[test]
    fn round8_function_composition_and_dot_products() {
        let s = tex_speech(r"f \circ g");
        assert!(!s.contains("degrees"), "composition is not degrees: {s}");
        let s = tex_speech(r"\vec{v} \cdot \vec{w}");
        assert!(s.contains("dot"), "a dot product is not 'times': {s}");
        let s = tex_speech(r"\nabla \cdot \vec{F}");
        assert!(s.contains("dot") || s.contains("divergence"), "divergence: {s}");
    }

    #[test]
    fn round8_logs_inverses_transpose_and_evaluation_bars() {
        assert!(tex_speech(r"\log_2 8").contains("base 2"), "{}", tex_speech(r"\log_2 8"));
        assert!(tex_speech(r"\log_{10} 100").contains("base 10"));
        assert!(tex_speech(r"A^T").contains("transpose"), "{}", tex_speech(r"A^T"));
        assert!(tex_speech(r"A^{-1}").contains("inverse"), "{}", tex_speech(r"A^{-1}"));
        assert!(tex_speech(r"\tan^{-1} x").contains("inverse"), "{}", tex_speech(r"\tan^{-1} x"));
        let s = tex_speech(r"\vert x \vert");
        assert!(!s.contains("vert"), "{s}");
        let s = tex_speech(r"\frac{dy}{dx}\bigg|_{x=2}");
        assert!(!s.contains("bigg") && !s.contains("divides"), "evaluated at: {s}");
        let s = tex_speech(r"\left. \frac{x^2}{2} \right|_0^1");
        assert!(!s.starts_with('.') && !s.contains("divides"), "evaluated from 0 to 1: {s}");
        let s = tex_speech(r"\oint_C F");
        assert!(!s.contains("from C"), "a contour integral goes around C: {s}");
    }

    #[test]
    fn round8_arcs_underbraces_and_cardinality() {
        for t in [r"\underbrace{a+b}_{2}", r"\overset{\frown}{AB}"] {
            let s = tex_speech(t);
            assert!(!s.contains("underbrace") && !s.contains("overset") && !s.contains("frown"), "{t}: {s}");
        }
        let s = tex_speech(r"m\widehat{AB}");
        assert!(s.contains("arc"), "an arc over letters is an arc, not 'hat': {s}");
        let s = tex_speech(r"|S| = 5");
        assert!(!s.contains("absolute value"), "the size of a set: {s}");
    }

    #[test]
    fn round9_set_notation_and_evaluation() {
        assert!(tex_speech(r"A^c").contains("complement"));
        assert!(tex_speech(r"x^c").contains("power"));
        assert_eq!(tex_speech(r"x^{1/2}"), "x to the power of 1 over 2");
        let s = tex_speech(r"\{x : x > 0\}");
        assert!(s.contains("such that"), "{s}");
        assert_eq!(tex_speech(r"|S| = 5"), "the size of S end size equals 5");
        assert_eq!(tex_speech(r"|x - 3|"), "absolute value of x minus 3 end absolute value");
        assert_eq!(tex_speech(r"x^\circ"), "x degrees");
        let s = tex_speech(r"\left. x^2 \right|_0^1");
        assert!(s.contains("evaluated from 0 to 1"), "{s}");
        let s = tex_speech(r"\oint_C F");
        assert!(s.contains("around C"), "{s}");
        assert!(tex_speech(r"m\widehat{AB}").contains("arc A B"));
        assert!(tex_speech(r"\mathbf{u} \cdot \mathbf{v}").contains("dot"));
        assert!(tex_speech(r"2 \cdot 3").contains("times"));
    }

    /// Critic round 9: evaluation brackets, maps-to, probability, big set operators, one-sided limits, closed
    /// fractions, cross products, derivatives, intervals, sets and set sizes.
    #[test]
    fn round10_critic_gaps() {
        assert_eq!(tex_speech(r"\left[ x^2 \right]_0^3"), "open bracket x squared close bracket evaluated from 0 to 3");
        assert_eq!(tex_speech(r"x \mapsto x^2"), "x maps to x squared");
        assert_eq!(tex_speech(r"\mathbb{P}(A \cap B)"), "probability open paren A intersection B close paren");
        assert_eq!(tex_speech(r"\bigcup_{i=1}^{n} A_i"), "the union from i equals 1 to n of A sub i");
        assert_eq!(tex_speech(r"\bigcap_{i} A_i"), "the intersection from i of A sub i");
        assert_eq!(tex_speech(r"x \to 0^+"), "x approaches 0 from the right");
        assert_eq!(tex_speech(r"x \to 2^{-}"), "x approaches 2 from the left");
        assert_eq!(tex_speech(r"\lim_{x \to 0^+} f(x)"), "the limit as x approaches 0 from the right of f open paren x close paren");
        assert_eq!(tex_speech(r"x \to 3"), "x to 3");
        assert_eq!(
            tex_speech(r"\frac{1}{2}+\frac{1}{3}"),
            "the fraction 1 over 2 end fraction plus the fraction 1 over 3 end fraction"
        );
        assert_eq!(tex_speech(r"\dfrac{a}{b}"), "the fraction a over b end fraction");
        assert_eq!(tex_speech(r"\frac{a}{b}\cdot\frac{c}{d}"), "the fraction a over b end fraction times the fraction c over d end fraction");
        assert_eq!(tex_speech(r"\overrightarrow{AB} \cdot \overrightarrow{CD}"), "vector A B dot vector C D");
        assert_eq!(tex_speech(r"\vec{u}\times\vec{v}"), "vector u cross vector v");
        assert_eq!(tex_speech(r"\mathbf{u}\times\mathbf{v}"), "u cross v");
        assert_eq!(tex_speech(r"\mathbf{A}\times\mathbf{B}"), "A times B");
        assert_eq!(tex_speech(r"3 \times 4"), "3 times 4");
        assert_eq!(tex_speech(r"f^{(n)}(x)"), "f nth derivative open paren x close paren");
        assert_eq!(tex_speech(r"f^{(3)}"), "f third derivative");
        assert_eq!(tex_speech(r"\log_2 8"), "log base 2 of 8");
        assert_eq!(tex_speech(r"\iint_D f"), "the double integral over D of f");
        assert_eq!(tex_speech(r"\{1,2,3\}"), "the set 1, 2, 3 end set");
        let s = tex_speech(r"[a,b)");
        assert!(s.contains("interval from a, including a, to b, not including b"), "{s}");
        let s = tex_speech(r"(0,1]");
        assert!(s.contains("interval from 0, not including 0, to 1, including 1"), "{s}");
        assert!(tex_speech(r"(a,b)").contains("open paren"));
        assert_eq!(tex_speech(r"\mathbb{R}^2"), "real numbers in 2 dimensions");
        assert_eq!(tex_speech(r"\mathbb{R}^+"), "positive real numbers");
        assert_eq!(tex_speech(r"|A \cap B|"), "the size of A intersection B end size");
        assert_eq!(tex_speech(r"A \setminus B"), "A set minus B");
        assert_eq!(tex_speech(r"a \equiv b \pmod{n}"), "a is congruent to b mod n");
        assert_eq!(tex_speech(r"a \equiv b"), "a is equivalent to b");
        assert_eq!(tex_speech(r"\sim"), "is distributed as");
        // a system of equations is not a set
        let s = tex_speech(r"\left\{ \begin{array}{l} x=1 \ y=2 \end{array} \right.");
        assert!(!s.contains("the set"), "{s}");
    }

    #[test]
    fn groups_and_landmarks() {
        assert_eq!(group_of("page-p1").map(|g| g.0), Some("pages"));
        assert_eq!(group_of("cs-new").map(|g| g.0), group_of("cs-student-0").map(|g| g.0));
        assert_eq!(group_of("done"), None);
        let h = Node::Heading { id: "student-name".into(), level: 1, text: "Maya R.".into(), region: TEXT, pos: (0, 0), at: (0, 0), size: (0, 0) };
        assert_eq!(landmark(TEXT, &[&h]), Some(("region", "Transcript".into())));
        assert_eq!(landmark(HEADER, &[]).map(|l| l.0), Some("banner"));
        assert_eq!(landmark(TRAY, &[]).map(|l| l.0), Some("complementary"));
        assert!(in_main(SCAN) && !in_main(TRAY));
    }

    // blind-critic round 10: spoken-math misreads found by reading ~150 invented expressions.
    #[test]
    fn critic_round10_spoken_math_gaps() {
        let cases: [(&str, &str); 14] = [
            (r"\left(\frac{1}{2}\right)^3", "evaluated at"),
            (r"\left[a+b\right]^2", "evaluated at"),
            (r"\int_C f\,ds", "from C"),
            (r"\exists! x", "factorial"),
            (r"\overleftrightarrow{AB}", "overleftrightarrow"),
            (r"\oplus", "oplus"),
            (r"\otimes", "otimes"),
            (r"\vdots", "vdots"),
            (r"\ddot{x}", "ddot"),
            (r"\underline{x}", "underline"),
            (r"\measuredangle ABC", "measured angle"),
            (r"\odot O", "odot"),
            (r"{}_nC_r", "sub n C"),
            (r"y^{\prime\prime}", "to the power of prime"),
        ];
        let found: Vec<String> = cases
            .iter()
            .filter(|(t, w)| tex_speech(t).contains(w))
            .map(|(t, _)| format!("{t} => {}", tex_speech(t)))
            .collect();
        assert!(found.is_empty(), "misreads:\n{}", found.join("\n"));
        assert_eq!(tex_speech(r"\left(\frac{1}{2}\right)^3"), "open paren the fraction 1 over 2 end fraction close paren cubed");
        assert_eq!(tex_speech(r"\left[a+b\right]^2"), "open bracket a plus b close bracket squared");
        assert_eq!(tex_speech(r"\left(x\right)_0^1"), "open paren x close paren evaluated from 0 to 1");
        assert_eq!(tex_speech(r"\int_C f\,ds"), "the integral over C of f d s");
        assert_eq!(tex_speech(r"\int_a^b f"), "the integral from a to b of f");
        assert_eq!(tex_speech(r"\exists! x"), "there exists exactly one x");
        assert_eq!(tex_speech(r"\exists x"), "there is some x");
        assert_eq!(tex_speech(r"y^{\prime\prime}"), "y double prime");
        assert_eq!(tex_speech(r"{}_nC_r"), "n choose r");
        assert_eq!(tex_speech(r"{}^{14}C"), "14 C");
        assert_eq!(tex_speech(r"\measuredangle ABC"), "the measure of angle A B C");
        assert_eq!(tex_speech(r"\odot O"), "circle O");
        assert_eq!(tex_speech(r"\overleftrightarrow{AB}"), "line A B");
        assert_eq!(tex_speech(r"\ddot{x}"), "x double dot");
        assert_eq!(tex_speech(r"\underline{x}"), "x");
    }
}
