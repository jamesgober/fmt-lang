//! The walk: turns the flattened tree into one pretty-lang [`Doc`].
//!
//! Output is produced token by token. Everything between two neighbouring
//! significant units (the *gap*) is decided at once when the second one
//! arrives: by then every node closing after the first unit has closed, so the
//! gap is written into the innermost node holding both units, and nodes
//! opening before the second unit are opened only afterwards. That placement is
//! what makes groups and indentation come out right: a comment ending a line
//! never sits inside a sibling's group, and a line break before a node is
//! indented by the node's parent, not by the node.
//!
//! # Comment attachment
//!
//! The comments in one gap are split by where they sit in the source:
//!
//! 1. *Trailing* comments: those with no line break between them and the unit
//!    before the gap. They stay on that unit's line, after one space.
//! 2. *Leading* comments: the rest, each of which started a line in the
//!    source. Each starts a line in the output too (with the source's blank
//!    lines, capped), at the indentation of the gap.
//! 3. *Dangling* comments: leading comments in the last gap of a delimited,
//!    indented body (just before the closing delimiter). They are indented
//!    with the body rather than with the closing delimiter.
//!
//! A comment is never moved across a significant unit, so comments keep their
//! order relative to the code and to each other, and each comment's text is
//! written exactly once, unchanged. A comment followed by a line break in the
//! source is followed by one in the output, so a line comment always ends its
//! line.
//!
//! # Why formatting is idempotent
//!
//! Every decision reads either the tree's shape and token texts (which the
//! output preserves) or one of a few facts about the source's whitespace, and
//! each of those facts survives into the output unchanged:
//!
//! - whether a gap had a line break before or after a comment (trailing versus
//!   leading, and "ends its line"): the output has a break exactly there;
//! - the number of blank lines at a hard break, after capping: the output has
//!   exactly that many;
//! - whitespace kept because no rule spoke: copied, so the next run copies it
//!   again;
//! - whether two units touched: they touch in the output only if they touched
//!   before or the touch test allowed it, and it allows it again.
//!
//! The one change between runs is a group line that broke becoming, in the next
//! run's source, a break after a comment, which turns into a hard break. That
//! renders the same: a line only breaks when every group around it broke, and
//! a hard break forces exactly those groups to break.

use alloc::string::String;
use alloc::vec::Vec;
use core::mem;

use pretty_lang::Doc;

use crate::flat::{Ev, Flat, NodeInfo, Sig, is_blank};
use crate::rules::Indent;
use crate::style::{NodeStyle, Sp, Style, join_opt};

/// A document under construction: committed parts plus a pending run of
/// plain text, so consecutive words, spaces and punctuation become one
/// [`Doc::text`] instead of one document node each.
struct Out {
    doc: Option<Doc>,
    run: String,
    /// The absolute indentation, in columns, line breaks in this document get.
    acc: u32,
}

impl Out {
    fn new(acc: u32) -> Self {
        Self {
            doc: None,
            run: String::new(),
            acc,
        }
    }

    #[inline]
    fn text(&mut self, s: &str) {
        self.run.push_str(s);
    }

    fn flush(&mut self) {
        if !self.run.is_empty() {
            let text = Doc::text(mem::take(&mut self.run));
            self.attach(text);
        }
    }

    fn doc(&mut self, d: Doc) {
        self.flush();
        self.attach(d);
    }

    #[inline]
    fn attach(&mut self, d: Doc) {
        self.doc = Some(match self.doc.take() {
            Some(prev) => prev.append(d),
            None => d,
        });
    }

    fn is_empty(&self) -> bool {
        self.doc.is_none() && self.run.is_empty()
    }

    /// A line break to column 0, whatever the indentation: for text whose
    /// line structure must be reproduced exactly (multi-line tokens, kept
    /// whitespace) and for blank lines (which must not carry indentation).
    fn newline_at_column_zero(&mut self, br: &mut Breaks) {
        let d = br.column_zero(self.acc);
        self.doc(d);
    }

    /// Writes `text` exactly, reproducing any line breaks inside it.
    fn raw(&mut self, text: &str, br: &mut Breaks) {
        let mut lines = text.split('\n');
        if let Some(first) = lines.next() {
            self.text(first);
        }
        for line in lines {
            self.newline_at_column_zero(br);
            self.text(line);
        }
    }

    fn finish(mut self) -> Doc {
        self.flush();
        self.doc.unwrap_or_default()
    }
}

/// The break documents, built once per walk and shared (a `Doc` clone is a
/// reference-count bump), so a million line breaks are not a million
/// allocations.
struct Breaks {
    line: Doc,
    softline: Doc,
    hardline: Doc,
    /// Breaks to column 0, by the indentation they cancel.
    column_zero: Vec<Option<Doc>>,
}

/// Indentations up to this many columns have their column-0 break cached.
const CACHED_COLUMNS: usize = 256;

impl Breaks {
    fn new() -> Self {
        Self {
            line: Doc::line(),
            softline: Doc::softline(),
            hardline: Doc::hardline(),
            column_zero: Vec::new(),
        }
    }

    fn column_zero(&mut self, acc: u32) -> Doc {
        if acc == 0 {
            return self.hardline.clone();
        }
        let i = acc as usize;
        if i >= CACHED_COLUMNS {
            return self.hardline.clone().nest(-cols(acc));
        }
        if self.column_zero.len() <= i {
            self.column_zero.resize(i + 1, None);
        }
        match self.column_zero.get_mut(i) {
            Some(Some(d)) => d.clone(),
            Some(slot) => {
                let d = self.hardline.clone().nest(-cols(acc));
                *slot = Some(d.clone());
                d
            }
            None => self.hardline.clone().nest(-cols(acc)),
        }
    }
}

#[inline]
fn cols(n: u32) -> isize {
    isize::try_from(n).unwrap_or(isize::MAX)
}

/// One open node.
struct Frame {
    /// Index into [`Flat::nodes`]; `None` for the file frame around the root.
    node: Option<u32>,
    /// Whether the node has an [`Out`] of its own on the out stack.
    owns: bool,
    /// The columns this node indents by (already clamped to the budget).
    nest: u32,
    /// For [`Indent::Block`]: 0 before the opening delimiter, 1 while the
    /// body's own [`Out`] is on the stack, 2 after it was closed.
    body: u8,
}

/// The significant unit before the current gap.
struct Prev<'a> {
    start: u32,
    end: u32,
    text: &'a str,
    /// The spacing the unit asks for after itself.
    after: Option<Sp>,
    /// The unit is its parent's list separator.
    is_sep: bool,
}

/// The unit after a gap.
#[derive(Clone, Copy)]
struct Next<'a> {
    start: u32,
    end: u32,
    text: &'a str,
    /// The spacing the unit asks for before itself.
    before: Option<Sp>,
}

/// Where a gap sits, worked out once before it is written.
struct GapCtx<'a, K> {
    next: Option<Next<'a>>,
    /// The rule of the innermost node holding both units.
    lca: Option<&'a NodeStyle<K>>,
    /// The unit before the gap is a direct child of that node.
    prev_direct: bool,
    /// The unit before the gap is that node's opening delimiter.
    opens: bool,
    /// The unit after the gap is that node's closing delimiter.
    closes: bool,
    /// The gap touches a kept region: written exactly as in the source.
    kept: bool,
    /// A trailing separator is inserted at the start of the gap.
    insert: bool,
}

/// One gap's whitespace and comments, split for attachment.
#[derive(Default)]
struct Gap {
    /// Comment spans, in order.
    comments: Vec<(u32, u32)>,
    /// Whitespace around them: `ws[i]` precedes `comments[i]`, and the last
    /// entry follows the last comment. Each is (start, end, line breaks).
    ws: Vec<(u32, u32, u32)>,
}

pub(crate) struct Walker<'a, K> {
    style: &'a Style<K>,
    src: &'a str,
    nodes: &'a [NodeInfo],
    outs: Vec<Out>,
    frames: Vec<Frame>,
    /// Nodes entered since the previous unit, not opened yet.
    pending: Vec<u32>,
    /// Nodes closed since the previous unit.
    exited: Vec<u32>,
    /// Trivia spans since the previous unit.
    trivia: Vec<(u32, u32)>,
    prev: Option<Prev<'a>>,
    gap: Gap,
    /// Regions to keep as written: sorted, disjoint byte ranges.
    keep: Vec<(u32, u32)>,
    br: Breaks,
    /// The last gap was kept exactly, so the source's own ending stands and no
    /// final line break is added.
    exact_end: bool,
    /// The last thing written was a token or comment ending in a lone `\r`. A
    /// line break written right after it would pair with that `\r` into
    /// `\r\n` and be read as part of the break, so the next break is written
    /// as `\r\n` instead.
    cr: bool,
}

/// Sorts and merges `spans` into disjoint ranges for [`Walker::kept`].
fn merge_spans(spans: &[syntax_lang::Span]) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = spans
        .iter()
        .map(|s| (s.start().to_u32(), s.end().to_u32()))
        .collect();
    v.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(v.len());
    for (start, end) in v {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

impl<'a, K: Ord> Walker<'a, K> {
    pub(crate) fn new(
        style: &'a Style<K>,
        src: &'a str,
        flat: &'a Flat<'a, K>,
        keep: &[syntax_lang::Span],
    ) -> Self {
        Self {
            style,
            src,
            nodes: &flat.nodes,
            outs: alloc::vec![Out::new(0)],
            frames: alloc::vec![Frame {
                node: None,
                owns: true,
                nest: 0,
                body: 0,
            }],
            pending: Vec::new(),
            exited: Vec::new(),
            trivia: Vec::new(),
            prev: None,
            gap: Gap::default(),
            keep: merge_spans(keep),
            br: Breaks::new(),
            exact_end: false,
            cr: false,
        }
    }

    /// Whether a kept region lies anywhere in a delimited list, from its
    /// opening to its closing delimiter: such lists get no separator edits.
    fn list_kept(&self, info: &NodeInfo) -> bool {
        match (info.open_at, info.close_end) {
            (Some(open), Some(close)) => self.kept(open, close, open, close),
            _ => false,
        }
    }

    /// Whether a kept region overlaps the units `start..end` (half open), or
    /// an empty kept region lies in the gap `gap_start..=gap_end` between them.
    fn kept(&self, start: u32, end: u32, gap_start: u32, gap_end: u32) -> bool {
        if self.keep.is_empty() {
            return false;
        }
        // Regions are disjoint and sorted, so both ends ascend: the first
        // region not entirely before `start` is the only candidate that
        // matters for overlap; empty ones are checked by position.
        let i = self.keep.partition_point(|k| k.1 < start);
        self.keep
            .get(i..)
            .unwrap_or(&[])
            .iter()
            .take_while(|k| k.0 <= end)
            .any(|k| {
                if k.0 == k.1 {
                    gap_start <= k.0 && k.0 <= gap_end
                } else {
                    k.0 < end && start < k.1
                }
            })
    }

    /// Runs the walk over `events` and returns the finished document.
    pub(crate) fn run(mut self, events: &'a [Ev<'a, K>], bom: bool) -> Doc {
        if bom {
            self.top().text("\u{feff}");
        }
        for ev in events {
            match *ev {
                Ev::Enter(id) => self.pending.push(id),
                Ev::Exit => self.exit(),
                Ev::Trivia(start, end) => self.trivia.push((start, end)),
                Ev::Sig(sig) => self.sig(sig),
            }
        }
        // Close anything a malformed event stream left open, then the gap
        // after the last unit.
        while self.frames.len() > 1 || !self.pending.is_empty() {
            self.exit();
        }
        self.write_gap(None);
        let content = self.outs.iter().any(|o| !o.is_empty());
        if self.style.final_newline && content && !self.exact_end {
            if mem::take(&mut self.cr) {
                self.top().text("\r");
            }
            let (out, br) = self.top_and_breaks();
            out.doc(br.hardline.clone());
        }
        let mut doc = Doc::nil();
        while let Some(out) = self.outs.pop() {
            doc = out.finish().append(doc);
        }
        doc
    }

    #[inline]
    fn top(&mut self) -> &mut Out {
        self.top_and_breaks().0
    }

    /// The innermost document and the shared break documents, borrowed
    /// together.
    #[inline]
    fn top_and_breaks(&mut self) -> (&mut Out, &mut Breaks) {
        if self.outs.is_empty() {
            self.outs.push(Out::new(0));
        }
        let last = self.outs.len() - 1;
        (&mut self.outs[last], &mut self.br)
    }

    /// Writes `text` exactly into the innermost document.
    fn raw(&mut self, text: &str) {
        let (out, br) = self.top_and_breaks();
        out.raw(text, br);
    }

    #[inline]
    fn info(&self, node: Option<u32>) -> Option<&'a NodeInfo> {
        node.and_then(|i| self.nodes.get(i as usize))
    }

    #[inline]
    fn rule_of(&self, node: Option<u32>) -> Option<&'a NodeStyle<K>> {
        self.style.node(self.info(node).and_then(|i| i.rule))
    }

    fn exit(&mut self) {
        // A node entered after the previous unit and closed before the next
        // one holds no significant unit: it was never opened.
        if self.pending.pop().is_some() {
            return;
        }
        if self.frames.len() <= 1 {
            return;
        }
        self.close_body();
        let Some(frame) = self.frames.pop() else {
            return;
        };
        if frame.owns {
            if let Some(out) = self.outs.pop() {
                let rule = self.rule_of(frame.node);
                let mut d = out.finish();
                if rule.is_some_and(|r| r.indent == Indent::Hanging) && frame.nest > 0 {
                    d = d.nest(cols(frame.nest));
                }
                if rule.is_some_and(|r| r.group) {
                    d = d.group();
                }
                self.top().doc(d);
            }
        }
        if let Some(node) = frame.node {
            self.exited.push(node);
        }
    }

    /// Ends the innermost frame's indented body, if one is open.
    fn close_body(&mut self) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        if frame.body != 1 {
            return;
        }
        frame.body = 2;
        let nest = frame.nest;
        if let Some(body) = self.outs.pop() {
            let d = body.finish().nest(cols(nest));
            self.top().doc(d);
        }
    }

    /// Opens the nodes entered since the previous unit, outermost first.
    fn open_pending(&mut self) {
        for i in 0..self.pending.len() {
            let Some(&id) = self.pending.get(i) else {
                break;
            };
            let info = self.info(Some(id));
            let rule = self.style.node(info.and_then(|i| i.rule));
            let base = self.top().acc;
            // A hanging node directly inside a node with the same rule (a
            // left- or right-nested chain such as `a + b + c`) shares its
            // parent's indentation instead of adding another level.
            let parent_rule = self
                .frames
                .last()
                .and_then(|f| self.info(f.node))
                .and_then(|i| i.rule);
            let chained =
                info.and_then(|i| i.rule).is_some() && parent_rule == info.and_then(|i| i.rule);
            let nest = match rule.map(|r| r.indent) {
                Some(Indent::Hanging) if chained => 0,
                Some(Indent::Block | Indent::Hanging) => self
                    .style
                    .indent
                    .min(self.style.max_indent.saturating_sub(base)),
                _ => 0,
            };
            let owns = rule.is_some_and(NodeStyle::owns_doc);
            if owns {
                let hanging = rule.is_some_and(|r| r.indent == Indent::Hanging);
                self.outs.push(Out::new(if hanging {
                    base.saturating_add(nest)
                } else {
                    base
                }));
            }
            self.frames.push(Frame {
                node: Some(id),
                owns,
                nest,
                body: 0,
            });
        }
        self.pending.clear();
    }

    fn sig(&mut self, sig: Sig<'a, K>) {
        let parent_node = match self.pending.last() {
            Some(&id) => Some(id),
            None => self.frames.last().and_then(|f| f.node),
        };
        let parent_info = self.info(parent_node);
        // A trailing separator the rules remove: skipped, and the gaps on
        // either side of it merge into one.
        if self.pending.is_empty()
            && parent_info.is_some_and(|i| i.drop_sep_at == Some(sig.start) && !self.list_kept(i))
        {
            return;
        }
        let parent = self.style.node(parent_info.and_then(|i| i.rule));
        let own = self.style.node(sig.rule);
        let text = self
            .src
            .get(sig.start as usize..sig.end as usize)
            .unwrap_or("");
        let before = join_opt(
            self.style.token_before(parent, sig.kind),
            own.and_then(|r| r.before),
        );
        self.write_gap(Some(Next {
            start: sig.start,
            end: sig.end,
            text,
            before,
        }));
        self.open_pending();

        self.unit(text);
        let info = self.info(self.frames.last().and_then(|f| f.node));
        let opens_body = info.is_some_and(|i| i.open_at == Some(sig.start))
            && parent.is_some_and(|r| r.indent == Indent::Block);
        if let Some(frame) = self.frames.last_mut() {
            if opens_body && frame.body == 0 && frame.owns {
                frame.body = 1;
                let acc = self
                    .outs
                    .last()
                    .map_or(0, |o| o.acc)
                    .saturating_add(frame.nest);
                self.outs.push(Out::new(acc));
            }
        }
        let after = join_opt(
            self.style.token_after(parent, sig.kind),
            own.and_then(|r| r.after),
        );
        let is_sep = parent
            .and_then(|r| r.sep.as_ref())
            .is_some_and(|s| s.kind == *sig.kind);
        self.prev = Some(Prev {
            start: sig.start,
            end: sig.end,
            text,
            after,
            is_sep,
        });
        self.trivia.clear();
        self.exited.clear();
    }

    /// Splits the pending trivia into comments and the whitespace around them.
    fn split_trivia(&mut self) {
        let gap = &mut self.gap;
        gap.comments.clear();
        gap.ws.clear();
        let mut ws: Option<(u32, u32, u32)> = None;
        for &(start, end) in &self.trivia {
            let text = self.src.get(start as usize..end as usize).unwrap_or("");
            if is_blank(text, start) {
                let lines = count_newlines(text);
                ws = Some(match ws {
                    Some((s, _, n)) => (s, end, n.saturating_add(lines)),
                    None => (start, end, lines),
                });
            } else {
                let w = ws.take().unwrap_or((start, start, 0));
                gap.ws.push(w);
                gap.comments.push((start, end));
                // A comment token that swallowed its line break (some lexers do)
                // ends its line just like one followed by a break.
                if text.ends_with('\n') {
                    ws = Some((end, end, 1));
                }
            }
        }
        let end = self.gap.comments.last().map_or(0, |c| c.1);
        self.gap.ws.push(ws.unwrap_or((end, end, 0)));
    }

    /// The blank lines a hard break keeps, given the line breaks the source had.
    fn blank_lines(&self, lines: u32, lca: Option<&NodeStyle<K>>) -> u32 {
        let limit = lca
            .and_then(|r| r.blank_lines)
            .unwrap_or(self.style.blank_lines);
        lines.saturating_sub(1).min(u32::from(limit))
    }

    /// Writes a token's or comment's text.
    fn unit(&mut self, text: &str) {
        self.raw(text);
        self.cr = text.ends_with('\r');
    }

    /// Writes one space.
    fn space(&mut self) {
        self.cr = false;
        self.top().text(" ");
    }

    fn emit_space(&mut self, sp: Sp, blank: u32) {
        let cr = mem::take(&mut self.cr);
        // A break that could fall right after a lone `\r` is made certain, so
        // the `\r` can be completed into a `\r\n` of its own.
        let sp = if cr && sp.brk >= 1 { Sp::HARD } else { sp };
        let (out, br) = self.top_and_breaks();
        match sp.brk {
            0 if sp.space => out.text(" "),
            0 => {}
            1 if sp.space => out.doc(br.line.clone()),
            1 => out.doc(br.softline.clone()),
            _ => {
                if cr {
                    out.text("\r");
                }
                for _ in 0..blank {
                    out.newline_at_column_zero(br);
                }
                out.doc(br.hardline.clone());
            }
        }
    }

    /// Writes a comment, minus a line break it swallowed (that break is
    /// accounted for as whitespace after it). At the end of the source the
    /// break is kept unless the final line break will stand in for it.
    fn emit_comment(&mut self, (start, end): (u32, u32)) {
        let text = self.src.get(start as usize..end as usize).unwrap_or("");
        let supplied = (end as usize) < self.src.len() || self.style.final_newline;
        match text.strip_suffix('\n') {
            // The break that follows reproduces the swallowed one, including a
            // `\r` before it.
            Some(stripped) if supplied => {
                self.raw(stripped);
                self.cr = false;
            }
            _ => self.unit(text),
        }
    }

    /// Writes kept whitespace: line breaks to column 0 followed by the
    /// original indentation, minus trailing spaces at line ends.
    fn emit_kept_whitespace(&mut self, (start, end, lines): (u32, u32, u32)) {
        let text = self.src.get(start as usize..end as usize).unwrap_or("");
        let cr = mem::take(&mut self.cr);
        let (out, br) = self.top_and_breaks();
        if lines == 0 {
            out.text(text);
            return;
        }
        if cr {
            out.text("\r");
        }
        for _ in 0..lines {
            out.newline_at_column_zero(br);
        }
        let indent = text.rsplit('\n').next().unwrap_or("");
        out.text(indent);
    }

    /// Writes the pending trivia byte for byte (a gap touching a kept region).
    fn write_exact(&mut self, closes: bool) {
        self.cr = false;
        for i in 0..self.trivia.len() {
            let (start, end) = self.trivia[i];
            let text = self.src.get(start as usize..end as usize).unwrap_or("");
            self.raw(text);
        }
        if closes {
            self.close_body();
        }
    }

    /// Resolves and writes the gap before `next` (or the end of the input).
    fn write_gap(&mut self, next: Option<Next<'a>>) {
        self.split_trivia();
        let ctx = self.gap_context(next);
        if ctx.kept {
            self.write_exact(ctx.closes);
            if ctx.next.is_none() {
                self.exact_end = true;
            }
            return;
        }
        let spacing = self.resolve(&ctx);
        let (left, touching) = self.insert_separator(&ctx);
        match spacing {
            None => self.write_preserved(&ctx),
            Some(sp) => self.write_formatted(&ctx, sp, left, touching),
        }
    }

    /// Where the gap sits: the innermost node holding both of its units, and
    /// whether those units are that node's delimiters.
    fn gap_context(&self, next: Option<Next<'a>>) -> GapCtx<'a, K> {
        let lca_node = self.frames.last().and_then(|f| f.node);
        let info = self.info(lca_node);
        let prev_direct = self.prev.is_some() && self.exited.is_empty();
        let opens = prev_direct
            && self
                .prev
                .as_ref()
                .is_some_and(|p| info.is_some_and(|i| i.open_at == Some(p.start)));
        let closes = self.pending.is_empty()
            && next.is_some_and(|n| info.is_some_and(|i| i.close_at == Some(n.start)));
        // A gap touching a kept region is written exactly as in the source,
        // and nothing is inserted into it. The first and last gaps are bounded
        // by their own trivia (an unterminated comment at the end of the input
        // is such a region).
        let first = self.trivia.first().map(|t| t.0);
        let last = self.trivia.last().map(|t| t.1);
        let (start, gap_start) = match &self.prev {
            Some(p) => (Some(p.start), Some(p.end)),
            None => (first, first),
        };
        let (end, gap_end) = match &next {
            Some(n) => (Some(n.end), Some(n.start)),
            None => (last, last),
        };
        let kept = match (start, end, gap_start, gap_end) {
            (Some(s), Some(e), Some(gs), Some(ge)) => self.kept(s, e, gs, ge),
            _ => false,
        };
        let insert = !kept && closes && info.is_some_and(|i| i.insert_sep && !self.list_kept(i));
        GapCtx {
            next,
            lca: self.rule_of(lca_node),
            prev_direct,
            opens,
            closes,
            kept,
            insert,
        }
    }

    /// The spacing the rules ask for; `None` when no rule speaks, and the
    /// source's whitespace is kept.
    fn resolve(&self, ctx: &GapCtx<'a, K>) -> Option<Sp> {
        let (Some(prev), Some(next)) = (&self.prev, &ctx.next) else {
            // The first and last gaps lay out comments only.
            return Some(Sp::NONE);
        };
        if ctx.opens && ctx.closes {
            return ctx.lca.map(|r| r.empty);
        }
        // A trailing separator (written or about to be inserted) takes only
        // the closing delimiter's spacing.
        let trailing = ctx.closes && (ctx.insert || (prev.is_sep && ctx.prev_direct));
        let mut acc = next.before;
        if !trailing {
            acc = join_opt(acc, prev.after);
            for &id in &self.exited {
                acc = join_opt(acc, self.rule_of(Some(id)).and_then(|r| r.after));
            }
        }
        if ctx.opens || ctx.closes {
            let inner = ctx.lca.and_then(|r| r.delims.as_ref()).map(|d| d.inner);
            acc = join_opt(acc, inner);
        }
        for &id in &self.pending {
            acc = join_opt(acc, self.rule_of(Some(id)).and_then(|r| r.before));
        }
        acc
    }

    /// Writes an inserted trailing separator, if the gap gets one: right after
    /// the last element, before any comment trailing it. Returns the text now
    /// left of the gap and whether it touches the next unit in the source.
    fn insert_separator(&mut self, ctx: &GapCtx<'a, K>) -> (Option<&'a str>, bool) {
        let touching = matches!((&self.prev, &ctx.next), (Some(p), Some(n)) if p.end == n.start);
        let left = self.prev.as_ref().map(|p| p.text);
        if !ctx.insert {
            return (left, touching);
        }
        let (Some(sep), Some(prev)) = (ctx.lca.and_then(|r| r.sep.as_ref()), &self.prev) else {
            return (left, touching);
        };
        // Spaced exactly as the gap before a separator written in the source
        // would be, so the next run sees nothing to change.
        let mut sp = sep.before;
        if let Some(after) = prev.after {
            sp = sp.join(after);
        }
        for &id in &self.exited {
            if let Some(after) = self.rule_of(Some(id)).and_then(|r| r.after) {
                sp = sp.join(after);
            }
        }
        if !sp.space && !sp.is_hard() && !(self.style.touch)(prev.text, &sep.text) {
            sp.space = true;
        }
        self.emit_space(sp, 0);
        let text: &'a str = &sep.text;
        self.unit(text);
        (Some(text), false)
    }

    /// No rule spoke: the source's whitespace and comments, trailing spaces at
    /// line ends trimmed.
    fn write_preserved(&mut self, ctx: &GapCtx<'a, K>) {
        let n = self.gap.comments.len();
        for i in 0..n {
            let w = self.gap.ws[i];
            self.emit_kept_whitespace(w);
            let c = self.gap.comments[i];
            self.emit_comment(c);
        }
        let w = self.gap.ws[n];
        self.emit_kept_whitespace(w);
        if ctx.closes {
            self.close_body();
        }
    }

    /// Writes a gap the rules resolved to `spacing`, with its comments
    /// attached as described in the module documentation.
    fn write_formatted(
        &mut self,
        ctx: &GapCtx<'a, K>,
        spacing: Sp,
        left: Option<&'a str>,
        touching: bool,
    ) {
        let n = self.gap.comments.len();
        let trailing = if self.prev.is_some() {
            self.gap.ws.iter().take(n).take_while(|w| w.2 == 0).count()
        } else {
            0
        };
        for i in 0..trailing {
            self.space();
            let c = self.gap.comments[i];
            self.emit_comment(c);
        }
        let last_ws = self.gap.ws[n];
        if trailing < n {
            self.write_leading(ctx, spacing, trailing);
            return;
        }
        if ctx.closes {
            self.close_body();
        }
        let (Some(next), Some(_)) = (&ctx.next, &self.prev) else {
            return;
        };
        if trailing > 0 {
            if last_ws.2 > 0 {
                let blank = self.blank_lines(last_ws.2, ctx.lca);
                self.emit_space(Sp::HARD, blank);
            } else {
                let kept = if last_ws.0 == last_ws.1 {
                    Sp::NONE
                } else {
                    Sp::SINGLE
                };
                self.emit_space(spacing.join(kept), 0);
            }
            return;
        }
        let mut sp = spacing;
        // Writing two units with nothing between them must not fuse them into
        // a different token; the source proves touching is safe only if they
        // already touched.
        if !sp.space && !sp.is_hard() && !touching {
            if let Some(l) = left {
                if !(self.style.touch)(l, next.text) {
                    sp.space = true;
                }
            }
        }
        let blank = if sp.is_hard() {
            self.blank_lines(last_ws.2, ctx.lca)
        } else {
            0
        };
        self.emit_space(sp, blank);
    }

    /// Writes the leading (or dangling) comments of a gap, from index `from`,
    /// each on its own line, then the break before the next unit.
    fn write_leading(&mut self, ctx: &GapCtx<'a, K>, spacing: Sp, from: usize) {
        let n = self.gap.comments.len();
        for j in from..n {
            let w = self.gap.ws[j];
            // The first comment of the file starts it: nothing before it.
            if j > from || self.prev.is_some() {
                if w.2 > 0 {
                    let blank = self.blank_lines(w.2, ctx.lca);
                    self.emit_space(Sp::HARD, blank);
                } else {
                    self.space();
                }
            }
            let c = self.gap.comments[j];
            self.emit_comment(c);
        }
        // Dangling comments were written inside the body; the closing
        // delimiter goes back to the node's level.
        if ctx.closes {
            self.close_body();
        }
        if ctx.next.is_none() {
            return;
        }
        let last_ws = self.gap.ws[n];
        if last_ws.2 > 0 {
            let blank = self.blank_lines(last_ws.2, ctx.lca);
            self.emit_space(Sp::HARD, blank);
        } else if spacing.is_hard() {
            self.emit_space(Sp::HARD, 0);
        } else {
            // The comment shared its line with the next unit: it leads that
            // unit, so they stay together.
            self.emit_space(Sp::SINGLE, 0);
        }
    }
}

#[inline]
fn count_newlines(text: &str) -> u32 {
    let n = text.bytes().filter(|&b| b == b'\n').count();
    u32::try_from(n).unwrap_or(u32::MAX)
}
