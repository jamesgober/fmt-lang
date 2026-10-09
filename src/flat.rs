//! Flattening: one iterative pass over the tree that validates it against the
//! source and turns it into a linear event stream for the walk.
//!
//! The stream is what the walk needs and nothing more: node entries and exits,
//! significant tokens, and trivia spans. Two things are settled here, where
//! every node's whole child list is in view, so the walk never has to look
//! ahead:
//!
//! - a node in the style's verbatim set collapses into one opaque significant
//!   unit covering its first to last significant token (trivia at its edges is
//!   still ordinary trivia, so comments there are attached normally);
//! - for nodes with delimiter and separator rules, where the delimiters are,
//!   and whether a trailing separator must be added or removed.

use alloc::vec::Vec;

use syntax_lang::{Element, Node, Token, TokenKind};

use crate::error::FormatError;
use crate::rules::Trailing;
use crate::style::Style;

/// One step of the flattened tree.
#[derive(Debug)]
pub(crate) enum Ev<'t, K> {
    /// A node opens; the index is into [`Flat::nodes`].
    Enter(u32),
    /// The innermost open node closes.
    Exit,
    /// A significant token, or a verbatim node collapsed into one unit.
    Sig(Sig<'t, K>),
    /// A trivia token's byte span.
    Trivia(u32, u32),
}

/// A significant unit: a token, or a whole verbatim node.
#[derive(Debug)]
pub(crate) struct Sig<'t, K> {
    pub(crate) kind: &'t K,
    pub(crate) start: u32,
    pub(crate) end: u32,
    /// For a collapsed verbatim node, the index of that node kind's own rule
    /// (its `before`/`after` still apply around it).
    pub(crate) rule: Option<u32>,
}

// Manual impls: the derive would demand `K: Copy`, but only `&K` is stored.
impl<K> Clone for Ev<'_, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> Copy for Ev<'_, K> {}
impl<K> Clone for Sig<'_, K> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<K> Copy for Sig<'_, K> {}

/// What the walk needs to know about one node.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NodeInfo {
    /// Index of the node's rule in the style, if it has one.
    pub(crate) rule: Option<u32>,
    /// Start of the opening delimiter, when the node's first significant
    /// child is its rule's opening delimiter.
    pub(crate) open_at: Option<u32>,
    /// Start of the closing delimiter, when the node's last significant child
    /// is its rule's closing delimiter.
    pub(crate) close_at: Option<u32>,
    /// End of that closing delimiter.
    pub(crate) close_end: Option<u32>,
    /// Start of a trailing separator that [`Trailing::Never`] removes.
    pub(crate) drop_sep_at: Option<u32>,
    /// Whether [`Trailing::Always`] adds a separator before the closing
    /// delimiter.
    pub(crate) insert_sep: bool,
}

/// The flattened tree.
pub(crate) struct Flat<'t, K> {
    pub(crate) events: Vec<Ev<'t, K>>,
    pub(crate) nodes: Vec<NodeInfo>,
    /// The source starts with a byte-order mark carried by a trivia token.
    pub(crate) bom: bool,
}

/// Whether a trivia token's text is whitespace (a byte-order mark counts at
/// the very start of the source) rather than a comment-like token.
#[inline]
pub(crate) fn is_blank(text: &str, start: u32) -> bool {
    text.chars()
        .all(|c| c.is_whitespace() || (start == 0 && c == '\u{feff}'))
}

/// One child as its parent's list analysis sees it.
#[derive(Clone, Copy)]
struct Kid<'t, K> {
    kind: &'t K,
    token: bool,
    sig: bool,
    end: u32,
    /// No error node, and no node with an opening delimiter but no closing
    /// one, anywhere in this child's subtree.
    clean: bool,
    start: u32,
}

/// Checks each token's span against the source and against its predecessor.
struct Checker<'s> {
    source: &'s str,
    prev_end: Option<u32>,
}

impl Checker<'_> {
    fn check(&mut self, start: u32, end: u32) -> Result<(), FormatError> {
        let len = self.source.len();
        if end as usize > len {
            return Err(FormatError::OutOfBounds { start, end, len });
        }
        for offset in [start, end] {
            if !self.source.is_char_boundary(offset as usize) {
                return Err(FormatError::NotCharBoundary { offset });
            }
        }
        if let Some(expected) = self.prev_end {
            if expected != start {
                return Err(FormatError::NotContiguous {
                    expected,
                    found: start,
                });
            }
        }
        self.prev_end = Some(end);
        Ok(())
    }
}

#[inline]
fn bounds<K>(t: &Token<K>) -> (u32, u32) {
    let s = t.span();
    (s.start().to_u32(), s.end().to_u32())
}

/// Flattens `root` (validated against `source`) under `style`.
pub(crate) fn flatten<'t, K: TokenKind + Ord>(
    root: &'t Node<K>,
    source: &str,
    style: &Style<K>,
) -> Result<Flat<'t, K>, FormatError> {
    let mut f = Flattener {
        style,
        check: Checker {
            source,
            prev_end: None,
        },
        events: Vec::new(),
        nodes: Vec::new(),
        kids: Vec::new(),
        bom: false,
    };
    // Each stack entry: the node's children still to visit, its index, and
    // where its children's entries begin in `kids`.
    let mut stack = Vec::new();
    if let Some(open) = f.node(root)? {
        stack.push((root.children(), root, open, f.kids.len()));
    }
    while let Some((children, _, _, _)) = stack.last_mut() {
        match children.next() {
            Some(Element::Token(t)) => f.token(t)?,
            Some(Element::Node(n)) => {
                if let Some(open) = f.node(n)? {
                    stack.push((n.children(), n, open, f.kids.len()));
                }
            }
            None => {
                if let Some((_, node, index, base)) = stack.pop() {
                    f.exit(node, index, base);
                }
            }
        }
    }
    Ok(Flat {
        events: f.events,
        nodes: f.nodes,
        bom: f.bom,
    })
}

struct Flattener<'t, 's, K> {
    style: &'s Style<K>,
    check: Checker<'s>,
    events: Vec<Ev<'t, K>>,
    nodes: Vec<NodeInfo>,
    /// Child summaries of every open node, innermost last.
    kids: Vec<Kid<'t, K>>,
    bom: bool,
}

impl<'t, K: TokenKind + Ord> Flattener<'t, '_, K> {
    /// Records a trivia token, noting a leading byte-order mark.
    fn trivia(&mut self, start: u32, end: u32) {
        if start == 0 && self.check.source.starts_with('\u{feff}') {
            self.bom = true;
        }
        self.events.push(Ev::Trivia(start, end));
    }

    fn token(&mut self, t: &'t Token<K>) -> Result<(), FormatError> {
        let (start, end) = bounds(t);
        self.check.check(start, end)?;
        // Empty tokens carry no text: an empty trivia token is nothing, and an
        // empty significant token (a parser's "assumed" token) is skipped so it
        // neither takes spacing nor counts as a list element.
        let sig = !t.is_trivia() && start != end;
        if start != end {
            if sig {
                self.events.push(Ev::Sig(Sig {
                    kind: t.kind(),
                    start,
                    end,
                    rule: None,
                }));
            } else {
                self.trivia(start, end);
            }
        }
        self.kids.push(Kid {
            kind: t.kind(),
            token: true,
            sig,
            clean: true,
            start,
            end,
        });
        Ok(())
    }

    /// Enters a node. Returns its index if the caller must descend into it, or
    /// `None` if it was a verbatim node, emitted whole here.
    fn node(&mut self, n: &'t Node<K>) -> Result<Option<u32>, FormatError> {
        if self.style.is_verbatim(n.kind()) {
            self.verbatim(n)?;
            return Ok(None);
        }
        let index = u32::try_from(self.nodes.len()).unwrap_or(u32::MAX);
        self.nodes.push(NodeInfo {
            rule: self.style.node_index(n.kind()),
            ..NodeInfo::default()
        });
        self.events.push(Ev::Enter(index));
        Ok(Some(index))
    }

    /// Emits a verbatim node: its edge trivia as trivia, the span from its
    /// first to its last significant token as one unit.
    fn verbatim(&mut self, n: &'t Node<K>) -> Result<(), FormatError> {
        let mut first = None;
        let mut last = None;
        for t in n.tokens() {
            let (start, end) = bounds(t);
            self.check.check(start, end)?;
            if !t.is_trivia() && start != end {
                first = first.or(Some(start));
                last = Some(end);
            }
        }
        let (Some(first), Some(last)) = (first, last) else {
            // Nothing significant inside: the whole node is trivia.
            for t in n.tokens() {
                let (start, end) = bounds(t);
                if start != end {
                    self.trivia(start, end);
                }
            }
            self.kids.push(Kid {
                kind: n.kind(),
                token: false,
                sig: false,
                clean: false,
                start: 0,
                end: 0,
            });
            return Ok(());
        };
        for t in n.tokens() {
            let (start, end) = bounds(t);
            if end <= first && start != end {
                self.trivia(start, end);
            }
        }
        self.events.push(Ev::Sig(Sig {
            kind: n.kind(),
            start: first,
            end: last,
            rule: self.style.node_index(n.kind()),
        }));
        for t in n.tokens() {
            let (start, end) = bounds(t);
            if start >= last && start != end {
                self.trivia(start, end);
            }
        }
        self.kids.push(Kid {
            kind: n.kind(),
            token: false,
            sig: true,
            clean: false,
            start: first,
            end: last,
        });
        Ok(())
    }

    /// Closes node `index`, whose children's summaries start at `base`:
    /// settles its delimiters and trailing separator, then replaces the
    /// children's summaries with the node's own.
    fn exit(&mut self, node: &'t Node<K>, index: u32, base: usize) {
        let kids = self.kids.get(base..).unwrap_or(&[]);
        let sig = kids.iter().any(|k| k.sig);
        let mut clean = kids.iter().all(|k| k.clean);
        if let Some(info) = self.nodes.get_mut(index as usize) {
            settle_list(self.style, info, kids);
            // A delimited node missing one of its delimiters is a recovered
            // parse error even when the parser left no error node.
            let delimited = self
                .style
                .node(info.rule)
                .is_some_and(|r| r.delims.is_some());
            if delimited && info.open_at.is_some() != info.close_at.is_some() {
                clean = false;
            }
        }
        self.kids.truncate(base);
        self.kids.push(Kid {
            kind: node.kind(),
            token: false,
            sig,
            clean,
            start: 0,
            end: 0,
        });
        self.events.push(Ev::Exit);
    }
}

/// Finds a node's delimiters and decides its trailing-separator edit.
fn settle_list<K: Ord>(style: &Style<K>, info: &mut NodeInfo, kids: &[Kid<'_, K>]) {
    let Some(rule) = style.node(info.rule) else {
        return;
    };
    let Some(delims) = &rule.delims else {
        return;
    };
    let mut sig = kids.iter().filter(|k| k.sig);
    let first = sig.next();
    let last = sig.next_back();
    if let Some(k) = first {
        if k.token && *k.kind == delims.open {
            info.open_at = Some(k.start);
        }
    }
    if let Some(k) = last {
        if k.token && *k.kind == delims.close {
            info.close_at = Some(k.start);
            info.close_end = Some(k.end);
        }
    }
    let Some(sep) = &rule.sep else {
        return;
    };
    if sep.trailing == Trailing::Preserve || info.open_at.is_none() || info.close_at.is_none() {
        return;
    }
    // Adding or removing a token next to a recovered parse error can change
    // how the parser recovers, so lists with an error node anywhere inside are
    // left exactly as written.
    if !kids.iter().all(|k| k.clean) {
        return;
    }
    // The items strictly between the delimiters must alternate element,
    // separator, element, ... ; anything else is malformed and left as
    // written.
    let mut expect_element = true;
    let mut elements = 0usize;
    let mut last_sep = None;
    for k in sig {
        let is_sep = k.token && *k.kind == sep.kind;
        if is_sep == expect_element {
            return;
        }
        if is_sep {
            last_sep = Some(k.start);
        } else {
            elements += 1;
        }
        expect_element = is_sep;
    }
    if elements == 0 {
        return;
    }
    match sep.trailing {
        Trailing::Always if !expect_element => info.insert_sep = true,
        Trailing::Never if expect_element => info.drop_sep_at = last_sep,
        _ => {}
    }
}
