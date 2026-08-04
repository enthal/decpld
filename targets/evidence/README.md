# Device evidence

Every fact deCPLD asserts about a device — a fuse position, a mode encoding, a product-term allocation, a pin role — has to come from somewhere checkable. This directory is where "somewhere" is recorded.

The rule it exists to serve is in [CLAUDE.md](../../CLAUDE.md): **never enter a numeric fuse position from memory, inference, or a plausible-looking pattern.** A wrong fuse does not fail a test or throw an exception; it produces a chip that misbehaves in a circuit, and the cost lands on whoever is debugging their hardware weeks later. Citations are the only defense that survives the original author forgetting what they knew.

## What is here

- **[references.toml](references.toml)** — the primary sources, each with a URL, a `sha256`, a byte count, a retrieval date, and what it is authoritative for.
- **[verify-references.sh](verify-references.sh)** — re-fetches each reference and checks it against the recorded hash.

The documents themselves are **not committed**. They are third party and in some cases proprietary; `.gitignore` excludes PDFs and the JEDEC text from this directory. Run the verify script to fetch your own copies.

## Why hashes rather than revision strings

Vendors reissue PDFs at the same URL without changing any visible revision string, and mirrors silently diverge. A citation that names only a title and a URL cannot be checked a year later — you cannot tell whether the document you are holding is the one the claim was made from. A citation that names a hash can. Where a revision string does exist it is recorded too, but the `sha256` is the identity.

This also means a reference changing under us is *detectable*: `verify-references.sh` fails loudly rather than letting a silently-updated datasheet quietly invalidate a mapping.

## How a mapping cites evidence

In the target definition, at the point the number appears:

```rust
// Evidence: atf22v10c-datasheet §"Logic Diagram", macrocell 0 product-term rows.
// Cross-check: galette @ <commit> src/gal.rs row ranges — agrees.
// Level: OpenSourceCrossChecked. Hardware test pending (see PLAN.md M3).
const MACROCELL_0_ROWS: Range<u32> = 0..8;
```

The `id` is the key from `references.toml`; the locator names the section, table, or figure. A comment that says only "from the datasheet" is not a citation — it does not survive the document being reissued.

## Evidence levels

From SPEC.md §5.31, weakest to strongest:

| Level | Means |
| --- | --- |
| `Hypothesis` | Read from a document, or inferred from a pattern. Not yet corroborated. Belongs only in oracle-analysis code or a disabled experimental target — never in a production target. |
| `DifferentiallyVerified` | A controlled WinCUPL experiment isolates this fuse: change exactly one thing, diff the fuse *vectors*, and the delta is where it was predicted. |
| `OpenSourceCrossChecked` | An independent implementation (Galette, GALasm) encodes the same mapping. |
| `HardwareVerified` | A physical part programmed with this mapping behaves as predicted. The only level that can contradict all the others. |

Multiple independent sources agreeing is what promotes a mapping, not one source stating it confidently. WinCUPL agreeing is evidence, not proof — it is one implementation with its own bugs, and deCPLD exists partly because those bugs are hard to see.

## When evidence turns out to be wrong

This will happen. The point of recording the reference `id` at every call site is that when it does, `grep` finds every claim that trusted the bad source — including the *tests* that encoded it, which is the failure mode that would otherwise be invisible. Fix the mapping, fix the tests, and record what changed and why in the same commit.
