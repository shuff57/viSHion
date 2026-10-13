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
        _ if id.starts_with("tab-") => Some(("filter", "group", "Show")),
        _ if id.starts_with("page-") => Some(("pages", "list", "Pages")),
        _ if id.starts_with("cs-student-") => Some(("students", "list", "Students")),
        _ if id.starts_with("alt-") => Some(("choices", "list", "Choices for this word")),
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

/// What a screen reader should say for a formula written in TeX: `\frac{s}{\sqrt{n}}` is "s over square
/// root of n". The TeX itself stays in the element's text, so it can still be read letter by letter.
pub fn tex_speech(tex: &str) -> String {
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
        "to" | "rightarrow" => "approaches",
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

/// The text of a `{group}` as written, for 	ext{...}: words stay words.
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

fn power(arg: &str) -> String {
    match arg {
        "2" => " squared ".into(),
        "3" => " cubed ".into(),
        "degrees" => " degrees ".into(),
        "prime" => " prime ".into(),
        a => format!(" to the power of {a} "),
    }
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
        "frac" | "dfrac" | "tfrac" => {
            let (a, b) = (atom(cs, p), atom(cs, p));
            if multi(&a) || multi(&b) {
                format!(" the fraction {a} over {b} end fraction ")
            } else {
                format!(" {a} over {b} ")
            }
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
        "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "min" | "max" | "lim" | "sup" | "inf" => format!(" {name} "),
        "Sigma" | "Delta" | "Omega" | "Gamma" | "Theta" | "Lambda" | "Pi" | "Phi" | "Psi" | "Xi" => {
            format!(" capital {} ", name.to_lowercase())
        }
        "varepsilon" => " epsilon ".into(),
        "varphi" => " phi ".into(),
        "vartheta" => " theta ".into(),
        n => match word_for(n) {
            Some(s) => format!(" {s} "),
            None => format!(" {n} "),
        },
    }
}

fn seq(cs: &[char], p: &mut usize, in_group: bool) -> String {
    let mut out = String::new();
    while *p < cs.len() {
        let c = cs[*p];
        match c {
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
                out.push_str(&format!(" sub {} ", a.trim()));
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
        // never panics on junk or a truncated formula
        assert_eq!(tex_speech(r"\frac{"), "over");
        let _ = tex_speech(r"\{^_}\\");
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
