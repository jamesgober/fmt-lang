//! Shared fixtures: two languages forged with lang-forge (a JSON-like data
//! language and a calc-like expression language), their formatting styles,
//! and the oracles the properties compare against.

// Each test binary includes this module and uses only part of it, so unused
// items are expected; fixtures unwrap because a broken fixture is a test bug.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use fmt_lang::{
    Indent, NodeRule, Rules, Space, Style, TokenRule, Trailing, format, format_keeping,
};
use lang_forge::{Kind, Language};

pub const JSON_LSF: &str = r#"
[language]
name = "jsonish"

[lexer]
strings = ['"']
line_comments = ["//"]
block_comments = [["/*", "*/"]]

[rules]
document = "value"
value    = "object | array | STRING | NUMBER | 'true' | 'false' | 'null'"
object   = "'{' (member (',' member)* ','?)? '}'"
member   = "STRING ':' value"
array    = "'[' (value (',' value)* ','?)? ']'"
"#;

pub const CALC_LSF: &str = r##"
[language]
name = "calc"

[lexer]
line_comments = ["#"]
block_comments = [["/*", "*/"]]

[rules]
program = "stmt*"
stmt    = "'let' IDENT '=' expr ';' | expr ';'"
group   = "'(' expr ')'"

[rules.expr]
operand = "NUMBER | IDENT | group"
levels  = [
    { left   = ["+", "-"] },
    { left   = ["*", "/"] },
    { prefix = ["-"] },
    { right  = ["^"] },
]
"##;

pub fn json() -> Language {
    Language::from_lsf(JSON_LSF).expect("the JSON-like schematic forges")
}

pub fn calc() -> Language {
    Language::from_lsf(CALC_LSF).expect("the calc schematic forges")
}

/// JSON rules with the given trailing-comma policy.
pub fn json_rules(trailing: Trailing) -> Rules {
    Rules::new()
        .indent(2)
        .verbatim("ERROR")
        .token(TokenRule::new(":").before(Space::None).after(Space::Single))
        .node(
            NodeRule::new("object")
                .group()
                .indent(Indent::Block)
                .delimiters("{", "}", Space::Line)
                .separator(",", Space::Line, trailing),
        )
        .node(
            NodeRule::new("array")
                .group()
                .indent(Indent::Block)
                .delimiters("[", "]", Space::SoftLine)
                .separator(",", Space::Line, trailing),
        )
}

pub fn json_style(lang: &Language, trailing: Trailing) -> Style<Kind> {
    json_rules(trailing).compile(|n| lang.kind(n)).unwrap()
}

pub fn calc_rules() -> Rules {
    let mut rules = Rules::new()
        .verbatim("ERROR")
        .max_blank_lines(1)
        .token(TokenRule::new(";").before(Space::None))
        .token(TokenRule::new("=").around(Space::Single))
        .token(TokenRule::new("let").after(Space::Single))
        .token(TokenRule::new("(").after(Space::None))
        .token(TokenRule::new(")").before(Space::None))
        .node(NodeRule::new("stmt").before(Space::Hard).after(Space::Hard))
        .node(NodeRule::new("binary").group().indent(Indent::Hanging))
        .node(NodeRule::new("group").group())
        .node(
            NodeRule::new("prefix").token(TokenRule::any().before(Space::None).after(Space::None)),
        );
    for op in ["+", "-", "*", "/", "^"] {
        rules = rules.token(TokenRule::new(op).before(Space::Single).after(Space::Line));
    }
    rules
}

pub fn calc_style(lang: &Language) -> Style<Kind> {
    calc_rules().compile(|n| lang.kind(n)).unwrap()
}

/// Parses and formats `src`.
pub fn fmt(lang: &Language, style: &Style<Kind>, src: &str, width: usize) -> String {
    let parse = lang.parse(src);
    format(parse.tree(), parse.source(), style, width).unwrap()
}

/// Parses and formats `src`, keeping the regions of lexical errors
/// (lang-forge's lexer reports unterminated strings and comments and
/// unexpected characters) as written: what an editor integration does with a
/// file that has errors.
pub fn fmt_keeping(lang: &Language, style: &Style<Kind>, src: &str, width: usize) -> String {
    let parse = lang.parse(src);
    let spans: Vec<_> = parse
        .diagnostics()
        .iter()
        .filter(|d| {
            let m = d.message();
            m.starts_with("unterminated") || m.starts_with("unexpected character")
        })
        .map(|d| d.primary().span())
        .collect();
    format_keeping(parse.tree(), parse.source(), style, width, &spans).unwrap()
}

/// The significant tokens of `src`, as (kind name, text).
pub fn significant(lang: &Language, src: &str) -> Vec<(String, String)> {
    let parse = lang.parse(src);
    parse
        .tree()
        .tokens()
        .filter(|t| !t.is_trivia())
        .map(|t| {
            let s = t.span();
            (
                lang.kind_name(*t.kind()).to_string(),
                src[s.start().to_usize()..s.end().to_usize()].to_string(),
            )
        })
        .collect()
}

/// The texts of every trivia token that is not all whitespace (comments and
/// unrecognized characters), in order.
pub fn comments(lang: &Language, src: &str) -> Vec<String> {
    lang.lex(src)
        .iter()
        .filter(|t| t.is_trivia())
        .map(|t| {
            let s = t.span();
            src[s.start().to_usize()..s.end().to_usize()].to_string()
        })
        .filter(|text| !text.chars().all(char::is_whitespace))
        .collect()
}

/// Removes trailing separators (a `,` directly before `]` or `}`) from a
/// significant token sequence: what the trailing policies may change.
pub fn without_trailing_commas(tokens: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(tokens.len());
    for (i, tok) in tokens.iter().enumerate() {
        let next = tokens.get(i + 1).map(|t| t.0.as_str());
        if tok.0 == "," && matches!(next, Some("]" | "}")) {
            continue;
        }
        out.push(tok.clone());
    }
    out
}

/// A reference implementation of formatting with no rules, written over the
/// lexer's flat token list instead of the tree: the source between the first
/// and last significant token is kept, minus trailing spaces at line ends;
/// comments before and after are laid out one space apart, or on their own
/// lines with at most one blank line between.
pub fn preserve_reference(lang: &Language, src: &str) -> String {
    let tokens = lang.lex(src);
    let text = |t: &lang_forge::syntax_lang::Token<Kind>| {
        let s = t.span();
        &src[s.start().to_usize()..s.end().to_usize()]
    };
    let blank = |t: &str| t.chars().all(char::is_whitespace);
    let first = tokens.iter().position(|t| !t.is_trivia());
    let last = tokens.iter().rposition(|t| !t.is_trivia());
    let mut out = String::new();
    let newlines = |n: usize| 1 + n.saturating_sub(1).min(1);
    // Line breaks; one written right after a token or comment ending in a
    // lone `\r` is written `\r\n`, so the `\r` stays where it was (unless that
    // `\r` belonged to a line break the comment swallowed).
    let breaks = |out: &mut String, n: usize, swallowed: bool| {
        if out.ends_with('\r') && !swallowed {
            out.push('\r');
        }
        out.push_str(&"\n".repeat(n));
    };
    // A comment that swallowed its line break is written without it; the
    // break is counted as whitespace after it instead.
    let comment = |out: &mut String, t: &str| -> bool {
        match t.strip_suffix('\n') {
            Some(stripped) => {
                out.push_str(stripped);
                true
            }
            None => {
                out.push_str(t);
                false
            }
        }
    };

    // Comments before the first significant token.
    let head = first.unwrap_or(tokens.len());
    let mut lines_since = 0usize;
    let mut swallowed = false;
    let mut seen = false;
    for t in &tokens[..head] {
        let t = text(t);
        if blank(t) {
            lines_since += t.matches('\n').count();
        } else {
            if seen {
                if lines_since > 0 {
                    breaks(&mut out, newlines(lines_since), swallowed);
                } else {
                    out.push(' ');
                }
            }
            swallowed = comment(&mut out, t);
            lines_since = usize::from(swallowed);
            seen = true;
        }
    }
    let (Some(first), Some(last)) = (first, last) else {
        if !out.is_empty() {
            breaks(&mut out, 1, swallowed);
        }
        return out;
    };
    if seen {
        if lines_since > 0 {
            breaks(&mut out, newlines(lines_since), swallowed);
        } else {
            out.push(' ');
        }
    }
    // The middle, whitespace runs trimmed at line ends.
    let mut ws = String::new();
    let flush = |ws: &mut String, out: &mut String| {
        if ws.contains('\n') {
            breaks(out, ws.matches('\n').count(), false);
            out.push_str(ws.rsplit('\n').next().unwrap_or(""));
        } else {
            out.push_str(ws);
        }
        ws.clear();
    };
    for t in &tokens[first..=last] {
        let s = text(t);
        if t.is_trivia() && blank(s) {
            ws.push_str(s);
        } else {
            flush(&mut ws, &mut out);
            out.push_str(s);
        }
    }
    // Comments after the last significant token.
    let mut lines_since = 0usize;
    let mut swallowed = false;
    for t in &tokens[last + 1..] {
        let t = text(t);
        if blank(t) {
            lines_since += t.matches('\n').count();
        } else {
            if lines_since > 0 {
                breaks(&mut out, newlines(lines_since), swallowed);
            } else {
                out.push(' ');
            }
            swallowed = comment(&mut out, t);
            lines_since = usize::from(swallowed);
        }
    }
    breaks(&mut out, 1, swallowed);
    out
}
