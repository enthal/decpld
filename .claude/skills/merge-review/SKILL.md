---
name: merge-review
description: Pre-merge code review of the current branch's diff for adherence to SPEC.md and CLAUDE.md, plus correctness, verification discipline, device-evidence grounding, determinism, DRYness, readability, and efficiency. Run as part of the merge ceremony (see CLAUDE.md) before squash-merging any PR, and any time a diff-level review is wanted. Not the same as /code-review — this one is spec-and-CLAUDE-aware and is the gate the merge ceremony requires.
---

# merge-review

A disciplined, spec-aware review of a branch's changes, run **before merge**. It exists because this project's whole method is "SPEC.md and CLAUDE.md are the source of truth, and verification is the product" — a review that doesn't check the diff against them is missing the point. Use it as the review step of the merge ceremony ([CLAUDE.md](../../../CLAUDE.md) → Branches and pull requests), or on demand for any branch.

## What it checks

Review the diff against these criteria, in priority order. The first three are what make this project different from an ordinary compiler; weight them accordingly.

1. **Correctness** — logic bugs, off-by-one, integer overflow/underflow, boundary conditions, error handling, and whether invariants actually hold on every path. Scrutinize the tests: do they prove what they claim, or would a trivial/no-op implementation satisfy them? Name concrete failing inputs. Pay special attention to bit and fuse index arithmetic, slice ordering (`pins[19..16]` versus `pins[16..19]`), and anything that converts between a logical bit position and a physical one.

2. **Verification discipline** — the project's load-bearing rule (CLAUDE.md → *Verification is the product*). Ask specifically:
   - Is any minimized equation emitted without an equivalence check against its source function?
   - Does any new encode path exist without the corresponding decode-and-compare?
   - Did a new device configuration become reachable without a round-trip test covering it?
   - Was a check that used to be on by default made optional, skippable, or conditional?
   - Does the code ever prefer producing *something* over refusing and explaining? A rejected build beats a wrong one.

3. **Device evidence** — for any diff touching a target definition, fuse mapping, matrix, macrocell, or JEDEC encoding:
   - Does every new or changed fuse position cite its evidence in a source comment?
   - Is the claimed `EvidenceLevel` (SPEC.md §5.31) actually supported by what was done — or is a hypothesis wearing a verified label?
   - **A numeric fuse position with no cited evidence is a High finding, always.** This is the single easiest way to poison the project, and it is invisible until hardware misbehaves.

4. **Spec adherence** — compare against [SPEC.md](../../../SPEC.md) for the changed area. Flag any divergence in type shapes, field names, trait signatures, semantics, diagnostic codes, CLI flags, or invariants. A normative change that did **not** update SPEC.md in the same commit is a finding (the never-drift rule).

5. **CLAUDE.md adherence** — layering (no fuse numbers above the device layer; no language concepts below the HIR; JEDEC ignorant of architecture), determinism (no `now()`, no unseeded RNG, no `HashMap` iteration order reaching output), typed IDs rather than bare integers, no `unwrap()`/`expect()` in non-test code, `BigInt` for compile-time integers, map-naming conventions, errors that identify object context, stable diagnostic codes, markdown never hard-wrapped, tests-first for the strict layer.

6. **DRYness** — duplication that should be factored; a formula or invariant expressed in more than one place. A fuse layout fact stated twice is a fuse layout fact that will drift.

7. **Readability & idiom** — naming, doc quality, idiomatic Rust. Does a reader learn *why* from the comments, or only *what*?

8. **Efficiency** — needless allocation or accidental O(n²), weighed against the project's "conservative and transparent" stance. Don't invent micro-optimizations the spec disowns; a fitter that is correct and explicable beats one that is fast and opaque.

## How to run it

1. **Scope the diff.** Default to the current branch versus the merge base with `main`: `git merge-base HEAD origin/main`, then `git diff <base>...HEAD --stat` and `--name-only`. If reviewing a GitHub PR, use its number.

2. **Spawn a review sub-agent** (read-only; it must not edit). Give it: the exact changed files, the specific SPEC.md sections and CLAUDE.md to read **first**, and the criteria above. Instruct it to cite `file:line`, rate each finding **High / Medium / Low**, distinguish real bugs from nits, briefly confirm what it verified as sound, and list spec/CLAUDE divergences explicitly. It may run `cargo clippy` / `cargo test` to confirm state, but should focus on what tooling does **not** catch. For a large or multi-area diff, use one sub-agent per area, in parallel — for example one on the device/fuse layer and one on the language layer, since the failure modes are entirely different.

3. **Triage and handle the findings yourself** (the sub-agent only reports):
   - Fix High and worthwhile Medium findings in the working tree.
   - For any normative change, update SPEC.md in the **same commit** (never-drift).
   - Consciously **defer** anything not worth doing now — say so out loud, with the reason; don't silently drop it.
   - Re-run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` until green.

4. **Report** a tight summary: the verdict, what was fixed, what was deferred and why.

## Definition of done

The review ran, its findings were either applied or explicitly deferred with a stated reason, and `fmt` + `clippy` + `test` are green. Only then does the merge ceremony proceed to the squash-merge.
