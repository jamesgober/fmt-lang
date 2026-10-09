//! Error types: [`RuleError`] for rules that cannot be compiled, and
//! [`FormatError`] for trees that do not match their source.

use alloc::string::String;
use core::fmt;

/// Why [`Rules::compile`](crate::Rules::compile) refused a set of rules.
///
/// Every variant names the offending kind, so the message can point at the
/// right line of a sketch.
///
/// # Examples
///
/// ```
/// use fmt_lang::{NodeRule, RuleError, Rules};
///
/// let err = Rules::new().node(NodeRule::new("objekt")).compile(|_| None::<u8>).unwrap_err();
/// assert_eq!(err, RuleError::UnknownKind { name: "objekt".into() });
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleError {
    /// A rule names a kind the language does not have. Fix the spelling, or
    /// mark the rule [`optional`](crate::TokenRule::optional) if it comes from
    /// a preset meant for several languages.
    UnknownKind {
        /// The name as written in the rule.
        name: String,
    },
    /// Two node rules name the same kind. Merge them into one rule.
    DuplicateNode {
        /// The kind both rules name.
        name: String,
    },
    /// The indentation step is wider than
    /// [`MAX_INDENT_STEP`](crate::MAX_INDENT_STEP) columns.
    IndentTooWide {
        /// The step that was asked for.
        columns: u8,
    },
    /// A node's separator is also one of its delimiters, so the formatter
    /// cannot tell the two roles apart. Use distinct kinds.
    SeparatorIsDelimiter {
        /// The node rule.
        node: String,
    },
    /// A node rule sets a trailing-separator policy other than
    /// [`Trailing::Preserve`](crate::Trailing::Preserve) but no delimiters.
    /// The policy needs the closing delimiter to know where the list ends;
    /// add [`delimiters`](crate::NodeRule::delimiters) or keep `Preserve`.
    TrailingNeedsDelimiters {
        /// The node rule.
        node: String,
    },
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind { name } => {
                write!(f, "format rule names unknown kind `{name}`")
            }
            Self::DuplicateNode { name } => {
                write!(f, "two format rules for node kind `{name}`; merge them")
            }
            Self::IndentTooWide { columns } => write!(
                f,
                "indentation step of {columns} columns is wider than the maximum of {}",
                crate::MAX_INDENT_STEP
            ),
            Self::SeparatorIsDelimiter { node } => write!(
                f,
                "the rule for `{node}` uses the same kind as separator and delimiter"
            ),
            Self::TrailingNeedsDelimiters { node } => write!(
                f,
                "the rule for `{node}` sets a trailing-separator policy without delimiters"
            ),
        }
    }
}

impl core::error::Error for RuleError {}

/// Why [`format()`](crate::format) could not format a tree.
///
/// The formatter accepts any tree shape, error nodes included; it refuses
/// only a tree whose tokens do not describe `source`, because formatting such a
/// tree would lose or invent text.
///
/// # Examples
///
/// ```
/// use fmt_lang::syntax_lang::{Element, Node, Span, Token, TokenKind};
/// use fmt_lang::{format, FormatError, Style};
///
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
/// struct Word;
/// impl TokenKind for Word {}
///
/// // A token reaching past the end of the source.
/// let tree = Node::new(Word, vec![Element::Token(Token::new(Word, Span::new(0, 9)))]);
/// let err = format(&tree, "short", &Style::default(), 80).unwrap_err();
/// assert_eq!(err, FormatError::OutOfBounds { start: 0, end: 9, len: 5 });
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatError {
    /// A token's span reaches past the end of the source. The tree was built
    /// from different text; parse `source` again.
    OutOfBounds {
        /// Start of the token's span.
        start: u32,
        /// End of the token's span.
        end: u32,
        /// Length of the source in bytes.
        len: usize,
    },
    /// A token boundary falls inside a multi-byte character. The tree was
    /// built from different text; parse `source` again.
    NotCharBoundary {
        /// The offending byte offset.
        offset: u32,
    },
    /// Consecutive tokens leave a gap or overlap, so the tree is not lossless:
    /// text between them would be lost or written twice. Every byte of the
    /// tree's range must belong to exactly one token (trivia included).
    NotContiguous {
        /// Where the next token should have started (the previous token's end).
        expected: u32,
        /// Where it actually started.
        found: u32,
    },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { start, end, len } => write!(
                f,
                "token span {start}..{end} lies outside the {len}-byte source"
            ),
            Self::NotCharBoundary { offset } => write!(
                f,
                "token boundary at byte {offset} is inside a multi-byte character"
            ),
            Self::NotContiguous { expected, found } => write!(
                f,
                "tree is not lossless: a token starts at byte {found}, but the previous one ended at byte {expected}"
            ),
        }
    }
}

impl core::error::Error for FormatError {}
