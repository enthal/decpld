//! The output-enable row, measured rather than inferred.
//!
//! SPEC.md §7.4's output-enable experiments. Until these ran, the
//! ATF22V10 model advertised `supports_input_only` on the strength of an
//! *inference*: `ioin14`…`ioin23` each left an undriven pin's
//! output-enable row entirely intact, and the enable row was the only
//! remaining control in the fuse map. That is a plausible reading of a
//! side effect, not a measurement of the mechanism.
//!
//! `oe-always`, `oe-var`, `oe-var-not`, and `oe-never` ask the question
//! directly — four designs differing only in the enable expression —
//! and are recorded in `targets/evidence/atf22v10-fuse-map.md`,
//! "Output enable".

use decpld_atf22v10::*;
use decpld_device::{
    FuseId, FuseMap, MacrocellId, MacrocellMode, OutputPolarity, PhysicalDesign, PinNumber,
    PlacedCube, ProductTermId,
};
use decpld_logic::{Cube, Literal, Polarity};

const G: Atf22v10Geometry = Atf22v10Geometry;

fn literal(pin: u8, polarity: Polarity) -> Literal {
    let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
    Literal::new(bool_input_of_source(source.index), polarity)
}

fn blown_runs(fuses: &FuseMap, end: u32) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for fuse in 0..end {
        if fuses.get(FuseId(fuse)) != Some(true) {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.1 + 1 == fuse => last.1 = fuse,
            _ => runs.push((fuse, fuse)),
        }
    }
    runs
}

/// Pin 2 drives pin 23 combinationally, with `enable` on its OE row.
fn design_with_enable(enable: Option<Cube>) -> PhysicalDesign {
    let mut design = blank_design().expect("a blank design");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present");
    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term =
        enable.map(|cube| PlacedCube { row: ProductTermId(block.output_enable_row), cube });
    cell.data_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start),
        cube: Cube::new([literal(2, Polarity::True)]),
    }];
    design
}

#[test]
fn an_always_enabled_pad_is_the_enable_row_entirely_blown() {
    // Experiment `oe-always`: `o0.oe = 'b'1` written out explicitly.
    //
    // It is bit-identical to `in2`, which says nothing about the enable
    // at all — zero fuse deltas across all 5892. So CUPL's default IS
    // "always enabled", and the encoding is the enable row with every
    // link blown: a product term with no literals, the empty AND,
    // constantly true.
    let fuses = encode_design(&design_with_enable(Some(Cube::always())), Footprint::Gal)
        .expect("encodable");
    assert_eq!(blown_runs(&fuses, 5808), [(44, 91), (93, 131)]);
}

#[test]
fn a_gated_pad_carries_its_enable_literal_in_the_enable_row() {
    // Experiment `oe-var`: `o0.oe = e` with `e` on pin 3.
    //
    // Exactly one fuse moves from `in2`: 52, which is row 1 column 8 —
    // pin 3's TRUE column, the same column any data term would use. The
    // enable row is an ordinary product-term row over the same column
    // map, not a special encoding.
    let enable = Cube::new([literal(3, Polarity::True)]);
    let fuses =
        encode_design(&design_with_enable(Some(enable)), Footprint::Gal).expect("encodable");
    assert_eq!(fuses.get(FuseId(52)), Some(false), "pin 3's true link in the enable row");
    assert_eq!(blown_runs(&fuses, 5808), [(44, 51), (53, 91), (93, 131)]);
}

#[test]
fn a_complemented_enable_uses_the_column_one_higher() {
    // Experiment `oe-var-not`: `o0.oe = !e`.
    //
    // The pair is what distinguishes the two senses. `oe-var` alone
    // leaves one intact link consistent with either, until a second
    // design moves it — here to 53, pin 3's complement column.
    let enable = Cube::new([literal(3, Polarity::Complement)]);
    let fuses =
        encode_design(&design_with_enable(Some(enable)), Footprint::Gal).expect("encodable");
    assert_eq!(fuses.get(FuseId(53)), Some(false), "pin 3's complement link");
    assert_eq!(blown_runs(&fuses, 5808), [(44, 52), (54, 91), (93, 131)]);
}

#[test]
fn a_permanently_disabled_pad_is_the_enable_row_entirely_intact() {
    // Experiment `oe-never`: `o0.oe = 'b'0`.
    //
    // THE measurement. WinCUPL leaves all 44 links of the enable row
    // connected — every literal at both polarities, which no input can
    // satisfy — while the data row keeps its term. That is precisely
    // what `disable_row` writes, so deCPLD's encoding of an undriven pad
    // is now the oracle's encoding rather than a reading of one.
    //
    // Note which rows moved: row 1 (fuses 44–87) went from entirely
    // blown to entirely intact, and row 2 (88–131) is untouched. A
    // compiler that expressed "off" by clearing the data term instead
    // would have moved the other row.
    let fuses = encode_design(&design_with_enable(None), Footprint::Gal).expect("encodable");
    for fuse in 44..88 {
        assert_eq!(fuses.get(FuseId(fuse)), Some(false), "enable-row fuse {fuse} must be intact");
    }
    assert_eq!(blown_runs(&fuses, 5808), [(88, 91), (93, 131)]);
}

#[test]
fn the_two_empty_enable_states_are_opposites_at_the_same_44_fuses() {
    // Stated as one assertion, because the pair is the point and a
    // reader consulting either test alone could take the wrong one for
    // the general rule.
    let always =
        encode_design(&design_with_enable(Some(Cube::always())), Footprint::Gal).expect("ok");
    let never = encode_design(&design_with_enable(None), Footprint::Gal).expect("ok");
    for fuse in 44..88 {
        assert_eq!(always.get(FuseId(fuse)), Some(true), "always: {fuse}");
        assert_eq!(never.get(FuseId(fuse)), Some(false), "never: {fuse}");
    }
    // And nothing outside the enable row distinguishes them, across the
    // WHOLE map rather than the array alone. This is the property the
    // oracle measured — `oe-never` differs from `in2` in the enable row
    // and in nothing else — and stopping at 5808 would exclude the
    // architecture region, which is exactly where a plausible wrong
    // encoder would put its difference: "a pad nobody drives is active
    // low, the way WinCUPL leaves an undriven cell" passes every other
    // test in this file.
    let footprint_end = Footprint::Gal.fuse_count();
    for fuse in (0..44).chain(88..footprint_end) {
        assert_eq!(always.get(FuseId(fuse)), never.get(FuseId(fuse)), "fuse {fuse}");
    }
}

#[test]
fn a_disabled_pad_keeps_the_architecture_bits_its_enabled_twin_has() {
    // The measurement the whole "the enable row is the mechanism"
    // reading rests on, asserted at absolute addresses.
    //
    // In all four `oe-*` runs the architecture region is identical:
    // 5808 and 5809 blown, 5810–5827 intact — pin 23 active high and
    // combinational even when its output is permanently off. Had
    // WinCUPL instead written S0 clear, S1 set (what `ioin14` … `ioin23`
    // leave an undriven cell at), `oe-never` would be
    // configuration-identical to those designs, two variables would
    // have moved together, and it could not resolve a confound it
    // shared with them.
    for enable in [Some(Cube::always()), Some(Cube::new([literal(3, Polarity::True)])), None] {
        let fuses = encode_design(&design_with_enable(enable), Footprint::Gal).expect("ok");
        assert_eq!(fuses.get(FuseId(5808)), Some(true), "S0: pin 23 active high");
        assert_eq!(fuses.get(FuseId(5809)), Some(true), "S1: pin 23 combinational");
    }

    // Those three assertions cannot by themselves say that 5808/5809 is
    // *pin 23's* pair: every macrocell in these designs is
    // combinational and active high, so all twenty architecture fuses
    // read 1 and the pair order is unobservable. `arch-comb-low`
    // locates it — the discriminating corner, checked in
    // `tests/macrocells.rs` — and this makes the same distinction here,
    // so a reader of this file is not left with a claim its own
    // assertions do not support.
    //
    // Pin 23 active low with the enable row off: S0 must clear at 5808
    // and nowhere else, and S1 must stay set. Under pin-ASCENDING pair
    // order the change would land at 5826 instead.
    let mut design = design_with_enable(None);
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present")
        .polarity = OutputPolarity::ActiveLow;

    let fuses = encode_design(&design, Footprint::Gal).expect("ok");
    assert_eq!(fuses.get(FuseId(5808)), Some(false), "pin 23's S0 is the FIRST pair, not the last");
    assert_eq!(fuses.get(FuseId(5809)), Some(true), "and its S1 is unchanged");
    for fuse in 5810..5828 {
        assert_eq!(fuses.get(FuseId(fuse)), Some(true), "no other pair moved: {fuse}");
    }
}

#[test]
fn a_bidirectional_pin_reads_back_through_its_feedback_column() {
    // Experiment `oe-bidir`: pin 23 is driven when `e` is high and read
    // into pin 22 the rest of the time — the same pin as output and
    // input at once, which `ioin*` could not reach because nothing drove
    // those pins.
    //
    // Pin 22's data term lands at 486 = 44·11 + 2, column 2. That is
    // both the column the `fb*` sweep recorded for pin 23's FEEDBACK and
    // the column `ioin23` recorded for pin 23 as an undriven INPUT. The
    // two paths measured separately are one path, used in both
    // directions.
    let mut design = design_with_enable(Some(Cube::new([literal(3, Polarity::True)])));
    let driven = G.macrocell_of_pin(PinNumber(22)).expect("an I/O pin");
    let block = G.row_block(driven).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(driven.0))
        .expect("present");
    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start),
        cube: Cube::new([literal(23, Polarity::True)]),
    }];

    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    assert_eq!(fuses.get(FuseId(486)), Some(false), "pin 23's column in pin 22's first data row");
    assert_eq!(blown_runs(&fuses, 5808), [(44, 51), (53, 91), (93, 131), (440, 485), (487, 527)]);
}
