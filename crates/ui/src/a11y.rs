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
    let cs: Vec<char> = tex.chars().collect();
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
        "to" | "rightarrow" | "longrightarrow" | "mapsto" | "longmapsto" => "to",
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
        "setminus" => "minus the set",
        "equiv" => "is equivalent to",
        "propto" => "is proportional to",
        "wedge" | "land" => "and",
        "vee" | "lor" => "or",
        "neg" | "lnot" => "not",
        "bmod" | "mod" => "mod",
        "angle" => "angle",
        "triangle" => "triangle",
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
        "circ" | "degree" => "degrees",
        "sum" => "the sum of",
        "prod" => "the product of",
        "int" => "the integral of",
        "in" => "is in",
        "cup" => "union",
        "cap" => "intersection",
        "quad" | "qquad" | "," | ";" | "!" | ":" | " " => " ",
        "%" => "percent",
        "{" => "open brace",
        "}" => "close brace",
        "\\" => ",",
        "left" | "right" | "big" | "Big" | "displaystyle" | "limits" => "",
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
    match arg {
        "2" => " squared ".into(),
        "3" => " cubed ".into(),
        "degrees" => " degrees ".into(),
        "prime" => " prime ".into(),
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
            // short only when nothing can attach to it: "1 over 2 x" would read as 1 over 2x
            if nested || multi(&a) || multi(&b) || !ends_here(cs, *p) {
                format!(" the fraction {a} over {b} end fraction ")
            } else {
                format!(" {a} over {b} ")
            }
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
        "bar" | "overline" => format!(" {} bar ", atom(cs, p)),
        "hat" | "widehat" => format!(" {} hat ", atom(cs, p)),
        "tilde" | "widetilde" => format!(" {} tilde ", atom(cs, p)),
        "dot" => format!(" {} dot ", atom(cs, p)),
        "vec" => format!(" vector {} ", atom(cs, p)),
        "text" | "textbf" | "textit" | "mathrm" | "mathbf" | "mathit" | "mathbb" | "mathcal" | "operatorname" | "mbox" => {
            format!(" {} ", raw_group(cs, p))
        }
        "sum" => limits(cs, p, "the sum"),
        "prod" => limits(cs, p, "the product"),
        "int" => limits(cs, p, "the integral"),
        "iint" => limits(cs, p, "the double integral"),
        "iiint" => limits(cs, p, "the triple integral"),
        "oint" => limits(cs, p, "the contour integral"),
        "lim" => limits(cs, p, "the limit"),
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
        "overrightarrow" | "overleftarrow" | "overrightarrow*" => format!(" vector {} ", atom(cs, p)),
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

fn divider(paren: i32, in_set: bool) -> &'static str {
    if in_set {
        " such that "
    } else if paren > 0 {
        " given "
    } else {
        " divides "
    }
}

fn seq(cs: &[char], p: &mut usize, in_group: bool) -> String {
    let mut out = String::new();
    // `|x - 3|`: the first bar opens an absolute value and the next closes it
    let mut bar_open = false;
    // `P(A|B)` and `\{x | x > 0\}`: a bar with no partner is a divider, not an absolute value
    let (mut paren, mut in_set) = (0i32, false);
    while *p < cs.len() {
        let c = cs[*p];
        match c {
            '|' => {
                *p += 1;
                if bar_open {
                    out.push_str(" end absolute value ");
                    bar_open = false;
                } else if bars_ahead(cs, *p, in_group) % 2 == 0 {
                    out.push_str(divider(paren, in_set));
                } else {
                    out.push_str(" absolute value of ");
                    bar_open = true;
                }
            }
            '(' => {
                paren += 1;
                *p += 1;
                out.push_str(" open paren ");
            }
            ')' => {
                paren -= 1;
                *p += 1;
                out.push_str(" close paren ");
            }
            '\\' if cs.get(*p + 1) == Some(&'{') => {
                in_set = true;
                out.push_str(&command(cs, p));
            }
            '\\' if cs[*p + 1..].starts_with(&['m', 'i', 'd']) && !cs.get(*p + 4).is_some_and(|c| c.is_ascii_alphabetic()) => {
                *p += 4;
                out.push_str(divider(paren, in_set));
            }
            '}' if in_group => break,
            '}' => *p += 1,
            '{' => {
                out.push(' ');
                out.push_str(&atom(cs, p));
                out.push(' ');
            }
            '^' => {
                *p += 1;
                let a = atom(cs, p);
                out.push_str(&power(a.trim()));
            }
            '_' => {
                *p += 1;
                let a = atom(cs, p);
                out.push_str(&sub(a.trim()));
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
                // `1rac{1}{2}` is a mixed number
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

    #[test]
    fn groups_and_landmarks() {
        assert_eq!(group_of("page-p1").map(|g| g.0), Some("pages"));
        assert_eq!(group_of("cs-new").map(|g| g.0), group_of("cs-student-0").map(|g| g.0));
        assert_eq!(group_of("done"), None);
        let h = Node::Heading { id: "student-name".into(), level: 1, text: "Maya R.".into(), region: TEXT, pos: (0, 0) };
        assert_eq!(landmark(TEXT, &[&h]), Some(("region", "Transcript".into())));
        assert_eq!(landmark(HEADER, &[]).map(|l| l.0), Some("banner"));
        assert_eq!(landmark(TRAY, &[]).map(|l| l.0), Some("complementary"));
        assert!(in_main(SCAN) && !in_main(TRAY));
    }
}
