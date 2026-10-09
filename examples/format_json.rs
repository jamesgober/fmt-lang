//! Formats a forged JSON-like language from rules alone.
//!
//! ```text
//! cargo run --example format_json
//! ```
//!
//! The language comes from a lang-forge schematic; its formatter is a handful
//! of rules. Nothing here is specific to JSON beyond the kind names.

use fmt_lang::{Indent, NodeRule, Rules, Space, TokenRule, Trailing, format, format_keeping};
use lang_forge::Language;

const SCHEMATIC: &str = r#"
[language]
name = "jsonc"

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lang = Language::from_lsf(SCHEMATIC)?;

    // The whole formatter: what a sketch's `[tooling.format]` would say.
    let style = Rules::new()
        .indent(2)
        .verbatim("ERROR")
        .token(TokenRule::new(":").before(Space::None).after(Space::Single))
        .node(
            NodeRule::new("object")
                .group()
                .indent(Indent::Block)
                .delimiters("{", "}", Space::Line)
                .separator(",", Space::Line, Trailing::Never),
        )
        .node(
            NodeRule::new("array")
                .group()
                .indent(Indent::Block)
                .delimiters("[", "]", Space::SoftLine)
                .separator(",", Space::Line, Trailing::Never),
        )
        .compile(|name| lang.kind(name))?;

    let messy = r#"{"name":"fmt-lang",   "version" :"0.2.0",
        // the formatter keeps comments where they belong
        "tags":["formatter","pretty-print",],"limits":{"indent":120,"blank_lines":1}, /* trailing */


        "ok":true}"#;

    let parse = lang.parse(messy);
    for width in [100, 40] {
        println!("--- width {width} ---");
        print!("{}", format(parse.tree(), parse.source(), &style, width)?);
    }

    // Broken input still formats: error nodes are written as they are, and
    // the regions of lexical errors (here an unterminated string) are kept.
    let broken = "{\"a\":[1 2,   3], \"b\": \"unterminated\n, \"c\":true}";
    let parse = lang.parse(broken);
    let lexical: Vec<_> = parse
        .diagnostics()
        .iter()
        .filter(|d| d.message().starts_with("unterminated"))
        .map(|d| d.primary().span())
        .collect();
    println!("--- with errors ---");
    print!(
        "{}",
        format_keeping(parse.tree(), parse.source(), &style, 40, &lexical)?
    );
    Ok(())
}
