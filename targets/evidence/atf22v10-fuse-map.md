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

## Array geometry

132 rows × 44 columns = 5808 array fuses. 44 columns is 22 signal sources × 2.

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

A design driving pin 23 uses rows 1–9; one driving pin 22 uses rows 10–20. Within a block the **first row is the output-enable term** (it appears as all-blown, a no-literal product term, i.e. permanently enabled) and the rest are data terms.

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

Experiments: `mc14` … `mc23` (ten designs), plus `arch-comb-high` and `fb22`.

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

`DifferentiallyVerified` for everything above except where noted: each fact comes from a controlled WinCUPL experiment, and the region boundaries are additionally `OpenSourceCrossChecked` against Galette (`af52987`) and GALasm (`c376d56`) and confirmed by the datasheet. None of it is `HardwareVerified` — nothing here has been programmed onto a physical part.

WinCUPL is one witness, not an authority. Where it is the *only* witness — the column map, S0/S1 semantics, the pair ordering — the claim stands on triangulation between the experiments themselves rather than on WinCUPL being correct.

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
- **Which fuse value means "connected"** is taken from JEDEC 3A's definition (0 is a low-resistance link) rather than measured on hardware.
