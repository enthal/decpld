# Claude Code Instructions

## The project

deCPLD (pronounced like *decoupled*) is a Rust compiler and hardware-description language for Microchip **ATF22V10** and **ATF16V8** simple programmable logic devices. It replaces WinCUPL for design entry, elaboration, synthesis, fitting, inspection, and JEDEC generation. The language is `deCPLD` (`*.decpld`), the compiler is `decpld`, the language server is `decpld-lsp`, and the primary output is a JEDEC fuse map (`*.jed`).

The compiler ends at a valid `.jed` file. High-voltage programming algorithms and programmer hardware protocols are outside it; `minipro` and friends are invoked only through an explicit, never-automatic convenience command.

The repo is **pre-implementation**: what exists today is the spec, the licence, the toolchain pin, CI, and a placeholder `decpld` binary. [PLAN.md](PLAN.md) tracks the milestone sequence; the first feature PRs deliver **M0 — JEDEC foundation**. Work in conversation with the user, and don't race ahead to later milestones before the current one is real.

## The spec is the source of truth

- [SPEC.md](SPEC.md) is canonical and authoritative. Read the relevant section before writing code that touches an area — it is normative, not aspirational, down to the Rust type shapes.
- When code and spec disagree, **raise the conflict with the user** and decide whether to change the code or the spec. Never silently drift from the spec.
- Any normative change (a language rule, a type shape, an IR node, a device field, a fuse mapping, a diagnostic code, a CLI flag, a JEDEC behavior) MUST update SPEC.md **in the same commit**. A normative change that ships without a spec update is a process bug, not a shortcut.
- The definition of done is [SPEC.md §5.34](SPEC.md). Read it when you need to know whether something is finished.

## The one architectural rule

> Each layer knows only its own vocabulary. Language semantics do not know about product terms. Target-independent IR does not know about fuses. Targets and fitters do not know about JEDEC syntax. JEDEC does not know what a macrocell is.

```text
Language semantics          define logical behavior
Target-independent IR       preserve and optimize behavior
Typed Rust targets/fitters  map behavior to product terms, macrocells, clocks, feedback, pins
JEDEC encoding              serialize the physical configuration for a programmer
```

Dependencies flow one way, downward. Concretely:

- **Nothing above the device layer may contain a fuse number, a macrocell index, or a product-term count.** If a fuse number appears outside `decpld-device` / `decpld-atf22v10` / `decpld-atf16v8`, the layering is broken.
- **Nothing below the HIR may know what a `signal`, a `module`, or a `param` is.** RTL and Boolean IR speak in values, widths, and Boolean functions.
- **JEDEC transfers numbered fuse/cell states; it does not define architecture.** `decpld-jedec` must be usable against a device it has never heard of. If it needs to know what fuse 5808 *means*, that knowledge belongs in the target.
- **The CLI and LSP share everything.** Both use the same parser, package index, resolver, elaborator, type checker, diagnostics, and target registry. If a behavior differs between `decpld check` and the editor's red squiggle, that is a bug in the layering, not a feature of the front end.

If you find yourself reaching downward for convenience — a fitter shortcut in the type checker, a device special case in the optimizer — stop. The boundary is the product.

## Verification is the product

A compiler bug here does not produce a stack trace. It produces a programmed chip that behaves subtly wrong in a circuit, and the user debugs their *hardware* for a week. **A wrong fuse is a corruption-class bug.** Everything below follows from that.

- **Never emit an unchecked minimized equation.** Every minimization result is verified against its source Boolean function — exhaustively for small support sets, by SAT/BDD beyond that (SPEC.md §3.9).
- **Encode, then decode, then compare.** The compile driver generates fuses, decodes them back into a physical design, and asserts equivalence with the design it intended, before writing any file (SPEC.md §5.27). This round-trip is enabled by default and must stay that way.
- **Every legal configuration round-trips.** Encode/decode is a device-model invariant, tested per macrocell, per mode, per polarity — not just on the designs that happen to appear in examples.
- **Validate physically before encoding** (SPEC.md §5.4): every output placed, no resource double-booked, every literal routable, reserved fuses unchanged, reconstructed equations equal to the requested mapped design.
- **The RTL simulator and the decoded-fuse simulator must agree** — exhaustively for small designs, randomized for larger ones. Two independent models disagreeing is how a mapping error announces itself.
- **Prefer a rejected build to a wrong one.** Where the compiler cannot prove it did the right thing, it must refuse and say why. Silently producing plausible fuses is the worst available outcome.

## Device knowledge is evidence-based

The ATF22V10 and ATF16V8 fuse maps are the project's factual bedrock, and getting one bit wrong is invisible until hardware misbehaves.

- **Never enter a numeric fuse position from memory, inference, or a plausible-looking pattern.** Not yours, not the user's, not another tool's. This is the single easiest way to poison the project.
- **Every fuse mapping cites its evidence in a source comment**, and the exact document revisions and hashes live in `targets/evidence/`. Primary references are listed in SPEC.md §5.33.
- **A mapping becomes verified only when independent sources agree**: official documentation, open-source cross-checking (Galette, GALasm), controlled WinCUPL differential experiments, encode/decode invariants, and — the highest bar — physical hardware tests. `EvidenceLevel` (SPEC.md §5.31) records which of these a field has actually reached.
- **Unverified hypotheses belong in oracle-analysis code or disabled experimental targets**, never in a production target definition.
- **WinCUPL is an oracle, not an authority.** It is one witness among several and is not assumed infallible. Triangulate. Production compilation must never require WinCUPL, Wine, or Windows.
- **Do not redistribute proprietary WinCUPL files**, device libraries, or embedded serial numbers. `.gitignore` blocks the install tree. WinCUPL's own output is not committed either: what goes in the repository is the experiment *input* and *recipe* — our `.pld` sources, the run metadata, and a runner — plus the measurements read out of the result, recorded in `targets/evidence/`. Freely-redistributable cross-checks (Galette, GALasm) are a separate case and their output may be committed with attribution. Oracle runs record wine version, WinCUPL version, executable and library hashes, and the exact command line (SPEC.md §5.9).

## Determinism is a first-class requirement

SPEC.md §5.32 makes it normative: the same source, compiler, target database, and options produce the same fuse vector. That forces rules that touch nearly every phase.

- **No ambient nondeterminism in compiler code.** No `SystemTime::now()`, no `Instant::now()`, no unseeded RNG, no hashing that leaks address randomness into output. Timestamps are excluded in reproducible mode.
- **No `HashMap` iteration order in anything that reaches output.** Use `BTreeMap` / `IndexMap` for symbol tables, package scopes, fuse regions, encodings, and report structures — SPEC.md's type shapes already say so; follow them.
- **The fitter is deterministic.** Backtracking search with a stable sort key (SPEC.md §5.2) and deterministic tie-breaking on cost, ending in the stable hierarchical name. Two runs on one input pick the same macrocell.
- **The minimizer is deterministic.** Where an exact method is impractical, the heuristic must still be deterministic; "good enough and stable" beats "slightly smaller and variable".
- **`decpld fmt` is idempotent**, and produces byte-identical output through the CLI and the LSP formatting request.

## Safety

- **The security fuse is clear by default.** Setting it requires *both* `--security-fuse` and `--acknowledge-readback-lock`, because it permanently prevents reading the device back.
- **A change to a reserved fuse is a hard error**, never a warning.
- **The build never applies programming voltage.** Programming is a separate, explicit, logged command, never a side effect of `build`.
- **`FuseMap` tracks writes** so two encoders silently fighting over the same fuse is detected, not averaged.

## Tests and discipline — hybrid rule

Split by layer: the cost of getting a fuse encoding wrong is not the cost of a mis-formatted report line.

### Strict layer — test written FIRST, same commit

Write a failing test **before** the implementation it covers; it must fail (or not compile) on the pre-change tree. That is how you know the test validates behavior rather than transcribing it.

**The unit is one change, not one milestone.** This does not mean writing a milestone's tests up front — that would lock interfaces before they are designed. It means each thing you add or change gets its test immediately before it. The interface commitment stays one change wide, so a later redesign rewrites a handful of tests rather than a suite.

**Establish the expected answer before you write the test.** A test is only as good as the evidence behind it, and there is almost always a source of truth to consult first. The order is *discovery → test → implementation*:

- **Device behavior and fuse mappings:** run the WinCUPL oracle experiment **first**, then write the test from what it reported, then write the code. Cite the fixture or run in the test so that if the oracle is later shown wrong, every test that trusted it can be found. Remember the oracle reports what WinCUPL does, which is evidence of correctness, not proof of it — triangulation still applies.
- **Language semantics, JEDEC, and IR:** SPEC.md is the oracle. Cite the section.
- **A bug fix:** the reproduction is the expected answer.

**If you are not certain the expected answer is right, say so in the test.** An `// UNVERIFIED:` comment naming what you are unsure of and what would settle it is honest and greppable. A confidently wrong test that nobody flagged costs far more than a hedged one.

**Experimenting in real code first is allowed, and should be rare.** If you genuinely cannot tell what the code should do until you have felt it out, do that — then throw the spike away and start again from the test. Needing this often is a signal that you are working ahead of the spec, not a signal that the rule is wrong.

Applies to:

- **JEDEC** — parsing, writing, fuse checksum, transmission checksum, `QF`/`F`/`L`/`C`/`G`/`N` fields, unknown-field preservation, line-ending and whitespace variants.
- **Device models** — fuse region classification, AND-matrix cube encoding, macrocell config fields, polarity, feedback selection, and encode/decode round-trip for every legal configuration.
- **Boolean and SOP** — minimization correctness (always paired with an equivalence check), true-vs-complement cover selection, cube contradiction rejection.
- **Semantics** — producer inference, one-producer-per-bit, clock domain rules, hold-vs-latch lowering, combinational-cycle rejection, width and signedness rules, parameter constraint solving.
- **Fitting** — placement, resource exhaustion boundaries, ATF16V8 global mode selection and the rejection reason for every mode that failed.
- **The formatter** — idempotence and comment preservation.
- **Any bug fix anywhere:** reproduce with a test that fails on the pre-fix tree, fix it, watch it go green.

### Pragmatic layer — test same commit, order doesn't matter

Everything else: CLI argument plumbing, report text layout, stage-dump formatting, LSP protocol glue. **If the logic can be tested without going through the CLI or the LSP, it must not live inside a CLI or LSP function** — extract it into a pure function and test that. Index arithmetic, name formatting, ordering decisions, and any `if/else` on computed values all belong in testable functions.

### What never moves layers

- "I'll add tests later" means there will be no tests. Both layers ship tests in the same commit.
- A passing test written after the code is a smell — it may assert what the code does, not what it should do.
- **Determinism in tests:** no `now()`, no unseeded random, no wall-clock. A flaky test is a bug in the test or the code, never "just the runner".

### Test tools (which for which job)

- **Unit** (`#[cfg(test)]` in-module): pure logic. The default and minimum bar.
- **Equivalence** (exhaustive or SAT/BDD): the mandatory companion to every minimization and every mapping transformation. A minimizer test that only checks cube *count* has tested nothing that matters.
- **Property** (`proptest`): JEDEC round-trip over random valid fuse vectors, encode/decode round-trip over random legal configurations, formatter idempotence, `!!f == f` through polarity selection.
- **Golden fixtures** (`targets/fixtures/`): checked-in `.jed` and normalized oracle output. Regenerate deliberately and `git diff` before committing. Compare at the level SPEC.md §5.25 defines — `ExactFuseVector` or `ExactPhysicalConfiguration` for normal acceptance, `ExactFile` only for deliberately pinned oracle experiments.
- **Snapshot** (`insta`): diagnostics, fit reports, and `decpld dump --stage …` output. These are the compiler explaining itself; a regression in the explanation is a real regression.
- **Integration** (`tests/`): end-to-end through the CLI, source file to `.jed`.
- **Fuzz** (later milestones): the lexer/parser and the JEDEC parser. Malformed input must never panic or allocate unbounded.
- **Hardware** (SPEC.md §5.15): the final authority. Record part marking, programmer version, JEDEC hash, vectors, and results.

## Build & test

- **Build:** `cargo build --workspace`
- **Test:** `cargo test --workspace`
- **Lint:** `cargo clippy --workspace --all-targets -- -D warnings`
- **Format:** `cargo fmt --all`
- **Docs:** `cargo doc --workspace --no-deps`
- **Markdown lint:** `scripts/markdown-no-hardwrap.sh`

Run `cargo fmt`, `cargo clippy`, and `cargo test --workspace` before every commit. Treat clippy warnings as errors.

`fmt` + `clippy` + the markdown lint run in **both** the developer-installable pre-commit hook and CI; the full `cargo test` suite is a **CI-only** gate. Install the hook with `scripts/install-git-hooks.sh` (it symlinks `scripts/git-hooks/*` into the shared hooks dir, so one install covers every worktree). The hook deliberately does not run the test suite: the tests-first discipline means failing tests are committed on purpose, so a hook that blocked them would block the workflow itself. CI runs the full suite and gates merge — behavioural failures still block the world, just at the right boundary. Bypass with `git commit --no-verify` only if the user explicitly says so.

## Design principles

Operational reminders; the rationale is in the spec.

- **Make wrong states unrepresentable.** Prefer a type that cannot express an invalid state over a runtime guard. A cube that has already been checked for contradictions should be a different type from one that hasn't. A fitted design should not be constructible without having passed validation. If an invariant requires "always do X before Y", put both in one function so callers cannot forget.
- **Fix bugs structurally, not with guards.** When a bug is stale or inconsistent state across a transition, replace the loose state with a struct updated atomically — don't sprinkle a check at the call site.
- **The compiler explains itself.** Every phase has a debug dump (`decpld dump --stage ast|hir|rtl|boolean|sop|placed|fuses`). Every fit failure identifies the *limiting resource* and actionable alternatives — "no ATF16V8 mode can implement this design" is useless without the reason each mode was rejected. `decpld jed inspect` must make a fuse map readable as macrocells and equations. Making the device's real architecture visible is a feature, not debug output.
- **Conservative and transparent beats clever.** Support a compact language completely and verify every transformation, rather than supporting more and trusting more. Reject ambiguity (underconstrained parameters, nonlinear constraints, combinational cycles) rather than guessing.
- **The source is authoritative for the target.** `device` inside the selected `top` decides the device and package; CLI `--device` / `--package` are *checked assertions* that must agree, never overrides.
- **One producer per signal bit** (SPEC.md §2.2) and **no inferred latches** (§2.3) are load-bearing language invariants. A regression against either is a P0 bug.
- **Declaration order never matters** (§2.5). Index files and declarations before resolving bodies. If a change makes file order or declaration order observable, it is wrong.

## Code style

- **Typed IDs for everything durable:** `FileId`, `FuseId`, `MacrocellId`, `ProductTermId`, `PadId`, `PinNumber`, `PackageId`, `ClockResourceId`, `BoolNodeId`, `BoolInputId`, `ValueId`, `LogicId`, `InstanceId`, `ModuleId`, `ParamId`, `PortId`, `LogicalOutputId`, `ConfigFieldId`. These are newtypes, not interchangeable, and the type system should say so. A `u32` that is sometimes a fuse index and sometimes a pin number is exactly the bug class this project cannot afford.
- **No `unsafe`.** Enforced centrally by `unsafe_code = "forbid"` in `[workspace.lints.rust]`, so a new crate cannot forget it. deCPLD is a pure data transformation with no FFI and no hardware access; if you want `unsafe`, stop and ask.
- **No `unwrap()` / `expect()` in non-test code**, except where a contract makes failure impossible — and even then prefer a typed `Result`. `expect` messages describe the invariant, not the operation.
- **Compile-time integers are arbitrary-precision** (`BigInt`) inside the compiler. Parameter arithmetic must not silently wrap; that is the language's own promise about widths turned inward.
- **Map naming:** `things_by_key` for a `Map<key, thing>` **field**. Nested: `things_by_inner_by_outer` means `Map<outer, Map<inner, thing>>` (read right-to-left). For collection values include the container: `cube_vecs_by_macrocell`. **`_by_` is for maps only** — a singular accessor is named for what it returns (`fn macrocell(&self, id)`), and if the key must be named use `_of_`: `fn pad_of_pin(...)`.
- **Errors identify object context.** A diagnostic names file, span, and the design object — top, instance path, signal, bit, pin, macrocell, product term — wherever it can. `error[E1302]: registered signal requires ATF22V10 pin 1 as global clock` is the bar. Errors are structured Rust types with a code and labels; the CLI and LSP render them (SPEC.md §5.18).
- **Diagnostic codes are stable.** Once `E0204` means "value does not fit", it always means that. Add new codes; don't renumber.
- **Structural safety over incidental correctness:** `char_indices()` over byte slicing; a lossless (Rowan-style) syntax tree so comments and malformed nodes survive formatting and LSP recovery; access fuse state through `FuseMap` methods, never a raw bit vector.
- **Markdown is never hard-wrapped.** One logical line per paragraph, list item, and block-quote — let the renderer soft-wrap. Do not insert newlines mid-paragraph to hit a column width; it carries no meaning and churns diffs. Applies to every `.md` file, SPEC.md included. Enforced by `scripts/markdown-no-hardwrap.sh` in CI and the pre-commit hook.

## Discussions with the user

When mentioning a GitHub Issue or PR, always use the Issue/PR number as a clickable link (e.g. `[#42](https://github.com/enthal/decpld/issues/42)`).

## Git commit protocol

- **Commit messages follow [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/).** Every subject is `<type>[optional scope]: <description>`, where `type` is one of `feat`, `fix`, `build`, `chore`, `ci`, `docs`, `style`, `refactor`, `perf`, `test`. Use a scope — usually the crate — when it helps: `feat(jedec): …`, `fix(atf16v8): …`. Breaking changes take `!` after the type/scope and/or a `BREAKING CHANGE:` footer.
- **Every commit is signed.** `main` requires signed commits, so this is enforced, not advisory. The repo is configured for SSH signing (`gpg.format = ssh`, `commit.gpgsign = true`, `tag.gpgsign = true`) against `~/.ssh/github.pub`, with `~/.ssh/allowed_signers` so `git log --show-signature` verifies locally as well as signing. A fresh clone needs that config re-applied; `git verify-commit HEAD` is the quick check that it took. Never disable signing to get a commit through — if signing fails, fix the key or the agent.

Before every commit:

1. **Tests first where the strict layer applies.** The test(s) MUST be present and MUST have failed on the pre-change tree. Mention the test in the commit body when useful.
2. **Format.** `cargo fmt --all`; stage the result.
3. **Lint.** `cargo clippy --workspace --all-targets -- -D warnings` must pass.
4. **Test.** `cargo test --workspace` must pass.
5. **Spec sync.** Any normative change updates SPEC.md in the same commit.
6. **Evidence.** Any new or changed fuse mapping cites its evidence in a source comment and records its `EvidenceLevel`.
7. **Include config changes.** If `CLAUDE.md`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, or `rustfmt.toml` changed, include them.

Commits should be small and reviewable. A commit that touches more than one crate without a clear reason is a signal to split.

## Branches and pull requests

- **Never commit directly to `main`.** All changes land via a feature branch → PR → squash merge. `main` is protected: PRs required, linear history, required status checks, enforced for admins.
- **Branch naming:** `<kind>/<slug>` where `kind` is one of `feat`, `fix`, `refactor`, `spec`, `docs`, `chore`, `ci`, `test`. Dashes in the slug, not underscores. Examples: `feat/jedec-parser`, `spec/sharpen-oe-inference`, `fix/cube-contradiction`.
- **Start the branch first**, from an up-to-date `main`: `git switch -c <kind>/<slug>`. If you catch yourself having already committed on local `main`: `git switch -c <kind>/<slug>` (takes the commits with you), then `git switch main && git reset --hard origin/main`.
- **One PR per logical change.** Small and reviewable. If a GitHub Issue exists, include `Closes #<n>` in the description.
- The PR summary reflects all commits on the branch, not just the latest.
- **Merge ceremony.** Before squash-merging any PR, in order: (1) run the [`merge-review`](.claude/skills/merge-review/SKILL.md) skill over the branch diff — it spawns read-only reviewers, and if sub-agents are gated off in this session, say so and ask rather than quietly reviewing your own diff — and handle its findings — fix the worthwhile ones (spec-syncing in the same commit), or defer the rest out loud with a reason; (2) confirm `fmt` + `clippy` + `test` are green and CI has passed; (3) update [README.md](README.md) for any user-facing change **and tick the relevant [PLAN.md](PLAN.md) checkboxes with a clickable PR link for every milestone step this PR advances** — a merged PR with its plan item still unchecked is a process bug. Only then squash-merge, switch to `main`, and pull. The review is not optional; it is the gate that keeps code honest against the spec and this file.
- **Squash on merge**; keep the branch.

## Watching CI on open PRs

- **Use the `Monitor` tool**, not polling loops. After `gh pr create`, set up a `Monitor` task that watches `gh pr checks` and emits one line per state change — the harness then notifies you when CI completes or a check turns red. Keep working in parallel; never sit idle waiting on CI.
- **Do not write `until` loops over `gh pr checks`.** They block the agent, burn context with retries, and reproduce what `Monitor` already does correctly. `sleep N && gh pr checks` chains are forbidden by the harness in any case.
- Acceptable one-shot pattern: `Bash` with `run_in_background: true` running a command that exits when a single condition is true (e.g. `gh pr checks N --watch --fail-fast`). Use `Monitor` when you want continuous events across a work session or across multiple PRs.
- Don't `gh pr merge` inside a polling loop. Merge only after the monitor — or the user — says a PR is green.
- The same rule applies to anything else with discrete events: log tails, oracle batch runs, external job state. If you're tempted to poll, reach for `Monitor`.

## Command governance

- Use relative paths in shell commands, not absolute paths. Avoid `git -C <abs-path>`; it breaks project-level Claude permissions.
- Don't skip hooks (`--no-verify`) or bypass signing unless the user explicitly asks. If a hook fails, fix the underlying issue.
- **Never run a programmer automatically.** `minipro` and any other device-writing tool are user-initiated only. Writing a wrong `.jed` to a real part costs the user a chip and an afternoon.
- **Only kill processes you started.** Capture the PID of anything you launch and kill *only* that PID. Never `pkill`/`kill` by name or pattern — you will hit instances the user or another agent started, and a pattern can match your own shell. Keep the PID in your working context, not a shared file like `/tmp/foo.pid`, which a concurrent session can overwrite.
- **Worktrees live under `./.claude/worktrees/<slug>`** (`git worktree add .claude/worktrees/<slug> …`), not in sibling directories. When operating inside a worktree, use its explicit path — the shell cwd may reset between commands.
