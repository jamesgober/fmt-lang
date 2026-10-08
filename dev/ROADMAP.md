# fmt-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
> Master plan: ../_lexersketch/ROADMAP.md and ../_lexersketch/NEW-LIBS.md
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt, DIRECTIVES, ROADMAP.

## v0.2.0 - Foundation
- [ ] Rule model, walk over syntax-lang trees, comment attachment, rendering via pretty-lang.
- [ ] Property tests: idempotence, token preservation.

## v0.5.0 - Implementation
- [ ] Alignment, breaking policies, range formatting for editors; benchmarks.

## v0.9.0 - Hardening
- [ ] Fuzzing; audit with the LexerSketch LSP formatting request as consumer.

## v1.0.0 - Stable
- [ ] Frozen after three target languages format through it (D18).
