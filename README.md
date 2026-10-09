<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>fmt-lang</b>
    <br>
    <sub><sup>RULE-DRIVEN FORMATTER</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/fmt-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/fmt-lang"></a>
    <a href="https://crates.io/crates/fmt-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/fmt-lang?color=%230099ff"></a>
    <a href="https://docs.rs/fmt-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/fmt-lang"></a>
    <a href="https://github.com/jamesgober/fmt-lang/actions"><img alt="CI" src="https://github.com/jamesgober/fmt-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>fmt-lang</strong> formats source code from declarative rules over a lossless syntax tree, and renders the result through <a href="https://crates.io/crates/pretty-lang"><code>pretty-lang</code></a>. Describe how a language's nodes and tokens are laid out &mdash; groups that break when they do not fit, indentation, spacing around tokens, list separators, blank lines &mdash; and any <a href="https://crates.io/crates/syntax-lang"><code>syntax-lang</code></a> tree of that language formats from those rules. Anything the rules do not mention keeps its original whitespace.
    </p>
    <p>
        The rules are data, keyed by kind names, so a language's formatter can come from configuration rather than code: it is the formatter stage of LexerSketch, where every forged language gets one from its sketch. Comments are attached by a documented algorithm and never lost or reordered, error nodes are written exactly as they are, and formatting is idempotent.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition). <code>no_std</code>-compatible (needs only <code>alloc</code>), <code>#![forbid(unsafe_code)]</code>, two dependencies from the family: <a href="https://crates.io/crates/syntax-lang"><code>syntax-lang</code></a> and <a href="https://crates.io/crates/pretty-lang"><code>pretty-lang</code></a>.
    </p>
    <blockquote>
        <strong>Status: pre-1.0 (v0.2.0, Foundation).</strong> The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./docs/API.md"><code>docs/API.md</code></a>, <a href="./dev/ROADMAP.md"><code>dev/ROADMAP.md</code></a>, and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## The model

- **[`Rules`](./docs/API.md#rules)** describe a language's layout as data: **[`NodeRule`](./docs/API.md#noderule)s** (group, indent, spacing before and after a node, list delimiters and separators with a trailing-separator policy, blank-line caps) and **[`TokenRule`](./docs/API.md#tokenrule)s** (spacing before and after a token kind, everywhere or inside one node kind).
- **[`Rules::compile`](./docs/API.md#rulescompile)** resolves the kind names against a language once and returns a **[`Style`](./docs/API.md#style)**.
- **[`format`](./docs/API.md#format)** formats a tree with a style at a width. [`format_keeping`](./docs/API.md#format_keeping) also takes the regions of lexical errors to leave as written; [`format_doc`](./docs/API.md#format_doc) returns the `pretty_lang::Doc` instead of a string.

<br>

What it guarantees, and how each guarantee is checked:

| Guarantee | How it is held |
|---|---|
| Formatting is idempotent: `fmt(fmt(x)) == fmt(x)`. | Property tests re-parse and re-format the output of random valid and invalid sources in two languages forged with lang-forge, under several rule sets and random widths. |
| The significant tokens are unchanged, except a trailing separator a `Trailing::Always` / `Never` policy adds or removes. | The same tests compare every non-trivia token, kind and text, before and after. |
| Every comment appears exactly once, unchanged and in order; a line comment always ends its line. | The same tests compare the lexed comments before and after, and check the character after every line comment. |
| Error nodes and anything unparsable are written as they are. | Parse errors are `ERROR` nodes written verbatim; lexical errors are kept as written with `format_keeping`. Token soups and random strings are in the property tests. |
| No panics on any tree, error nodes included; a tree that does not match its source is an error value. | Arbitrary hand-built trees and hostile token spans in the property tests. |
| Deterministic. | Every property test formats twice and compares. |
| With no rules, only whitespace changes, exactly as specified. | Compared with a reference implementation written over the lexer's flat token list. |
| Deep and large input is safe. | The walk is iterative and linear; a tree nested 200,000 levels deep formats in a test, and indentation is capped (`Rules::max_indent`) so output cannot explode. |

The property tests run 400 cases each by default; before this release they were soaked at 400,000 cases each.

<hr>
<br>

## Installation

```toml
[dependencies]
fmt-lang = "0.2"
```

Without the standard library:

```toml
[dependencies]
fmt-lang = { version = "0.2", default-features = false }
```

<hr>
<br>

## Quick start

A JSON-like language forged with [`lang-forge`](https://crates.io/crates/lang-forge), and its whole formatter:

```rust
use fmt_lang::{format, Indent, NodeRule, Rules, Space, TokenRule, Trailing};
use lang_forge::Language;

let json = Language::from_lsf(r#"
    [language]
    name = "json"
    [lexer]
    strings = ['"']
    line_comments = ["//"]
    [rules]
    document = "value"
    value    = "object | array | STRING | NUMBER | 'true' | 'false' | 'null'"
    object   = "'{' (member (',' member)* ','?)? '}'"
    member   = "STRING ':' value"
    array    = "'[' (value (',' value)* ','?)? ']'"
"#)?;

let style = Rules::new()
    .indent(2)
    .verbatim("ERROR")
    .token(TokenRule::new(":").before(Space::None).after(Space::Single))
    .node(NodeRule::new("object").group().indent(Indent::Block)
        .delimiters("{", "}", Space::Line)
        .separator(",", Space::Line, Trailing::Never))
    .node(NodeRule::new("array").group().indent(Indent::Block)
        .delimiters("[", "]", Space::SoftLine)
        .separator(",", Space::Line, Trailing::Never))
    .compile(|name| json.kind(name))?;

let parse = json.parse(r#"{"tags":["a","b",],  "ok" :true // done
}"#);

assert_eq!(
    format(parse.tree(), parse.source(), &style, 80)?,
    "{\n  \"tags\": [\"a\", \"b\"],\n  \"ok\": true // done\n}\n",
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The line comment must end its line, so the object cannot be laid out flat and breaks, one member per line; the array still fits and stays flat. Without the comment the whole object fits on one line at width 80; at width 12 the array breaks too.

### Comments stay where they belong

Comments are trailing (on the line of the token before them), leading (on their own line before the next token), or dangling (on their own line at the end of a block, indented with it):

```rust
use fmt_lang::{format, Indent, NodeRule, Rules, Space, TokenRule, Trailing};
use lang_forge::Language;

let json = Language::from_lsf(r#"
    [language]
    name = "json"
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
"#)?;
let style = Rules::new()
    .indent(2)
    .token(TokenRule::new(":").before(Space::None).after(Space::Single))
    .node(NodeRule::new("object").group().indent(Indent::Block)
        .delimiters("{", "}", Space::Line)
        .separator(",", Space::Line, Trailing::Preserve))
    .compile(|name| json.kind(name))?;

let src = "{\n// leading\n\"a\":1, /* trailing */\n\n\n\"b\":2\n    // dangling\n}";
let parse = json.parse(src);
assert_eq!(
    format(parse.tree(), parse.source(), &style, 80)?,
    "{\n  // leading\n  \"a\": 1, /* trailing */\n\n  \"b\": 2\n  // dangling\n}\n",
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Expression languages

Token rules give operators their spacing, a node rule makes a prefix operator hug its operand, and a hanging indent wraps long expressions. Nothing ever fuses two tokens: `- -3` keeps its space.

```rust
use fmt_lang::{format, Indent, NodeRule, Rules, Space, TokenRule};
use lang_forge::Language;

let calc = Language::from_lsf(r#"
    [language]
    name = "calc"
    [rules]
    program = "stmt*"
    stmt    = "'let' IDENT '=' expr ';' | expr ';'"
    group   = "'(' expr ')'"
    [rules.expr]
    operand = "NUMBER | IDENT | group"
    levels  = [ { left = ["+", "-"] }, { left = ["*", "/"] }, { prefix = ["-"] } ]
"#)?;

let mut rules = Rules::new()
    .token(TokenRule::new("let").after(Space::Single))
    .token(TokenRule::new("=").around(Space::Single))
    .token(TokenRule::new(";").before(Space::None))
    .token(TokenRule::new("(").after(Space::None))
    .token(TokenRule::new(")").before(Space::None))
    .node(NodeRule::new("stmt").before(Space::Hard))
    .node(NodeRule::new("binary").group().indent(Indent::Hanging))
    .node(NodeRule::new("prefix").token(TokenRule::any().before(Space::None).after(Space::None)));
for op in ["+", "-", "*", "/"] {
    rules = rules.token(TokenRule::new(op).before(Space::Single).after(Space::Line));
}
let style = rules.compile(|name| calc.kind(name))?;

let parse = calc.parse("let   x=-(1+2)*y;- - 3;");
assert_eq!(format(parse.tree(), parse.source(), &style, 80)?, "let x = -(1 + 2) * y;\n- -3;\n");
# Ok::<(), Box<dyn std::error::Error>>(())
```

<hr>
<br>

## Examples

| Example | What it shows |
|---|---|
| [`format_json`](./examples/format_json.rs) | A forged JSON-with-comments language formatted at two widths from six rules, then a broken file formatted with its error nodes and lexical errors kept. `cargo run --example format_json` |

<hr>
<br>

## Performance

The formatter is two linear passes over the tree, both iterative: one flattens the tree into an event list (checking it against the source and settling list delimiters), one builds a single `pretty_lang::Doc`, merging runs of tokens and spaces into one text node, and pretty-lang renders it in one linear pass. Rule lookups are a binary search over rules sorted by kind, once per node and once per token.

Measured with the benchmarks in [`benches/`](./benches), Windows x86_64, Rust stable, release profile. Parsing is done once, outside the measurement; the numbers are the formatter alone:

| Benchmark | Input | Time | Throughput |
|---|---|---:|---:|
| `format_json/rules` | 7.9k nodes (37 KB) | ~2.5 ms | ~3.1M nodes/s (~14 MB/s) |
| `format_json/rules` | 79k nodes (381 KB) | ~51 ms | ~1.6M nodes/s (~7 MB/s) |
| `format_json/rules` | 792k nodes (4.0 MB) | ~645 ms | ~1.2M nodes/s (~6 MB/s) |
| `format_json/doc_only` | 792k nodes, building the `Doc` without rendering it | ~510 ms | ~1.6M nodes/s |
| `format_json/no_rules` | 792k nodes, empty style (whitespace kept) | ~300 ms | ~2.6M nodes/s |
| `format_deep/nested_100k` | a tree nested 100,000 levels deep | ~155 ms | ~0.64M nodes/s |

These are first numbers, not tuned ones, and they were taken on a machine running other builds, so expect ±20%. Throughput falls as inputs grow. A phase split at 792k nodes (one run, same input) showed roughly 15% flattening, 45% building the `Doc`, 15% rendering it, and 25% dropping it: most of the cost is allocating and freeing pretty-lang's reference-counted document nodes (one per text run, break, group, and join). The `Doc` in pretty-lang 1.x has no arena, so that is the next thing to change for speed.

```bash
cargo bench --bench bench
```

<hr>
<br>

## Limits

- Whether two tokens may touch is decided by a conservative heuristic ([`can_touch`](./docs/API.md#can_touch)) unless the style supplies an exact test.
- Tokens whose extent depends on the whitespace after them (an unterminated string ends at the line break) need their regions passed to [`format_keeping`](./docs/API.md#format_keeping); without them, removing that line break changes the token.
- A parser's recovery that leaves no trace in the tree is invisible to the formatter; trailing-separator edits skip lists with error nodes or unclosed delimiters, but `Trailing::Preserve` is the policy that never edits.
- In languages whose line breaks are tokens, rules that break lines add tokens.
- Indentation is spaces; widths count `char`s, not display columns.
- Range formatting (for LSP `rangeFormatting`) and a trailing comma "only when broken" are planned (see the [ROADMAP](./dev/ROADMAP.md)).

<hr>
<br>

## Contributing

See <a href="./dev/DIRECTIVES.md"><code>dev/DIRECTIVES.md</code></a> for engineering standards and the definition of done. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober.</strong></sup>
</div>
