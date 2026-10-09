//! The rule model: formatting described as plain data, keyed by kind *names*.
//!
//! A [`Rules`] value is what a sketch's `[tooling.format]` section describes:
//! per-node layout ([`NodeRule`]), per-token spacing ([`TokenRule`]), and a few
//! file-wide settings. Kinds are named by text (`"object"`, `","`, `"ERROR"`),
//! the way a sketch names them, and [`Rules::compile`] resolves the names
//! against a language once, producing the [`Style`](crate::Style) the
//! formatter runs on.
//!
//! Anything the rules do not mention keeps its original whitespace.

use alloc::string::String;
use alloc::vec::Vec;

/// What goes between two neighbouring pieces of output.
///
/// The five values form a small lattice. When several rules speak about the
/// same gap, the formatter takes the most generous answer: a gap gets a space if
/// any rule asks for one, may break if any rule allows it, and always breaks if
/// any rule demands it. So [`SoftLine`](Space::SoftLine) combined with
/// [`Single`](Space::Single) is [`Line`](Space::Line), and anything combined
/// with [`Hard`](Space::Hard) is `Hard`.
///
/// | Value | When the enclosing group fits | When it breaks |
/// |---|---|---|
/// | `None` | nothing | nothing |
/// | `Single` | one space | one space |
/// | `SoftLine` | nothing | a line break |
/// | `Line` | one space | a line break |
/// | `Hard` | a line break | a line break |
///
/// A gap that resolves to `Hard` keeps up to the configured number of blank
/// lines the source had there ([`Rules::max_blank_lines`],
/// [`NodeRule::blank_lines`]).
///
/// # Examples
///
/// ```
/// use fmt_lang::{Rules, Space, TokenRule};
///
/// // No space before a comma, one after it.
/// let rules = Rules::new().token(TokenRule::new(",").before(Space::None).after(Space::Single));
/// # let _ = rules;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Space {
    /// Nothing: the neighbours touch.
    None,
    /// Exactly one space.
    Single,
    /// Nothing, or a line break when the enclosing group does not fit.
    SoftLine,
    /// One space, or a line break when the enclosing group does not fit.
    Line,
    /// Always a line break.
    Hard,
}

/// How a node indents its contents.
///
/// # Examples
///
/// ```
/// use fmt_lang::{Indent, NodeRule, Space};
///
/// // A block: `{`, an indented body, then `}` back at the block's level.
/// let block = NodeRule::new("block").delimiters("{", "}", Space::Hard).indent(Indent::Block);
/// # let _ = block;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Indent {
    /// No indentation of its own.
    #[default]
    None,
    /// Indent the body between the node's delimiters ([`NodeRule::delimiters`]):
    /// every line break after the opening delimiter and before the closing one.
    /// The closing delimiter itself returns to the node's level. Without
    /// delimiters present in the tree, nothing is indented.
    Block,
    /// Indent every line break inside the node (a hanging indent, as for a long
    /// binary expression that wraps).
    Hanging,
}

/// What to do with a separator after the last element of a delimited list.
///
/// Policies other than [`Preserve`](Trailing::Preserve) change the token
/// stream (they add or remove one separator token), so they apply only to
/// lists that are well formed: delimiters present at both ends, elements and
/// separators strictly alternating, and no error node among the elements.
/// Anything else is left as written.
///
/// # Examples
///
/// ```
/// use fmt_lang::{NodeRule, Space, Trailing};
///
/// let array = NodeRule::new("array")
///     .delimiters("[", "]", Space::SoftLine)
///     .separator(",", Space::Line, Trailing::Never);
/// # let _ = array;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Trailing {
    /// Keep a trailing separator if there is one; never add one.
    #[default]
    Preserve,
    /// Ensure a non-empty list ends with a separator.
    Always,
    /// Remove a trailing separator.
    Never,
}

/// Spacing rules for one token kind (or for every token), on either side.
///
/// A token rule lives either at the top level of [`Rules`] (a default that
/// applies wherever the token appears) or inside a [`NodeRule`] (applies only
/// to tokens that are direct children of that node, and wins over the
/// top-level default). On each side the most specific rule wins: a node's rule
/// for this exact kind, then the node's rule for any token, then the top-level
/// rule for this kind, then the top-level rule for any token.
///
/// # Examples
///
/// ```
/// use fmt_lang::{Space, TokenRule};
///
/// let plus = TokenRule::new("+").around(Space::Single);
/// let open_paren = TokenRule::new("(").after(Space::None);
/// // Every direct child token of a node (used inside a `NodeRule`).
/// let tight = TokenRule::any().before(Space::None).after(Space::None);
/// # let _ = (plus, open_paren, tight);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TokenRule {
    pub(crate) kind: Option<String>,
    pub(crate) before: Option<Space>,
    pub(crate) after: Option<Space>,
    pub(crate) optional: bool,
}

impl TokenRule {
    /// A rule for tokens of the kind named `kind`, with no spacing set yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Space, TokenRule};
    ///
    /// let semi = TokenRule::new(";").before(Space::None);
    /// # let _ = semi;
    /// ```
    #[must_use]
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: Some(kind.into()),
            before: None,
            after: None,
            optional: false,
        }
    }

    /// A rule for every token, whatever its kind. Inside a [`NodeRule`] it
    /// covers every direct child token of that node.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space, TokenRule};
    ///
    /// // A prefix operator hugs its operand: `-x`, not `- x`.
    /// let prefix = NodeRule::new("prefix").token(TokenRule::any().before(Space::None).after(Space::None));
    /// # let _ = prefix;
    /// ```
    #[must_use]
    pub fn any() -> Self {
        Self {
            kind: None,
            before: None,
            after: None,
            optional: false,
        }
    }

    /// Spacing in the gap before the token.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Space, TokenRule};
    ///
    /// let close = TokenRule::new(")").before(Space::None);
    /// # let _ = close;
    /// ```
    #[must_use]
    pub fn before(mut self, space: Space) -> Self {
        self.before = Some(space);
        self
    }

    /// Spacing in the gap after the token.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Space, TokenRule};
    ///
    /// let comma = TokenRule::new(",").after(Space::Single);
    /// # let _ = comma;
    /// ```
    #[must_use]
    pub fn after(mut self, space: Space) -> Self {
        self.after = Some(space);
        self
    }

    /// The same spacing on both sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Space, TokenRule};
    ///
    /// let assign = TokenRule::new("=").around(Space::Single);
    /// # let _ = assign;
    /// ```
    #[must_use]
    pub fn around(self, space: Space) -> Self {
        self.before(space).after(space)
    }

    /// Marks the rule optional: if the language has no kind by this name,
    /// [`Rules::compile`] drops the rule instead of reporting
    /// [`RuleError::UnknownKind`](crate::RuleError::UnknownKind). Meant for
    /// presets shared across languages, such as [`Rules::conventional`].
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Rules, Space, TokenRule};
    ///
    /// let rules = Rules::new().token(TokenRule::new("=>").around(Space::Single).optional());
    /// // The language below has no `=>`; the rule is dropped, not an error.
    /// let style = rules.compile(|name| (name == "+").then_some(1u8))?;
    /// # let _ = style;
    /// # Ok::<(), fmt_lang::RuleError>(())
    /// ```
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// A list delimiter pair and the spacing just inside it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DelimitersRule {
    pub(crate) open: String,
    pub(crate) close: String,
    pub(crate) inner: Space,
}

/// A list separator and its trailing policy.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SeparatorRule {
    pub(crate) kind: String,
    pub(crate) text: Option<String>,
    pub(crate) before: Space,
    pub(crate) after: Space,
    pub(crate) trailing: Trailing,
}

/// Layout rules for one node kind.
///
/// A node rule can make the node a *group* (laid out on one line if it fits,
/// broken at its line opportunities otherwise), *indent* its contents, ask for
/// spacing *before* and *after* the node, name the *delimiters* and
/// *separator* of a list, cap the *blank lines* kept inside it, and carry
/// *token rules* for its direct child tokens.
///
/// # Examples
///
/// ```
/// use fmt_lang::{Indent, NodeRule, Space, Trailing};
///
/// // A JSON object: `{ "a": 1, "b": 2 }` when it fits, one member per line
/// // (indented) when it does not, and no trailing comma.
/// let object = NodeRule::new("object")
///     .group()
///     .indent(Indent::Block)
///     .delimiters("{", "}", Space::Line)
///     .separator(",", Space::Line, Trailing::Never);
/// # let _ = object;
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeRule {
    pub(crate) kind: String,
    pub(crate) group: bool,
    pub(crate) indent: Indent,
    pub(crate) before: Option<Space>,
    pub(crate) after: Option<Space>,
    pub(crate) delimiters: Option<DelimitersRule>,
    pub(crate) empty: Space,
    pub(crate) separator: Option<SeparatorRule>,
    pub(crate) blank_lines: Option<u8>,
    pub(crate) tokens: Vec<TokenRule>,
    pub(crate) optional: bool,
}

impl NodeRule {
    /// A rule for nodes of the kind named `kind`, with nothing set yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::NodeRule;
    ///
    /// let stmt = NodeRule::new("stmt");
    /// # let _ = stmt;
    /// ```
    #[must_use]
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            group: false,
            indent: Indent::None,
            before: None,
            after: None,
            delimiters: None,
            empty: Space::None,
            separator: None,
            blank_lines: None,
            tokens: Vec::new(),
            optional: false,
        }
    }

    /// Makes the node a group: its [`Line`](Space::Line) and
    /// [`SoftLine`](Space::SoftLine) gaps all stay flat if the whole node fits
    /// in the remaining width, and all break otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::NodeRule;
    ///
    /// let call_args = NodeRule::new("args").group();
    /// # let _ = call_args;
    /// ```
    #[must_use]
    pub fn group(mut self) -> Self {
        self.group = true;
        self
    }

    /// Sets how the node indents its contents (by [`Rules::indent`] columns).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Indent, NodeRule};
    ///
    /// let binary = NodeRule::new("binary").indent(Indent::Hanging);
    /// # let _ = binary;
    /// ```
    #[must_use]
    pub fn indent(mut self, indent: Indent) -> Self {
        self.indent = indent;
        self
    }

    /// Spacing in the gap before the node (before its first token).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space};
    ///
    /// // Every statement starts a new line.
    /// let stmt = NodeRule::new("stmt").before(Space::Hard);
    /// # let _ = stmt;
    /// ```
    #[must_use]
    pub fn before(mut self, space: Space) -> Self {
        self.before = Some(space);
        self
    }

    /// Spacing in the gap after the node (after its last token).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space};
    ///
    /// let stmt = NodeRule::new("stmt").after(Space::Hard);
    /// # let _ = stmt;
    /// ```
    #[must_use]
    pub fn after(mut self, space: Space) -> Self {
        self.after = Some(space);
        self
    }

    /// Names the node's delimiter tokens and the spacing just inside them (after
    /// `open` and before `close`). An empty body (`open` immediately followed by
    /// `close`) gets [`Space::None`] unless [`empty`](NodeRule::empty) says
    /// otherwise.
    ///
    /// A delimiter is recognised only as the node's first (for `open`) or last
    /// (for `close`) significant direct child, so the same kinds may appear
    /// elsewhere inside the node without confusion.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space};
    ///
    /// // `[1, 2]` flat, or one element per line when broken.
    /// let array = NodeRule::new("array").group().delimiters("[", "]", Space::SoftLine);
    /// # let _ = array;
    /// ```
    #[must_use]
    pub fn delimiters(
        mut self,
        open: impl Into<String>,
        close: impl Into<String>,
        inner: Space,
    ) -> Self {
        self.delimiters = Some(DelimitersRule {
            open: open.into(),
            close: close.into(),
            inner,
        });
        self
    }

    /// Spacing between the delimiters when the body is empty. Has no effect
    /// unless [`delimiters`](NodeRule::delimiters) is set (before or after this
    /// call).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space};
    ///
    /// // `{ }` rather than `{}` for an empty block.
    /// let block = NodeRule::new("block").delimiters("{", "}", Space::Hard).empty(Space::Single);
    /// # let _ = block;
    /// ```
    #[must_use]
    pub fn empty(mut self, space: Space) -> Self {
        self.empty = space;
        self
    }

    /// Names the list separator among the node's direct child tokens, the
    /// spacing after it (before it is [`Space::None`]), and what to do with a
    /// trailing one.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space, Trailing};
    ///
    /// let args = NodeRule::new("args").separator(",", Space::Line, Trailing::Preserve);
    /// # let _ = args;
    /// ```
    #[must_use]
    pub fn separator(mut self, kind: impl Into<String>, after: Space, trailing: Trailing) -> Self {
        self.separator = Some(SeparatorRule {
            kind: kind.into(),
            text: None,
            before: Space::None,
            after,
            trailing,
        });
        self
    }

    /// The text written when [`Trailing::Always`] adds a separator. Defaults to
    /// the separator's kind name, which is its text in languages (such as
    /// those forged by lang-forge) that name symbol tokens by their text. Has
    /// no effect without a [`separator`](NodeRule::separator).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space, Trailing};
    ///
    /// let list = NodeRule::new("list")
    ///     .delimiters("(", ")", Space::SoftLine)
    ///     .separator("COMMA", Space::Line, Trailing::Always)
    ///     .separator_text(",");
    /// # let _ = list;
    /// ```
    #[must_use]
    pub fn separator_text(mut self, text: impl Into<String>) -> Self {
        if let Some(sep) = &mut self.separator {
            sep.text = Some(text.into());
        }
        self
    }

    /// Caps the blank lines kept between lines inside this node (overrides
    /// [`Rules::max_blank_lines`] here).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::NodeRule;
    ///
    /// // No blank lines inside an argument list.
    /// let args = NodeRule::new("args").blank_lines(0);
    /// # let _ = args;
    /// ```
    #[must_use]
    pub fn blank_lines(mut self, max: u8) -> Self {
        self.blank_lines = Some(max);
        self
    }

    /// Adds a token rule for the node's direct child tokens. It wins over a
    /// top-level [`Rules::token`] rule for the same side.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Space, TokenRule};
    ///
    /// let member = NodeRule::new("member").token(TokenRule::new(":").before(Space::None).after(Space::Single));
    /// # let _ = member;
    /// ```
    #[must_use]
    pub fn token(mut self, rule: TokenRule) -> Self {
        self.tokens.push(rule);
        self
    }

    /// Marks the rule optional: dropped by [`Rules::compile`] if the language
    /// lacks any kind it names, instead of an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Rules, Space};
    ///
    /// let rules = Rules::new().node(NodeRule::new("block").before(Space::Hard).optional());
    /// let style = rules.compile(|_| None::<u8>)?; // no `block` here: dropped
    /// # let _ = style;
    /// # Ok::<(), fmt_lang::RuleError>(())
    /// ```
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// A language's formatting rules, as data.
///
/// Built with chained calls, then [`compile`](Rules::compile)d against the
/// language's kinds. An empty `Rules` formats nothing: every gap keeps its
/// original whitespace (trailing spaces at line ends and blank space at the
/// start and end of the file are still trimmed, and a final newline is added).
///
/// # Examples
///
/// ```
/// use fmt_lang::{Indent, NodeRule, Rules, Space, TokenRule, Trailing};
///
/// let rules = Rules::new()
///     .indent(2)
///     .verbatim("ERROR")
///     .token(TokenRule::new(":").before(Space::None).after(Space::Single))
///     .node(
///         NodeRule::new("array")
///             .group()
///             .indent(Indent::Block)
///             .delimiters("[", "]", Space::SoftLine)
///             .separator(",", Space::Line, Trailing::Never),
///     );
/// # let _ = rules;
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rules {
    pub(crate) indent: u8,
    pub(crate) max_indent: u16,
    pub(crate) max_blank_lines: u8,
    pub(crate) final_newline: bool,
    pub(crate) verbatim: Vec<String>,
    pub(crate) tokens: Vec<TokenRule>,
    pub(crate) nodes: Vec<NodeRule>,
}

/// The widest indentation step [`Rules::indent`] accepts.
pub const MAX_INDENT_STEP: u8 = 16;

impl Default for Rules {
    fn default() -> Self {
        Self::new()
    }
}

impl Rules {
    /// No rules: indentation step 4, at most one blank line kept, indentation
    /// capped at 120 columns, a final newline.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new();
    /// assert_eq!(rules, Rules::default());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            indent: 4,
            max_indent: 120,
            max_blank_lines: 1,
            final_newline: true,
            verbatim: Vec::new(),
            tokens: Vec::new(),
            nodes: Vec::new(),
        }
    }

    /// Conventional token spacing shared by most C-family languages, every
    /// rule [`optional`](TokenRule::optional) so the preset compiles against
    /// any language:
    ///
    /// - nothing before `,` `;` `)` `]`, one space after `,` and `;`;
    /// - nothing after `(` `[`;
    /// - one space around `=` `==` `!=` `<=` `>=` `+=` `-=` `*=` `/=` `%=`
    ///   `&&` `||` `=>` `->`.
    ///
    /// Operators that are also prefix operators in common languages (`-`,
    /// `+`, `*`, `&`, `!`) and the angle brackets (`<`, `>`, generics in many
    /// languages) are left out on purpose: spacing them needs node context,
    /// which a [`NodeRule`] supplies.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Rules, Space};
    ///
    /// let rules = Rules::conventional().node(NodeRule::new("stmt").before(Space::Hard));
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn conventional() -> Self {
        let mut rules = Self::new();
        for kind in [",", ";"] {
            rules = rules.token(
                TokenRule::new(kind)
                    .before(Space::None)
                    .after(Space::Single)
                    .optional(),
            );
        }
        for kind in [")", "]"] {
            rules = rules.token(TokenRule::new(kind).before(Space::None).optional());
        }
        for kind in ["(", "["] {
            rules = rules.token(TokenRule::new(kind).after(Space::None).optional());
        }
        for kind in [
            "=", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "%=", "&&", "||", "=>", "->",
        ] {
            rules = rules.token(TokenRule::new(kind).around(Space::Single).optional());
        }
        rules
    }

    /// Columns per indentation level (default 4, at most
    /// [`MAX_INDENT_STEP`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new().indent(2);
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn indent(mut self, columns: u8) -> Self {
        self.indent = columns;
        self
    }

    /// The deepest indentation, in columns, the formatter will produce
    /// (default 120). Nesting past it stops adding indentation. This is the
    /// output budget against hostile input: without it, a file nested a
    /// million levels deep would format to terabytes of spaces.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new().max_indent(60);
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn max_indent(mut self, columns: u16) -> Self {
        self.max_indent = columns;
        self
    }

    /// The most blank lines kept wherever a [`Space::Hard`] gap had blank
    /// lines in the source (default 1). Gaps that keep their original
    /// whitespace are not capped.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new().max_blank_lines(2);
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn max_blank_lines(mut self, max: u8) -> Self {
        self.max_blank_lines = max;
        self
    }

    /// Whether non-empty output ends with a line break (default `true`).
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new().final_newline(false);
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn final_newline(mut self, yes: bool) -> Self {
        self.final_newline = yes;
        self
    }

    /// Names a node kind whose contents are always written exactly as in the
    /// source, such as a parser's error node (`"ERROR"` in languages forged by
    /// lang-forge). Only the whitespace around such a node is formatted.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::Rules;
    ///
    /// let rules = Rules::new().verbatim("ERROR").verbatim("raw_block");
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn verbatim(mut self, kind: impl Into<String>) -> Self {
        self.verbatim.push(kind.into());
        self
    }

    /// Adds a top-level token rule: a default for that token kind (or, with
    /// [`TokenRule::any`], for every token) wherever it appears.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{Rules, Space, TokenRule};
    ///
    /// let rules = Rules::new().token(TokenRule::new("+").around(Space::Single));
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn token(mut self, rule: TokenRule) -> Self {
        self.tokens.push(rule);
        self
    }

    /// Adds a node rule.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Rules, Space};
    ///
    /// let rules = Rules::new().node(NodeRule::new("stmt").before(Space::Hard));
    /// # let _ = rules;
    /// ```
    #[must_use]
    pub fn node(mut self, rule: NodeRule) -> Self {
        self.nodes.push(rule);
        self
    }
}
