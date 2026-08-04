# deCPLD

**A modern compiler and hardware-description language for ATF22V10 and ATF16V8 programmable logic.**

Pronounced like *decoupled* — **dee·kuh·pld** (/diːˈkʌpld/).

[![Rust](https://img.shields.io/badge/Rust-2024_edition-orange)](rust-toolchain.toml) [![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue)](#license) [![Status](https://img.shields.io/badge/status-pre--implementation-lightgrey)](PLAN.md)

[Spec](SPEC.md) · [Plan](PLAN.md) · [Issues](https://github.com/enthal/decpld/issues)

---

## What is deCPLD?

The Microchip (formerly Atmel) **ATF22V10** and **ATF16V8** are still the most practical way to put a handful of gates, a decoder, or a small state machine into one 20- or 24-pin DIP. They are cheap, available, 5V-tolerant, and breadboard-friendly. The tooling is the problem: the reference compiler, **WinCUPL**, is a 1990s Windows application with a language to match, and getting it to run at all on a modern machine means Wine, a GUI, and a certain amount of hope.

deCPLD replaces it. It is a Rust compiler that takes a modern hardware-description language and emits a JEDEC fuse map you can hand to any programmer:

```text
design.decpld  →  decpld build  →  design.jed  →  minipro  →  ATF22V10
```

It is deliberately **not** a nicer front end that shells out to CUPL. It is a complete, self-contained compiler: its own parser, type system, elaborator, Boolean optimizer, minimizer, device models, fitter, and JEDEC encoder. Production builds require neither Wine nor Windows nor any Atmel software.

## A taste of the language

A four-bit synchronous counter with a synchronous reset and an enable — a reusable module, plus a top that pins it to a real device:

```decpld
/// A synchronous binary counter.
module Counter {
  param width: int;
  require width > 0;

  port signal clock;
  port signal reset;
  port signal enable;
  port signal[width] count;

  on posedge clock {
    count = match {
      reset  => 0,
      enable => count + 1,
      else   => count,
    };
  }
}

top Main {
  device ATF22V10C DIP24;

  signal clock;
  signal reset;
  signal enable;
  signal[4] count;

  clock  = pins[1];
  reset  = pins[2];
  enable = pins[3];
  pins[19..16] = count;

  Counter counter {
    clock: clock,
    reset: reset,
    enable: enable,
    count: count,
  }
}
```

Note what you did *not* have to write: no `wire` versus `reg`, no direction declarations, no manual next-state equations, no product-term budgeting, no macrocell assignment, no polarity choice, no explicit `width: 4` on the instance. All of it is inferred — and every inference is reported back to you.

The CUPL equivalent of that counter is four hand-derived D equations with explicit hold terms. That is the difference deCPLD is for.

## Design ideas

- **`signal`, not `wire` and `reg`.** One declaration keyword names a digital signal and its shape. Whether it becomes a pad, a combinational function, or a register is *inferred* from how it is used. Module boundaries stay explicit through `port` — interfaces are never guessed from incidental use.
- **One producer per signal bit.** Every bit is driven by exactly one thing: a pad sample, one combinational definition, or one clocked next-state definition. Ambiguity is an error, not a race.
- **No inferred latches, ever.** A combinational value must be defined on all paths. Inside `on posedge`, an omitted assignment means the register *holds* — which lowers to D-input feedback, not to a level-sensitive latch.
- **Expressions define hardware values.** `if` and `match` are expressions; multiple outputs are one packed-vector expression and one destructuring target.
- **Declaration order never matters.** Files and declarations are indexed before bodies are resolved, so nothing depends on file order, declaration order, or source-root order.
- **Strict widths and signedness.** No silent narrowing. Unsized literals are exact mathematical values until context supplies a width. Mixed-signedness arithmetic requires an explicit conversion.
- **Named arguments only.** Parameters and ports connect by name; there is no positional instantiation to get subtly wrong.
- **Directories are packages.** Declarations are private to their exact package unless marked `public`. There is no `export` keyword and no export list.

The full language definition is in [SPEC.md](SPEC.md).

## Verification is the point

A compiler bug in an ordinary toolchain gives you a stack trace. A compiler bug here gives you a programmed chip that misbehaves in a circuit, and you spend a week debugging your *hardware*. deCPLD is built around that asymmetry:

- **No unchecked minimization.** Every minimized equation is verified against the Boolean function it came from — exhaustively where the support set allows, by SAT/BDD beyond that.
- **Encode, decode, compare.** Every build generates the fuse vector, decodes it back into a physical device configuration, and asserts that configuration equals the one it intended — before writing a file. On by default.
- **Two independent simulators.** An RTL simulator and a simulator that runs the *decoded fuse map* must agree. Disagreement is how a mapping error announces itself.
- **Physical validation before encoding.** Every output placed, no resource double-booked, every literal routable, reserved fuses untouched.
- **Evidence-based device models.** No fuse position is ever entered from memory or from a plausible-looking pattern. Every mapping cites its evidence, and each field records how far it has actually been verified: datasheet hypothesis, differential experiment, open-source cross-check, or physical hardware test.
- **Reproducible.** The same source, compiler, target database, and options always produce the same fuse vector.

Where the compiler cannot prove it did the right thing, it refuses and says why. A rejected build beats a wrong one.

## Inspectable by design

The device's real architecture is meant to be visible, not hidden behind an abstraction:

```sh
decpld build design.decpld              # → design.jed plus a fit report
decpld dump design.decpld --stage sop   # ast | hir | rtl | boolean | sop | placed | fuses
decpld jed inspect design.jed --device ATF22V10C --package DIP24
```

`jed inspect` reads *any* JEDEC file back — including one WinCUPL produced — and reports the selected mode, and each macrocell's pin, mode, polarity, output enable, feedback, and equations, plus unused product terms and signature/security status. Fit failures name the limiting resource rather than shrugging:

```text
error[E2207]: no ATF16V8 mode can implement this design
  registered: pin 11 used as ordinary input
  complex: design contains registers
  simple: design contains registers
```

## Supported devices

| Device | Package | Status |
| --- | --- | --- |
| ATF22V10C family | DIP-24 | Planned — first target |
| ATF16V8B family | DIP-20 | Planned — registered, complex, and simple modes |

The device layer is a typed Rust specification with an `ArchitectureKind` seam, so larger PAL/GAL-style parts and other architectures can be added without disturbing the language or the IR.

## Status

**Pre-implementation.** The specification is complete and normative; the code is being built milestone by milestone. What exists today is the spec, CI, and a placeholder binary. See [PLAN.md](PLAN.md) for what has landed and what is next.

| Milestone | What it delivers |
| --- | --- |
| M0 | JEDEC foundation — parse, validate, canonicalize, checksum |
| M1 | ATF22V10 decoder/encoder with round-trip invariants |
| M2 | Minimal combinational language — working hardware |
| M3 | Registered logic — counters and shift registers on real parts |
| M4 | Modules, parameters, `if` / `match`, concatenation |
| M5 | ATF16V8 with all three global modes |
| M6 | Language server |
| M7 | Release quality — fuzzing, packaging, full evidence matrix |

## Getting started

There is nothing useful to run yet. To build the tree as it stands:

```sh
git clone https://github.com/enthal/decpld
cd decpld
cargo build --workspace
```

Rust **1.95+** is required and pinned in [rust-toolchain.toml](rust-toolchain.toml); `rustup` installs it automatically on the first `cargo` invocation.

## Development

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
scripts/markdown-no-hardwrap.sh
```

Install the pre-commit hook once — it symlinks into the shared hooks dir, so a single install covers every worktree:

```sh
scripts/install-git-hooks.sh
```

The hook runs `fmt` + `clippy` + the markdown lint. It deliberately does **not** run the test suite: the project commits failing tests *before* the code that makes them pass, so the test gate belongs in CI, which runs the full suite on a macOS + Linux matrix and gates merge.

`main` is protected: pull requests only, linear history, required status checks, and **signed commits**. All work lands on a `<kind>/<slug>` branch and is squash-merged. [CLAUDE.md](CLAUDE.md) documents the full working protocol — it is written for AI coding agents but describes the project's actual engineering rules, so it is worth reading either way.

## The WinCUPL oracle

WinCUPL has one role here: an independent witness for reverse-engineering device fuse maps. Controlled differential experiments — compile a minimal design, change exactly one thing, diff the resulting fuse *vectors* rather than the file text — reveal what each fuse controls. Those findings are then triangulated against the datasheets, against open-source implementations ([Galette](https://github.com/simon-frankau/galette), [GALasm](https://github.com/daveho/GALasm)), against encode/decode invariants, and finally against physical hardware.

WinCUPL is not assumed to be correct, and it is never required to compile anything. The oracle harness lives under `tools/wincupl/` and is developer-only; no proprietary Atmel files are redistributed from this repository.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
