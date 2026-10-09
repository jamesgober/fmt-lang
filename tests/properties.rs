//! Property tests for every invariant in `dev/DIRECTIVES.md` section 4:
//! idempotence, token and comment preservation, no panics (error nodes
//! included), and determinism, over random valid and invalid sources of two
//! forged languages and over arbitrary hand-built trees. Formatting with no
//! rules is checked against a reference implementation written over the
//! lexer's flat token list.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::OnceLock;

use common::*;
use fmt_lang::syntax_lang::{Builder, Element, Node, Span, Token, TokenKind};
use fmt_lang::{Indent, NodeRule, Rules, Space, Style, TokenRule, Trailing, format};
use lang_forge::{Kind, Language};
use proptest::prelude::*;

struct Fixtures {
    json: Language,
    calc: Language,
    json_styles: Vec<(Trailing, Style<Kind>)>,
    calc_style: Style<Kind>,
    json_plain: Style<Kind>,
    calc_plain: Style<Kind>,
}

fn fixtures() -> &'static Fixtures {
    static F: OnceLock<Fixtures> = OnceLock::new();
    F.get_or_init(|| {
        let json = json();
        let calc = calc();
        let json_styles = [Trailing::Preserve, Trailing::Never, Trailing::Always]
            .into_iter()
            .map(|t| (t, json_style(&json, t)))
            .collect();
        let calc_style = calc_style(&calc);
        Fixtures {
            json,
            calc,
            json_styles,
            calc_style,
            json_plain: Style::default(),
            calc_plain: Style::default(),
        }
    })
}

/// Cases per property: 400, or `PROPTEST_CASES` for a longer soak.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400)
}

// ---------------------------------------------------------- generators --

/// Whitespace and comments placed between tokens. Line comments always carry
/// their line break.
fn json_sep() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => Just(String::new()),
        4 => Just(" ".to_string()),
        2 => Just("\n".to_string()),
        1 => Just("\n\n\n".to_string()),
        1 => Just("  \t".to_string()),
        1 => Just("\r\n".to_string()),
        1 => Just(" /* c */ ".to_string()),
        1 => Just("/*a*//*b*/".to_string()),
        1 => Just(" // line\n".to_string()),
        1 => Just("\n  // own line\n".to_string()),
        1 => Just("\n/* multi\n   line */\n".to_string()),
        1 => Just("  \n\n  ".to_string()),
    ]
}

fn json_scalar() -> impl Strategy<Value = Vec<String>> {
    prop_oneof![
        Just(vec!["1".to_string()]),
        Just(vec!["23".to_string()]),
        Just(vec!["\"s\"".to_string()]),
        Just(vec!["\"key with space\"".to_string()]),
        Just(vec!["true".to_string()]),
        Just(vec!["null".to_string()]),
    ]
}

/// A valid value as a token list.
fn json_value() -> impl Strategy<Value = Vec<String>> {
    json_scalar().prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            (prop::collection::vec(inner.clone(), 0..5), any::<bool>()).prop_map(
                |(items, trailing)| {
                    let mut out = vec!["[".to_string()];
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(",".into());
                        }
                        out.extend(item.iter().cloned());
                    }
                    if trailing && !items.is_empty() {
                        out.push(",".into());
                    }
                    out.push("]".into());
                    out
                }
            ),
            (prop::collection::vec(inner, 0..4), any::<bool>()).prop_map(|(items, trailing)| {
                let mut out = vec!["{".to_string()];
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(",".into());
                    }
                    out.push(format!("\"k{i}\""));
                    out.push(":".into());
                    out.extend(item.iter().cloned());
                }
                if trailing && !items.is_empty() {
                    out.push(",".into());
                }
                out.push("}".into());
                out
            }),
        ]
    })
}

fn interleave(tokens: Vec<String>, seps: Vec<String>) -> String {
    let mut out = String::new();
    let mut seps = seps.into_iter();
    out.push_str(&seps.next().unwrap_or_default());
    for t in tokens {
        out.push_str(&t);
        out.push_str(&seps.next().unwrap_or_default());
    }
    out
}

fn with_seps(
    tokens: impl Strategy<Value = Vec<String>>,
    sep: fn() -> BoxedStrategy<String>,
) -> impl Strategy<Value = String> {
    tokens.prop_flat_map(move |toks| {
        let n = toks.len() + 1;
        (Just(toks), prop::collection::vec(sep(), n)).prop_map(|(t, s)| interleave(t, s))
    })
}

fn json_sep_boxed() -> BoxedStrategy<String> {
    json_sep().boxed()
}

fn valid_json() -> impl Strategy<Value = String> {
    with_seps(json_value(), json_sep_boxed)
}

/// Token soup: mostly invalid sources with recovery, error nodes, and stray
/// and unknown characters; with `lexical_errors`, also unterminated strings
/// and comments.
fn json_soup(lexical_errors: bool) -> impl Strategy<Value = String> {
    let mut alphabet = vec!["{", "}", "[", "]", ",", ":", "1", "\"s\"", "true", "@", "-"];
    if lexical_errors {
        alphabet.extend(["\"open", "/* open"]);
    }
    let tok = prop::sample::select(alphabet).prop_map(String::from);
    with_seps(prop::collection::vec(tok, 0..30), json_sep_boxed)
}

fn calc_sep() -> BoxedStrategy<String> {
    prop_oneof![
        4 => Just(" ".to_string()),
        2 => Just(String::new()),
        2 => Just("\n".to_string()),
        1 => Just("\n\n\n".to_string()),
        1 => Just(" # note\n".to_string()),
        1 => Just("\n# own\n".to_string()),
        1 => Just(" /* b */ ".to_string()),
        1 => Just("\t".to_string()),
    ]
    .boxed()
}

fn calc_expr() -> impl Strategy<Value = Vec<String>> {
    let leaf = prop_oneof![
        Just(vec!["1".to_string()]),
        Just(vec!["42".to_string()]),
        Just(vec!["x".to_string()]),
        Just(vec!["long_name".to_string()]),
    ];
    leaf.prop_recursive(5, 40, 2, |inner| {
        prop_oneof![
            (
                inner.clone(),
                prop::sample::select(vec!["+", "-", "*", "/", "^"]),
                inner.clone()
            )
                .prop_map(|(a, op, b)| {
                    let mut v = vec!["(".to_string()];
                    v.extend(a);
                    v.push(op.to_string());
                    v.extend(b);
                    v.push(")".into());
                    v
                }),
            (
                inner.clone(),
                prop::sample::select(vec!["+", "*"]),
                inner.clone()
            )
                .prop_map(|(a, op, b)| {
                    let mut v = a;
                    v.push(op.to_string());
                    v.extend(b);
                    v
                }),
            inner.prop_map(|a| {
                let mut v = vec!["-".to_string()];
                v.extend(a);
                v
            }),
        ]
    })
}

fn calc_program() -> impl Strategy<Value = Vec<String>> {
    let stmt = (any::<bool>(), calc_expr()).prop_map(|(is_let, e)| {
        let mut v = Vec::new();
        if is_let {
            v.extend(["let".to_string(), "v".to_string(), "=".to_string()]);
        }
        v.extend(e);
        v.push(";".into());
        v
    });
    prop::collection::vec(stmt, 0..6).prop_map(|s| s.concat())
}

/// Calc sources: separators that could fuse tokens (`let` next to a name) are
/// only drawn where the grammar never puts two words side by side, so valid
/// programs stay valid; the soup below covers everything else.
fn valid_calc() -> impl Strategy<Value = String> {
    calc_program().prop_flat_map(|toks| {
        let n = toks.len() + 1;
        (Just(toks), prop::collection::vec(calc_sep(), n)).prop_map(|(t, s)| {
            let mut out = String::new();
            let mut seps = s.into_iter();
            out.push_str(&seps.next().unwrap_or_default());
            for (i, tok) in t.iter().enumerate() {
                out.push_str(tok);
                let mut sep = seps.next().unwrap_or_default();
                let next = t.get(i + 1);
                let word = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '_');
                if sep.is_empty() && next.is_some_and(|n| word(tok) && word(n)) {
                    sep = " ".into();
                }
                out.push_str(&sep);
            }
            out
        })
    })
}

fn calc_soup() -> impl Strategy<Value = String> {
    let tok = prop_oneof![
        Just("let"),
        Just("x"),
        Just("1"),
        Just("="),
        Just(";"),
        Just("+"),
        Just("-"),
        Just("*"),
        Just("^"),
        Just("("),
        Just(")"),
        Just("$"),
    ]
    .prop_map(String::from);
    with_seps(prop::collection::vec(tok, 0..40), calc_sep)
}

/// Any short string over the characters both languages care about.
fn raw_source() -> impl Strategy<Value = String> {
    "[\\[\\]{}(),:;=+*^\"#/ \n\t\r\u{00e9}a1@-]{0,80}"
}

// ------------------------------------------------------------- checks --

/// How a check formats: plainly, or keeping every diagnostic's region as
/// written (needed once the source has lexical errors).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Plain,
    Keeping,
}

fn run(mode: Mode, lang: &Language, style: &Style<Kind>, src: &str, width: usize) -> String {
    match mode {
        Mode::Plain => fmt(lang, style, src, width),
        Mode::Keeping => fmt_keeping(lang, style, src, width),
    }
}

/// Every invariant for one source under one style.
fn check(
    mode: Mode,
    lang: &Language,
    style: &Style<Kind>,
    src: &str,
    width: usize,
    trailing: Trailing,
) -> Result<(), TestCaseError> {
    let out = run(mode, lang, style, src, width);
    // Determinism.
    let again = run(mode, lang, style, src, width);
    prop_assert_eq!(&out, &again, "not deterministic");
    // Token preservation.
    let before = significant(lang, src);
    let after = significant(lang, &out);
    if trailing == Trailing::Preserve {
        prop_assert_eq!(&before, &after, "tokens changed: {:?} -> {:?}", src, out);
    } else {
        prop_assert_eq!(
            without_trailing_commas(&before),
            without_trailing_commas(&after),
            "tokens changed beyond trailing commas: {:?} -> {:?}",
            src,
            out
        );
    }
    // Comment preservation, exactly once each and in order.
    prop_assert_eq!(
        comments(lang, src),
        comments(lang, &out),
        "comments changed: {:?} -> {:?}",
        src,
        out
    );
    // Idempotence.
    let twice = run(mode, lang, style, &out, width);
    prop_assert_eq!(&out, &twice, "not idempotent: {:?}", src);
    // Outside regions kept as written (error nodes, kept regions), whitespace
    // never ends a line with spaces or tabs (a lone `\r` may stay, completing
    // a `\r\n`).
    if mode == Mode::Plain {
        let reparse = lang.parse(&out);
        let error = lang.kind("ERROR").unwrap();
        let errors: Vec<_> = reparse
            .tree()
            .descendants()
            .filter(|n| *n.kind() == error)
            .map(|n| n.span())
            .collect();
        for t in reparse.tree().tokens() {
            let s = t.span();
            let text = &out[s.start().to_usize()..s.end().to_usize()];
            let in_error = errors
                .iter()
                .any(|e| e.start() <= s.start() && s.end() <= e.end());
            if t.is_trivia() && !in_error && text.chars().all(char::is_whitespace) {
                for line in text.split('\n').rev().skip(1) {
                    prop_assert!(
                        line.is_empty() || line == "\r",
                        "trailing whitespace: {:?} -> {:?}",
                        src,
                        out
                    );
                }
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn prop_json_valid(src in valid_json(), width in 1usize..100, which in 0usize..3) {
        let f = fixtures();
        let (trailing, style) = &f.json_styles[which];
        check(Mode::Plain, &f.json, style, &src, width, *trailing)?;
    }

    #[test]
    fn prop_json_soup_lexically_valid(src in json_soup(false), width in 1usize..100, which in 0usize..3) {
        let f = fixtures();
        let (trailing, style) = &f.json_styles[which];
        check(Mode::Plain, &f.json, style, &src, width, *trailing)?;
    }

    #[test]
    fn prop_json_soup(src in json_soup(true), width in 1usize..100, which in 0usize..3) {
        let f = fixtures();
        let (trailing, style) = &f.json_styles[which];
        check(Mode::Keeping, &f.json, style, &src, width, *trailing)?;
    }

    #[test]
    fn prop_json_raw(src in raw_source(), width in 1usize..100, which in 0usize..3) {
        let f = fixtures();
        let (trailing, style) = &f.json_styles[which];
        check(Mode::Keeping, &f.json, style, &src, width, *trailing)?;
    }

    #[test]
    fn prop_calc_valid(src in valid_calc(), width in 1usize..100) {
        let f = fixtures();
        prop_assert!(!f.calc.parse(&src).has_errors(), "generator produced an invalid program: {:?}", src);
        check(Mode::Plain, &f.calc, &f.calc_style, &src, width, Trailing::Preserve)?;
    }

    #[test]
    fn prop_calc_soup(src in calc_soup(), width in 1usize..100) {
        let f = fixtures();
        // `$` is a lexical error, but an inert one (trivia): no regions kept.
        check(Mode::Plain, &f.calc, &f.calc_style, &src, width, Trailing::Preserve)?;
    }

    #[test]
    fn prop_calc_raw(src in raw_source(), width in 1usize..100) {
        let f = fixtures();
        check(Mode::Keeping, &f.calc, &f.calc_style, &src, width, Trailing::Preserve)?;
    }

    #[test]
    fn prop_line_comments_end_their_line(src in valid_json(), width in 1usize..60) {
        let f = fixtures();
        let out = fmt(&f.json, &f.json_styles[0].1, &src, width);
        for t in f.json.lex(&out) {
            let s = t.span();
            let text = &out[s.start().to_usize()..s.end().to_usize()];
            if text.starts_with("//") {
                let next = out[s.end().to_usize()..].chars().next();
                prop_assert!(next.is_none() || next == Some('\n'), "{:?}", out);
            }
        }
    }

    #[test]
    fn prop_no_rules_matches_reference(src in prop_oneof![valid_json(), json_soup(true), raw_source()]) {
        let f = fixtures();
        let out = fmt(&f.json, &f.json_plain, &src, 80);
        prop_assert_eq!(out, preserve_reference(&f.json, &src), "{:?}", src);
    }

    #[test]
    fn prop_no_rules_matches_reference_calc(src in prop_oneof![valid_calc(), calc_soup()]) {
        let f = fixtures();
        let out = fmt(&f.calc, &f.calc_plain, &src, 80);
        prop_assert_eq!(out, preserve_reference(&f.calc, &src), "{:?}", src);
    }

    #[test]
    fn prop_conventional_preset_is_safe(src in prop_oneof![valid_calc(), calc_soup(), raw_source()], width in 1usize..100) {
        let f = fixtures();
        let style = Rules::conventional().verbatim("ERROR").compile(|n| f.calc.kind(n)).unwrap();
        check(Mode::Keeping, &f.calc, &style, &src, width, Trailing::Preserve)?;
    }
}

// ------------------------------------------------------ arbitrary trees --

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum T {
    A,
    B,
    C,
    Err,
    Word,
    Punct,
    Open,
    Close,
    Comma,
    Ws,
    Comment,
}

impl TokenKind for T {
    fn is_trivia(&self) -> bool {
        matches!(self, T::Ws | T::Comment)
    }
}

/// One step of a random tree: open a node, add a token, or close a node.
#[derive(Clone, Debug)]
enum Step {
    Open(T),
    Tok(T, &'static str),
    Close,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => prop::sample::select(vec![T::A, T::B, T::C, T::Err]).prop_map(Step::Open),
        2 => Just(Step::Close),
        2 => prop::sample::select(vec!["a", "bb", "x1"]).prop_map(|s| Step::Tok(T::Word, s)),
        1 => prop::sample::select(vec!["+", "-", "*"]).prop_map(|s| Step::Tok(T::Punct, s)),
        1 => Just(Step::Tok(T::Open, "(")),
        1 => Just(Step::Tok(T::Close, ")")),
        1 => Just(Step::Tok(T::Comma, ",")),
        2 => prop::sample::select(vec![" ", "\n", "  \n\n ", "\t"]).prop_map(|s| Step::Tok(T::Ws, s)),
        1 => prop::sample::select(vec!["/* c */", "/*\n m \n*/", "// l\n", "# h"]).prop_map(|s| Step::Tok(T::Comment, s)),
        1 => Just(Step::Tok(T::Word, "")),
    ]
}

/// Builds a well-formed tree from random steps (closes are ignored when
/// nothing is open; everything open is closed at the end).
fn build(steps: &[Step]) -> (Node<T>, String) {
    let mut b = Builder::new();
    let mut src = String::new();
    b.start_node(T::A);
    let mut open = 0usize;
    for s in steps {
        match s {
            Step::Open(k) => {
                b.start_node(*k);
                open += 1;
            }
            Step::Close if open > 0 => {
                b.finish_node();
                open -= 1;
            }
            Step::Close => {}
            Step::Tok(k, text) => {
                let start = src.len() as u32;
                src.push_str(text);
                b.token(Token::new(*k, Span::new(start, src.len() as u32)));
            }
        }
    }
    for _ in 0..=open {
        b.finish_node();
    }
    (b.finish().unwrap(), src)
}

fn tree_styles() -> Vec<Style<T>> {
    let resolve = |n: &str| match n {
        "a" => Some(T::A),
        "b" => Some(T::B),
        "c" => Some(T::C),
        "err" => Some(T::Err),
        "word" => Some(T::Word),
        "punct" => Some(T::Punct),
        "open" => Some(T::Open),
        "close" => Some(T::Close),
        "comma" => Some(T::Comma),
        _ => None,
    };
    let rich = Rules::new()
        .verbatim("err")
        .token(TokenRule::new("punct").around(Space::Line))
        .token(
            TokenRule::new("comma")
                .before(Space::None)
                .after(Space::Line),
        )
        .node(
            NodeRule::new("a")
                .group()
                .indent(Indent::Block)
                .delimiters("open", "close", Space::SoftLine)
                .separator("comma", Space::Line, Trailing::Always),
        )
        .node(
            NodeRule::new("b")
                .group()
                .indent(Indent::Hanging)
                .before(Space::Hard),
        )
        .node(
            NodeRule::new("c")
                .delimiters("open", "close", Space::Hard)
                .separator("comma", Space::Single, Trailing::Never)
                .blank_lines(0)
                .token(TokenRule::any().around(Space::None)),
        );
    vec![
        Style::default(),
        rich.compile(resolve).unwrap(),
        rich.clone()
            .max_indent(3)
            .indent(16)
            .compile(resolve)
            .unwrap(),
    ]
}

/// The texts `needles` appear in `hay` in order, without overlapping.
fn in_order(hay: &str, needles: &[String]) -> bool {
    let mut at = 0;
    for n in needles {
        match hay[at..].find(n.as_str()) {
            Some(i) => at += i + n.len(),
            None => return false,
        }
    }
    true
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn prop_arbitrary_trees_keep_tokens_and_comments(
        steps in prop::collection::vec(step(), 0..80),
        width in 1usize..60,
        which in 0usize..3,
    ) {
        let (tree, src) = build(&steps);
        let style = &tree_styles()[which];
        let out = format(&tree, &src, style, width).unwrap();
        prop_assert_eq!(&out, &format(&tree, &src, style, width).unwrap());
        let text = |t: &Token<T>| src[t.span().start().to_usize()..t.span().end().to_usize()].to_string();
        // Trailing commas may be added or removed; every other significant
        // token, and every comment, must appear in order.
        let sig: Vec<String> = tree
            .tokens()
            .filter(|t| !t.is_trivia() && t.kind != T::Comma)
            .map(text)
            .filter(|s| !s.is_empty())
            .collect();
        prop_assert!(in_order(&out, &sig), "{:?} -> {:?}", src, out);
        let comments: Vec<String> = tree.tokens().filter(|t| t.kind == T::Comment).map(text).collect();
        prop_assert!(in_order(&out, &comments), "{:?} -> {:?}", src, out);
        if which == 0 {
            // With no rules nothing but whitespace changes.
            let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
            prop_assert_eq!(squeeze(&out), squeeze(&src));
        }
    }

    #[test]
    fn prop_hostile_spans_never_panic(
        spans in prop::collection::vec((0u32..40, 0u32..40, any::<bool>()), 0..20),
        src in "[a-z\u{00e9} \n]{0,30}",
    ) {
        let children: Vec<Element<T>> = spans
            .iter()
            .map(|&(s, e, trivia)| Element::Token(Token::new(if trivia { T::Ws } else { T::Word }, Span::new(s, e))))
            .collect();
        let tree = Node::new(T::A, children);
        for style in tree_styles() {
            let _ = format(&tree, &src, &style, 10);
        }
    }
}
