//! Criterion benchmarks: formatting forged-language trees of 10k to 1M nodes,
//! and a hand-built tree nested 100k levels deep.
//!
//! ```text
//! cargo bench
//! ```
//!
//! Parsing happens once, outside the measurement: the numbers are the
//! formatter alone (flatten, walk, and pretty-lang rendering), reported per
//! tree node.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fmt_lang::syntax_lang::{Builder, Node, Span, Token, TokenKind};
use fmt_lang::{Indent, NodeRule, Rules, Space, Style, TokenRule, Trailing, format, format_doc};
use lang_forge::{Kind, Language};

const JSON: &str = r#"
[language]
name = "jsonish"

[lexer]
strings = ['"']
line_comments = ["//"]

[rules]
document = "value"
value    = "object | array | STRING | NUMBER | 'true' | 'false' | 'null'"
object   = "'{' (member (',' member)* ','?)? '}'"
member   = "STRING ':' value"
array    = "'[' (value (',' value)* ','?)? ']'"
"#;

fn json_style(lang: &Language) -> Style<Kind> {
    Rules::new()
        .indent(2)
        .verbatim("ERROR")
        .token(TokenRule::new(":").before(Space::None).after(Space::Single))
        .node(
            NodeRule::new("object")
                .group()
                .indent(Indent::Block)
                .delimiters("{", "}", Space::Line)
                .separator(",", Space::Line, Trailing::Always),
        )
        .node(
            NodeRule::new("array")
                .group()
                .indent(Indent::Block)
                .delimiters("[", "]", Space::SoftLine)
                .separator(",", Space::Line, Trailing::Never),
        )
        .compile(|n| lang.kind(n))
        .unwrap_or_default()
}

/// A JSON-like document of `records` records, unevenly spaced and commented,
/// so the formatter has whitespace to fix, comments to attach, and groups
/// that both fit and break.
fn source(records: usize) -> String {
    let mut s = String::from("[\n");
    for i in 0..records {
        s.push_str(&format!(
            "  {{\"id\":{i},  \"name\" : \"record {i}\", // note\n \"tags\":[1,2,  3],\"nested\":{{\"ok\":true,\"list\":[{i},{i}]}}}},\n"
        ));
    }
    s.push_str("]\n");
    s
}

fn bench_json(c: &mut Criterion) {
    let lang = match Language::from_lsf(JSON) {
        Ok(lang) => lang,
        Err(e) => panic!("benchmark schematic does not forge: {e:?}"),
    };
    let style = json_style(&lang);
    let plain: Style<Kind> = Style::default();
    let mut group = c.benchmark_group("format_json");
    group.sample_size(10);
    // Roughly 28 nodes per record: 360, 3.6k, and 36k records give about
    // 10k, 100k, and 1M nodes.
    for records in [360usize, 3_600, 36_000] {
        let src = source(records);
        let parse = lang.parse(&src);
        let nodes = parse.tree().descendants().count() as u64;
        group.throughput(Throughput::Elements(nodes));
        group.bench_function(format!("rules/{nodes}_nodes"), |b| {
            b.iter(|| format(black_box(parse.tree()), black_box(&src), &style, 80))
        });
        group.bench_function(format!("doc_only/{nodes}_nodes"), |b| {
            b.iter(|| format_doc(black_box(parse.tree()), black_box(&src), &style, &[]))
        });
        group.bench_function(format!("no_rules/{nodes}_nodes"), |b| {
            b.iter(|| format(black_box(parse.tree()), black_box(&src), &plain, 80))
        });
    }
    group.finish();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum K {
    Array,
    Open,
    Close,
    Num,
}

impl TokenKind for K {}

fn deep(depth: u32) -> (Node<K>, String) {
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
    match b.finish() {
        Ok(tree) => (tree, src),
        Err(e) => panic!("unbalanced benchmark tree: {e:?}"),
    }
}

fn bench_deep(c: &mut Criterion) {
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
        .unwrap_or_default();
    let mut group = c.benchmark_group("format_deep");
    group.sample_size(10);
    let depth = 100_000;
    let (tree, src) = deep(depth);
    group.throughput(Throughput::Elements(u64::from(depth)));
    group.bench_function("nested_100k", |b| {
        b.iter(|| format(black_box(&tree), black_box(&src), &style, 80))
    });
    group.finish();
}

criterion_group!(benches, bench_json, bench_deep);
criterion_main!(benches);
