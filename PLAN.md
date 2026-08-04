# deCPLD implementation plan

The milestone sequence from [SPEC.md](SPEC.md) §11.2, expanded into checkable steps. **Tick a box in the same PR that delivers it**, with a clickable PR link — a merged PR whose plan item is still unchecked is a process bug (see [CLAUDE.md](CLAUDE.md) → merge ceremony).

The ordering is deliberate: JEDEC and the device model come *before* the language. Reading and writing a real ATF22V10 fuse map, and proving the round-trip, is what makes every later phase checkable. A language that compiles to an unverified encoder is a language that compiles to nothing trustworthy.

Crates are created by the milestone that needs them; SPEC.md §0.3 records the full target layout.

---

## M-1 — Repository bootstrap

- [x] GitHub repository, dual MIT OR Apache-2.0 licensing, `.gitignore`, pinned toolchain — [#1](https://github.com/enthal/decpld/pull/1)
- [x] Cargo workspace and the placeholder `decpld` binary — [#2](https://github.com/enthal/decpld/pull/2)
- [x] Markdown no-hardwrap lint; SPEC.md soft-wrapped — [#3](https://github.com/enthal/decpld/pull/3)
- [x] CI (fmt, clippy, test, markdown) on Linux, pre-commit hook, PR template — [#4](https://github.com/enthal/decpld/pull/4), [#6](https://github.com/enthal/decpld/pull/6)
- [x] Branch protection: PRs only, linear history, required status checks, signed commits — [#4](https://github.com/enthal/decpld/pull/4)
- [x] CLAUDE.md, README.md, this plan, and the `merge-review` skill — [#5](https://github.com/enthal/decpld/pull/5)
- [x] `targets/evidence/`: primary references recorded with hashes, plus a verifier — [#8](https://github.com/enthal/decpld/pull/8)

## M0 — JEDEC foundation

Parse, validate, canonicalize, and rewrite known JEDEC files with correct checksums. No device knowledge at all — `decpld-jedec` must work against a part it has never heard of.

- [x] `decpld-jedec` crate: `JedecFile` model, STX/ETX framing, `QF` / `F` / `L` / `C` / `N` / `G` fields — [#11](https://github.com/enthal/decpld/pull/11)
- [x] Fuse checksum and transmission checksum, from the standard's own worked examples — [#10](https://github.com/enthal/decpld/pull/10)
- [x] Parser modes: strict, compatible, preserve-unknown — [#12](https://github.com/enthal/decpld/pull/12)
- [x] Writer styles: canonical and compact — [#13](https://github.com/enthal/decpld/pull/13). WinCUPL-comparable deferred to M1: matching it requires real WinCUPL output to match against
- [x] Line-ending and whitespace variants; unknown fields preserved and reported — [#12](https://github.com/enthal/decpld/pull/12)
- [x] Property test: round-trip over random valid fuse vectors — [#13](https://github.com/enthal/decpld/pull/13)
- [x] `decpld jed validate` / `canonicalize` / `diff` wired into the CLI — [#14](https://github.com/enthal/decpld/pull/14), [#15](https://github.com/enthal/decpld/pull/15) (exit codes), [#17](https://github.com/enthal/decpld/pull/17) (diagnostics to stderr)
- [x] `decpld-diagnostics`: `Span`, `FileId`, `LineIndex`, `Diagnostic`, severity, labels, notes, fixes, and stable diagnostic codes — [#9](https://github.com/enthal/decpld/pull/9), [#17](https://github.com/enthal/decpld/pull/17) (line/column correctness)

M0 hardening, from the sub-agent reviews over the milestone's diff:

- [x] Silent-corruption fixes: bare-CR line termination, offsets inside a multi-byte unit, a writer that could emit text it could not reparse — [#17](https://github.com/enthal/decpld/pull/17)
- [x] Contradictory `F` and `G` fields refused rather than resolved by reading order — [#17](https://github.com/enthal/decpld/pull/17)
- [x] JEDEC identifier tables re-transcribed with locators; reserved identifiers honoured — [#25](https://github.com/enthal/decpld/pull/25)
- [x] Writer refuses unencodable content by the standard's `<field character>` class, and the parser reports it at an offset — [#25](https://github.com/enthal/decpld/pull/25)
- [x] A file with no `F` field no longer silently means `F0` — [#26](https://github.com/enthal/decpld/pull/26)
- [x] `apply_fuse_list` applies atomically — [#25](https://github.com/enthal/decpld/pull/25)
- [x] Fields with no identifier reported and preserved rather than silently deleted — [#25](https://github.com/enthal/decpld/pull/25)
- [x] SPEC.md sync and property-test coverage gaps — [#27](https://github.com/enthal/decpld/pull/27)

## M1 — ATF22V10 decoder and encoder

Decode WinCUPL and Galette output into macrocells and equations; encode canonical equivalents; satisfy the round-trip invariants. Still no language.

- [ ] `decpld-device`: `DeviceTarget` trait, `FuseMap` with write tracking, `FuseRegion`, `ConfigField`, `AndMatrixSpec`, `MacrocellSpec`, `PackageSpec` — `FuseMap`, `FuseRegions`, `FuseId`, `FuseMutability` landed in [#28](https://github.com/enthal/decpld/pull/28); `PackageSpec`, `PackagePin`, `PinNumber`, `PadId`, `ClockResourceId`, `InputResourceId`, `PowerRail` in [#36](https://github.com/enthal/decpld/pull/36); the rest follow
- [ ] `decpld-atf22v10`: DIP-24 package map, global clock, AND matrix, ten macrocells — every field citing its evidence. Fuse regions, array geometry, column and row mapping, and the three JEDEC footprints landed in [#31](https://github.com/enthal/decpld/pull/31); the array's fuse addressing, measured rather than assumed, in [#35](https://github.com/enthal/decpld/pull/35); the DIP-24 package map and the global clock in [#36](https://github.com/enthal/decpld/pull/36); the matrix and macrocell specs follow
- [ ] Encode: cube encoding with contradiction rejection, then row decode-and-verify
- [ ] Decode: fuse vector → `PhysicalDesign`
- [ ] Invariants: matrix cells map 1:1 to fuses, no undocumented field overlap, every fuse classified, every legal configuration round-trips
- [ ] `decpld jed inspect --device ATF22V10C` with a `--json` form
- [x] WinCUPL oracle harness: runner, run-metadata capture, scratch run directories — [#30](https://github.com/enthal/decpld/pull/30). Lives at `targets/experiments/<device>/run.sh`, not `tools/wincupl/`, and keeps run output in a scratch directory rather than committed fixture directories (SPEC.md §7.7)
- [x] Differential experiment suite: literal mapping, polarity, registered, feedback, macrocell, fuse-count modes, signature — [#30](https://github.com/enthal/decpld/pull/30); pin roles, shared clock/input, I/O-as-input, and expected-failure supply-rail designs in [#36](https://github.com/enthal/decpld/pull/36). OE and capacity still to do
- [ ] `decpld oracle diff` with delta classification
- [ ] Evidence level recorded per field; `targets/evidence/` populated — ATF22V10 fuse map measured and recorded in [#30](https://github.com/enthal/decpld/pull/30), with the complete fuse-address map added in [#35](https://github.com/enthal/decpld/pull/35); per-field levels land with the target definition

## M2 — Minimal combinational language

`signal`, pins, Boolean expressions, one `top`, SOP minimization, fitting — ending in working combinational hardware.

- [ ] `decpld-syntax`: lexer and lossless (Rowan-style) parser with error recovery; parser snapshot tests
- [ ] `decpld-package`: package index, source-root discovery, visibility, duplicate-name diagnostics
- [ ] `decpld-hir` + `decpld-types`: name resolution, widths, signedness, no implicit narrowing
- [ ] Producer inference: one producer per bit, combinational-cycle rejection
- [ ] `decpld-rtl`: RTL IR, lowering, and the safe optimization passes (`-O0` / `-O1` / `-O2`)
- [ ] `decpld-logic`: hash-consed Boolean graph, SOP, minimization with a mandatory equivalence check
- [ ] Output polarity optimization: minimize both `f` and `!f`, choose the cheaper
- [ ] ATF22V10 fitter: deterministic backtracking, stable ordering, `FitCost`, resource-naming failures
- [ ] Physical validation, then encode → decode → compare in the driver
- [ ] `decpld build` end to end; `decpld dump --stage …` for every phase
- [ ] `decpld fmt`, idempotent, two-space indentation, comment-preserving
- [ ] **Hardware:** a combinational design programmed and verified on a physical ATF22V10

## M3 — Registered logic

- [ ] `on posedge`, clock-domain rules, one clocked assignment site per bit
- [ ] Hold lowering: omitted assignment → D-input feedback mux, never a latch
- [ ] Registered macrocell mode, feedback selection, global clock validation
- [ ] `decpld-sim`: RTL simulator with the specified cycle semantics and `Z` on pads
- [ ] Physical simulator over the decoded fuse map; RTL and decoded results must agree
- [ ] `decpld sim --vectors`
- [ ] **Hardware:** counter and shift-register on a physical ATF22V10, including hold and synchronous reset

## M4 — Modules and parameters

- [ ] Modules, `port`, `param`, named arguments only, `require` constraints
- [ ] Affine integer constraint solver; ambiguity and inconsistency rejected, never guessed
- [ ] `if` and `match` expressions, including priority-condition `match`
- [ ] Concatenation, slicing, destructuring, pre-edge evaluation inside `on posedge`
- [ ] Enums with deterministic minimum-width encoding, reported
- [ ] Package mode: `decpld.toml`, multiple source roots, path dependencies
- [ ] `--top` selection and its diagnostics

## M5 — ATF16V8

- [ ] `decpld-atf16v8`: registered / complex / simple global mode model with per-mode resources
- [ ] Mode-aware fitting: try every legal mode, record why each rejection happened, choose deterministically
- [ ] Pin 1 / pin 11 role reservation per mode
- [ ] Bidirectional pads and product-term OE (`when`) across modes
- [ ] Differential fixture suite covering all three modes
- [ ] **Hardware:** registered-mode, complex-mode OE, and simple-mode designs on a physical ATF16V8

## M6 — Language server

- [ ] `decpld-lsp` on `tower-lsp` with a Salsa-style incremental query layer
- [ ] Diagnostics without a target; target-aware pin and fitting diagnostics with one
- [ ] Semantic tokens, completion, hover, go-to-definition, find-references, rename, symbols
- [ ] Formatting identical to the CLI, byte for byte
- [ ] Signature help for module instances; inlay hints for inferred parameters
- [ ] Code actions: missing `else`, missing match arms, explicit narrowing slice, missing arguments, compatible-pin suggestions after a fit failure
- [ ] Debounced fitting with stale-work cancellation

## M7 — Release quality

- [ ] Fuzzing: lexer/parser and the JEDEC parser — no panics, no unbounded allocation
- [ ] Reproducible builds verified; timestamps excluded in reproducible mode
- [ ] Stable JSON report schema
- [ ] `decpld program` convenience command — explicit, logged, never automatic
- [ ] Security-fuse path behind both `--security-fuse` and `--acknowledge-readback-lock`
- [ ] Packaging and installation
- [ ] Complete evidence and hardware matrix; every writable fuse classified and explained
- [ ] [SPEC.md](SPEC.md) §13.4 definition of done satisfied, item by item
