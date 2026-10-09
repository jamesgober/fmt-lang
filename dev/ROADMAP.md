# fmt-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation (DONE)
- [x] Rule model, walk over syntax-lang trees, comment attachment, rendering via pretty-lang.
- [x] Property tests: idempotence, token preservation.

Delivered:
- **Rule model as data**, keyed by kind names so a sketch's `[tooling.format]`
  can describe it: `Rules` (indent step, indentation budget, blank-line cap,
  final newline, verbatim kinds, top-level token rules, node rules,
  `conventional()` preset), `NodeRule` (group, `Indent::{Block, Hanging}`,
  before/after spacing, delimiters with inner and empty-body spacing,
  separator with `Trailing::{Preserve, Always, Never}` and insertion text,
  blank-line cap, context token rules, optional), `TokenRule` (exact kind or
  any token, before/after/around, optional), `Space::{None, Single, SoftLine,
  Line, Hard}` as a join lattice. `Rules::compile` resolves names once into a
  `Style<K>` (rules sorted by kind, binary-searched once per node and per
  token); unknown names are `RuleError`s unless optional.
- **Comment attachment** (trailing / leading / dangling), documented on
  `format` and in `src/walk.rs`: comments never cross a significant token, are
  written unchanged, and a comment followed by a line break keeps it.
- **Walk and render**: one iterative flatten pass (validates the tree against
  the source, collapses verbatim nodes, settles list delimiters and separator
  edits), then an iterative walk building one `pretty_lang::Doc` (text runs
  merged, gaps written into the innermost node holding both neighbours),
  rendered at a width. Error nodes are verbatim; unspecified gaps keep their
  whitespace. `format` (Tier-1), `format_keeping` (regions of lexical errors
  kept byte for byte), `format_doc`, `can_touch` / `Style::with_touch` (the
  test that keeps tokens from fusing).
- **Guarantees, property-tested** (`tests/properties.rs`) over random valid
  and invalid sources of two languages forged with lang-forge 1.0.1 (a
  JSON-like and a calc-like language) and over arbitrary hand-built trees:
  idempotence, significant-token preservation (modulo trailing-separator
  policies), every comment exactly once and in order, line comments end their
  line, no trailing spaces introduced, no panics (hostile spans included),
  determinism, and no-rules formatting against a reference implementation over
  the lexer's flat token list. Soaked at 400,000 cases per property.
- Benches at roughly 10k, 100k, and 1M nodes plus a 100k-deep tree; the
  `format_json` example.

Found and fixed by the property soak while building this milestone (each has a
regression test in `tests/format.rs`): an inserted trailing separator glued to
the element instead of spaced like a written one; a lone `\r` at the end of a
token or comment fusing with the next line break; separator edits in lists
whose element had a recovered parse error (unclosed delimiter); unterminated
strings and comments changed by removing or adding a line break (now the
kept-region contract).

Dependency wiring (decided here, recorded per the anti-deferral rule):
- **syntax-lang 1: wired.** The input tree (`Node`, `Element`, `Token`,
  `TokenKind`, `Span`); re-exported whole.
- **pretty-lang 1.0.1: wired.** The layout algebra; `format_doc` returns its
  `Doc`; re-exported whole.
- **lang-forge 1.0.1: dev-dependency only** (tests, benches, example). The
  formatter is generic over any syntax-lang tree; it does not depend on how the
  tree was produced.
- **diag-lang: not wired.** Kept regions are plain `Span`s, so the formatter
  needs no diagnostic type; a caller maps its diagnostics to spans.

Moved out of v0.2.0 (anti-deferral record):
- **Range formatting** (format only the nodes covering a byte range, for LSP
  `rangeFormatting`) -> **v0.5.0**, as the task for this milestone allowed. The
  gap model already makes it local (a gap depends only on its two neighbours,
  the nodes around them, and its own trivia), so the work is choosing the
  covering nodes and splicing; it is listed below.
- **Trailing separator "only when the list breaks"** -> **blocked on
  pretty-lang**. It needs a document that renders differently in a group's
  flat and broken modes (`if_break`); pretty-lang 1.x has only text, line,
  softline, hardline, nest, and group. Recorded for pretty-lang 2.0 (the
  LexerSketch plan already schedules pretty-lang 2.0 next to fmt-lang). Until
  then `Trailing` offers `Preserve`, `Always`, and `Never`.

## v0.5.0 - Implementation
- [ ] Range formatting for editors (moved here from v0.2.0, see above): the
      smallest node sequence covering a byte range, formatted with its
      surrounding indentation, returned as an edit.
- [ ] Alignment (columns of `=`/`:` in consecutive lines) and further breaking
      policies (fill, one-per-line when any breaks).
- [ ] `Trailing::IfBroken` once pretty-lang offers a break-conditional
      document (see above).
- [ ] Speed. v0.2.0 baseline (Windows, noisy machine): ~1.2M nodes/s at 792k
      nodes, ~3.1M nodes/s at 8k. Measured split at 792k: ~15% flatten, ~45%
      building the `Doc`, ~15% rendering, ~25% dropping it; the cost is
      pretty-lang's one reference-counted allocation per document node. Options:
      fewer document nodes (merge more text runs, share break nodes across
      groups), or an arena-backed `Doc` in pretty-lang 2.0. Re-measure
      against this baseline.

## v0.9.0 - Hardening
- [ ] Fuzzing; audit with the LexerSketch LSP formatting request as consumer.
- [ ] An exact touch test generated from a forged language's symbol table
      (replacing the `can_touch` heuristic for forged languages; belongs in the
      LexerSketch adapter, consumed here through `Style::with_touch`).

## v1.0.0 - Stable
- [ ] Frozen after three target languages format through it (D18).
