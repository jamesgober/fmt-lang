//! Behaviour tests with exact expected output, over the forged test languages
//! and hand-built trees.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use fmt_lang::syntax_lang::{Builder, Element, Node, Span, Token, TokenKind};
use fmt_lang::{
    FormatError, Indent, NodeRule, Rules, Space, Style, TokenRule, Trailing, can_touch, format,
    format_doc,
};

// ---------------------------------------------------------------- spacing --

#[test]
fn test_json_flat_when_it_fits() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    assert_eq!(
        fmt(&lang, &style, r#"{"a":1,"b":[1,2,3],"c":{}}"#, 80),
        "{ \"a\": 1, \"b\": [1, 2, 3], \"c\": {} }\n"
    );
}

#[test]
fn test_json_breaks_outer_group_first() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = r#"{"name": "fmt-lang", "tags": ["a", "b"], "nested": {"deep": [1, 2]}}"#;
    assert_eq!(
        fmt(&lang, &style, src, 40),
        "{\n  \"name\": \"fmt-lang\",\n  \"tags\": [\"a\", \"b\"],\n  \"nested\": { \"deep\": [1, 2] }\n}\n"
    );
    assert_eq!(
        fmt(&lang, &style, src, 12),
        "{\n  \"name\": \"fmt-lang\",\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ],\n  \"nested\": {\n    \"deep\": [\n      1,\n      2\n    ]\n  }\n}\n"
    );
}

#[test]
fn test_calc_spacing_and_prefix_operators() {
    let lang = calc();
    let style = calc_style(&lang);
    assert_eq!(
        fmt(&lang, &style, "let   a=-(1+2)*x^2;b ;", 80),
        "let a = -(1 + 2) * x ^ 2;\nb;\n"
    );
    // A prefix minus after a binary minus keeps one space; two prefix minuses
    // must not fuse into `--`.
    assert_eq!(fmt(&lang, &style, "a- -b;", 80), "a - -b;\n");
    assert_eq!(fmt(&lang, &style, "- - 3;", 80), "- -3;\n");
    // Already touching in the source: may stay touching.
    assert_eq!(fmt(&lang, &style, "--3;", 80), "--3;\n");
}

#[test]
fn test_calc_hanging_indent_when_broken() {
    let lang = calc();
    let style = calc_style(&lang);
    let out = fmt(
        &lang,
        &style,
        "let total = first_value + second_value + third_value;",
        30,
    );
    assert_eq!(
        out,
        "let total = first_value +\n    second_value +\n    third_value;\n"
    );
}

#[test]
fn test_unspecified_kinds_keep_original_whitespace() {
    let lang = json();
    let style: Style<_> = Style::default();
    let src = "  {\"a\" :   [1 ,2],   \n\n\n  \"b\":true}   \n\n";
    // Leading and trailing blank space of the file goes, trailing spaces at
    // line ends go, a final newline comes; everything else is kept.
    assert_eq!(
        fmt(&lang, &style, src, 80),
        "{\"a\" :   [1 ,2],\n\n\n  \"b\":true}\n"
    );
}

#[test]
fn test_no_rule_keeps_whitespace_inside_formatted_nodes() {
    let lang = calc();
    // Only `=` has a rule; the gap between `let` and the name has none.
    let style = Rules::new()
        .token(TokenRule::new("=").around(Space::Single))
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &style, "let    x=1;", 80), "let    x = 1;\n");
}

#[test]
fn test_hard_breaks_keep_capped_blank_lines() {
    let lang = calc();
    let style = calc_style(&lang);
    assert_eq!(fmt(&lang, &style, "a;\n\n\n\nb;c;", 80), "a;\n\nb;\nc;\n");
    let two = calc_rules()
        .max_blank_lines(2)
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &two, "a;\n\n\n\n\nb;", 80), "a;\n\n\nb;\n");
    let none = calc_rules()
        .node(NodeRule::new("program").blank_lines(0))
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &none, "a;\n\n\nb;", 80), "a;\nb;\n");
}

#[test]
fn test_empty_delimited_body() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    assert_eq!(fmt(&lang, &style, "[  ]", 80), "[]\n");
    // Even with blank lines between them, empty delimiters close up.
    assert_eq!(fmt(&lang, &style, "{\n\n}", 80), "{}\n");
    let roomy = Rules::new()
        .node(
            NodeRule::new("object")
                .delimiters("{", "}", Space::Line)
                .empty(Space::Single),
        )
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &roomy, "{}", 80), "{ }\n");
}

// --------------------------------------------------------------- comments --

#[test]
fn test_trailing_leading_and_dangling_comments() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "{\n  // leading\n  \"a\": 1, /* trailing */\n\n\n  \"b\": [1,\n 2] // tail\n  // dangling\n}\n// after\n";
    assert_eq!(
        fmt(&lang, &style, src, 80),
        "{\n  // leading\n  \"a\": 1, /* trailing */\n\n  \"b\": [1, 2] // tail\n  // dangling\n}\n// after\n"
    );
}

#[test]
fn test_line_comment_always_ends_its_line() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    // The array would fit flat, but the comment forces it to break.
    assert_eq!(
        fmt(&lang, &style, "[1, // one\n2]", 80),
        "[\n  1, // one\n  2\n]\n"
    );
    let lang = calc();
    let style = calc_style(&lang);
    assert_eq!(
        fmt(&lang, &style, "let a = 1 + # why\n 2;", 80),
        "let a = 1 + # why\n    2;\n"
    );
}

#[test]
fn test_comments_in_empty_bodies() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    assert_eq!(fmt(&lang, &style, "{ // only\n}", 80), "{ // only\n}\n");
    assert_eq!(fmt(&lang, &style, "{\n// only\n}", 80), "{\n  // only\n}\n");
    assert_eq!(fmt(&lang, &style, "[/* x */]", 80), "[ /* x */]\n");
}

#[test]
fn test_comments_before_first_and_after_last_token() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    assert_eq!(
        fmt(
            &lang,
            &style,
            "\n\n/* a */   /* b */\n\n\n\n// c\n[1] // d\n\n\n// e",
            80
        ),
        "/* a */ /* b */\n\n// c\n[1] // d\n\n// e\n"
    );
}

#[test]
fn test_comment_only_and_empty_sources() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    assert_eq!(fmt(&lang, &style, "", 80), "");
    assert_eq!(fmt(&lang, &style, "  \n\t\n", 80), "");
    assert_eq!(fmt(&lang, &style, "  // hi  ", 80), "// hi  \n");
    assert_eq!(
        fmt(&lang, &style, "/* a */\n\n\n/* b */", 80),
        "/* a */\n\n/* b */\n"
    );
}

#[test]
fn test_multiline_block_comment_kept_exactly() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "[1,\n      /* one\n         two   \n      three */ 2]";
    let out = fmt(&lang, &style, src, 80);
    assert_eq!(
        out,
        "[\n  1,\n  /* one\n         two   \n      three */ 2\n]\n"
    );
    assert_eq!(comments(&lang, &out), comments(&lang, src));
}

// --------------------------------------------------- lists and separators --

#[test]
fn test_trailing_separator_policies() {
    let lang = json();
    let never = json_style(&lang, Trailing::Never);
    let always = json_style(&lang, Trailing::Always);
    let keep = json_style(&lang, Trailing::Preserve);
    assert_eq!(fmt(&lang, &never, "[1, 2,]", 80), "[1, 2]\n");
    assert_eq!(fmt(&lang, &always, "[1, 2]", 80), "[1, 2,]\n");
    assert_eq!(fmt(&lang, &keep, "[1, 2,]", 80), "[1, 2,]\n");
    assert_eq!(fmt(&lang, &keep, "[1, 2]", 80), "[1, 2]\n");
    // Empty lists never gain a separator.
    assert_eq!(fmt(&lang, &always, "[]", 80), "[]\n");
    // The comment after the removed comma survives, and so does one after an
    // inserted comma.
    assert_eq!(
        fmt(&lang, &never, "[1, 2, // c\n]", 80),
        "[\n  1,\n  2 // c\n]\n"
    );
    assert_eq!(
        fmt(&lang, &always, "[1, 2 // c\n]", 80),
        "[\n  1,\n  2, // c\n]\n"
    );
}

#[test]
fn test_trailing_policy_skips_malformed_lists() {
    let lang = json();
    let never = json_style(&lang, Trailing::Never);
    let always = json_style(&lang, Trailing::Always);
    // `,,` is a parse error the parser recovers from without an ERROR node:
    // the list does not alternate, so it is left as written.
    // The second comma is never removed, only spaced like any separator.
    assert_eq!(fmt(&lang, &never, "[1,,]", 80), "[1, ,]\n");
    assert_eq!(fmt(&lang, &always, "[1 2]", 80), "[1 2]\n");
}

#[test]
fn test_custom_separator_text() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum K {
        List,
        Open,
        Close,
        Comma,
        Item,
    }
    impl TokenKind for K {}
    let src = "(a)";
    let tree = Node::new(
        K::List,
        vec![
            Element::Token(Token::new(K::Open, Span::new(0, 1))),
            Element::Token(Token::new(K::Item, Span::new(1, 2))),
            Element::Token(Token::new(K::Close, Span::new(2, 3))),
        ],
    );
    let style = Rules::new()
        .final_newline(false)
        .node(
            NodeRule::new("list")
                .delimiters("open", "close", Space::None)
                .separator("comma", Space::Single, Trailing::Always)
                .separator_text(","),
        )
        .compile(|n| match n {
            "list" => Some(K::List),
            "open" => Some(K::Open),
            "close" => Some(K::Close),
            "comma" => Some(K::Comma),
            "item" => Some(K::Item),
            _ => None,
        })
        .unwrap();
    assert_eq!(format(&tree, src, &style, 80).unwrap(), "(a,)");
}

// ----------------------------------------------------------- error nodes --

#[test]
fn test_error_nodes_are_verbatim() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "[1   2 ,   }";
    let parse = lang.parse(src);
    assert!(parse.has_errors());
    // No rule covers the gap before the stray `2` (an ERROR node), so its
    // spacing is kept; the separator's rule still applies around the comma.
    assert_eq!(fmt(&lang, &style, src, 80), "[1   2, }\n");
    // Inside an error node, the original spacing is kept exactly.
    let src = "{\"a\": 1 \"b\"   :   2}";
    let out = fmt(&lang, &style, src, 80);
    assert!(out.contains("\"b\"   :   2"), "{out:?}");
    assert_eq!(significant(&lang, &out), significant(&lang, src));
}

#[test]
fn test_unknown_characters_are_kept() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "[1,@ 2]";
    let out = fmt(&lang, &style, src, 80);
    assert_eq!(comments(&lang, &out), comments(&lang, src));
    assert_eq!(significant(&lang, &out), significant(&lang, src));
}

// ------------------------------------------------------- source handling --

#[test]
fn test_crlf_and_bom() {
    let lang = calc();
    let style = calc_style(&lang);
    assert_eq!(
        fmt(&lang, &style, "\u{feff}a;\r\n\r\n\r\nb;\r\n", 80),
        "\u{feff}a;\n\nb;\n"
    );
}

#[test]
fn test_final_newline_off() {
    let lang = calc();
    let style = calc_rules()
        .final_newline(false)
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &style, "a;b;", 80), "a;\nb;");
}

#[test]
fn test_subtree_formats_its_own_range() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "{\"a\" : [3,4]}";
    let parse = lang.parse(src);
    let array = lang.kind("array").unwrap();
    let node = parse
        .tree()
        .descendants()
        .find(|n| *n.kind() == array)
        .unwrap();
    assert_eq!(format(node, src, &style, 80).unwrap(), "[3, 4]\n");
}

#[test]
fn test_format_doc_renders_at_any_width() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let parse = lang.parse("[1, 2]");
    let doc = format_doc(parse.tree(), parse.source(), &style, &[]).unwrap();
    assert_eq!(doc.render(80), "[1, 2]\n");
    assert_eq!(doc.render(3), "[\n  1,\n  2\n]\n");
}

#[test]
fn test_max_indent_caps_deep_nesting() {
    let lang = json();
    let style = json_rules(Trailing::Never)
        .max_indent(6)
        .compile(|n| lang.kind(n))
        .unwrap();
    let out = fmt(&lang, &style, "[[[[[[1]]]]]]", 1);
    let widest = out
        .lines()
        .map(|l| l.len() - l.trim_start().len())
        .max()
        .unwrap();
    assert_eq!(widest, 6);
}

#[test]
fn test_deep_forged_tree_does_not_overflow() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    // Past lang-forge's own nesting limit the parser reports an error and
    // wraps the rest in an ERROR node, which is written verbatim.
    let depth = 200_000;
    let src = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    let out = fmt(&lang, &style, &src, 80);
    assert_eq!(significant(&lang, &out), significant(&lang, &src));
}

#[test]
fn test_deep_hand_built_tree_formats_with_capped_indent() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum K {
        Array,
        Open,
        Close,
        Num,
    }
    impl TokenKind for K {}
    let depth: u32 = 200_000;
    let src = format!(
        "{}1{}",
        "[".repeat(depth as usize),
        "]".repeat(depth as usize)
    );
    let mut b = Builder::new();
    for i in 0..depth {
        b.start_node(K::Array);
        b.token(Token::new(K::Open, Span::new(i, i + 1)));
    }
    b.token(Token::new(K::Num, Span::new(depth, depth + 1)));
    for i in 0..depth {
        let at = depth + 1 + i;
        b.token(Token::new(K::Close, Span::new(at, at + 1)));
        b.finish_node();
    }
    let tree = b.finish().unwrap();
    let style = Rules::new()
        .node(
            NodeRule::new("array")
                .group()
                .indent(Indent::Block)
                .delimiters("open", "close", Space::SoftLine),
        )
        .compile(|n| match n {
            "array" => Some(K::Array),
            "open" => Some(K::Open),
            "close" => Some(K::Close),
            _ => None,
        })
        .unwrap();
    let out = format(&tree, &src, &style, 80).unwrap();
    // Every bracket on its own line, indentation capped at 120 columns.
    assert_eq!(out.lines().count(), 2 * depth as usize + 1);
    assert!(out.lines().all(|l| l.len() <= 121));
    assert_eq!(out.matches('[').count(), depth as usize);
}

#[test]
fn test_hand_built_tree_and_custom_touch() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum K {
        Root,
        Minus,
        Num,
        Ws,
    }
    impl TokenKind for K {
        fn is_trivia(&self) -> bool {
            matches!(self, K::Ws)
        }
    }
    let src = "- 1";
    let mut b = Builder::new();
    b.start_node(K::Root);
    b.token(Token::new(K::Minus, Span::new(0, 1)));
    b.token(Token::new(K::Ws, Span::new(1, 2)));
    b.token(Token::new(K::Num, Span::new(2, 3)));
    b.finish_node();
    let tree = b.finish().unwrap();
    let rules = Rules::new()
        .final_newline(false)
        .token(TokenRule::new("minus").after(Space::None));
    let resolve = |n: &str| (n == "minus").then_some(K::Minus);
    let style = rules.compile(resolve).unwrap();
    assert_eq!(format(&tree, src, &style, 80).unwrap(), "-1");
    fn no_negative_literals(l: &str, r: &str) -> bool {
        !(l == "-" && r.starts_with(|c: char| c.is_ascii_digit())) && can_touch(l, r)
    }
    let style = rules
        .compile(resolve)
        .unwrap()
        .with_touch(no_negative_literals);
    assert_eq!(format(&tree, src, &style, 80).unwrap(), "- 1");
}

#[test]
fn test_invalid_trees_are_errors() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct K;
    impl TokenKind for K {}
    let style = Style::<K>::default();
    let tok = |s, e| Element::Token(Token::new(K, Span::new(s, e)));
    let gap = Node::new(K, vec![tok(0, 1), tok(2, 3)]);
    assert_eq!(
        format(&gap, "abc", &style, 80),
        Err(FormatError::NotContiguous {
            expected: 1,
            found: 2
        })
    );
    let overlap = Node::new(K, vec![tok(0, 2), tok(1, 3)]);
    assert!(matches!(
        format(&overlap, "abc", &style, 80),
        Err(FormatError::NotContiguous { .. })
    ));
    let split = Node::new(K, vec![tok(0, 1)]);
    assert_eq!(
        format(&split, "é", &style, 80),
        Err(FormatError::NotCharBoundary { offset: 1 })
    );
    let long = Node::new(K, vec![tok(0, 9)]);
    assert!(matches!(
        format(&long, "abc", &style, 80),
        Err(FormatError::OutOfBounds { .. })
    ));
    assert!(
        !FormatError::NotCharBoundary { offset: 1 }
            .to_string()
            .is_empty()
    );
}

#[test]
fn test_empty_and_trivia_only_nodes() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum K {
        Root,
        Empty,
        Word,
        Ws,
    }
    impl TokenKind for K {
        fn is_trivia(&self) -> bool {
            matches!(self, K::Ws)
        }
    }
    let src = "a  b";
    let tree = Node::new(
        K::Root,
        vec![
            Element::Node(Node::new(K::Empty, vec![])),
            Element::Token(Token::new(K::Word, Span::new(0, 1))),
            Element::Node(Node::new(
                K::Empty,
                vec![Element::Token(Token::new(K::Ws, Span::new(1, 3)))],
            )),
            Element::Token(Token::new(K::Word, Span::new(3, 4))),
            // An empty significant token: skipped.
            Element::Token(Token::new(K::Word, Span::new(4, 4))),
        ],
    );
    let style = Rules::new()
        .node(
            NodeRule::new("empty")
                .group()
                .indent(Indent::Hanging)
                .before(Space::Hard),
        )
        .compile(|n| (n == "empty").then_some(K::Empty))
        .unwrap();
    assert_eq!(format(&tree, src, &style, 80).unwrap(), "a  b\n");
}

// ------------------------------------- regressions found by the soak runs --

#[test]
fn test_inserted_separator_is_spaced_like_a_written_one() {
    let lang = json();
    let always = json_style(&lang, Trailing::Always);
    // `"s":` is missing its value; the comma goes after the colon with the
    // colon's own spacing, exactly as a comma written there would get.
    let once = fmt(&lang, &always, "{\"s\":}", 80);
    assert_eq!(once, "{ \"s\": , }\n");
    assert_eq!(fmt(&lang, &always, &once, 80), once);
}

#[test]
fn test_unclosed_delimiters_block_separator_edits() {
    let lang = json();
    let always = json_style(&lang, Trailing::Always);
    // The array lacks its `]`: a comma inserted after `1` would land inside
    // it on the next parse, so the object is left without one.
    let src = "{\"a\": [1}";
    let once = fmt(&lang, &always, src, 80);
    assert!(!once.contains(','), "{once:?}");
    assert_eq!(fmt(&lang, &always, &once, 80), once);
}

#[test]
fn test_lone_carriage_return_is_completed_not_absorbed() {
    let lang = calc();
    let style = calc_style(&lang);
    // `# a\r` is a comment ending in a lone `\r` (the lexer only ends line
    // comments at `\n` or `\r\n`). Writing `\n` after it would make `\r\n`,
    // shortening the comment; the break is written `\r\n` instead.
    let src = "x; # a\r\r\ny;";
    let out = fmt(&lang, &style, src, 80);
    assert_eq!(out, "x; # a\r\r\ny;\n");
    assert_eq!(comments(&lang, &out), comments(&lang, src));
    assert_eq!(fmt(&lang, &style, &out, 80), out);
}

#[test]
fn test_kept_regions_protect_unterminated_strings() {
    let lang = json();
    let style = json_style(&lang, Trailing::Always);
    let src = "[\"open\n, 1]";
    let kept = fmt_keeping(&lang, &style, src, 80);
    assert_eq!(significant(&lang, &kept), significant(&lang, src));
    assert_eq!(fmt_keeping(&lang, &style, &kept, 80), kept);
    // No separator is inserted into a list holding a kept region.
    assert!(!kept.contains("1,"), "{kept:?}");
    // Without the region, the line break goes and the comma joins the string.
    let plain = fmt(&lang, &style, src, 80);
    assert_ne!(significant(&lang, &plain), significant(&lang, src));
}

#[test]
fn test_unterminated_comment_at_end_is_kept_exactly() {
    let lang = json();
    let style = json_style(&lang, Trailing::Never);
    let src = "[1]   /* open  ";
    let kept = fmt_keeping(&lang, &style, src, 80);
    assert_eq!(kept, "[1]   /* open  ");
    assert_eq!(comments(&lang, &kept), comments(&lang, src));
}

#[test]
fn test_comment_swallowing_its_line_break_at_the_end() {
    let lang = json();
    // An unterminated block comment runs to the end and takes the final
    // `\n` with it; the final line break stands in for it.
    let src = "[1] /* open\n";
    let style = json_style(&lang, Trailing::Never);
    let out = fmt(&lang, &style, src, 80);
    assert_eq!(out, src);
    let bare = json_rules(Trailing::Never)
        .final_newline(false)
        .compile(|n| lang.kind(n))
        .unwrap();
    assert_eq!(fmt(&lang, &bare, src, 80), src);
}

#[test]
fn test_chained_hanging_nodes_indent_once() {
    let lang = calc();
    let style = calc_style(&lang);
    let out = fmt(
        &lang,
        &style,
        "let r = aaaa * bbbb * cccc * dddd * eeee;",
        20,
    );
    for line in out.lines().skip(1) {
        assert!(
            line.starts_with("    ") && !line.starts_with("        "),
            "{out:?}"
        );
    }
}
