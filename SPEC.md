# deCPLD

## Complete project, language, compiler, device-backend, CLI, LSP, and verification specification

**Purpose:** a modern Rust compiler and hardware-description language for Microchip ATF22V10 and ATF16V8 simple programmable logic devices, designed to replace WinCUPL for ordinary design, synthesis, fitting, and JEDEC generation.

**Name and pronunciation:** **deCPLD** is pronounced like *decoupled*:
**dee·kuh·pld** (/diːˈkʌpld/).

- Language: **deCPLD**
- Compiler and command-line tool: `decpld`
- Language server: `decpld-lsp`
- Source extension: `*.decpld`
- Primary output: JEDEC fuse maps (`*.jed`)
- Implementation language: Rust

**Status:** normative implementation specification and authoritative project document. A capable human or AI coding agent should be able to implement the complete system from this document.

---

## 1. Objective

deCPLD replaces WinCUPL as the design-entry, elaboration, synthesis, fitting, inspection, and JEDEC-generation tool for small PAL/GAL-compatible devices.

The first release shall support:

- ATF22V10C-family devices, initially DIP-24;
- ATF16V8B-family devices, initially DIP-20;
- combinational logic;
- positive-edge registered logic;
- synchronous reset and conditional register hold;
- output polarity optimization;
- output-enable equations and bidirectional pads;
- internal macrocell feedback;
- deterministic JEDEC generation;
- decoded-fuse inspection;
- RTL and physical-model simulation;
- formal or exhaustive equivalence checks;
- a package-aware CLI and LSP.

The compiler ends at a valid `.jed` file. High-voltage programming algorithms and programmer hardware protocols are outside the primary compiler. Existing software such as `minipro` may be invoked only through an explicit convenience command.

```text
deCPLD package
    ↓ source discovery, parsing, package indexing
selected top
    ↓ name resolution, visibility, parameter inference, elaboration
typed RTL
    ↓ producer/storage/pad/clock inference
target-independent logical network
    ↓ optimization and Boolean lowering
sum-of-products equations
    ↓ target fitting: mode, pins, macrocells, product terms, feedback
physical device configuration
    ↓ exact device-specific fuse encoding
JEDEC
    ↓ external programmer
ATF22V10 / ATF16V8
```

WinCUPL is a development oracle only. Production compilation must not require WinCUPL, Wine, or Windows.

---

## 2. Design principles

### 2.1 Signals, not `wire` and `reg`

The source declaration keyword is `signal`. It names a digital signal and its shape without claiming storage class or physical direction.

The compiler infers:

- physical input, output, or bidirectional pad behavior;
- combinational or registered implementation;
- clock and feedback requirements.

Module boundaries remain explicit through `port`; module interfaces are never inferred from incidental use.

### 2.2 One producer per signal bit

Every signal bit has exactly one producer:

- a physical pad sample;
- one combinational definition; or
- one clocked next-state definition.

A curly-brace target is one composite assignment. Its constituent bits must be disjoint from every other producer target.

### 2.3 No inferred latches

A combinational value must be defined for all paths.

Within `on posedge`, omitted assignment under a condition means that the register retains its current state. This is D-input feedback, not a level-sensitive latch.

### 2.4 Expressions define hardware values

`if` and `match` are expressions. Multiple outputs are represented by one curly-brace packed vector expression and destructuring target.

### 2.5 Declaration order never matters

Files and declarations are indexed before bodies are resolved. A top or module may refer to declarations appearing later in the file or anywhere else in the package. File order, declaration order, and source-root order have no semantic effect.

Recommended style is root-first:

1. top declarations;
2. modules directly instantiated by those tops;
3. supporting modules;
4. leaf implementation details.

The formatter never reorders declarations.

### 2.6 Strict widths and signedness

Unsized integer literals are exact mathematical values until context supplies a width. No silent narrowing occurs. Unsigned widening zero-extends; signed widening sign-extends. Mixed-signedness arithmetic requires explicit conversion.

### 2.7 Named module arguments only

Parameters and ports are connected by name. Positional instantiation is not part of the language.

### 2.8 Explicit package boundaries

Directories form hierarchical packages. Declarations are private to their exact package by default. `public` exposes a declaration outside that package. Subpackages do not receive private access to parents. There is no `export` keyword or export list.

### 2.9 Verified device knowledge

Targets are typed Rust specifications. Every fuse mapping must be supported by official documentation, independent open-source cross-checking, controlled WinCUPL differential experiments, encode/decode invariants, or hardware tests.

---

## 3. Repository structure

```text
decpld/
├── Cargo.toml
├── crates/
│   ├── decpld-syntax/
│   ├── decpld-package/
│   ├── decpld-hir/
│   ├── decpld-types/
│   ├── decpld-rtl/
│   ├── decpld-logic/
│   ├── decpld-device/
│   ├── decpld-atf22v10/
│   ├── decpld-atf16v8/
│   ├── decpld-jedec/
│   ├── decpld-sim/
│   ├── decpld-diagnostics/
│   ├── decpld-driver/
│   ├── decpld-cli/
│   ├── decpld-lsp/
│   └── decpld-oracle/
├── examples/
├── targets/
│   ├── evidence/
│   └── fixtures/
├── tests/
└── tools/wincupl/
```

The CLI and LSP use the same parser, package index, resolver, elaborator, type checker, diagnostics, and target registry.

---

# 1. Language and package definition

## 1.1 Source modes and manifests

### 1.1.1 Single-file mode

```bash
decpld build design.decpld
```

No `decpld.toml` is required. The file forms one implicit package.

### 1.1.2 Package mode

More than one file, external dependencies, multiple source roots, or persistent project settings use `decpld.toml`.

```toml
[package]
name = "video-plds"

[sources]
roots = ["tops", "lib"]

[dependencies]
components = { path = "../components" }

[build]
optimization = 2
```

All `.decpld` files recursively beneath the source roots belong to the package.

For a manifest-free local multi-tree build:

```bash
decpld build \
  --source-root tops \
  --source-root lib \
  --top deCPLDer
```

External dependencies require a manifest.

## 1.2 Hierarchical packages

Each directory beneath a source root contributes one package segment.

Given source root `lib`:

```text
lib/display/counter.decpld
```

the file contributes declarations to package:

```text
display
```

Given:

```text
lib/display/control/counter.decpld
```

the package is:

```text
display.control
```

Files at the source-root root contribute to the package root.

### 1.2.1 File package declaration

A file may have one package declaration as its first non-comment declaration.

```decpld
package;
```

This appends the file stem as one package segment.

```text
lib/display/counter.decpld + package;
→ display.counter
```

An explicit segment may be supplied:

```decpld
package counters;
```

which yields:

```text
display.counters
```

The declaration adds exactly one segment; it does not replace the directory-derived path.

### 1.2.2 Package and file independence

A package may span many files. A file may contain many declarations. File names do not need to match declaration names. File boundaries and ordering have no semantic effect beyond deriving package location and hosting file-scoped `use` aliases.

### 1.2.3 Duplicate names

Two declarations with the same name in the same package are errors. A name may not simultaneously identify a declaration and a child package in the same parent package.

## 1.3 Visibility and name resolution

Declarations are private to their exact package unless marked `public`.

```decpld
module CarryStage {
  // visible only within this exact package
}

public module Counter {
  // visible from other packages and dependencies
}
```

The same applies to enums, constants, and other package-level declarations.

Rules:

- files in the same exact package may access private declarations;
- parent and child packages are distinct;
- sibling packages are distinct;
- `public` is the only visibility modifier;
- there is no `export` keyword;
- module `param` and `port` declarations are members of the module interface and need no visibility modifier;
- internal signals are never externally addressable.

### 1.3.1 Qualified names

Fully qualified package paths always work when visibility permits:

```decpld
display.counter.Counter row_counter {
  ...
}
```

Dependency aliases occupy the first path segment:

```toml
[dependencies]
components = { path = "../components" }
```

```decpld
components.io.Debouncer reset_filter {
  ...
}
```

### 1.3.2 `use`

`use` creates file-scoped aliases only. It does not discover files, add dependencies, alter package membership, or bypass visibility.

```decpld
use display.counter.Counter;
use display.counter as ctr;
use components.io.Debouncer as ResetDebouncer;
```

Then:

```decpld
Counter counter { ... }
ctr.Divider divider { ... }
ResetDebouncer reset_filter { ... }
```

Glob imports are not supported in version 1.

Lookup order for an unqualified name:

1. local lexical names;
2. module parameters and ports;
3. file-scoped `use` aliases;
4. declarations in the current exact package;
5. built-ins;
6. otherwise error.

The compiler never searches arbitrary sibling packages or undeclared external dependencies.

## 1.4 Tops and devices

A package may contain multiple named `top` declarations.

```decpld
top deCPLDer {
  device ATF22V10C DIP24;

  signal clock;
  signal[4] row;

  clock = pins[1];
  pins[19..16] = row;
}
```

Rules:

- a `top` is a build root, not an instantiable module;
- each `top` has a package-qualified name;
- a build selects exactly one top;
- one top is selected automatically if it is the only one;
- `--top Qualified.Name` is required when more than one is available;
- duplicate qualified top names are errors;
- `device` is the first declaration inside a top;
- each top contains exactly one `device`;
- `device` is illegal outside a top;
- `pins` is in scope only inside a top;
- a top may instantiate ordinary modules;
- a module may not instantiate a top.

The source target is authoritative. CLI `--device` and `--package` options are checked assertions and must agree.

## 1.5 Lexical syntax and documentation

Identifiers:

```text
[A-Za-z_][A-Za-z0-9_]*
```

Comments:

```decpld
// line comment
/* block comment */
```

Documentation comments use `///` and may precede any declaration:

```decpld
/// A synchronous binary counter.
public module Counter {
  /// Counter width in bits.
  param width: int;

  /// Positive-edge clock.
  port signal clock;

  /// Current registered value.
  port signal[width] count;
}
```

Consecutive `///` lines form one documentation block. Documentation is retained in the syntax tree and HIR and is exposed through LSP hover, completion, signature help, symbols, and generated documentation.

## 1.6 Runtime type: `signal`

```decpld
signal ready;
signal[8] byte;
signal[4][8] matrix;
signal signed[8] delta;
```

`signal` denotes a digital signal. It does not by itself mean combinational, registered, input, or output.

- `signal` has width one;
- `signal[N]` is an ordered packed vector of `N` bits;
- multidimensional shapes are permitted;
- unsigned is the default;
- signedness affects arithmetic interpretation;
- dimensions are positive compile-time integers.

A module interface is explicit:

```decpld
public module Register {
  param width: int;

  port signal clock;
  port signal enable;
  port signal[width] input;
  port signal[width] output;

  signal internal_ready;
}
```

Only `param` and `port` declarations form the externally connectable interface.


## 1.7 Compile-time parameter types

Required types:

```text
int
bool
bits[N]
enum types
```

Example:

```decpld
param width: int;
param invert: bool = false;
param reset_value: bits[width] = 0;
```

Compile-time integers use arbitrary-precision mathematical representation inside the compiler.

## 1.8 Enums

```decpld
enum Phase {
    Visible,
    FrontPorch,
    Sync,
    BackPorch,
}
```

Version 1 uses a deterministic minimum-width binary encoding in declaration order and reports the chosen encoding. The IR must leave room for future encoding policies.

## 1.9 Pins

`pins` is a built-in package-specific array of physical pad objects introduced by the `device` declaration. It is in scope only within the selected `top`; referring to `pins` in an ordinary module is a compile-time error.

```decpld
clock = pins[1];
data = pins[9..2];
pins[23..16] = result;
```

In expression position, a pin means sample the pad. On the left of a drive assignment, it means drive the pad.

A pin is not merely a signal bit internally: it has a number, package identity, resource mapping, and capability set.

The target validates:

- pin existence;
- VCC/GND use;
- dedicated clock requirements;
- macrocell and input capability;
- ATF16V8 mode-dependent restrictions.

Slices are ordered. `pins[19..16] = count` maps `count[3]` to pin 19 and `count[0]` to pin 16. Reversing the written range reverses the mapping.

Conditional drive:

```decpld
pins[19] = transmit when output_enable;
receive = pins[19];
```

This infers bidirectional use and an OE equation.

## 1.10 Indexes, slices, concatenation, and destructuring

```decpld
byte[0]
byte[7..4]
byte[0..3]
```

A slice preserves written order:

```text
byte[7..4] = {byte[7], byte[6], byte[5], byte[4]}
byte[0..3] = {byte[0], byte[1], byte[2], byte[3]}
```

Curly braces form a packed vector:

```decpld
{a, b, c}
{page, character, row}
{shift[6..0], serial_in}
```

The first item occupies the most-significant portion.

Curly braces are also legal targets:

```decpld
{carry, sum} = full_result;
{page, character, row} = address;
```

Target members must not overlap.

Inside `on posedge`, right-hand values are evaluated from pre-edge state:

```decpld
on posedge clock {
    {a, b} = {b, a};
}
```

This swaps the two registers.

## 1.11 Numeric literals and widths

Supported forms:

```decpld
0
42
1_000
0b1010
0b1111_0000
0x2a
0xDEAD_BEEF
0o755
```

Radix does not define width. An unsized literal is an exact abstract integer until context supplies width and signedness.

```decpld
signal[4] count;
count = 5;   // 0101
count = 16;  // error: needs five bits
```

Explicit widths:

```decpld
0_u8
15_u4
0xff_u8
0b1010_u4
-1_s8
-12_s8
```

Inside concatenation, unsized `0` and `1` are one bit. Other unsized literals are ambiguous and rejected:

```decpld
{a, 0, b}       // valid
{a, 3, b}       // error
{a, 3_u2, b}    // valid
```

Widening:

- unsigned to wider unsigned: zero extension;
- signed to wider signed: sign extension.

Narrowing is never implicit:

```decpld
signal[8] source;
signal[4] destination;

destination = source;       // error
destination = source[3..0]; // valid
```

Required explicit conversion functions:

```decpld
signed(value)
unsigned(value)
extend[width](value)
truncate[width](value)
```

Mixed signed/unsigned arithmetic requires conversion.

## 1.12 Operators

Required operators:

```text
!  Boolean negation, one-bit operand
~  bitwise complement
&  |  ^  bitwise Boolean
&& || Boolean, one-bit operands
== != < <= > >= comparisons
+ - unsigned/signed modular arithmetic at resolved width
<< >> width-preserving shifts
```

Ordinary addition is width-preserving in context:

```decpld
signal[4] count;
count = count + 1; // wraps 15 → 0
```

Capture carry by explicitly widening:

```decpld
{carry, sum} = extend[5](a) + extend[5](b);
```

The RTL retains operations such as `Add`, `Compare`, `Mux`, and `Shift` until target lowering.

## 1.13 Combinational assignment

```decpld
ready = state == Done;
result = (a & b) | (!c & d);
pins[20] = result;
```

A combinational signal bit has exactly one complete definition. Combinational cycles are errors in version 1.

## 1.14 Clocked assignment

```decpld
on posedge clock {
    count = count + 1;
}
```

Normative semantics:

1. The clock is one bit.
2. A register belongs to one clock domain.
3. A register bit has one clocked assignment site.
4. Multiple assignments to the same bit are illegal.
5. All register assignments update simultaneously.
6. Missing clocked assignment means hold.
7. The target must possess a legal clock route.
8. ATF22V10 and ATF16V8 registered implementations use their dedicated global clock resource.

Gated update:

```decpld
on posedge clock {
    if enable {
        count = count + 1;
    }
}
```

This lowers to:

```text
next(count) = enable ? count + 1 : count
```

It is not a latch.

## 1.15 `if` expressions

```decpld
result = if select {
    a
} else {
    b
};
```

All branches must produce compatible shapes and signedness. A combinational `if` must be exhaustive.

Within `on posedge`, the restricted statement form without `else` is permitted as gated-assignment sugar. It may contain assignments to distinct targets only.

## 1.16 `match` expressions

Value match:

```decpld
next_state = match state {
    Idle    => if start { Running } else { Idle },
    Running => if done  { Done }    else { Running },
    Done    => Done,
};
```

Priority-condition match:

```decpld
count = match {
    reset  => 0,
    enable => count + 1,
    else   => count,
};
```

Condition arms are tested top to bottom.

Multiple outputs use a concatenated vector:

```decpld
{ready, busy, error} = match state {
    Idle    => {1, 0, 0},
    Running => {0, 1, 0},
    Fault   => {0, 0, 1},
};
```

This is one expression and one composite producer.

## 1.17 Modules

Definition:

```decpld
module Register {
    param width: int;
    require width > 0;

    port signal clock;
    port signal enable;
    port signal[width] input;
    port signal[width] output;

    on posedge clock {
        if enable {
            output = input;
        }
    }
}
```

Instantiation:

```decpld
Register decpld_row {
    clock: pixel_clock,
    enable: load,
    input: rom_data,
    output: shift_data,
}
```

Rules:

- instance name required;
- named arguments only;
- positional syntax rejected;
- unknown, duplicate, or missing required fields rejected;
- input-like ports accept expressions;
- output-like ports require assignable targets;
- direction is inferred from the module body.

### 1.17.1 Parameter inference as constraints

```decpld
module Register {
    param width: int;
    port signal[width] input;
    port signal[width] output;
    // ...
}
```

Connecting two eight-bit values infers `width = 8`. An explicit `width: 8` adds an assertion rather than overriding inference.

Resolution order:

1. collect explicit parameter arguments;
2. collect width/shape equations from port connections;
3. solve uniquely determined values;
4. apply defaults only to still-unconstrained parameters;
5. solve again;
6. validate `require` constraints;
7. reject ambiguity or inconsistency.

Example error:

```text
error: inconsistent parameter `width`
  width = 8  from `input: source`
  width = 16 from `output: destination`
```

Version 1 needs a deterministic affine integer constraint solver, not a general SMT system.

---

# 2. Reference circuits and CUPL comparisons

## 2.1 Combinational logic

deCPLD:

```decpld
top Main {
    signal a;
    signal b;
    signal y_and;
    signal y_or;

    a = pins[2];
    b = pins[3];

    y_and = a & b;
    y_or = a | b;

    pins[23] = y_and;
    pins[22] = y_or;
}
```

CUPL:

```cupl
Name logic_demo;
PartNo 00;
Date 2026-08-02;
Revision 01;
Designer deCPLD;
Company deCPLD;
Assembly None;
Location None;
Device g22v10;

PIN 2 = a;
PIN 3 = b;
PIN 23 = y_and;
PIN 22 = y_or;

y_and = a & b;
y_or  = a # b;
```

CUPL uses `#` for OR.

## 2.2 Four-bit synchronous counter

deCPLD module:

```decpld
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
```

ATF22V10 top:

```decpld
top Main {
    signal clock;
    signal reset;
    signal enable;
    signal[4] count;

    clock = pins[1];
    reset = pins[2];
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

CUPL reference:

```cupl
Name counter4;
PartNo 00;
Date 2026-08-02;
Revision 01;
Designer deCPLD;
Company deCPLD;
Assembly None;
Location None;
Device g22v10;

PIN 1 = clock;
PIN 2 = reset;
PIN 3 = enable;
PIN 16 = q0;
PIN 17 = q1;
PIN 18 = q2;
PIN 19 = q3;

[q3..q0].CK = clock;

q0.D = !reset & ((enable & !q0) # (!enable & q0));
q1.D = !reset & ((enable & (q1 $ q0)) # (!enable & q1));
q2.D = !reset & ((enable & (q2 $ (q1 & q0))) # (!enable & q2));
q3.D = !reset & ((enable & (q3 $ (q2 & q1 & q0))) # (!enable & q3));
```

CUPL uses `$` for XOR. The oracle harness shall test whether the selected WinCUPL device library requires explicit `.CK`; retain explicit `.CK` in fixtures where accepted.

Acceptance:

- positive-edge clocking;
- synchronous reset to zero;
- hold when disabled;
- increment modulo 16 when enabled;
- all outputs decoded as registered macrocells.

For ATF16V8, any register selects registered mode. Pin 1 becomes common clock and pin 11 common OE, and the target must reserve them accordingly.

## 2.3 Eight-bit shift register

deCPLD:

```decpld
module ShiftRegister {
    param width: int;
    require width > 1;

    port signal clock;
    port signal enable;
    port signal serial_in;
    port signal[width] value;

    on posedge clock {
        if enable {
            value = {value[width - 2..0], serial_in};
        }
    }
}
```

Top:

```decpld
top Main {
    signal clock;
    signal enable;
    signal serial_in;
    signal[8] shift;

    clock = pins[1];
    enable = pins[2];
    serial_in = pins[3];
    pins[23..16] = shift;

    ShiftRegister shifter {
        clock: clock,
        enable: enable,
        serial_in: serial_in,
        value: shift,
    }
}
```

CUPL:

```cupl
Name shift8;
PartNo 00;
Date 2026-08-02;
Revision 01;
Designer deCPLD;
Company deCPLD;
Assembly None;
Location None;
Device g22v10;

PIN 1 = clock;
PIN 2 = enable;
PIN 3 = serial_in;
PIN 16 = q0;
PIN 17 = q1;
PIN 18 = q2;
PIN 19 = q3;
PIN 20 = q4;
PIN 21 = q5;
PIN 22 = q6;
PIN 23 = q7;

[q7..q0].CK = clock;

q0.D = (enable & serial_in) # (!enable & q0);
q1.D = (enable & q0)        # (!enable & q1);
q2.D = (enable & q1)        # (!enable & q2);
q3.D = (enable & q2)        # (!enable & q3);
q4.D = (enable & q3)        # (!enable & q4);
q5.D = (enable & q4)        # (!enable & q5);
q6.D = (enable & q5)        # (!enable & q6);
q7.D = (enable & q6)        # (!enable & q7);
```

## 2.4 Decoder

deCPLD:

```decpld
module Decoder2to4 {
    port signal[2] select;
    port signal enable;
    port signal[4] output;

    output = if enable {
        match select {
            0 => 0b0001_u4,
            1 => 0b0010_u4,
            2 => 0b0100_u4,
            3 => 0b1000_u4,
        }
    } else {
        0
    };
}
```

CUPL:

```cupl
y0 = enable & !s1 & !s0;
y1 = enable & !s1 &  s0;
y2 = enable &  s1 & !s0;
y3 = enable &  s1 &  s0;
```

## 2.5 Tri-state driver

deCPLD:

```decpld
top Main {
    signal enable;
    signal[4] data;

    enable = pins[2];
    data = pins[6..3];
    pins[19..16] = data when enable;
}
```

CUPL:

```cupl
y0 = d0;
y1 = d1;
y2 = d2;
y3 = d3;
[y3..y0].OE = enable;
```

ATF16V8 fixtures must test this separately in complex and registered modes because OE and feedback capabilities differ.

---


## 2.6 Package indexing implementation

Before ordinary name resolution, the compiler builds a package index.

```rust
pub struct PackageIndex {
    pub package_name: String,
    pub source_roots: Vec<SourceRoot>,
    pub packages: BTreeMap<PackagePath, PackageScope>,
    pub tops: BTreeMap<QualifiedName, TopDeclId>,
    pub dependencies: BTreeMap<String, DependencyPackage>,
}

pub struct SourceRoot {
    pub canonical_path: PathBuf,
    pub origin: SourceRootOrigin,
}

pub struct PackagePath(pub Vec<Name>);

pub struct PackageScope {
    pub declarations: BTreeMap<Name, DeclId>,
    pub child_packages: BTreeMap<Name, PackagePath>,
}
```

For each file:

1. determine which source root owns it;
2. canonicalize and reject discovery through multiple roots;
3. derive directory package segments relative to that root;
4. parse the optional file-level package declaration;
5. append zero or one file package segment;
6. index package-level declarations;
7. attach file-scoped `use` aliases;
8. diagnose duplicate names and declaration/package collisions.

The resolver checks exact-package privacy before returning an external declaration. `public` is represented on the declaration, not in a separate export table.

Top selection occurs after package indexing and before elaboration.


# 3. Compiler internals

## 3.1 Compilation phases

1. Lex source.
2. Parse a lossless concrete syntax tree.
3. Build AST/HIR and resolve names.
4. Elaborate modules.
5. Collect and solve parameter and shape constraints.
6. Type-check widths and signedness.
7. Infer producers, storage, direction, clocks, and pad use.
8. Lower to typed target-independent RTL.
9. Optimize RTL safely.
10. Lower each combinational output and register D function to a Boolean graph.
11. Map the selected target to SOP equations and controls.
12. Minimize true and complemented output functions.
13. Fit outputs to pins/macrocells and allocate product terms.
14. Validate the complete physical design.
15. Encode fuse states.
16. Decode the generated fuse vector and compare it to the intended physical design.
17. Write JEDEC and reports.

Every phase should have a debug dump:

```bash
decpld dump design.decpld --stage ast
decpld dump design.decpld --stage hir
decpld dump design.decpld --stage rtl
decpld dump design.decpld --stage boolean
decpld dump design.decpld --stage sop
decpld dump design.decpld --stage placed
decpld dump design.decpld --stage fuses
```

## 3.2 Source spans and diagnostics

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub range: TextRange,
}
```

Use a lossless tree, preferably Rowan-style, so comments and malformed nodes survive formatting and LSP recovery.

## 3.3 AST outline

```rust
pub struct SourceFile {
    pub device: Option<DeviceDecl>,
    pub enums: Vec<EnumDecl>,
    pub modules: Vec<ModuleDecl>,
}

pub struct ModuleDecl {
    pub name: Name,
    pub is_top: bool,
    pub items: Vec<ModuleItem>,
    pub span: Span,
}

pub enum ModuleItem {
    Param(ParamDecl),
    Port(LogicDecl),
    Logic(LogicDecl),
    Require(Expr),
    Assignment(Assignment),
    Clocked(ClockedBlock),
    Instance(ModuleInstance),
}

pub struct Assignment {
    pub target: TargetExpr,
    pub value: Expr,
    pub condition: Option<Expr>, // `when`, only for pad drive
}
```

## 3.4 Typed HIR and elaboration

```rust
pub struct HirDesign {
    pub top: HirModuleInstance,
    pub symbols: SymbolTable,
    pub enums: EnumTable,
}

pub struct HirModuleInstance {
    pub id: InstanceId,
    pub module: ModuleId,
    pub params: IndexMap<ParamId, ConstValue>,
    pub ports: IndexMap<PortId, HirConnection>,
    pub locals: Vec<HirLogic>,
    pub children: Vec<HirModuleInstance>,
}

pub struct LogicType {
    pub dimensions: Vec<u32>,
    pub signedness: Signedness,
}

pub enum ConstValue {
    Int(BigInt),
    Bool(bool),
    Bits(BitVec),
    Enum(EnumTypeId, u32),
}
```

Constraint representation:

```rust
pub enum Constraint {
    Equal(ParamExpr, ParamExpr, ConstraintOrigin),
    GreaterThan(ParamExpr, ParamExpr, ConstraintOrigin),
    GreaterEqual(ParamExpr, ParamExpr, ConstraintOrigin),
    Boolean(ConstBoolExpr, ConstraintOrigin),
}

pub enum ParamExpr {
    Const(BigInt),
    Param(ParamId),
    Add(Box<ParamExpr>, Box<ParamExpr>),
    Sub(Box<ParamExpr>, Box<ParamExpr>),
    MulConst(BigInt, Box<ParamExpr>),
    ShiftLeft(Box<ParamExpr>, u32),
}
```

Version 1 solves direct substitutions and affine integer equations. Underconstrained or nonlinear relationships are rejected rather than guessed.

## 3.5 Producer inference

Flatten assignable values to bit identities:

```rust
pub struct BitRef {
    pub logic: LogicId,
    pub linear_index: u32,
}

pub enum Producer {
    PhysicalInput { pin: PhysicalPinId, span: Span },
    Combinational { expr: HirExprId, span: Span },
    Clocked { clock: HirExprId, next: HirExprId, span: Span },
}
```

Physical output drive is a consumer of an internal value, not that value's producer.

Reject:

- duplicate producers;
- overlapping targets;
- combinational cycles;
- multiple clock domains for one register;
- a target assigned both combinationally and clocked.

## 3.6 RTL IR

```rust
pub struct RtlDesign {
    pub inputs: Vec<RtlInput>,
    pub outputs: Vec<RtlPadDrive>,
    pub nodes: Vec<RtlNode>,
    pub registers: Vec<RtlRegister>,
}

pub enum RtlNodeKind {
    Constant(BitVec),
    Slice { value: ValueId, order: Vec<u32> },
    Concat(Vec<ValueId>),
    Not(ValueId),
    And(Vec<ValueId>),
    Or(Vec<ValueId>),
    Xor(Vec<ValueId>),
    Eq(ValueId, ValueId),
    Lt(ValueId, ValueId, Signedness),
    Mux { select: ValueId, when_true: ValueId, when_false: ValueId },
    Add { lhs: ValueId, rhs: ValueId, width: u32, signedness: Signedness },
    Sub { lhs: ValueId, rhs: ValueId, width: u32, signedness: Signedness },
    Shift { value: ValueId, amount: ValueId, direction: ShiftDirection, arithmetic: bool },
}

pub struct RtlRegister {
    pub output: ValueId,
    pub next: ValueId,
    pub clock: ClockRef,
    pub width: u32,
    pub source: Span,
}

pub struct RtlPadDrive {
    pub pin: PhysicalPinId,
    pub data: ValueId,
    pub enable: Option<ValueId>,
}
```

Clocked gated assignment lowers to a mux with current state as fallback.

## 3.7 Optimization

Required safe passes:

- constant folding;
- dead-node elimination;
- concat/slice simplification;
- mux simplification;
- Boolean identities;
- common-subexpression elimination;
- width normalization;
- next-state simplification.

Optimization levels:

```text
-O0 structural only
-O1 local simplification
-O2 Boolean and target-aware optimization
```

## 3.8 Boolean graph

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoolRef {
    pub node: BoolNodeId,
    pub inverted: bool,
}

pub enum BoolNode {
    ConstFalse,
    Input(BoolInputId),
    And(BoolRef, BoolRef),
    Xor(BoolRef, BoolRef),
    Mux { select: BoolRef, when_true: BoolRef, when_false: BoolRef },
}
```

Hash-cons nodes. Each output records data function, output kind, OE, feedback need, and pin constraint.

## 3.9 SOP representation and minimization

```rust
pub struct Sop {
    pub cubes: Vec<Cube>,
}

pub struct Cube {
    pub literals: Vec<Literal>,
}

pub struct Literal {
    pub input: BoolInputId,
    pub polarity: Polarity,
}
```

A cube is an AND term; an SOP is an OR of cubes.

For every output compute and retain minimized covers for both `f` and `!f`, since output polarity may make one cheaper.

Recommended version-1 minimizer:

- exact truth-table generation for manageable support sizes;
- Quine–McCluskey plus Petrick's method where practical;
- deterministic heuristic cube minimization beyond a threshold;
- exhaustive, BDD, or SAT equivalence verification for every result.

Primary cost: product terms. Secondary cost: literals. Never emit an unchecked minimized equation.

---

# 4. Rust device specification system

## 4.1 Target trait

```rust
pub trait DeviceTarget: Send + Sync {
    fn id(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str];
    fn architecture(&self) -> ArchitectureKind;
    fn packages(&self) -> &[PackageSpec];

    fn validate_logical_design(
        &self,
        design: &MappedLogicDesign,
        package: PackageId,
    ) -> Result<(), DiagnosticBundle>;

    fn fit(
        &self,
        design: &MappedLogicDesign,
        package: PackageId,
        constraints: &PhysicalConstraints,
    ) -> Result<PhysicalDesign, FitError>;

    fn encode(&self, design: &PhysicalDesign) -> Result<FuseMap, EncodeError>;
    fn decode(&self, fuses: &FuseMap) -> Result<PhysicalDesign, DecodeError>;
    fn jedec_metadata(&self) -> JedecDeviceMetadata;
}
```

```rust
pub enum ArchitectureKind {
    PalGalSop,
    BlockSop, // future CPLDs
    LutFabric, // future FPGA path
}
```

Version 1 uses Rust builders rather than an external target DSL. All target structures should be serializable so a future generated architecture database can construct the same model.

## 4.2 Fuse map and regions

```rust
pub struct FuseId(pub u32);

pub struct FuseMap {
    bits: BitVec,
    written: BitVec,
}

pub struct FuseRegion {
    pub name: &'static str,
    pub range: Range<u32>,
    pub erased_value: bool,
    pub mutability: FuseMutability,
}

pub enum FuseMutability {
    Programmable,
    Reserved(bool),
    UserSignature,
    Security,
}
```

Track writes to detect conflicting encoders. Security is clear by default and requires an explicit dangerous option.

## 4.3 Configuration fields

```rust
pub struct ConfigField<T> {
    pub id: ConfigFieldId,
    pub name: &'static str,
    pub bits: SmallVec<[FuseId; 4]>,
    pub encoding: BTreeMap<T, SmallVec<[bool; 4]>>,
}

pub enum OutputPolarity { ActiveHigh, ActiveLow }
pub enum MacrocellMode { Combinational, Registered, InputOnly }
pub enum FeedbackSource { Pin, Combinational, Registered, None }
```

Only this layer knows whether a logical option is encoded by fuse zero or one.

## 4.4 AND matrix

```rust
pub struct AndMatrixSpec {
    pub rows: Vec<ProductTermSpec>,
    pub sources: Vec<LiteralSource>,
    pub cells: Vec<Vec<MatrixCellSpec>>,
}

pub struct MatrixCellSpec {
    pub fuse: FuseId,
    pub connected_value: bool,
    pub disconnected_value: bool,
}

pub struct LiteralSource {
    pub id: BoolInputId,
    pub true_column: MatrixColumn,
    pub complement_column: MatrixColumn,
    pub physical_source: PhysicalSignalSource,
}
```

Encoding a cube initializes the selected row to disconnected, connects each required literal, rejects contradictions, and then decodes the row to verify it.

## 4.5 Macrocells

```rust
pub struct MacrocellSpec {
    pub id: MacrocellId,
    pub pad: Option<PadId>,
    pub data_terms: Vec<ProductTermId>,
    pub oe_term: Option<ProductTermId>,
    pub supports_registered: bool,
    pub supports_combinational: bool,
    pub supports_input_only: bool,
    pub feedback_modes: Vec<FeedbackSource>,
    pub mode_field: Option<ConfigField<MacrocellMode>>,
    pub polarity_field: Option<ConfigField<OutputPolarity>>,
    pub fixed_clock: Option<ClockResourceId>,
}

pub struct MacrocellConfig {
    pub id: MacrocellId,
    pub assigned_signal: Option<LogicalOutputId>,
    pub mode: MacrocellMode,
    pub polarity: OutputPolarity,
    pub feedback: FeedbackSource,
    pub data_terms: Vec<PlacedCube>,
    pub oe_term: Option<PlacedCube>,
    pub pad_enabled: bool,
}
```

## 4.6 Packages

```rust
pub struct PackageSpec {
    pub id: PackageId,
    pub name: &'static str,
    pub pins: BTreeMap<PinNumber, PackagePin>,
}

pub enum PackagePin {
    Power(PowerRail),
    DedicatedInput(InputResourceId),
    Clock(ClockResourceId),
    Pad(PadId),
    SharedClockInput { clock: ClockResourceId, input: InputResourceId },
    NoConnect,
}
```

## 4.7 ATF22V10 model

Encode and verify:

- twelve input paths including clock/input role;
- ten output macrocells;
- one D flip-flop per macrocell;
- per-macrocell registered/combinational choice;
- per-macrocell polarity;
- per-output OE product term;
- variable data product-term allocations;
- common global clock;
- macrocell feedback;
- reset/preset resources if and as represented in the verified map;
- matrix, architecture, signature, security, and reserved fuse regions.

Builder:

```rust
pub fn atf22v10c() -> PalGalDevice {
    let mut b = PalGalDeviceBuilder::new("ATF22V10C");
    b.jedec_fuse_count(verified_fuse_count());
    b.default_fuse_state(true);
    b.add_package(atf22v10_dip24());
    b.add_global_clock(verified_global_clock());
    b.add_and_matrix(atf22v10_and_matrix());
    for mc in atf22v10_macrocells() {
        b.add_macrocell(mc);
    }
    b.build().expect("validated ATF22V10 definition")
}
```

Do not enter uncertain numeric fuse positions from memory. Each mapping must cite its evidence in source comments and have fixtures.

Required invariants:

- matrix cells map one-to-one to fuses;
- fields do not overlap unless explicitly documented;
- every fuse is classified;
- every product term belongs to the expected resource;
- package mappings are unique;
- every legal configuration round-trips through encode/decode.

## 4.8 ATF16V8 model

ATF16V8 requires a global mode model:

```rust
pub enum Atf16v8Mode {
    Registered,
    Complex,
    Simple,
}
```

### Registered mode

- selected when any register is used;
- pin 1 is common clock;
- pin 11 is common OE for registered outputs;
- those pins are not ordinary inputs in this mode;
- registered macrocell: eight data terms;
- combinational macrocell in registered mode: seven data terms plus one OE term;
- mode-specific input/feedback restrictions apply.

### Complex mode

- combinational only;
- selected when product-term-controlled OE is required;
- pins 1 and 11 act as inputs;
- seven data terms plus one OE term;
- outer macrocell input/feedback restrictions apply.

### Simple mode

- combinational outputs without product-term OE;
- eight data terms;
- center and adjacent feedback restrictions apply;
- pins 1 and 11 are inputs.

Fitting evaluates every compatible mode, records rejection reasons, honors an optional user mode constraint, and chooses deterministically among successful modes.

---

# 5. Fitting and physical validation

## 5.1 Mapped logical outputs

```rust
pub struct LogicalOutputSop {
    pub id: LogicalOutputId,
    pub name: HierPath,
    pub pin_constraint: Option<PinNumber>,
    pub registered: bool,
    pub data_true: Sop,
    pub data_complement: Sop,
    pub output_enable: Option<Sop>,
    pub needs_internal_feedback: bool,
    pub external_observation: bool,
}
```

## 5.2 ATF22V10 fitter

Use deterministic backtracking.

Candidate compatibility considers:

- explicit pin constraint;
- register capability;
- input/readback and feedback capability;
- OE support;
- true and complemented product-term counts;
- clock compatibility;
- reserved resources.

Sort outputs by:

1. explicit pin constraint;
2. fewest candidate macrocells;
3. registered and OE restrictions;
4. descending product-term need;
5. stable hierarchical name.

Try each compatible macrocell and polarity, reserve owned rows, recurse, and backtrack.

Cost:

```rust
pub struct FitCost {
    pub total_product_terms: u32,
    pub total_literals: u32,
    pub buried_macrocells: u32,
    pub nonpreferred_pin_moves: u32,
    pub polarity_inversions: u32,
}
```

Fit failures must explain exact resource limits.

## 5.3 ATF16V8 fitter

Outer loop:

```text
for each legal global mode:
    construct mode-specific resources
    validate pin/global-role constraints
    attempt fit
choose the lowest-cost success
```

Report why every failed mode was rejected.

## 5.4 Physical validation

Before fuse encoding independently verify:

- every logical output placed;
- no macrocell or product term duplicated;
- every literal routable;
- clock/OE resources legal;
- pin behavior legal;
- every field encodable;
- reserved fuses unchanged;
- every equation fits its rows;
- required feedback selected;
- reconstructed physical equations equal the requested mapped design.

---

# Part VI — Fuse encoding and JEDEC

## 5.5 Encoding order

1. Start with target erased/default fuse state.
2. Encode global architecture mode.
3. Encode macrocell modes.
4. Encode polarity.
5. Encode feedback choices.
6. Encode data product-term rows.
7. Encode OE rows.
8. Encode verified common reset/preset resources when used.
9. Encode safe unused-resource states.
10. Leave signature blank unless requested.
11. Leave security clear unless explicitly requested.
12. Validate reserved values.
13. Decode generated fuses and compare to the physical design.
14. Write JEDEC.

## 5.6 JEDEC model

JEDEC transfers numbered fuse/cell states; it does not define the target architecture.

Required support:

- STX/ETX framing;
- `QF` fuse count;
- `F` default state;
- `L` fuse data ranges;
- `C` fuse checksum;
- `N` notes;
- `G` security field when present;
- transmission checksum where present;
- line-ending and whitespace variants;
- preservation/reporting of unknown fields.

```rust
pub struct JedecFile {
    pub design_specification: String,
    pub fuses: FuseVector,
    pub default_fuse: Option<bool>,
    pub notes: Vec<String>,
    pub security: Option<bool>,
    pub fuse_checksum: Option<u16>,
    pub transmission_checksum: Option<u16>,
    pub unknown_fields: Vec<JedecField>,
}
```

Three deviations from the original sketch, all deliberate:

- `fuse_count` and `fuses: BitVec` are collapsed into one `FuseVector`, which owns the count. A `fuse_count` that disagrees with the length of the fuse data is not a state worth being able to represent.
- `default_fuse` is an `Option`, not a `bool`. `None` means the file carried no `F` field, which JEDEC 3A permits only when every fuse state is stated explicitly — a different claim from "unlisted fuses are 0", and one the type must be able to hold. As a plain `bool` the parser zero-filled whatever the `L` fields did not reach, said nothing about it, and the writer then emitted the `F0*` that made the invention look deliberate. Same argument as `security` below: silence is not an instruction.
- `design_specification` is added, and is **not** optional. The free-text header between STX and the first `*` is part of the file and must survive a rewrite. JEDEC cannot express its absence — the header *is* the first field, so even `<STX>*…` has one, empty. An `Option` would let the type say something the format cannot; a round-trip property test found exactly that, with a `None` header returning as `Some("")` because there was nowhere to record the difference.

`FuseVector` stores JEDEC's own 8-bit word layout: fuse *N* is bit `N % 8` of word `N / 8`, least-significant bit first. The fuse checksum is then the sum of the words, with no separate packing step that could be written backwards.

The `L` field's separator between fuse number and fuse states is **required**, not merely conventional. Fuse states are `0` and `1`, which are also decimal digits, so without the separator the field is ambiguous and must be rejected rather than guessed at.

**A file with no `F` field must state every fuse.** JEDEC 3A: *"If no F field is specified, all fuse states must be defined after the QF field …"* This is checked, not assumed: the parser records which fuses each `L` field actually stated and refuses a file that leaves any unstated, naming how many and the first one. Zero-filling the gap would invent states the file never gave, and a device whose fuse map is 12 fuses' worth of guesswork is exactly the artifact this project exists not to produce.

Coverage is tracked separately from `FuseVector` rather than as a flag on it. It is a fact about how a *file* was written, not about a device's fuse states, and a vector carrying it would have to exclude it from equality by hand — otherwise a parsed vector would compare unequal to an identical constructed one, breaking `jed diff` and every round-trip property.

The writer never invents a default: a file that arrived without an `F` field leaves without one. Compact style consequently states every fuse for such a file, since with no default there is nothing to differ from.

**Cardinality and ordering of `F`.** JEDEC 3A gives `<fuse information> ::= [<default state>] <fuse list> {<fuse list>} [<fuse checksum>]`: at most one `F`, and it precedes every `L`. Both are enforced, because the fuse vector is built from `F` before any `L` is applied — so an `F` arriving late would retroactively become the base for fuse lists written against a different default. An `F` after an `L` is always an error: which fuses it governs depends on reading order.

**Repetition versus contradiction.** For the two fields where a repeat could change what a file means — `F` and `G` — a second field naming the *same* state is a warning, and one naming a *different* state is an error. The distinction is load-bearing: `F0*F0*` has exactly one possible meaning and refusing it would reject a file whose intent is not in doubt, while `F0*F1*` has none. The rule matters most for `G`: the security fuse is irreversible and setting it requires two explicit CLI flags, so resolving a self-contradictory file by last-writer-wins would infer "permanently lock this part" and walk around that gate entirely. A repeated `C` needs no such rule — the checksum is recomputed on every write, and a `C` disagreeing with the fuse data already fails its own check.

**Allocation ceiling.** `QF` alone drives allocation, so an unbounded value turns a 19-byte file into hundreds of megabytes. A device-independent ceiling — this layer knows nothing about devices — rejects absurd counts before allocating. It sits far above any real part and is a guard against malformed input, not a device limit.

Parser modes: strict, compatible, preserve-unknown, selected by `--strictness`. Writer styles: canonical and compact, selected by `--style`.

`WinCuplComparable` is **deferred to M1**, not merely unimplemented. Matching WinCUPL's layout requires having WinCUPL's output to match against, and inventing a format from memory and calling it "WinCUPL-comparable" is precisely the unevidenced guess §2.9 forbids. It lands with the oracle harness, against captured files.

**Field identifiers are classified three ways, not two.** JEDEC 3A gives two BNF productions: `<field identifier>` (`A C D F G L N P Q R S T V X`) and `<reserved identifier>` (`B E H I J K M O U W Y Z`). They are disjoint and together cover A–Z exactly, which is the check a transcription of them must satisfy.

- **Defined** — structurally legal JEDEC, whether or not deCPLD models it. Test vectors and pin lists are not the user's problem and produce no diagnostic. Classification is by the *first* character, because the standard permits multi-character identifiers as subfields (`A1`, `A$`, `AB3`), so `QX` is a subfield of the defined `Q` rather than an invented identifier. Only `QF`, `QP` and `QV` are *defined* `Q` subfields, but the standard nowhere forbids others, and deCPLD does not invent a rejection it cannot cite.
- **Reserved** — the standard says receiving equipment should *ignore* these, so deCPLD raises no diagnostic in any mode. Real Atmel-toolchain files carry `J` and `U`. "Ignore" means "do not complain", not "discard": the field is still retained in preserve-unknown mode, because dropping a vendor's data is the same loss as dropping test vectors.
- **Not in the standard** — since the two tables partition the alphabet, this means the field did not begin with an *upper-case* letter. Lower case lands here too: no tool emits it, and accepting it would silently widen the standard's table. Such a field is reported (error in strict, warning otherwise) **and retained**, so a rewrite still emits it. Discarding it was silent data loss in the default mode.

Splitting a field into identifier and body is a separate question from classifying it, and must use a separate table. Conflating them is what dropped `T` and `Q` from the identifier set: a letter had to earn its place by being splittable at one character, and `Q` is not.

**Writable content is JEDEC 3A's `<field character>` class**: `0x20–0x29`, `0x2B–0x7E`, CR, LF. The gap at `0x2A` is the field terminator `*`, so one predicate covers the whole class — including the asterisk case — rather than a check per offending character. It applies to the header, every note, and **both the identifier and the body** of every unmodelled field. Content outside the class is refused with an error naming the character, because "this text cannot be spelled in JEDEC" is a user-fixable condition and must not surface as an internal error. The **parser** reports the same condition independently, at the offset where it occurs — by the time the writer sees a `String` there is no position left to point at, so the only symptom would be a failed rewrite naming a category rather than a place. Both sides share one predicate: two copies of this class would eventually disagree about which files are writable. Note that the class stops at `0x7E`: JEDEC predates Unicode and non-ASCII genuinely cannot be encoded.

An unmodelled field must also survive its own notation. The writer renders each one and splits it back; if it would not read as the field it came from — an empty identifier followed by a body starting with a letter has no JEDEC spelling, because the body becomes the identifier — it is refused, naming what was asked for and what it would have become. Asking the question by round-tripping rather than by a rule about safe shapes means the check cannot drift from the splitter it has to agree with. Without it these cases still failed safely, via the whole-file verification, but reported an internal error for a caller-fixable condition.

Retaining a non-conformant field means `decpld jed canonicalize` can emit a file that `decpld jed validate --strictness strict` then rejects. That is the intended trade and not a defect: preserving what the input actually contained beats inventing a conformant substitute for it, and the non-conformance is reported both times. A user who wants the field gone can ask for it with compatible mode.

**An `L` field applies all or nothing.** States are accumulated and committed only once the whole field is known good, so no fault can leave the fuse vector half-updated. Every bad state character in a field is reported, not just the first — one run should tell the user everything wrong with the file. A fuse number past `QF` is the exception and stops the field: every state after it is out of range too, so continuing would emit one diagnostic per remaining character, all saying the same thing.

The writer always **recomputes** both checksums rather than copying what the input declared. Both are derived values: the fuse checksum from the fuse data, the transmission checksum from the emitted bytes. Preserving them would propagate a defect — a file carrying `C0000` ("not computed") should leave canonicalisation with a real checksum, and any file whose bytes changed must carry a new transmission checksum or it fails its own verification. The round-trip invariant is therefore stated over a file's *content* — header, fuses, default state, notes, security bit, and unmodelled fields — and not over its checksums or the source positions of its fields.

JEDEC has no escape mechanism, so free text containing `*` cannot be encoded. The writer refuses rather than emitting a file that would silently read back as something else.

**The writer verifies itself.** `write` parses its own output and compares it with the file it was given, and returns an error instead of a string if they differ. This is §5.27's encode-then-decode-then-compare rule applied to the writer rather than only to a test property: a test can only check the cases someone thought to generate, and this check does not depend on anyone having imagined the failure. It costs one extra parse of a file measured in kilobytes.

Two limits on what it proves, both deliberate. It is a *self-consistency* check — the writer against deCPLD's own parser — not a conformance check against JEDEC 3A; any assumption the two share passes silently. And the comparison is made against the file as the writer is entitled to emit it, with value fields hoisted ahead of the programming fields as JEDEC 3A requires, since that reordering is a service to hand-built files rather than a corruption. A round-trip failure is therefore always a deCPLD bug, never a user error, and says so.

Cross-check checksum implementations against WinCUPL, GALasm, Galette, and golden fixtures.

## 5.7 JEDEC inspection

```bash
decpld jed inspect design.jed --device ATF22V10C --package DIP24
```

Report:

- fuse count/checksums;
- selected mode;
- each macrocell's pin, mode, polarity, OE, feedback, and equation;
- unused product terms;
- reserved/security/signature status.

Also provide `--json`.

---

# Part VII — WinCUPL oracle workflow

## 5.8 Purpose

Use WinCUPL to independently generate:

- minimized equations;
- fit and pin reports;
- device-mode selections;
- JEDEC files;
- metadata outputs.

Triangulate results with official datasheets, GALasm/Galette, round-trip invariants, and physical hardware. WinCUPL is not assumed infallible.

## 5.9 Wine and command-line environment

WinCUPL is a GUI over command-line programs including `cupl.exe` and device libraries. Provide a project wrapper with configurable paths:

```bash
DECPLD_WINEPREFIX
DECPLD_WIN_CUPL_ROOT
DECPLD_WIN_CUPL_EXE
DECPLD_WIN_CUPL_LIBRARY
```

Representative wrapper:

```bash
#!/usr/bin/env bash
set -euo pipefail

: "${DECPLD_WINEPREFIX:=$HOME/.wine-wincupl}"
: "${DECPLD_WIN_CUPL_ROOT:=C:\\Wincupl}"
: "${DECPLD_WIN_CUPL_EXE:=${DECPLD_WIN_CUPL_ROOT}\\Shared\\cupl.exe}"
: "${DECPLD_WIN_CUPL_LIBRARY:=${DECPLD_WIN_CUPL_ROOT}\\Shared\\cupl.dl}"

export WINEPREFIX="$DECPLD_WINEPREFIX"
work_dir="$(realpath "$1")"
base="${2%.pld}"

wine cmd /c \
  "set LIBCUPL=${DECPLD_WIN_CUPL_LIBRARY} && cd /d Z:${work_dir//\//\\} && \"${DECPLD_WIN_CUPL_EXE}\" -jaxfl ${base}"
```

CUPL documentation gives commands such as `cupl -jaxfl p16r4 sample`, with `j` requesting JEDEC output. The harness must capture the installed executable's help/usage and record exact behavior rather than assuming all releases are identical.

For every run record:

```json
{
  "wine_version": "...",
  "wincupl_version": "...",
  "cupl_exe_sha256": "...",
  "device_library_sha256": "...",
  "command_line": "...",
  "environment": { "LIBCUPL": "..." }
}
```

Do not redistribute proprietary WinCUPL files or embedded serial numbers.

## 5.10 Output files

Retain every generated artifact, including where supported:

- `.JED` fuse map;
- `.DOC` compiled/minimized equations and device information;
- `.LST` listing and diagnostics;
- `.ABS` device-specific absolute output;
- `.EQN` equations;
- `.PLA` PLA representation;
- fuse plot;
- simulation output;
- stdout and stderr.

Fixture directory:

```text
fixture/
├── input.pld
├── command.json
├── stdout.txt
├── stderr.txt
├── output.jed
├── output.doc
├── output.lst
├── output.abs
├── output.eqn
├── normalized/
│   ├── fuses.bin
│   ├── equations.json
│   ├── pins.json
│   └── metadata.json
└── manifest.json
```

## 5.11 Experiment suite

Generate minimal controlled designs.

Baseline:

- blank if accepted;
- constant zero;
- constant one;
- each output pin.

Literal mapping, for every input/feedback and output:

- `Y=A`;
- `Y=!A`;
- `Y=A&B`;
- `Y=A#B`.

Polarity:

- function and complement;
- active-high/low declarations;
- every macrocell.

Registered logic:

- `Y.D=A`;
- explicit and implicit clock forms;
- `Y.D=!Y`;
- registered feedback;
- hold mux;
- reset/preset resources where supported.

Output enable:

- always enabled/disabled;
- `Y.OE=E` and `!E`;
- bidirectional readback.

Capacity:

- one through N independent product terms per macrocell;
- determine exact row ownership and fit boundary.

ATF16V8 global modes, using installed mnemonics such as:

```text
G16V8MS registered
G16V8MA complex
G16V8AS simple
G16V8   auto
```

## 5.12 Differential analysis

Parse JEDEC files and compare fuse vectors, not raw text.

```rust
pub struct FuseDelta {
    pub index: u32,
    pub before: bool,
    pub after: bool,
}

pub struct JedecDiff {
    /// Set when the two files declare different fuse counts, in which
    /// case `fuses` is empty.
    pub fuse_count: Option<(u32, u32)>,
    pub fuses: Vec<FuseDelta>,
    pub default_fuse: Option<(Option<bool>, Option<bool>)>,
    pub security: Option<(Option<bool>, Option<bool>)>,
    pub design_specification: Option<(String, String)>,
    pub notes: Option<(Vec<String>, Vec<String>)>,
    pub unknown_fields: Option<(Vec<String>, Vec<String>)>,
}
```

`index` is a bare `u32`, not a `FuseId`, and that is deliberate: `FuseId` is a device-layer concept, and `decpld-jedec` is architecture-free by construction. Classifying a delta as "polarity" or "mode" requires knowing what a fuse *means* and therefore belongs to the target that knows — so `decpld oracle diff --device` performs that classification over a `JedecDiff`, rather than the JEDEC layer producing pre-classified deltas it has no basis for.

Differing fuse counts suppress the fuse comparison entirely. Fuse *N* of a 16-fuse device and fuse *N* of a 32-fuse device are not the same fuse, and listing deltas between them would bury the one finding that matters under a wall of noise.

`JedecDiff` and `JedecFile::describes_same_device_as` must agree on what "the same file" means. Two notions that disagreed would let `jed diff` bless a rewrite that silently deleted a device's test vectors, which is why unmodelled fields are compared here and not merely preserved.

Classify each delta as matrix connection, mode, polarity, OE, architecture-wide mode, signature/checksum, or unknown.

```bash
decpld oracle diff baseline.jed changed.jed --device ATF22V10C
```

A mapping becomes verified only when multiple independent fixtures agree, invariants hold, encode/decode round-trips, and preferably hardware testing succeeds.

## 5.13 Metadata parsing

Normalize `.DOC`, `.LST`, `.ABS`, and `.EQN` into:

```rust
pub struct CuplReport {
    pub device: Option<String>,
    pub pins: Vec<CuplPinAssignment>,
    pub equations: Vec<CuplEquation>,
    pub product_terms: Vec<CuplProductTermUse>,
    pub diagnostics: Vec<CuplDiagnostic>,
    pub raw_sections: Vec<RawReportSection>,
}
```

Extract selected device, pins, equations, `.D/.CK/.OE` extensions, product-term counts, mode, warnings, and compiler version where present. Treat report grammar as oracle tooling, not a stable production dependency.

## 5.14 Fixture generation

Generate CUPL fixtures from a typed fixture model rather than hand-writing hundreds of files:

```rust
pub struct CuplFixture {
    pub name: String,
    pub device: String,
    pub pins: Vec<CuplPin>,
    pub equations: Vec<CuplAssignment>,
    pub options: CuplOptions,
}
```

For sequential fixtures, generate D truth tables from an independent expected-behavior model, not by invoking the deCPLD compiler.

## 5.15 Hardware validation

A mapping is not fully trusted only because WinCUPL agrees.

Required hardware tests:

- exhaustive combinational truth tables;
- counter and shift sequences at conservative clock rate;
- synchronous hold/reset;
- output enable and high impedance;
- repeated erase/program/verify cycles.

Store part marking, programmer version, JEDEC hash, test vectors, and results.

Programming remains external, e.g.:

```bash
minipro -p ATF22V10C -w build/design.jed
```

Discover the exact programmer part name from the installed tool.

---

# Part VIII — CLI and reports

## 5.16 Build and package discovery

deCPLD has two build modes.

### 5.16.1 Single-file mode

A single source file requires no manifest:

```bash
decpld build design.decpld
```

The implicit package contains only that file. It may contain any number of modules, enums, constants, and tops.

Top selection rules:

- no `top`: build error;
- exactly one `top`: selected automatically;
- more than one `top`: `--top <qualified-name>` is required.

```bash
decpld build design.decpld --top CounterDemo
```

Single-file mode has no external package dependencies.

### 5.16.2 Multi-file package mode

A `decpld.toml` manifest defines the package source roots, dependencies, and persistent options.

```toml
[package]
name = "video-plds"

[sources]
roots = ["tops", "lib"]

[dependencies]
components = { path = "../components" }

[build]
optimization = 2
```

All `.decpld` files recursively beneath each source root belong to the package. Directory paths relative to a source root determine package namespaces.

```bash
decpld build --top deCPLDer
```

When no manifest is present, multiple local source roots may be supplied explicitly:

```bash
decpld build \
  --source-root tops \
  --source-root lib \
  --top deCPLDer
```

`--source-root` may be repeated. This creates an implicit local package but does not support external dependencies. The shorter alias `--dir` may be accepted, but documentation and diagnostics should use `--source-root`.

The compiler indexes the entire package, selects one named top, and elaborates only the module graph reachable from that top. Syntax, duplicate-name, visibility, and interface errors are package-level; device fitting applies only to the selected reachable design.

### 5.16.3 Build options

```text
--top QUALIFIED_NAME
--source-root DIR
--device NAME      checked assertion; must match selected top's `device`
--package NAME     checked assertion; must match selected top's `device`
-o, --output PATH
-O0/-O1/-O2
--emit jed,report,eqn,json,fuses
--out-dir DIR
--pin NAME=NUMBER
--deny-warnings
--no-polarity-opt
--mode auto|registered|complex|simple
--user-signature VALUE
--security-fuse
--acknowledge-readback-lock
--verify-roundtrip
--verify-equivalence
```

Round-trip and equivalence validation are enabled by default.

## 5.17 Other commands

```bash
decpld check design.decpld
decpld fmt design.decpld
decpld fmt --check .
decpld sim design.decpld --vectors vectors.json
decpld report design.decpld --format text
decpld report design.decpld --format json

decpld jed inspect file.jed --device ATF22V10C
decpld jed validate file.jed --strictness strict|compatible|preserve-unknown
decpld jed canonicalize input.jed -o output.jed --style canonical|compact
decpld jed diff a.jed b.jed

decpld oracle env
decpld oracle compile fixture.pld --out-dir result/
decpld oracle generate-suite --device ATF22V10C
decpld oracle analyze-suite targets/fixtures/atf22v10
```

`--strictness` selects the parser mode. It is deliberately **not** called `--mode`: §5.16.3 already gives `decpld build --mode auto|registered|complex|simple`, which is the ATF16V8 datasheet's own word for its global modes, and `jed inspect --device` will report one. Two unrelated meanings of `--mode` on one command is a collision worth spending a longer flag name to avoid.

`--style` selects the writer style. Both default to the tolerant choice: `preserve-unknown` and `canonical`.

### 5.17.1 Exit codes

The `jed` commands follow `diff(1)`, so they compose into scripts:

| Code | Meaning |
| --- | --- |
| `0` | Nothing to report |
| `1` | A finding — the command did its job and the answer is negative |
| `2` | Trouble — the command could not do its job at all |

The distinction is what makes the commands scriptable: collapsing "the files differ" into the same code as "the file could not be read" forces every caller to parse stderr to tell a result from a failure. Clap already exits `2` on a usage error, which is trouble of exactly the same kind.

Which condition is a *finding* depends on what the command was asked to do, and the same words map to different codes:

- `jed validate` on an invalid file exits **1**. "This file is not valid" *is* the answer.
- `jed diff` on files that differ exits **1**; on an unreadable input, **2**.
- `jed canonicalize` on an unreadable input exits **2**. It was asked to rewrite a file and could not.

**Diagnostics always go to stderr and results always to stdout**, whether or not the operation succeeded. Which stream carries a diagnostic must depend on it being a diagnostic, never on the luck of the parse — otherwise `decpld jed canonicalize in.jed > out.jed` produces a fuse map with a warning glued to the front.

Optional programming convenience must be explicit and never run automatically:

```bash
decpld program build/design.jed --programmer minipro --part ATF22V10C
```

## 5.18 Diagnostics

```rust
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub fixes: Vec<Fix>,
}
```

Examples:

```text
error[E0204]: value 16 does not fit in signal[4]
```

```text
error[E1302]: registered signal requires ATF22V10 pin 1 as global clock
```

```text
error[E2207]: no ATF16V8 mode can implement this design
  registered: pin 11 used as ordinary input
  complex: design contains registers
  simple: design contains registers
```

Every fit error must identify the limiting resource and actionable alternatives.

### 5.18.1 Diagnostic code ranges

Codes render as `E` followed by four zero-padded digits. The thousands digit selects the layer, so a code identifies where a failure came from before anyone looks it up:

| Range | Layer |
| --- | --- |
| `0xxx` | Lexing, parsing, types, widths, signedness, literals |
| `1xxx` | Producers, storage, clocks, pads, visibility, packages |
| `2xxx` | Fitting, device resources, mode selection |
| `3xxx` | JEDEC parsing, writing, checksums, fuse encoding |
| `4xxx` | CLI, manifest, and I/O |
| `9xxx` | Internal compiler errors — always a deCPLD bug, never a design error |

Ranges `5xxx` through `8xxx` are unallocated. A code outside every allocated range is reported as unclassified rather than folded into a neighbouring layer, so a mis-numbered code stays visible.

Codes are permanent. Once `E0204` means "value does not fit", it always means that: add new codes, never renumber. A code whose meaning changes silently invalidates every issue report, script, and suppression list that quotes it.

---

# Part IX — Simulation and equivalence

## 5.19 RTL simulator

Cycle semantics:

1. set external inputs;
2. settle combinational network;
3. sample outputs;
4. on positive edge evaluate every next-state from current state;
5. update all registers simultaneously;
6. settle again.

Support `Z` for pad observation. Internal simulation may remain two-state in version 1.

## 5.20 Physical simulator

Decode a generated fuse map into physical equations and simulate that model. Require RTL and decoded-fuse simulation to agree for exhaustive small tests and randomized larger tests.

## 5.21 Equivalence checks

Verify:

- Boolean graph versus minimized SOP;
- complement plus output polarity versus requested function;
- RTL next-state versus placed macrocell equations;
- physical design versus decoded fuse vector;
- decoded behavior versus mapped design.

Use exhaustive enumeration for small support sets and a SAT or BDD implementation for larger equations.

---

## 5.22 Autoformatter

`decpld` includes a canonical source formatter:

```bash
decpld fmt src/main.decpld
decpld fmt --check src/main.decpld
decpld fmt --check .
```

Formatting is normative and uses **two spaces per indentation level**. Tabs are never emitted for indentation. The formatter must:

- preserve ordinary and documentation comments;
- keep `///` comments attached to their declarations;
- use two-space indentation in modules, blocks, `match` arms, and multiline concatenations;
- place one declaration or assignment per line except where a short expression remains readable;
- include trailing commas in multiline enums, module instances, concatenations, and `match` arms;
- normalize spacing around operators and after commas;
- format idempotently;
- produce identical output through the CLI and LSP formatting request.

The formatter operates on the lossless syntax tree and should format syntactically incomplete files on a best-effort basis for editor use. `decpld fmt --check` exits nonzero when any selected file would change.

---

# Part X — Language server

## 5.23 Executable and architecture

Executable: `decpld-lsp`.

Use `tower-lsp` or a comparable maintained Rust framework. Keep protocol-independent analysis in shared crates. Use an incremental query system such as Salsa.

```rust
#[salsa::input]
fn source_text(db: &dyn Db, file: FileId) -> Arc<str>;

#[salsa::tracked]
fn parse(db: &dyn Db, file: FileId) -> ParsedFile;

#[salsa::tracked]
fn module_index(db: &dyn Db, workspace: WorkspaceId) -> ModuleIndex;

#[salsa::tracked]
fn elaborate_top(db: &dyn Db, top: TopSelection, target: TargetSelection)
    -> ElaboratedDesign;

#[salsa::tracked]
fn diagnostics(db: &dyn Db, file: FileId, target: TargetSelection)
    -> Arc<Vec<Diagnostic>>;
```

The LSP gives syntax/type diagnostics without a target and target-specific pin/fitting diagnostics when a device is known.

## 5.24 Required LSP features

- diagnostics;
- semantic tokens;
- completion;
- hover;
- go to definition;
- find references;
- rename;
- document/workspace symbols;
- formatting;
- code actions;
- module-instance signature help;
- inlay hints;
- folding ranges;
- documentation-comment hover and completion rendering.

Completion examples:

- module names;
- missing named parameters/ports;
- enum variants;
- signal names;
- legal `pins[...]` values;
- uncovered match variants.

Hover displays attached `///` documentation before inferred type and physical information.

Hover examples:

```text
count: signal[4]
inferred registered output
clock: clock / pin 1
placed on pins 19..16
```

```text
pins[1]
ATF22V10C DIP24 CLK/IN
used as global positive-edge clock
```

Inlay hints should show inferred parameters such as `width: 8`.

Code actions:

- add missing `else`;
- add missing enum match arms;
- insert explicit narrowing slice;
- add missing module arguments;
- add target declaration;
- suggest compatible output pins after fit failure.

Workspace file:

```toml
[project]
root = "src/main.decpld"

# Optional checked assertions; the source top remains authoritative.
device = "ATF22V10C"
package = "DIP24"

[build]
optimization = 2

[lsp]
show_inferred_widths = true
run_fitter_on_save = true
```

Debounce fitting and cancel stale work.

---

# Part XI — Testing and milestones

## 5.25 Test layers

- lexer/parser snapshots and error recovery;
- formatter idempotence;
- width, signedness, and parameter inference;
- producer and clock semantics;
- Boolean minimization and equivalence;
- fitting boundaries and diagnostics;
- ATF16V8 mode selection;
- JEDEC parsing/writing/checksums;
- WinCUPL differential fixtures;
- physical hardware tests;
- LSP protocol tests.

Not all valid JEDEC files are byte-identical. Define comparison levels:

```rust
pub enum ComparisonLevel {
    ExactFile,
    ExactFuseVector,
    ExactPhysicalConfiguration,
    SemanticEquivalent,
    HardwareEquivalent,
}
```

Use exact comparison only for deliberately pinned oracle experiments. Normal compilation acceptance requires semantic and physical correctness.

## 5.26 Milestones

### M0 — JEDEC foundation

Parse, validate, canonicalize, and rewrite known JEDEC files with correct checksums.

### M1 — ATF22V10 decoder/encoder

Decode WinCUPL/Galette files into macrocells and equations; encode canonical equivalents; satisfy round-trip invariants.

### M2 — Minimal combinational language

`signal`, pins, Boolean expressions, top, SOP minimization, fitting, and working combinational hardware.

### M3 — Registered logic

`on posedge`, next-state lowering, counter, shifter, decoded-fuse simulation, and physical ATF22V10 tests.

### M4 — Modules and parameters

Named arguments, typed parameters, constraint inference, `if`, `match`, concatenation, and destructuring.

### M5 — ATF16V8

Registered/complex/simple mode model, differential suite, and hardware validation.

### M6 — LSP

Diagnostics, completion, hover, navigation, formatting, signature help, inlay hints, and target-aware pins.

### M7 — Release quality

Fuzzing, reproducible builds, stable JSON reports, packaging, complete evidence and hardware matrix.

---

# Part XII — Key logic

## 5.27 Compile driver

```rust
pub fn compile(req: CompileRequest) -> Result<CompileArtifacts, DiagnosticBundle> {
    let parsed = syntax::parse(&req.source)?;
    let workspace = hir::index(parsed)?;
    let target = target_registry::resolve(&req.target)?;

    let elaborated = hir::elaborate(
        &workspace,
        req.top,
        &req.parameter_overrides,
    )?;

    types::check(&elaborated)?;
    semantics::infer_and_validate_producers(&elaborated)?;

    let rtl = rtl::lower(&elaborated, &target.package())?;
    let rtl = rtl::optimize(rtl, req.optimization)?;

    let mapped = target.map_to_sop(&rtl)?;
    logic::verify_mapping(&rtl, &mapped)?;

    let physical = target.fit(
        &mapped,
        req.package,
        &req.physical_constraints,
    )?;

    target.validate_physical(&physical)?;

    let fuses = target.encode(&physical)?;
    let decoded = target.decode(&fuses)?;

    physical::assert_equivalent(&physical, &decoded)?;
    logic::assert_equivalent(&mapped, &decoded.logical_view())?;

    let jed = jedec::write(
        &fuses,
        target.jedec_metadata(),
        req.jedec_style,
    )?;

    Ok(CompileArtifacts {
        jed,
        report: report::build(&elaborated, &mapped, &physical, &fuses),
        physical,
        fuses,
    })
}
```

## 5.28 Priority match lowering

```rust
fn lower_priority_match(arms: &[ConditionArm], else_value: ValueId) -> ValueId {
    arms.iter().rev().fold(else_value, |fallback, arm| {
        make_mux(arm.condition, arm.value, fallback)
    })
}
```

## 5.29 Cube encoding

```rust
fn encode_cube(
    map: &mut FuseMap,
    matrix: &AndMatrixSpec,
    row: ProductTermId,
    cube: &Cube,
) -> Result<(), EncodeError> {
    for cell in matrix.row(row).cells() {
        map.set(cell.fuse, cell.disconnected_value)?;
    }

    let mut seen = HashMap::<BoolInputId, Polarity>::new();
    for literal in &cube.literals {
        if let Some(previous) = seen.insert(literal.input, literal.polarity) {
            if previous != literal.polarity {
                return Err(EncodeError::ContradictoryCube);
            }
        }
        let cell = matrix.cell_for_literal(row, *literal)?;
        map.set(cell.fuse, cell.connected_value)?;
    }
    Ok(())
}
```

## 5.30 Fitter recursion

```rust
fn search(
    index: usize,
    ordered: &[LogicalOutputSop],
    state: &mut FitState,
    best: &mut Option<PhysicalDesign>,
) {
    if index == ordered.len() {
        let candidate = state.finish();
        if best.as_ref().is_none_or(|b| candidate.cost < b.cost) {
            *best = Some(candidate);
        }
        return;
    }

    let output = &ordered[index];
    for candidate in state.candidates(output) {
        if !state.can_place(output, &candidate) {
            continue;
        }
        let checkpoint = state.checkpoint();
        state.place(output, candidate);
        if !state.lower_bound_worse_than(best.as_ref()) {
            search(index + 1, ordered, state, best);
        }
        state.restore(checkpoint);
    }
}
```

---

# Part XIII — Evidence, safety, and definition of done

## 5.31 Evidence levels

```rust
pub enum EvidenceLevel {
    Hypothesis,
    DifferentiallyVerified,
    OpenSourceCrossChecked,
    HardwareVerified,
}
```

Production target fields must meet the project's configured evidence threshold. Unverified hypotheses belong only in oracle-analysis code or disabled experimental targets.

## 5.32 Safety

- Security fuse clear by default.
- Programming security requires both `--security-fuse` and `--acknowledge-readback-lock`.
- Reserved fuse changes are hard errors.
- Build never applies programming voltage.
- External programming is explicit and logged.
- Same source, compiler, target database, and options produce the same fuse vector.
- Timestamps are excluded in reproducible mode.

## 5.33 Primary references

Keep exact revisions and hashes in `targets/evidence/`.

1. Microchip/Atmel, **ATF22V10C(Q) datasheet**: architecture, pinout, product terms, registers, signature/security behavior.  
   <https://ww1.microchip.com/downloads/en/DeviceDoc/doc0735.pdf>

2. Microchip/Atmel, **ATF16V8B/BQ/BQL datasheet**: registered, complex, and simple modes and compiler mnemonics.  
   <https://ww1.microchip.com/downloads/en/DeviceDoc/Atmel-0364-PLD-ATF16V8B-8BQ-8BQL-Datasheet.pdf>

3. Atmel, **WinCUPL User's Manual**: CUPL extensions and compiler behavior.  
   <https://ww1.microchip.com/downloads/en/DeviceDoc/doc0737.pdf>

4. **CUPL Programmer's Reference Guide**: syntax, command-line flags, register extensions, minimization, and output reports.  
   <https://ece-classes.usc.edu/ee459/library/documents/CUPL_Reference.pdf>

5. **JEDEC File Standard 3A** historical text.  
   <https://k1.spdns.de/Develop/Projects/GalAsm/info/JEDEC%20File%20Standard%203A.txt>

6. **Galette**, Rust GAL assembler and useful architecture cross-check.  
   <https://github.com/simon-frankau/galette>

7. **GALasm**, device maps and JEDEC/checksum implementation.  
   <https://github.com/daveho/GALasm>  
   <https://github.com/dwery/galasm>

8. Command-line WinCUPL wrapper prior art.  
   <https://github.com/adrienkohlbecker/cupl.bat>

Numeric fuse mappings are not accepted merely because one source states them; they must satisfy differential and round-trip tests.

## 5.34 Definition of done

Version 1 is complete when:

1. The Rust workspace builds and all non-oracle tests pass on a clean machine.
2. Combinational examples generate valid, programmable JEDEC for ATF22V10 and ATF16V8.
3. Counter and shift-register examples work on physical ATF22V10 hardware.
4. Registered logic works on physical ATF16V8 in registered mode.
5. Complex-mode OE and simple-mode combinational examples work on physical ATF16V8.
6. Every emitted fuse vector decodes to an equivalent physical configuration.
7. Every minimized equation is verified against its source Boolean function.
8. Differential fixtures cover every input source, output macrocell, polarity, register mode, OE path, and ATF16V8 global mode.
9. Fit reports explain exact resource use and failure.
10. The LSP supplies diagnostics, completion, hover, navigation, formatting, signature help, inlay hints, and target-aware pin information.
11. Production builds require neither Wine nor WinCUPL.
12. At least one common external programmer successfully programs and verifies emitted files.
13. No writable target fuse remains unclassified or unexplained.
14. Hardware validation records exact device variants, programmer versions, JEDEC hashes, and test outcomes.

---

## Closing architecture statement

deCPLD separates four concerns:

```text
Language semantics
    define logical behavior

Target-independent RTL and Boolean IR
    preserve and optimize behavior

Typed Rust targets and fitters
    map behavior to product terms, macrocells, clocks, feedback, and pins

JEDEC encoding
    serialize the physical configuration for a programmer
```

The first implementation should be conservative and transparent: support a compact language completely, verify every transformation, and make the devices' real architecture visible in reports. The resulting tool should not merely be a more pleasant CUPL front end; it should be a trustworthy, inspectable compiler for the ATF22V10 and ATF16V8.