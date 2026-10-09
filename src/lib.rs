//! # fmt_lang
//!
//! A source formatter driven by declarative rules over a lossless syntax
//! tree, rendering through [`pretty_lang`].
//!
//! A language gets a formatter from configuration, not code: describe how
//! its nodes and tokens are laid out as [`Rules`] (data, keyed by kind names,
//! the way a sketch names them), [`compile`](Rules::compile) them against the
//! language's kinds once, and [`format()`] any [`syntax_lang`] tree of that
//! language. Whatever the rules do not mention keeps its original whitespace.
//!
//! ## Quick start
//!
//! ```
//! use fmt_lang::{format, Indent, NodeRule, Rules, Space, TokenRule, Trailing};
//! use lang_forge::Language;
//!
//! let json = Language::from_lsf(
//!     r#"
//!     [language]
//!     name = "json"
//!
//!     [lexer]
//!     strings = ['"']
//!     line_comments = ["//"]
//!
//!     [rules]
//!     document = "value"
//!     value    = "object | array | STRING | NUMBER | 'true' | 'false' | 'null'"
//!     object   = "'{' (member (',' member)* ','?)? '}'"
//!     member   = "STRING ':' value"
//!     array    = "'[' (value (',' value)* ','?)? ']'"
//!     "#,
//! )?;
//!
//! let style = Rules::new()
//!     .indent(2)
//!     .verbatim("ERROR")
//!     .token(TokenRule::new(":").before(Space::None).after(Space::Single))
//!     .node(
//!         NodeRule::new("object")
//!             .group()
//!             .indent(Indent::Block)
//!             .delimiters("{", "}", Space::Line)
//!             .separator(",", Space::Line, Trailing::Never),
//!     )
//!     .node(
//!         NodeRule::new("array")
//!             .group()
//!             .indent(Indent::Block)
//!             .delimiters("[", "]", Space::SoftLine)
//!             .separator(",", Space::Line, Trailing::Never),
//!     )
//!     .compile(|name| json.kind(name))?;
//!
//! let parse = json.parse("{\"a\":[1,2,],   \"b\" :{}}");
//! let wide = format(parse.tree(), parse.source(), &style, 80)?;
//! assert_eq!(wide, "{ \"a\": [1, 2], \"b\": {} }\n");
//!
//! // Too wide for 16 columns: the object breaks, the array still fits.
//! let narrow = format(parse.tree(), parse.source(), &style, 16)?;
//! assert_eq!(narrow, "{\n  \"a\": [1, 2],\n  \"b\": {}\n}\n");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## The rule model
//!
//! - A [`NodeRule`] makes a node a *group* (flat if it fits, broken at its
//!   line opportunities otherwise), *indents* its body or its whole content,
//!   asks for spacing *before* and *after* the node, names the *delimiters*
//!   and *separator* of a list (with a [`Trailing`] separator policy), caps
//!   *blank lines*, and carries *token rules* for its direct child tokens.
//! - A [`TokenRule`] asks for spacing before and after one token kind, at the
//!   top level (a default everywhere) or inside a node rule (that context
//!   only, and it wins).
//! - Spacing is a [`Space`]; when several rules speak about one gap, the most
//!   generous answer wins. When no rule speaks, the gap keeps its original
//!   whitespace (with trailing spaces at line ends removed).
//! - [`Rules::verbatim`] names node kinds written exactly as in the source,
//!   such as a parser's `ERROR` nodes.
//!
//! ## Guarantees
//!
//! Property-tested over random valid and invalid sources of forged languages
//! and over arbitrary trees (see `tests/`):
//!
//! - **Idempotent:** formatting formatted output changes nothing.
//! - **Token-preserving:** the significant token sequence is unchanged (the
//!   only exception is a trailing separator that a [`Trailing::Always`] or
//!   [`Trailing::Never`] policy adds or removes), and every comment's text
//!   appears exactly once, in its original order. A line comment always ends
//!   its line. How comments are attached is documented on [`format()`].
//! - **Total:** any tree whose tokens describe the source formats without
//!   panicking, error nodes included; a tree that does not match its source is
//!   a [`FormatError`], never a panic.
//! - **Deterministic:** the same input gives the same output.
//!
//! For sources with lexical errors (an unterminated string or comment), the
//! guarantees hold when the errors' regions are passed to [`format_keeping`]:
//! such a token's extent depends on the whitespace after it, which the
//! formatter cannot see from the tree.
//!
//! The walk is iterative and linear in the size of the tree, so trees nested
//! hundreds of thousands of levels deep format without exhausting the stack;
//! indentation stops growing at [`Rules::max_indent`], which bounds the output
//! for hostile input.
//!
//! ## Limits
//!
//! - Whether two tokens may touch is decided by [`can_touch`], a conservative
//!   heuristic, unless the style supplies an exact test
//!   ([`Style::with_touch`]).
//! - A parser's recovery that leaves no trace in the tree (an assumed missing
//!   token) is invisible here. [`Trailing::Always`] and [`Trailing::Never`]
//!   edit only lists with no error node and no unclosed delimited node inside,
//!   but a recovery these checks miss could still read an edited list
//!   differently; [`Trailing::Preserve`] never edits.
//! - In languages whose line breaks are significant tokens, rules that break
//!   lines ([`Space::Line`], [`Space::Hard`]) would add tokens; use them only
//!   where the language allows a line break.
//! - Indentation is spaces; widths are counted in `char`s (pretty-lang's
//!   measure), not display columns.
//!
//! ## Features
//!
//! - `std` (default): the standard library, forwarded to `syntax-lang` and
//!   `pretty-lang`. Without it the crate is `no_std` and needs only `alloc`.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]
#![deny(clippy::undocumented_unsafe_blocks)]

extern crate alloc;

mod error;
mod flat;
mod rules;
mod style;
mod walk;

use alloc::string::String;

pub use error::{FormatError, RuleError};
pub use rules::{Indent, MAX_INDENT_STEP, NodeRule, Rules, Space, TokenRule, Trailing};
pub use style::Style;

// Re-exported whole, so callers name the exact tree and document types this
// crate's API is built on.
pub use pretty_lang;
pub use syntax_lang;

use pretty_lang::Doc;
use syntax_lang::{Node, Span, TokenKind};

/// Compiles and runs the `rust` code blocks in `README.md` and `docs/API.md` as
/// part of `cargo test`, so the published examples cannot drift from the API.
///
/// Present only while collecting doctests (`#[cfg(doctest)]`); it is not part of
/// the public surface and does not appear in the built library or its docs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
#[doc = include_str!("../docs/API.md")]
pub struct MarkdownDocTests;

/// Formats the source `tree` was parsed from, at `width` columns.
///
/// `tree` must be a lossless tree of `source` (its tokens, trivia included,
/// cover a contiguous range of `source`); it may be a whole file or any
/// subtree, and may contain error nodes. The output covers the tree's range.
///
/// # Layout
///
/// Between every two neighbouring significant tokens (a node named by
/// [`Rules::verbatim`] counts as one token), the formatter asks every rule that
/// applies to the gap: the left token's `after`, the right token's `before`,
/// the `after` of nodes ending there and the `before` of nodes starting there,
/// and the delimiter and separator rules of the node holding both tokens. The
/// most generous answer wins; if none applies, the source's whitespace is kept.
/// Groups and indentation come from the node rules; [`pretty_lang`] then picks
/// the line breaks that fit `width`.
///
/// # Comments
///
/// Comments (any trivia token that is not all whitespace) are attached by
/// where they sit in the source:
///
/// - **trailing**: no line break between the comment and the token before it.
///   It stays on that token's line, after one space.
/// - **leading**: the comment started a line. It starts a line in the output,
///   at the indentation of the gap it sits in, keeping up to the configured
///   number of blank lines before it.
/// - **dangling**: a leading comment just before the closing delimiter of an
///   indented body ([`Indent::Block`]). It is indented with the body.
///
/// A comment followed by a line break in the source is followed by one in the
/// output, so a line comment always ends its line. Comments never cross a
/// token, so their order is preserved, and their text is written unchanged.
///
/// # Errors
///
/// A [`FormatError`] if the tree's tokens do not describe `source`: a span out
/// of bounds or inside a character, or tokens with gaps or overlaps between
/// them.
///
/// # Examples
///
/// ```
/// use fmt_lang::syntax_lang::{Builder, Span, Token, TokenKind};
/// use fmt_lang::{format, Rules, Space, TokenRule};
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// enum K { Sum, Num, Plus, Ws }
/// impl TokenKind for K {
///     fn is_trivia(&self) -> bool { matches!(self, K::Ws) }
/// }
///
/// // `1   +2`, lexed and built by hand.
/// let source = "1   +2";
/// let mut b = Builder::new();
/// b.start_node(K::Sum);
/// b.token(Token::new(K::Num, Span::new(0, 1)));
/// b.token(Token::new(K::Ws, Span::new(1, 4)));
/// b.token(Token::new(K::Plus, Span::new(4, 5)));
/// b.token(Token::new(K::Num, Span::new(5, 6)));
/// b.finish_node();
/// let tree = b.finish()?;
///
/// let style = Rules::new()
///     .final_newline(false)
///     .token(TokenRule::new("+").around(Space::Single))
///     .compile(|name| (name == "+").then_some(K::Plus))?;
/// assert_eq!(format(&tree, source, &style, 80)?, "1 + 2");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn format<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    width: usize,
) -> Result<String, FormatError> {
    Ok(format_doc(tree, source, style, &[])?.render(width))
}

/// Formats like [`format()`], but leaves the whitespace around and inside the
/// `keep` regions exactly as written.
///
/// Pass the spans of lexical errors here (an LSP server has them as
/// diagnostics). Some tokens end where the source's whitespace happens to be:
/// an unterminated string usually ends at the line break, so removing that
/// line break would pull the next token into the string. The formatter cannot
/// tell such a token from a complete one, but every gap that touches a kept
/// region keeps its original whitespace, no separator is inserted there, and a
/// separator inside one is never removed, so the token stays as it was.
///
/// A gap touches a kept region when the region overlaps the token before the
/// gap, the gap itself, or the token after it; an empty region (a diagnostic
/// pointing between two tokens) keeps the gap it falls in. Regions may overlap
/// and come in any order.
///
/// Idempotence holds as long as the regions mark the same tokens when the
/// output is formatted again. Lexical errors do (they belong to a token);
/// parse errors such as "expected `;`" may point elsewhere once whitespace has
/// changed, so keeping their regions can shift between runs.
///
/// # Errors
///
/// As for [`format()`].
///
/// # Examples
///
/// ```
/// use fmt_lang::{format, format_keeping, NodeRule, Rules, Space, Trailing};
/// use lang_forge::Language;
///
/// let json = Language::from_lsf(
///     "[language]\nname = \"j\"\n[lexer]\nstrings = ['\"']\n[rules]\n\
///      list = \"'[' (STRING (',' STRING)*)? ']'\"\n",
/// )?;
/// let style = Rules::new()
///     .node(NodeRule::new("list").delimiters("[", "]", Space::None).separator(",", Space::Single, Trailing::Preserve))
///     .compile(|n| json.kind(n))?;
///
/// // `"open` is unterminated: the string ends at the line break.
/// let parse = json.parse("[\"open\n, \"b\"]");
/// let spans: Vec<_> = parse.diagnostics().iter().map(|d| d.primary().span()).collect();
///
/// let kept = format_keeping(parse.tree(), parse.source(), &style, 80, &spans)?;
/// assert_eq!(kept, "[\"open\n, \"b\"]\n");
///
/// // Without the error spans the break goes, and the comma joins the string.
/// let lost = format(parse.tree(), parse.source(), &style, 80)?;
/// assert_eq!(lost, "[\"open, \"b\"]\n");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn format_keeping<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    width: usize,
    keep: &[Span],
) -> Result<String, FormatError> {
    Ok(format_doc(tree, source, style, keep)?.render(width))
}

/// Builds the [`Doc`] that [`format_keeping`] (or, with no `keep` regions,
/// [`format()`]) renders, for callers that render it themselves: into an
/// existing buffer or writer, or at several widths.
///
/// # Errors
///
/// As for [`format()`].
///
/// # Examples
///
/// ```
/// use fmt_lang::syntax_lang::{Element, Node, Span, Token, TokenKind};
/// use fmt_lang::{format_doc, Style};
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// struct Word;
/// impl TokenKind for Word {}
///
/// let tree = Node::new(Word, vec![Element::Token(Token::new(Word, Span::new(0, 5)))]);
/// let doc = format_doc(&tree, "hello", &Style::default(), &[])?;
/// let mut out = String::new();
/// doc.render_into(80, &mut out)?;
/// assert_eq!(out, "hello\n");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn format_doc<K: TokenKind + Ord>(
    tree: &Node<K>,
    source: &str,
    style: &Style<K>,
    keep: &[Span],
) -> Result<Doc, FormatError> {
    let flat = flat::flatten(tree, source, style)?;
    Ok(walk::Walker::new(style, source, &flat, keep).run(&flat.events, flat.bom))
}

/// The default test for whether two tokens may be written touching, with no
/// whitespace between them: `left` is the earlier token's text, `right` the
/// later one's.
///
/// The formatter consults it only where a rule would remove whitespace the
/// source had; tokens that touched in the source may always touch. It answers
/// `false` when the touching characters could plausibly lex as one token:
/// two word characters (`a` `b`), two ASCII punctuation characters (`-` `-`,
/// `/` `/`), a quote next to a word character or another quote (`b` `"x"`,
/// `"a"` `"b"`), or a dot next to a digit (`1` `.5`). A quote next to other
/// punctuation may touch (`"k"` `:`). Brackets, parentheses, commas, and semicolons may touch
/// anything except an identical bracket or brace (`[[`, `]]`, `{{`, `}}` are
/// tokens in some languages).
///
/// It is a heuristic. A language that knows its own lexer supplies an exact
/// test with [`Style::with_touch`].
///
/// # Examples
///
/// ```
/// use fmt_lang::can_touch;
///
/// assert!(can_touch("f", "("));
/// assert!(can_touch("x", ","));
/// assert!(can_touch("a", "."));
/// assert!(!can_touch("let", "x"));
/// assert!(!can_touch("-", "-"));
/// assert!(!can_touch("[", "["));
/// ```
#[must_use]
pub fn can_touch(left: &str, right: &str) -> bool {
    let (Some(a), Some(b)) = (left.chars().next_back(), right.chars().next()) else {
        return true;
    };
    let fixed = |c: char| matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';');
    if fixed(a) || fixed(b) {
        return !(a == b && matches!(a, '[' | ']' | '{' | '}'));
    }
    let word = |c: char| c.is_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace());
    let quote = |c: char| matches!(c, '"' | '\'' | '`');
    if quote(a) || quote(b) {
        // A quote fuses with a word (a string prefix or suffix: `b"x"`, `"x"s`)
        // or another quote (`""` is an escape in some languages), but other
        // punctuation cannot reach into a string.
        return !(word(a) || word(b) || (quote(a) && quote(b)));
    }
    if word(a) && word(b) {
        return false;
    }
    if a.is_ascii_punctuation() && b.is_ascii_punctuation() {
        return false;
    }
    !((a == '.' && b.is_ascii_digit()) || (a.is_ascii_digit() && b == '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_touch_rules() {
        assert!(can_touch("", "x"));
        assert!(can_touch("x", ""));
        assert!(can_touch(")", ";"));
        assert!(can_touch("(", "("));
        assert!(!can_touch("}", "}"));
        assert!(!can_touch("a", "b"));
        assert!(!can_touch("a", "1"));
        assert!(!can_touch("é", "x"));
        assert!(!can_touch("+", "="));
        assert!(!can_touch("b", "\"s\""));
        assert!(!can_touch("'c'", "x"));
        assert!(!can_touch("\"a\"", "\"b\""));
        assert!(can_touch("\"k\"", ":"));
        assert!(can_touch("=", "\"v\""));
        assert!(!can_touch("1", ".5"));
        assert!(!can_touch(".", "5"));
        assert!(can_touch("-", "x"));
        assert!(can_touch("x", "+"));
    }
}
