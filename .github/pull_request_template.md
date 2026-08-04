<!-- Delete any section that doesn't apply. Keep this short — the diff is the detail. -->

## What and why

<!-- What changes, and what problem it solves. Link the milestone (PLAN.md) or issue: `Closes #N`. -->

## Checklist

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are green.
- [ ] Tests ship in the same commit. For the strict layer (see CLAUDE.md) the test was written **first** and failed on the pre-change tree.
- [ ] Any normative change updates SPEC.md in the **same commit** (the never-drift rule).
- [ ] README.md / PLAN.md updated for user-facing changes and completed milestone steps.

## Device evidence

<!-- Only for PRs that touch a target definition, fuse mapping, fitter, or JEDEC encoding. -->

- [ ] Every new or changed fuse mapping cites its evidence in a source comment.
- [ ] Evidence level reached (SPEC.md §5.31): `Hypothesis` / `DifferentiallyVerified` / `OpenSourceCrossChecked` / `HardwareVerified`
- [ ] Encode → decode round-trip holds, and the decoded design compares equal to the intended physical design.
- [ ] No writable fuse left unclassified.
