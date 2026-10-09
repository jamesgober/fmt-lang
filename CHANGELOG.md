<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>fmt-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

---

## [0.2.0] - 2026-10-08

The foundation: a formatter driven by declarative rules over a lossless
`syntax-lang` tree, rendering through `pretty-lang`. Rules are data keyed by
kind names, so a language gets a formatter from configuration, not code.

### Added

- `format(tree, source, style, width)`, the Tier-1 entry point;
  `format_keeping` (leaves the regions of lexical errors byte for byte as
  written); `format_doc` (returns the `pretty_lang::Doc`).
- The rule model: `Rules` (indent step, `max_indent` output budget,
  blank-line cap, final newline, verbatim kinds such as `ERROR`, top-level
  token rules, node rules, the `conventional()` preset), `NodeRule` (group,
  `Indent::Block`/`Hanging`, before/after spacing, delimiters with inner and
  empty-body spacing, separator with a `Trailing` policy and insertion text,
  blank-line cap, context token rules), `TokenRule` (one kind or any token),
  and `Space`, a join lattice of `None`/`Single`/`SoftLine`/`Line`/`Hard`.
- `Rules::compile` resolves kind names against a language into a `Style<K>`;
  `RuleError` reports unknown kinds (unless a rule is `optional`), duplicate
  node rules, an over-wide indent step, and inconsistent list rules.
- Comment attachment (trailing, leading, dangling), documented on `format`:
  comments never cross a token, keep their text, and a line comment always ends
  its line.
- Unspecified gaps keep their original whitespace (trailing spaces at line ends
  removed); error nodes are written verbatim; trailing-separator edits apply
  only to well-formed, error-free lists.
- `can_touch` and `Style::with_touch`: the test that keeps a rule from fusing
  two tokens (`- -x` never becomes `--x`).
- `FormatError` for trees that do not describe their source (out of bounds,
  inside a character, gaps or overlaps between tokens).
- Property tests for idempotence, token and comment preservation, line
  comments ending their line, no trailing spaces, no panics, and determinism,
  over random valid and invalid sources of two lang-forge languages and over
  arbitrary trees, plus a reference implementation for formatting with no
  rules; 36 behaviour tests; benches at about 10k, 100k, and 1M nodes and a
  100k-deep tree; the `format_json` example.
- Dependencies: `syntax-lang` 1 and `pretty-lang` 1.0.1 (both re-exported);
  `lang-forge` 1.0.1 as a dev-dependency.

### Changed

- Version 0.2.0. `README.md`, `docs/API.md`, and `dev/ROADMAP.md` describe the
  foundation surface.

---

## [0.1.0] - 2026-10-08

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/fmt-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/fmt-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/fmt-lang/releases/tag/v0.1.0
