# ATF22V10 fuse map — measured evidence

Every claim here was produced by a differential experiment, not by reading a pattern off a table. The sources are in [`targets/experiments/atf22v10/`](../experiments/atf22v10/) and the command is [`run.sh`](../experiments/atf22v10/run.sh); anyone with WinCUPL can reproduce any line below.

**WinCUPL's output is not committed anywhere in this repository.** The `.pld` inputs are ours and the command line is in the runner, which is all that is needed to re-derive a result. What ships is the device model, with each constant citing the experiment that established it. CI never runs the oracle — deCPLD's own invariants (encode/decode round-trip, region partition, equivalence) are what CI checks, and compiling must never require WinCUPL, Wine, or Windows.

## Oracle identification

| | |
|---|---|
| WinCUPL | II 1.1.0 (2026-02-28) |
| `cupl.exe` sha256 | `7b503d85dc76502e0c339d8c3472775d87949409d5d1a0d5b3302f4e5ea3d544` |
| `cupl.dl` sha256 | `9dd8197f01a07adf053ce237e4cfae95e22b705ac441e2bec5bbf88f6b2a47a7` |
| wine | 11.0 |
| device type | `g22v10` |
| command | `cupl.exe -jaxfl g22v10 <experiment>` |

`Atmel.dl` is byte-identical to `cupl.dl` in this installation.

WinCUPL's `g22v10` emits `QF5892` — GAL mode in the datasheet's Table 10-1, confirming that table against a second source. deCPLD's independently written fuse checksum agreed with WinCUPL's declared `C` field on every file produced (`C0C68`, `C1738`, `C0C77`, …). Over 5892 fuses that is agreement on fuse *placement*, not merely on arithmetic.

## Method

Each experiment drives one output from one source and changes exactly one thing. In an AND array a `0` is an intact link, so a product term implementing a single literal leaves exactly **one** column intact — and that column identifies the source. `F0` makes every unmentioned fuse intact, so an unused row reads as all-zero and contributes nothing.

[The complete fuse address map](#the-complete-fuse-address-map) at the end of this document collects every result below into one table, read in fuse order. Each section here is the evidence for one part of it.

## Array geometry

132 rows × 44 columns = 5808 array fuses. 44 columns is 22 signal sources × 2.

## Fuse addressing: the array is row-major, stride 44

Cell (*row*, *column*) is fuse **44·*row* + *column***, so row *r* occupies the contiguous run 44*r* … 44*r*+43.

This relation is what every other measurement in this document is expressed in, and until it was measured it was the one thing here that had never been checked. Each experiment was read by dividing a fuse address by 44 and calling the results "row" and "column" — which makes the row/column view a *consequence* of the formula and worthless as evidence for it. Stated plainly: a column-major array would have been described by exactly the same words, with every claim below transposed.

So it was measured again, in absolute fuse addresses with nothing divided by anything. Eight designs already in the suite, read a second way:

| experiment | product terms in the source | blown runs (absolute fuse addresses) | intact | written extent |
|---|---|---|---|---|
| `in1` | 2 (OE, one data) | 44–87, 89–131 | 88 | 44–131 |
| `in2` | 2 (OE, one data) | 44–91, 93–131 | 92 | 44–131 |
| `in3` | 2 (OE, one data) | 44–95, 97–131 | 96 | 44–131 |
| `in4` | 2 (OE, one data) | 44–99, 101–131 | 100 | 44–131 |
| `nc13` | 2 (OE, one data) | 44–130 | 131 | 44–131 |
| `mc19` | 2 (OE, one data) | 2156–2203, 2205–2243 | 2204 | 2156–2243 |
| `mc14` | 2 (OE, one data) | 5368–5415, 5417–5455 | 5416 | 5368–5455 |
| `global-ar-sp` | 4 (OE, data, AR, SP) | 0–7, 9–91, 93–131, 5764–5775, 5777–5807 | 8, 92, 5776 | 0–131 and 5764–5807 |

The *runs* are not the unit. Adjacent product terms merge into one run, and an intact link splits a run in two — `global-ar-sp` writes four terms and produces three runs. The unit is the **written extent**: the region a design's terms occupy, closed over the intact links inside it.

Three things follow, each from a different feature of the data.

**A product term is 44 fuses.** Each of `in1`, `in2`, `in3`, `in4` and `nc13` contains exactly two product terms — an always-enabled output-enable term and one single-literal data term — and each writes the same extent, 44–131, which is 88 fuses. The one intact link must lie in the data term, since the output-enable term has no literals and is entirely blown. Across those five designs that link appears at 88, 92, 96, 100 and **131**. So one product term spans at least 88…131, which is 44 addresses; two terms in 88 fuses makes each exactly 44. `global-ar-sp` gives the same answer independently from a different direction — four terms in 176 written fuses — and `mc14` and `mc19` each two terms in 88.

Nothing here divides an address by 44. The inputs are absolute addresses and a count of product terms read off the source text.

**Terms are contiguous and 44-aligned.** Every extent begins on a multiple of 44 — 0, 44, 2156, 5368, 5764 — and is a whole number of 44s: 44, 88, 132. Together with the width, that is a row-major array of 44-wide rows. It does not by itself exclude an array whose rows are subdivided further, which is why the width above is measured rather than inferred from alignment.

**A pin's column is invariant across rows.** The same pin's literal, placed in different product terms, lands at addresses differing by an exact multiple of 44:

| pin | appears at | difference | = |
|---|---|---|---|
| 3 | 96 (`in3`, pin 23's data term) and 8 (`global-ar-sp`, the AR term) | 88 | 2 × 44 |
| 4 | 100 (`in4`) and 5776 (`global-ar-sp`, the SP term) | 5676 | 129 × 44 |
| 2 | 92 (`in2`), 2204 (`mc19`), 5416 (`mc14`) | 2112 and 5324 | 48 × 44 and 121 × 44 |

Three pins across six distinct product terms, spanning rows 0, 2, 50, 123 and 131 — both device-wide control rows and three macrocell blocks. Every difference is a multiple of 44, so a pin occupies the same offset within every term, and that offset is what this document calls its column.

Note what is *not* evidence here: fuse 92 is where `in2`'s own literal lands, and column 4 was read off that very address. "92 = 44·2 + 4" is an identity, not a corroboration. It is the anchor the other measurements are compared against, which is why the table above states differences rather than decompositions.

The extents also show that WinCUPL packs a single product term into the *first* data row of a block, immediately after that block's output-enable row.

Experiments: `in1`, `in2`, `in3`, `in4`, `nc13`, `mc14`, `mc19`, `global-ar-sp` — all already in the suite; this reads them a second way rather than adding designs.

Galette and GALasm both encode the array as row·44 + column, so this is `OpenSourceCrossChecked` as well as differentially verified. That agreement is a check, not the source: both are third-party implementations of the same device and could share an error, which is the situation the measurement above exists to resolve.

## Columns: true and complement

`o0 = i0` versus `o0 = !i0`, on four **input** pins (2, 3, 11, 13) and three **feedback** sources (pins 22, 18, 14). In every case the complement column is the true column **+ 1**, so source *s* occupies columns *(2s, 2s+1)* with the true sense on the even column.

Both kinds were measured deliberately. The relation was first established on inputs alone and generalised to all 22 sources, which is an assumption of uniform structure rather than a measurement — and getting it wrong on a feedback inverts a literal on hardware, where nothing downstream would notice.

Experiments: `in2`, `in3`, `in11`, `in13`, `nc2`, `nc3`, `nc11`, `nc13`, `nfb22`, `nfb18`, `nfb14`.

## Columns: signal sources

Input pins, from the `in*` sweep — output fixed on pin 23, input pin varied:

| pin | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| true column | 0 | 4 | 8 | 12 | 16 | 20 | 24 | 28 | 32 | 36 | 40 | 42 |
| source | 0 | 2 | 4 | 6 | 8 | 10 | 12 | 14 | 16 | 18 | 20 | 21 |

Macrocell feedback, from the `fb*` sweep — pin *n* drives pin 23, so pin 23's data term names pin *n*'s feedback column:

| pin | 23 | 22 | 21 | 20 | 19 | 18 | 17 | 16 | 15 | 14 |
|---|---|---|---|---|---|---|---|---|---|---|
| true column | 2 | 6 | 10 | 14 | 18 | 22 | 26 | 30 | 34 | 38 |
| source | 1 | 3 | 5 | 7 | 9 | 11 | 13 | 15 | 17 | 19 |

All 22 sources are accounted for: even sources 0–20 are input pins 1–11, odd sources 1–19 are feedback from pins 23 down to 14, and source 21 is pin 13.

**Note the exception.** All ten feedbacks sit at odd sources, so "odd means feedback" holds for every feedback — but it fails in the other direction, on one of the *eleven* odd sources: source 21 is input pin 13. Each of the ten feedbacks was measured rather than extrapolated from pin 22, which is the only reason the boundary is known.

## Rows: macrocell blocks

A design driving pin 23 uses rows 1–9; one driving pin 22 uses rows 10–20. Within a block the **first row is the output-enable term** and the rest are data terms. It appears all-blown — a no-literal product term, permanently enabled — in every design that does not write an `.oe`, which is what identified it; [Output enable](#output-enable) measures what the row holds when a design does write one.

This matches Galette's `OLMC_ROWS_22V10 = [122,111,98,83,66,49,34,21,10,1]` and `OLMC_SIZE_22V10 = [9,11,13,15,17,17,15,13,11,9]` under

```text
row-block index i  <->  pin 14 + i
```

measured at *i* = 9 (pin 23) and *i* = 8 (pin 22). Block sizes minus the OE row give 8–16 data terms, matching the datasheet's Figure 1-1 "8 TO 16 PRODUCT TERMS".

Experiments: `in2` (pin 23 alone), `fb22` (pins 22 and 23).

## Architecture bits S0 and S1

The S0/S1 block begins at fuse 5808, interleaved, S0 on the even fuse — cross-checked in Galette and GALasm and consistent with the datasheet's PAL-mode total of 5828.

Pair 0 is fuses 5808 (S0) and 5809 (S1). Four experiments varying mode and polarity one at a time:

| experiment | design | 5808 (S0) | 5809 (S1) |
|---|---|---|---|
| `arch-comb-high` | combinational, active high | 1 | 1 |
| `arch-comb-low` | combinational, active low | 0 | 1 |
| `arch-reg-high` | registered, active high | 1 | 0 |
| `arch-reg-low` | registered, active low | 0 | 0 |

**S0 selects polarity** — 1 is active high. **S1 selects mode** — 1 is combinational, 0 is registered.

## S0/S1 pair order is reversed relative to the row blocks

A design using only pin 23 sets pair 0 (5808, 5809). Adding pin 22 additionally sets pair 1 (5810, 5811). So

```text
S0/S1 pair index j  <->  pin 23 - j        (descending)
row-block index i   <->  pin 14 + i        (ascending)
therefore            j = 9 - i
```

The two orderings run in opposite directions. Cross-checking the open-source implementations surfaced this as a discrepancy and could not settle it; only the experiment could. It is the single most likely place for a mapping to be silently transposed, which is why it is stated explicitly here.

Measured for **every** macrocell, one design each:

| pin | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 |
|---|---|---|---|---|---|---|---|---|---|---|
| first row of block | 122 | 111 | 98 | 83 | 66 | 49 | 34 | 21 | 10 | 1 |
| S0 fuse | 5826 | 5824 | 5822 | 5820 | 5818 | 5816 | 5814 | 5812 | 5810 | 5808 |
| S0/S1 pair index | 9 | 8 | 7 | 6 | 5 | 4 | 3 | 2 | 1 | 0 |

Both relations hold at every point: *i* = pin − 14 and *j* = 23 − pin, hence *j* = 9 − *i*.

This began as five measurements with the other five interpolated, and the row table was adopted from Galette. The measured table turns out identical to Galette's `OLMC_ROWS_22V10` — but that is now a *cross-check* rather than the source. Interpolating between measured points is still not measuring, and a device whose two fuse orderings run in opposite directions is exactly the shape where an interpolation looks right at both ends and is wrong in the middle.

Block sizes follow from contiguity: block *i* ends where block *i−1* begins, and the topmost ends at row 131. They agree with Galette's `OLMC_SIZE_22V10`.

Experiments: `mc14` … `mc23`, one single-macrocell design each. Pins 22 and 23 are additionally corroborated by `fb22` and `arch-comb-high`, which read the same values out of two-output designs.

## Rows 0 and 131: the device-wide control terms

The ten macrocell blocks cover 130 of the 132 rows. Galette prints AR before its block listing and SP after it, which is suggestive — but print order is not a fuse map, and this was the last claim resting on inference rather than measurement.

Driving each from a distinct pin identifies the row by which column stays intact:

| row | intact column | that column belongs to | so the row is |
|---|---|---|---|
| 0 | 8 | pin 3 (`o0.ar = rst`) | asynchronous reset |
| 131 | 12 | pin 4 (`o0.sp = pre`) | synchronous preset |

Both are device-wide rather than per-macrocell, which is why they sit outside the ten blocks. The same design also reads S0 = 1, S1 = 0 on pin 23's pair — active high and registered — independently consistent with `o0.d`.

Experiment: `global-ar-sp`.

## Evidence level

This statement governs **every claim in this document**, including the sections below it and the summary at the end.

`DifferentiallyVerified` for everything except where noted: the fact comes from a controlled WinCUPL experiment. The region boundaries are additionally `OpenSourceCrossChecked` against Galette (`af52987`) and GALasm (`c376d56`) and confirmed by the datasheet.

`DatasheetSpecified` (SPEC.md §13.1) for the parts of [Pin roles](#pin-roles-the-dip-24-package) that only the datasheet can supply: each pin's name and function, which rail pin 12 and pin 24 are, and pin 4's power-down role. No fuse experiment can observe what a pin is bonded to. Everything in that section stated as *behaviour* — pin 1 serving both roles at once, an I/O pin reaching the array through its feedback column, the two rails being refused as signals — is `DifferentiallyVerified` in the usual way.

None of it is `HardwareVerified`. Nothing here has been programmed onto a physical part.

WinCUPL is one witness, not an authority. Where it is the *only* witness — the column map, S0/S1 semantics, the pair ordering, and pin 1 serving as clock and array input simultaneously — the claim stands on triangulation between the experiments themselves rather than on WinCUPL being correct.

The array's fuse addressing is WinCUPL-only in the same way, and is the one claim here with an independent implementation to check against: Galette and GALasm both encode row·44 + column. That makes it `OpenSourceCrossChecked` in addition to `DifferentiallyVerified`.

## Pin roles: the DIP-24 package

Pin roles are the one part of this document the datasheet is authoritative for. What a pin is *called* and what it is *bonded to* is a fact about the package, not something a compiler's output reveals — so `atf22v10c-datasheet` Table 2-1 "Pin Configurations" and Figure 2-2 "DIP/SOIC" are the source:

| pins | 1 | 2–3 | 4 | 5–11 | 12 | 13 | 14–23 | 24 |
|---|---|---|---|---|---|---|---|---|
| datasheet name | CLK/IN | IN | IN/PD | IN | GND | IN | I/O | VCC |

Table 2-1's legend defines the names: CLK is Clock, IN is Logic Inputs, I/O is Bi-directional Buffers, GND is Ground, VCC is +5V Supply, PD is Power-down.

What the datasheet cannot settle is how the compiler may *use* those pins, and that is what the model encodes. Three claims were measured.

### Pin 1 is a clock and an array input at the same time

`CLK/IN` could mean either role or both. Experiment `clk-shared` drives a registered output on pin 23 from pin 1: the design needs the clock, and the data term needs pin 1 as a literal. It compiles, leaving fuse 88 intact — column 0, pin 1's true column — with pin 23's architecture pair reading S0 = 1, S1 = 0, active high and registered.

So the roles are simultaneous, not alternative. A model that made them alternatives would reject that design with a resource error, and the error would be the compiler's, not the user's.

### Every I/O pin can be an input, through its own feedback column

Experiments `ioin14` … `ioin23`. Each drives one I/O pin from a *different* I/O pin that is never itself driven. The literal lands on the undriven pin's macrocell feedback column in every case:

| undriven pin | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 |
|---|---|---|---|---|---|---|---|---|---|---|
| intact fuse | 126 | 122 | 118 | 114 | 110 | 106 | 102 | 98 | 94 | 486 |
| written extent | 44–131 | 44–131 | 44–131 | 44–131 | 44–131 | 44–131 | 44–131 | 44–131 | 44–131 | 440–527 |
| column | 38 | 34 | 30 | 26 | 22 | 18 | 14 | 10 | 6 | 2 |

Addresses first, columns derived — the same discipline as [Fuse addressing](#fuse-addressing-the-array-is-row-major-stride-44), because a table of columns alone is not re-derivable from a fresh run. Nine of the ten drive pin 23 and so write its block, 44–131, with the literal in row 2. `ioin23` is the exception: pin 23 is the undriven one there, so it drives pin 22 instead and writes 440–527, pin 22's block, with the literal at 486 = 44·11 + 2.

The columns are identical to the `fb*` sweep's feedback columns. An I/O pin reaching the array as an input therefore uses that macrocell's feedback path rather than a separate input resource — which is why the model gives a pad no input resource of its own, and why "how many inputs does this device have" has no single answer.

All ten were measured rather than generalised from one. The feedback column map runs opposite to the pin numbering, so confirming pin 14 and pin 23 says nothing about the eight between them, and this document has already been wrong once in exactly that shape.

The undriven macrocell is left combinational and active low in every one of the ten (S1 set, S0 clear in its architecture pair), and its output-enable row is left entirely intact. Which of the two holds the pin off is settled by [Output enable](#output-enable) below, not by these experiments: `oe-never` asks for a permanently disabled output *without* changing the architecture bits and gets the same all-intact enable row, so the row is the mechanism and the architecture bits are incidental.

### Pins 12 and 24 are refused as signals

Experiments `pwr12` and `pwr24` ask WinCUPL to use a supply rail as a signal. It reports `invalid input` and produces no JEDEC, while the otherwise identical `in2` compiles.

That the datasheet calls them GND and VCC is what says *which rail each is* — no fuse experiment can see what a pin is bonded to. That the compiler refuses them is what makes "not usable by a design" a checked claim rather than a transcription of a diagram.

Both declare `EXPECT refusal` on a line of their own, which `run.sh` reads: it reports the refusal as a result and exits 0, so a batch re-run of the suite does not stop on them, and it fails loudly if the oracle ever *accepts* one — an accepted design would mean the claim is wrong, which is the outcome worth shouting about.

The marker sits inside a CUPL comment. `marker-inert` is `in2` carrying the same words and must still compile: `pwr12` and `pwr24` fail for their own reasons, so neither can show that the marker is inert rather than a syntax error being mistaken for the refusal under test.

## Output enable

SPEC.md §7.4's output-enable experiments. Four designs on pin 23, differing only in the enable expression — and, for the two needing a control signal, in declaring `PIN 3 = e` — with the data term `o0 = i0` (pin 2) held constant throughout. Pin 23's block is rows 1–9; the two rows in play here are row 1, the enable row at fuses 44–87, and row 2, its first data row at 88–131.

| experiment | `.oe` | array blown | enable row | pin 23's S0, S1 |
|---|---|---|---|---|
| `in2` | *not written* | 44–91, 93–131 | 44–87 entirely blown | 1, 1 |
| `oe-always` | `'b'1` | 44–91, 93–131 | 44–87 entirely blown | 1, 1 |
| `oe-var` | `e` (pin 3) | 44–51, 53–91, 93–131 | intact at 52 | 1, 1 |
| `oe-var-not` | `!e` | 44–52, 54–91, 93–131 | intact at 53 | 1, 1 |
| `oe-never` | `'b'0` | 88–91, 93–131 | 44–87 entirely **intact** | 1, 1 |

**The architecture column is the load-bearing one**, and it is measured rather than assumed. All four designs write the identical architecture region: 5808 and 5809 blown, 5810–5827 intact. Pin 23's pair reads 1, 1 — active high, combinational — in every one of them, including the design whose output is permanently off; the other nine macrocells read 0, 0 throughout.

Without that column the experiment would not do the job it is here for. `ioin14` … `ioin23` leave an undriven cell at S0 clear, S1 set. Had `oe-never` done the same it would be configuration-identical to those designs, two variables would have moved together, and it could not resolve a confound it shared. It does not: `oe-never` differs from `in2` in the enable row and in nothing else, across all 5892 fuses.

Four findings, in increasing order of consequence.

**CUPL's default enable is "always", and it is the empty product term.** `oe-always` is bit-identical to `in2` — zero fuse deltas across all 5892 — so writing `o0.oe = 'b'1` and writing nothing produce the same part. The encoding is the enable row with every link blown, which is a product term with no literals: the empty AND, constantly true.

**The enable row is an ordinary product-term row.** `oe-var` moves exactly one fuse from `in2`, to 52 = 44·1 + 8, and column 8 is pin 3's true column in [Columns: signal sources](#columns-signal-sources). Same column map, same addressing, no special encoding.

**Complement is true + 1 there too.** `oe-var-not` moves the intact link to 53, column 9. The pair is what establishes the sense: one design alone leaves a single intact link consistent with either polarity until a second design moves it.

**A permanently disabled pad is the enable row entirely intact.** `oe-never` leaves all 44 links of row 1 connected — every literal at both polarities, which no input can satisfy — while row 2 keeps its data term and the architecture bits keep the values `in2` gives them. One variable moved. Note which row moved, too: a compiler expressing "off" by emptying the data term would have moved row 2 instead.

The two states are therefore opposites at the same 44 fuses, and getting them the wrong way round turns every undriven pin into a permanently driven one.

**What this establishes, stated precisely.** It is a measurement of the *encoding*: asked for an output that is never enabled, the oracle writes an all-intact enable row and changes nothing else. That the product term is what physically gates the pad is the reading of that encoding, not a separate measurement — the [Evidence level](#evidence-level) caveat governs it like everything else here, nothing in this document is `HardwareVerified`, and WinCUPL is one witness rather than an authority.

One residual dependency is worth naming. Treating a permanently disabled output as *the same silicon state* as an input-only pin also requires feedback to be taken at the **pad** rather than at an internal node. Were it internal, an input-only cell would read back its own never-true data row instead of the external pin, and the two would not be the same thing at all. `oe-bidir` below shows WinCUPL routing a gated pin's readback through the feedback column, which is consistent with a pad tap without proving one, and the `fb*` and `ioin*` sweeps landing on one column is the same kind of evidence.

Note in passing that `in2` and `oe-always` carry different `Name` fields and are still bit-identical, so `Name` reaches no fuse — unlike `PartNo`, which [the user signature](#the-user-signature-carries-cupls-partno) records as landing in the signature region.

### Bidirectional readback uses the feedback column

Experiment `oe-bidir`: pin 23 is driven when `e` is high and read into pin 22 the rest of the time — one pin as output and input at once, which the `ioin*` designs could not reach because nothing drove those pins.

| row | extent | intact | column | meaning |
|---|---|---|---|---|
| 1 | 44–87 | 52 | 8 | pin 23's enable = pin 3 |
| 2 | 88–131 | 92 | 4 | pin 23's data = pin 2 |
| 10 | 440–483 | none | — | pin 22's enable, always |
| 11 | 484–527 | 486 | 2 | pin 22's data = pin 23 |

Column 2 is both what the `fb*` sweep recorded for pin 23's **feedback** and what `ioin23` recorded for pin 23 as an undriven **input**. Measured separately, they are one path used in both directions — which is why a pad has no input resource of its own in the model.

Pin 22's architecture pair reads S0 = 1, S1 = 1 (fuses 5810 and 5811 blown), combinational and active high, as expected for `o1 = io0`.

These runs also extend [Columns: signal sources](#columns-signal-sources) in a direction it had not reached. `oe-var` places column 8 in **row 1** and `oe-var-not` places **column 9** there — the first measurement of any column in an output-enable row, and the first of a *complement* column outside pin 23's first data row.

Experiments: `oe-always`, `oe-var`, `oe-var-not`, `oe-never`, `oe-bidir`. All five compile; none needs an `EXPECT refusal` marker.

## The three JEDEC footprints, and the power-down fuse

Compiling one design under three device types confirms the datasheet's Table 10-1 by experiment:

| device type | `QF` | tail beyond the array |
|---|---|---|
| `p22v10` | 5828 | S0/S1 only — no signature |
| `g22v10` | 5892 | S0/S1 + 64-bit signature |
| `g22v10cp` | 5893 | + one further fuse |

PAL mode truncating at exactly 5828, with no signature bits set, confirms the signature region boundary from a third direction — independently of Galette, GALasm, and the datasheet.

The power-down output is identical to the GAL output for the same design **plus fuse 5892 set**. So **fuse 5892 is the power-down enable**, and it is `1` when the feature is enabled. Measured rather than assumed from `5893 = 5892 + 1`.

Experiments: `mode-pal`, `mode-powerdown`, `arch-comb-high`.

## The user signature carries CUPL's PartNo

Fuses 5830, 5831, 5838 and 5839 were set identically in every early experiment, which looked like a fixed CUPL default. It was not: **every one of those experiments held `PartNo 00`.** A variable held constant cannot support a conclusion that it has no effect, and treating it as one was a methodology error in a document whose whole argument is differential rigour.

Varying it:

| `PartNo` | set offsets from 5828 |
|---|---|
| `00` | 2, 3 · 10, 11 |
| `41` | 2, 3, 5 · 10, 11, 15 |
| `5A` | 2, 3, 5, 7 · 9, 15 |

Decoded as ASCII, one byte per eight fuses, **most significant bit first**, starting at fuse 5828:

| character | hex | bits set (MSB-first) | matches |
|---|---|---|---|
| `'0'` | 0x30 | 2, 3 | ✓ |
| `'4'` | 0x34 | 2, 3, 5 | ✓ |
| `'5'` | 0x35 | 2, 3, 5, 7 | ✓ |
| `'1'` | 0x31 | 2, 3, 7 | ✓ |
| `'A'` | 0x41 | 1, 7 | ✓ |

All six bytes match exactly. So the 64-bit signature region holds the `PartNo` field as ASCII text, and deCPLD must treat it as user data it writes deliberately rather than as an opaque constant to copy.

Experiments: `arch-comb-high` (PartNo 00), `sig-partno-41`, `sig-partno-5A`.

## Not established

- **Which fuse value means "connected"** rests on `jedec-3a` lines 344-348 — "a zero, specifying a low resistance link … or a one, specifying a high resistance link" — rather than on hardware. `DatasheetSpecified`, one witness.

  Every experiment above is *consistent* with it: a single-literal design leaves exactly one `0`, at the column measured for the pin driving it. That is not independent corroboration. The reader that decodes these files and the encoder that will write them share the convention, so a world in which both are inverted produces exactly the same observations. Only programming a part and measuring its behaviour distinguishes the two, and nothing here is `HardwareVerified`.

  It is the single most consequential bit in the project: inverting it computes the complement of every design behind a perfectly valid checksum.

## The complete fuse address map

Every result above, in fuse order. **These tables are a summary and carry no evidence of their own.** Every claim in them is established by a section above, and the evidence-level statement there governs them too — nothing below this line is more certain than what it summarises, despite reading as flat fact.

### Top level

| fuses | count | region | footprints | evidence |
|---|---|---|---|---|
| 0 – 5807 | 5808 | AND array, 132 rows × 44 columns | all | Array geometry; Fuse addressing |
| 5808 – 5827 | 20 | architecture S0/S1, ten interleaved pairs | all | Architecture bits S0 and S1 |
| 5828 – 5891 | 64 | user signature, eight ASCII bytes MSB-first | GAL, power-down | The user signature carries CUPL's PartNo |
| 5892 | 1 | power-down enable | power-down only | The three JEDEC footprints |

`QF` is 5828 (`p22v10`), 5892 (`g22v10`), or 5893 (`g22v10cp`).

The security fuse is not in this space. Every footprint's count is fully accounted for without it — 5808 + 20 (+ 64) (+ 1) — and in all the output examined it appears as the JEDEC `G` field rather than a numbered fuse. The datasheet §6 says the device has one; no experiment here locates it at a fuse index, and the model therefore declines to invent one.

### The AND array, by row

Row *r* occupies fuses 44*r* … 44*r*+43. The first row of each macrocell block is that macrocell's output-enable term; the rest are data terms.

| rows | fuses | belongs to | OE row | data terms |
|---|---|---|---|---|
| 0 | 0 – 43 | asynchronous reset, device-wide | — | 1 |
| 1 – 9 | 44 – 439 | pin 23 | 1 | 8 |
| 10 – 20 | 440 – 923 | pin 22 | 10 | 10 |
| 21 – 33 | 924 – 1495 | pin 21 | 21 | 12 |
| 34 – 48 | 1496 – 2155 | pin 20 | 34 | 14 |
| 49 – 65 | 2156 – 2903 | pin 19 | 49 | 16 |
| 66 – 82 | 2904 – 3651 | pin 18 | 66 | 16 |
| 83 – 97 | 3652 – 4311 | pin 17 | 83 | 14 |
| 98 – 110 | 4312 – 4883 | pin 16 | 98 | 12 |
| 111 – 121 | 4884 – 5367 | pin 15 | 111 | 10 |
| 122 – 130 | 5368 – 5763 | pin 14 | 122 | 8 |
| 131 | 5764 – 5807 | synchronous preset, device-wide | — | 1 |

The blocks run **pin-descending** through the array: pin 23 nearest fuse 0.

### Columns

Offset within the row. Even columns carry the true sense, odd the complement.

| col | source | col | source | col | source | col | source |
|---|---|---|---|---|---|---|---|
| 0/1 | pin 1 | 12/13 | pin 4 | 24/25 | pin 7 | 36/37 | pin 10 |
| 2/3 | feedback pin 23 | 14/15 | feedback pin 20 | 26/27 | feedback pin 17 | 38/39 | feedback pin 14 |
| 4/5 | pin 2 | 16/17 | pin 5 | 28/29 | pin 8 | 40/41 | pin 11 |
| 6/7 | feedback pin 22 | 18/19 | feedback pin 19 | 30/31 | feedback pin 16 | 42/43 | **pin 13** |
| 8/9 | pin 3 | 20/21 | pin 6 | 32/33 | pin 9 | | |
| 10/11 | feedback pin 21 | 22/23 | feedback pin 18 | 34/35 | feedback pin 15 | | |

Columns 42/43 are the exception the column map turns on: an odd-numbered source that is an input pin rather than feedback.

**The column map is measured on one row and generalised.** Every one of the 22 sources was placed by a design driving pin 23, so every column position comes from that macrocell's first data row. What has been measured in *other* rows is three columns — 4, 8 and 12, in rows 0, 50, 123 and 131 — which is what the invariance argument under [Fuse addressing](#fuse-addressing-the-array-is-row-major-stride-44) rests on. Uniformity across all 132 rows is the natural reading of an AND array and is consistent with everything observed, but it is an inference from four rows, not a measurement of 132.

This table gives each pin's **array columns** and says nothing about its **package role**. Which pin is the clock, which is a power rail, and which carries the power-down input are separate claims needing separate evidence — see [Pin roles: the DIP-24 package](#pin-roles-the-dip-24-package).

### Architecture pairs

| pin | 23 | 22 | 21 | 20 | 19 | 18 | 17 | 16 | 15 | 14 |
|---|---|---|---|---|---|---|---|---|---|---|
| S0 — polarity, 1 is active high | 5808 | 5810 | 5812 | 5814 | 5816 | 5818 | 5820 | 5822 | 5824 | 5826 |
| S1 — mode, 1 is combinational | 5809 | 5811 | 5813 | 5815 | 5817 | 5819 | 5821 | 5823 | 5825 | 5827 |

Pin-descending in *fuse address* order, the same direction as the row blocks: pin 23 takes both the lowest row and the lowest architecture fuse.

This is the same fact the section [S0/S1 pair order is reversed relative to the row blocks](#s0s1-pair-order-is-reversed-relative-to-the-row-blocks) states as a reversal, read the other way round, and the two are worth reconciling explicitly because this is the document's most transposable claim. The *addresses* run the same way. The *indices* run opposite: the macrocell index ascends with the pin (*i* = pin − 14) while the pair index descends (*j* = 23 − pin), so *j* = 9 − *i*. Whether the orderings agree depends entirely on which of the two you name, which is why the model indexes macrocells by pin and derives both.

### User signature

Byte *b* is fuses 5828+8*b* … 5835+8*b*, most significant bit first. CUPL writes its `PartNo` field here as ASCII.

| byte | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 |
|---|---|---|---|---|---|---|---|---|
| fuses | 5828–5835 | 5836–5843 | 5844–5851 | 5852–5859 | 5860–5867 | 5868–5875 | 5876–5883 | 5884–5891 |
