//! [`Style`]: rules compiled against one language's kinds, ready to format
//! with.
//!
//! Compiling turns every kind name into the language's own kind value once,
//! and sorts the rules by kind, so the formatter's per-token lookups are a
//! binary search over a small, dense array rather than string comparisons.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::RuleError;
use crate::rules::{Indent, MAX_INDENT_STEP, NodeRule, Rules, Space, TokenRule, Trailing};

/// A gap's spacing as two independent facts: is there a space when flat, and
/// how readily does it break. Joining two gaps takes the larger of each, which
/// is exactly the "most generous rule wins" semantics of [`Space`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Sp {
    /// A space when laid out flat.
    pub(crate) space: bool,
    /// 0 never breaks, 1 breaks with its group, 2 always breaks.
    pub(crate) brk: u8,
}

impl Sp {
    pub(crate) const NONE: Sp = Sp {
        space: false,
        brk: 0,
    };
    pub(crate) const SINGLE: Sp = Sp {
        space: true,
        brk: 0,
    };
    pub(crate) const HARD: Sp = Sp {
        space: false,
        brk: 2,
    };

    #[inline]
    pub(crate) fn join(self, other: Sp) -> Sp {
        Sp {
            space: self.space | other.space,
            brk: self.brk.max(other.brk),
        }
    }

    #[inline]
    pub(crate) fn is_hard(self) -> bool {
        self.brk >= 2
    }
}

impl From<Space> for Sp {
    #[inline]
    fn from(space: Space) -> Self {
        match space {
            Space::None => Sp::NONE,
            Space::Single => Sp::SINGLE,
            Space::SoftLine => Sp {
                space: false,
                brk: 1,
            },
            Space::Line => Sp {
                space: true,
                brk: 1,
            },
            Space::Hard => Sp::HARD,
        }
    }
}

/// Joins an optional contribution into an optional accumulator: `None` means
/// "no rule spoke", which is different from a rule asking for nothing.
#[inline]
pub(crate) fn join_opt(acc: Option<Sp>, add: Option<Sp>) -> Option<Sp> {
    match (acc, add) {
        (Some(a), Some(b)) => Some(a.join(b)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// One side's spacing for a token kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sides {
    pub(crate) before: Option<Sp>,
    pub(crate) after: Option<Sp>,
}

impl Sides {
    fn merge(self, later: Sides) -> Sides {
        Sides {
            before: later.before.or(self.before),
            after: later.after.or(self.after),
        }
    }
}

/// Token rules in one place (top level or one node), sorted by kind.
#[derive(Clone, Debug)]
pub(crate) struct TokenTable<K> {
    exact: Vec<(K, Sides)>,
    any: Sides,
}

impl<K> Default for TokenTable<K> {
    fn default() -> Self {
        Self {
            exact: Vec::new(),
            any: Sides::default(),
        }
    }
}

impl<K: Ord> TokenTable<K> {
    #[inline]
    fn sides(&self, kind: &K) -> Sides {
        match self.exact.binary_search_by(|(k, _)| k.cmp(kind)) {
            Ok(i) => self.exact.get(i).map_or(Sides::default(), |(_, s)| *s),
            Err(_) => Sides::default(),
        }
    }

    #[inline]
    fn before(&self, kind: &K) -> Option<Sp> {
        self.sides(kind).before.or(self.any.before)
    }

    #[inline]
    fn after(&self, kind: &K) -> Option<Sp> {
        self.sides(kind).after.or(self.any.after)
    }
}

/// A list's delimiters, compiled.
#[derive(Clone, Debug)]
pub(crate) struct Delims<K> {
    pub(crate) open: K,
    pub(crate) close: K,
    pub(crate) inner: Sp,
}

/// A list's separator, compiled.
#[derive(Clone, Debug)]
pub(crate) struct Sep<K> {
    pub(crate) kind: K,
    pub(crate) text: String,
    pub(crate) before: Sp,
    pub(crate) after: Sp,
    pub(crate) trailing: Trailing,
}

/// A node rule, compiled.
#[derive(Clone, Debug)]
pub(crate) struct NodeStyle<K> {
    pub(crate) kind: K,
    pub(crate) group: bool,
    pub(crate) indent: Indent,
    pub(crate) before: Option<Sp>,
    pub(crate) after: Option<Sp>,
    pub(crate) delims: Option<Delims<K>>,
    pub(crate) empty: Sp,
    pub(crate) sep: Option<Sep<K>>,
    pub(crate) blank_lines: Option<u8>,
    tokens: TokenTable<K>,
}

impl<K: Ord> NodeStyle<K> {
    /// Whether nodes of this kind need their own document (to group or
    /// indent); nodes that do not write straight into their parent's.
    #[inline]
    pub(crate) fn owns_doc(&self) -> bool {
        self.group || self.indent != Indent::None
    }
}

/// Formatting rules compiled against one language, ready for
/// [`format()`](crate::format).
///
/// Build one with [`Rules::compile`]. [`Style::default`] is the empty style:
/// every gap keeps its original whitespace.
///
/// # Examples
///
/// ```
/// use fmt_lang::{Rules, Space, Style, TokenRule};
///
/// // Kinds here are plain strings; a real language resolves names to its own
/// // kind type (for lang-forge: `|name| lang.kind(name)`).
/// let style: Style<&str> = Rules::new()
///     .token(TokenRule::new("+").around(Space::Single))
///     .compile(|name| ["+", "num"].into_iter().find(|k| *k == name))?;
/// # let _ = style;
/// # Ok::<(), fmt_lang::RuleError>(())
/// ```
#[derive(Clone, Debug)]
pub struct Style<K> {
    pub(crate) indent: u32,
    pub(crate) max_indent: u32,
    pub(crate) blank_lines: u8,
    pub(crate) final_newline: bool,
    pub(crate) touch: fn(&str, &str) -> bool,
    verbatim: Vec<K>,
    tokens: TokenTable<K>,
    nodes: Vec<NodeStyle<K>>,
}

impl<K> Default for Style<K> {
    fn default() -> Self {
        let rules = Rules::new();
        Self {
            indent: u32::from(rules.indent),
            max_indent: u32::from(rules.max_indent),
            blank_lines: rules.max_blank_lines,
            final_newline: rules.final_newline,
            touch: crate::can_touch,
            verbatim: Vec::new(),
            tokens: TokenTable::default(),
            nodes: Vec::new(),
        }
    }
}

impl<K> Style<K> {
    /// Replaces the test that decides whether two tokens may be written with
    /// nothing between them (default: [`can_touch`](crate::can_touch)).
    ///
    /// The formatter calls it only where a rule would remove whitespace the
    /// source had between two tokens; if it answers `false`, one space is kept.
    /// A language whose lexer would read the touching texts differently (for
    /// example one with a `-1` literal, where `-` and `1` must not touch)
    /// supplies its own test here.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{can_touch, Style};
    ///
    /// fn no_minus_digit(left: &str, right: &str) -> bool {
    ///     let digit = right.starts_with(|c: char| c.is_ascii_digit());
    ///     !(left.ends_with('-') && digit) && can_touch(left, right)
    /// }
    /// let style = Style::<u8>::default().with_touch(no_minus_digit);
    /// # let _ = style;
    /// ```
    #[must_use]
    pub fn with_touch(mut self, touch: fn(&str, &str) -> bool) -> Self {
        self.touch = touch;
        self
    }
}

impl<K: Ord> Style<K> {
    /// The index of the rule for node kind `kind`.
    #[inline]
    pub(crate) fn node_index(&self, kind: &K) -> Option<u32> {
        self.nodes
            .binary_search_by(|n| n.kind.cmp(kind))
            .ok()
            .and_then(|i| u32::try_from(i).ok())
    }

    #[inline]
    pub(crate) fn node(&self, index: Option<u32>) -> Option<&NodeStyle<K>> {
        index.and_then(|i| self.nodes.get(i as usize))
    }

    #[inline]
    pub(crate) fn is_verbatim(&self, kind: &K) -> bool {
        self.verbatim.binary_search(kind).is_ok()
    }

    /// Spacing a token asks for after itself, given its parent's rule: the
    /// parent's separator role first, then the most specific token rule.
    pub(crate) fn token_after(&self, parent: Option<&NodeStyle<K>>, kind: &K) -> Option<Sp> {
        if let Some(p) = parent {
            if let Some(sep) = &p.sep {
                if sep.kind == *kind {
                    return Some(sep.after);
                }
            }
            if let Some(sp) = p.tokens.after(kind) {
                return Some(sp);
            }
        }
        self.tokens.after(kind)
    }

    /// Spacing a token asks for before itself, given its parent's rule.
    pub(crate) fn token_before(&self, parent: Option<&NodeStyle<K>>, kind: &K) -> Option<Sp> {
        if let Some(p) = parent {
            if let Some(sep) = &p.sep {
                if sep.kind == *kind {
                    return Some(sep.before);
                }
            }
            if let Some(sp) = p.tokens.before(kind) {
                return Some(sp);
            }
        }
        self.tokens.before(kind)
    }
}

impl Rules {
    /// Resolves every kind name with `resolve` and compiles the rules into a
    /// [`Style`] for that language.
    ///
    /// `resolve` maps a name to the language's kind, or `None` if the language
    /// has no such kind. For a language forged by lang-forge it is
    /// `|name| lang.kind(name)`.
    ///
    /// Later top-level token rules for the same kind refine earlier ones (a
    /// side set later wins), so a preset such as [`Rules::conventional`] can be
    /// adjusted by adding rules after it. The same holds for token rules within
    /// one node rule.
    ///
    /// # Errors
    ///
    /// - [`RuleError::UnknownKind`] if a non-optional rule names a kind
    ///   `resolve` does not know.
    /// - [`RuleError::DuplicateNode`] if two node rules name the same kind.
    /// - [`RuleError::IndentTooWide`] if [`Rules::indent`] exceeds
    ///   [`MAX_INDENT_STEP`](crate::MAX_INDENT_STEP).
    /// - [`RuleError::SeparatorIsDelimiter`] and
    ///   [`RuleError::TrailingNeedsDelimiters`] for inconsistent list rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use fmt_lang::{NodeRule, Rules, Space};
    ///
    /// #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    /// enum Kind { Stmt, Semi }
    ///
    /// let style = Rules::new()
    ///     .node(NodeRule::new("stmt").before(Space::Hard))
    ///     .compile(|name| match name {
    ///         "stmt" => Some(Kind::Stmt),
    ///         ";" => Some(Kind::Semi),
    ///         _ => None,
    ///     })?;
    /// # let _ = style;
    /// # Ok::<(), fmt_lang::RuleError>(())
    /// ```
    pub fn compile<K: Ord + Clone>(
        &self,
        mut resolve: impl FnMut(&str) -> Option<K>,
    ) -> Result<Style<K>, RuleError> {
        if self.indent > MAX_INDENT_STEP {
            return Err(RuleError::IndentTooWide {
                columns: self.indent,
            });
        }
        let mut verbatim = Vec::with_capacity(self.verbatim.len());
        for name in &self.verbatim {
            verbatim.push(required(&mut resolve, name)?);
        }
        verbatim.sort();
        verbatim.dedup();

        let tokens = compile_tokens(&self.tokens, &mut resolve)?;

        let mut nodes: Vec<(NodeStyle<K>, &str)> = Vec::with_capacity(self.nodes.len());
        for rule in &self.nodes {
            if let Some(node) = compile_node(rule, &mut resolve)? {
                nodes.push((node, rule.kind.as_str()));
            }
        }
        nodes.sort_by(|a, b| a.0.kind.cmp(&b.0.kind));
        for pair in nodes.windows(2) {
            if let [a, b] = pair {
                if a.0.kind == b.0.kind {
                    return Err(RuleError::DuplicateNode { name: b.1.into() });
                }
            }
        }

        Ok(Style {
            indent: u32::from(self.indent),
            max_indent: u32::from(self.max_indent),
            blank_lines: self.max_blank_lines,
            final_newline: self.final_newline,
            touch: crate::can_touch,
            verbatim,
            tokens,
            nodes: nodes.into_iter().map(|(n, _)| n).collect(),
        })
    }
}

fn required<K>(resolve: &mut impl FnMut(&str) -> Option<K>, name: &str) -> Result<K, RuleError> {
    resolve(name).ok_or_else(|| RuleError::UnknownKind { name: name.into() })
}

/// Resolves a name, honouring `optional`: `Ok(None)` means "drop the rule".
fn resolve_in<K>(
    resolve: &mut impl FnMut(&str) -> Option<K>,
    name: &str,
    optional: bool,
) -> Result<Option<K>, RuleError> {
    match resolve(name) {
        Some(k) => Ok(Some(k)),
        None if optional => Ok(None),
        None => Err(RuleError::UnknownKind { name: name.into() }),
    }
}

fn compile_tokens<K: Ord>(
    rules: &[TokenRule],
    resolve: &mut impl FnMut(&str) -> Option<K>,
) -> Result<TokenTable<K>, RuleError> {
    let mut table = TokenTable::default();
    let mut exact: Vec<(K, Sides)> = Vec::with_capacity(rules.len());
    for rule in rules {
        let sides = Sides {
            before: rule.before.map(Sp::from),
            after: rule.after.map(Sp::from),
        };
        match &rule.kind {
            None => table.any = table.any.merge(sides),
            Some(name) => {
                if let Some(kind) = resolve_in(resolve, name, rule.optional)? {
                    exact.push((kind, sides));
                }
            }
        }
    }
    // Stable sort keeps rules for one kind in the order written, so the merge
    // below lets a later rule refine an earlier one.
    exact.sort_by(|a, b| a.0.cmp(&b.0));
    for (kind, sides) in exact {
        match table.exact.last_mut() {
            Some((last, merged)) if *last == kind => *merged = merged.merge(sides),
            _ => table.exact.push((kind, sides)),
        }
    }
    Ok(table)
}

fn compile_node<K: Ord + Clone>(
    rule: &NodeRule,
    resolve: &mut impl FnMut(&str) -> Option<K>,
) -> Result<Option<NodeStyle<K>>, RuleError> {
    let opt = rule.optional;
    let Some(kind) = resolve_in(resolve, &rule.kind, opt)? else {
        return Ok(None);
    };
    let delims = match &rule.delimiters {
        None => None,
        Some(d) => {
            let (Some(open), Some(close)) = (
                resolve_in(resolve, &d.open, opt)?,
                resolve_in(resolve, &d.close, opt)?,
            ) else {
                return Ok(None);
            };
            Some(Delims {
                open,
                close,
                inner: Sp::from(d.inner),
            })
        }
    };
    let sep = match &rule.separator {
        None => None,
        Some(s) => {
            let Some(sep_kind) = resolve_in(resolve, &s.kind, opt)? else {
                return Ok(None);
            };
            if let Some(d) = &delims {
                if d.open == sep_kind || d.close == sep_kind {
                    return Err(RuleError::SeparatorIsDelimiter {
                        node: rule.kind.clone(),
                    });
                }
            }
            if s.trailing != Trailing::Preserve && delims.is_none() {
                return Err(RuleError::TrailingNeedsDelimiters {
                    node: rule.kind.clone(),
                });
            }
            Some(Sep {
                kind: sep_kind,
                text: s.text.clone().unwrap_or_else(|| s.kind.clone()),
                before: Sp::from(s.before),
                after: Sp::from(s.after),
                trailing: s.trailing,
            })
        }
    };
    let tokens = if opt {
        // An optional node rule drops unknown token kinds inside it too.
        let relaxed: Vec<TokenRule> = rule.tokens.iter().cloned().map(|t| t.optional()).collect();
        compile_tokens(&relaxed, resolve)?
    } else {
        compile_tokens(&rule.tokens, resolve)?
    };
    Ok(Some(NodeStyle {
        kind,
        group: rule.group,
        indent: rule.indent,
        before: rule.before.map(Sp::from),
        after: rule.after.map(Sp::from),
        delims,
        empty: Sp::from(rule.empty),
        sep,
        blank_lines: rule.blank_lines,
        tokens,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::NodeRule;

    fn names(name: &str) -> Option<&'static str> {
        [
            "a", "b", ",", "(", ")", "[", "]", "+", "list", "stmt", "ERROR",
        ]
        .into_iter()
        .find(|k| *k == name)
    }

    #[test]
    fn test_join_is_the_lattice_max() {
        let soft = Sp::from(Space::SoftLine);
        let single = Sp::from(Space::Single);
        assert_eq!(soft.join(single), Sp::from(Space::Line));
        assert_eq!(Sp::from(Space::Line).join(Sp::HARD).brk, 2);
        assert_eq!(Sp::NONE.join(Sp::NONE), Sp::NONE);
        assert_eq!(join_opt(None, None), None);
        assert_eq!(join_opt(Some(Sp::NONE), None), Some(Sp::NONE));
    }

    #[test]
    fn test_later_token_rules_refine_earlier() {
        let style = Rules::new()
            .token(TokenRule::new("+").before(Space::None).after(Space::None))
            .token(TokenRule::new("+").after(Space::Single))
            .compile(names)
            .unwrap_or_default();
        assert_eq!(style.token_before(None, &"+"), Some(Sp::NONE));
        assert_eq!(style.token_after(None, &"+"), Some(Sp::SINGLE));
        assert_eq!(style.token_after(None, &"a"), None);
    }

    #[test]
    fn test_context_rules_win_over_top_level() {
        let style = Rules::new()
            .token(TokenRule::new("+").around(Space::Single))
            .node(NodeRule::new("list").token(TokenRule::any().after(Space::None)))
            .compile(names)
            .unwrap_or_default();
        let list = style.node(style.node_index(&"list"));
        assert!(list.is_some());
        assert_eq!(style.token_after(list, &"+"), Some(Sp::NONE));
        // The node says nothing about `before`, so the top-level rule applies.
        assert_eq!(style.token_before(list, &"+"), Some(Sp::SINGLE));
    }

    #[test]
    fn test_unknown_kinds_are_errors_unless_optional() {
        let err = Rules::new()
            .token(TokenRule::new("nope").before(Space::None))
            .compile(names)
            .map(|_| ());
        assert_eq!(
            err,
            Err(RuleError::UnknownKind {
                name: "nope".into()
            })
        );
        assert!(
            Rules::new()
                .token(TokenRule::new("nope").optional())
                .node(NodeRule::new("missing").optional())
                .node(
                    NodeRule::new("list")
                        .delimiters("{", "}", Space::Line)
                        .optional()
                )
                .compile(names)
                .is_ok()
        );
        assert!(Rules::conventional().compile(names).is_ok());
    }

    #[test]
    fn test_rule_consistency_errors() {
        let dup = Rules::new()
            .node(NodeRule::new("list"))
            .node(NodeRule::new("list"))
            .compile(names)
            .map(|_| ());
        assert_eq!(
            dup,
            Err(RuleError::DuplicateNode {
                name: "list".into()
            })
        );
        let wide = Rules::new().indent(17).compile(names).map(|_| ());
        assert_eq!(wide, Err(RuleError::IndentTooWide { columns: 17 }));
        let same = Rules::new()
            .node(
                NodeRule::new("list")
                    .delimiters("(", ")", Space::None)
                    .separator(")", Space::Single, Trailing::Preserve),
            )
            .compile(names)
            .map(|_| ());
        assert_eq!(
            same,
            Err(RuleError::SeparatorIsDelimiter {
                node: "list".into()
            })
        );
        let trailing = Rules::new()
            .node(NodeRule::new("list").separator(",", Space::Single, Trailing::Always))
            .compile(names)
            .map(|_| ());
        assert_eq!(
            trailing,
            Err(RuleError::TrailingNeedsDelimiters {
                node: "list".into()
            })
        );
    }

    #[test]
    fn test_verbatim_and_node_lookup() {
        let style = Rules::new()
            .verbatim("ERROR")
            .verbatim("ERROR")
            .node(NodeRule::new("stmt").group())
            .node(NodeRule::new("list"))
            .compile(names)
            .unwrap_or_default();
        assert!(style.is_verbatim(&"ERROR"));
        assert!(!style.is_verbatim(&"stmt"));
        let stmt = style.node(style.node_index(&"stmt"));
        assert!(stmt.is_some_and(NodeStyle::owns_doc));
        let list = style.node(style.node_index(&"list"));
        assert!(list.is_some_and(|n| !n.owns_doc()));
        assert!(style.node_index(&"a").is_none());
    }

    #[test]
    fn test_separator_text_defaults_to_kind_name() {
        let style = Rules::new()
            .node(
                NodeRule::new("list")
                    .delimiters("[", "]", Space::None)
                    .separator(",", Space::Single, Trailing::Always),
            )
            .compile(names)
            .unwrap_or_default();
        let sep = style
            .node(style.node_index(&"list"))
            .and_then(|n| n.sep.as_ref())
            .map(|s| s.text.as_str());
        assert_eq!(sep, Some(","));
    }
}
