//! The ATF22V10C's ten macrocells, checked against the architecture-bit
//! experiments.
//!
//! Evidence for the encoding is four designs varying one thing at a
//! time — `arch-comb-high`, `arch-comb-low`, `arch-reg-high`,
//! `arch-reg-low` — recorded in `targets/evidence/atf22v10-fuse-map.md`.

use decpld_atf22v10::*;
use decpld_device::{
    FuseId, FuseMap, MacrocellId, MacrocellMode, OutputPolarity, PadId, PinNumber,
};

const G: Atf22v10Geometry = Atf22v10Geometry;

fn erased_gal() -> FuseMap {
    FuseMap::erased(regions_for(Footprint::Gal).expect("a valid layout"))
}

#[test]
fn there_are_ten_macrocells_each_owning_its_pad_and_its_rows() {
    let cells = macrocells().expect("the measured geometry describes valid macrocells");
    assert_eq!(cells.len(), 10);

    for (index, cell) in cells.iter().enumerate() {
        let index = u8::try_from(index).expect("ten fits");
        assert_eq!(cell.id, MacrocellId(index));
        assert_eq!(cell.pad, Some(PadId(index)));

        // The rows this cell owns are exactly its measured block.
        let block = G.row_block(MacrocellIndex(index)).expect("a measured block");
        assert_eq!(cell.oe_term, Some(product_term_of_row(block.output_enable_row)));
        let expected: Vec<_> = block.data_rows.clone().map(product_term_of_row).collect();
        assert_eq!(cell.data_terms, expected, "macrocell {index}");
    }

    // Datasheet Figure 1-1: "8 TO 16 PRODUCT TERMS", and the measured
    // blocks give the distribution.
    let capacities: Vec<usize> = cells.iter().map(|c| c.data_term_capacity()).collect();
    assert_eq!(capacities, [8, 10, 12, 14, 16, 16, 14, 12, 10, 8]);
}

#[test]
fn no_two_macrocells_share_a_product_term() {
    // A row owned twice would let one output's logic silently appear in
    // another's sum.
    let cells = macrocells().expect("valid");
    let mut seen = std::collections::BTreeSet::new();
    for cell in &cells {
        for term in cell.data_terms.iter().chain(cell.oe_term.iter()) {
            assert!(seen.insert(*term), "{term:?} belongs to two macrocells");
        }
    }
    // Ten blocks cover 130 of the 132 rows; the other two are the
    // device-wide reset and preset terms.
    assert_eq!(seen.len(), 130);
}

#[test]
fn polarity_and_mode_encode_as_the_experiments_measured() {
    // | experiment       | design                      | S0 | S1 |
    // |------------------|-----------------------------|----|----|
    // | arch-comb-high   | combinational, active high  | 1  | 1  |
    // | arch-comb-low    | combinational, active low   | 0  | 1  |
    // | arch-reg-high    | registered, active high     | 1  | 0  |
    // | arch-reg-low     | registered, active low      | 0  | 0  |
    //
    // So S0 is polarity (1 = active high) and S1 is mode
    // (1 = combinational).
    //
    // Scope: those four designs all drive pin 23, so the SEMANTICS were
    // measured on pin 23's pair and generalised to the other nine.
    // `mc14` … `mc23` measure each pair's ADDRESS, not its meaning —
    // every one of them is `o0 = i0`, a single corner of the 2x2. The
    // generalisation is very likely right and it is still a
    // generalisation, in a file that has already been burned once by
    // interpolating between measured macrocells.
    let cells = macrocells().expect("valid");
    for cell in &cells {
        let macrocell = MacrocellIndex(cell.id.0);
        let pair = G.architecture_pair(macrocell).expect("a measured pair");
        let polarity = cell.polarity_field.as_ref().expect("every cell has a polarity field");
        let mode = cell.mode_field.as_ref().expect("every cell has a mode field");

        assert_eq!(polarity.bits(), [pair.polarity], "macrocell {}", cell.id.0);
        assert_eq!(mode.bits(), [pair.mode], "macrocell {}", cell.id.0);

        for (want, s0) in [(OutputPolarity::ActiveHigh, true), (OutputPolarity::ActiveLow, false)] {
            let mut map = erased_gal();
            polarity.encode(&mut map, want).expect("encodable");
            assert_eq!(map.get(pair.polarity), Some(s0), "macrocell {} {want:?}", cell.id.0);
            assert_eq!(polarity.decode(&map), Ok(want));
        }

        for (want, s1) in [(MacrocellMode::Combinational, true), (MacrocellMode::Registered, false)]
        {
            let mut map = erased_gal();
            mode.encode(&mut map, want).expect("encodable");
            assert_eq!(map.get(pair.mode), Some(s1), "macrocell {} {want:?}", cell.id.0);
            assert_eq!(mode.decode(&map), Ok(want));
        }
    }
}

#[test]
fn arch_comb_high_on_pin_23_writes_the_fuses_the_oracle_wrote() {
    // The oracle's `arch-comb-high` sets exactly 5808 and 5809 in the
    // architecture region — pin 23's pair — for a combinational,
    // active-high output. Encoding both fields must reproduce that.
    let cells = macrocells().expect("valid");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let cell = &cells[usize::from(macrocell.0)];
    let mut map = erased_gal();

    cell.polarity_field
        .as_ref()
        .expect("polarity")
        .encode(&mut map, OutputPolarity::ActiveHigh)
        .expect("encodable");
    cell.mode_field
        .as_ref()
        .expect("mode")
        .encode(&mut map, MacrocellMode::Combinational)
        .expect("encodable");

    assert_eq!(map.get(FuseId(5808)), Some(true), "S0: active high");
    assert_eq!(map.get(FuseId(5809)), Some(true), "S1: combinational");
    // Nothing else in the architecture region was touched.
    for fuse in 5810..5828 {
        assert!(!map.is_written(FuseId(fuse)), "fuse {fuse}");
    }
}

#[test]
fn arch_comb_low_distinguishes_polarity_from_mode() {
    // The discriminating corner. `arch-comb-high` (1,1) and
    // `arch-reg-low` (0,0) are both symmetric in S0 and S1, so neither
    // would notice the two fields being swapped. `arch-comb-low` sets
    // S1 and clears S0, which pins polarity to 5808 and mode to 5809 at
    // absolute addresses.
    let cells = macrocells().expect("valid");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let cell = &cells[usize::from(macrocell.0)];
    let mut map = erased_gal();

    cell.polarity_field
        .as_ref()
        .expect("polarity")
        .encode(&mut map, OutputPolarity::ActiveLow)
        .expect("encodable");
    cell.mode_field
        .as_ref()
        .expect("mode")
        .encode(&mut map, MacrocellMode::Combinational)
        .expect("encodable");

    assert_eq!(map.get(FuseId(5808)), Some(false), "S0: active low");
    assert_eq!(map.get(FuseId(5809)), Some(true), "S1: combinational");
}

#[test]
fn arch_reg_low_on_pin_23_writes_the_fuses_the_oracle_wrote() {
    // The opposite corner: registered and active low clears both.
    let cells = macrocells().expect("valid");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let cell = &cells[usize::from(macrocell.0)];
    let mut map = erased_gal();

    cell.polarity_field
        .as_ref()
        .expect("polarity")
        .encode(&mut map, OutputPolarity::ActiveLow)
        .expect("encodable");
    cell.mode_field
        .as_ref()
        .expect("mode")
        .encode(&mut map, MacrocellMode::Registered)
        .expect("encodable");

    assert_eq!(map.get(FuseId(5808)), Some(false), "S0: active low");
    assert_eq!(map.get(FuseId(5809)), Some(false), "S1: registered");
}

#[test]
fn every_legal_configuration_round_trips_for_every_macrocell() {
    // SPEC.md §4.7: "every legal configuration round-trips through
    // encode/decode". Exhaustive over the whole space this device has —
    // ten macrocells times two modes times two polarities.
    let cells = macrocells().expect("valid");
    for cell in &cells {
        for mode in [MacrocellMode::Combinational, MacrocellMode::Registered] {
            for polarity in [OutputPolarity::ActiveHigh, OutputPolarity::ActiveLow] {
                let mut map = erased_gal();
                let mode_field = cell.mode_field.as_ref().expect("mode");
                let polarity_field = cell.polarity_field.as_ref().expect("polarity");
                mode_field.encode(&mut map, mode).expect("encodable");
                polarity_field.encode(&mut map, polarity).expect("encodable");
                assert_eq!(mode_field.decode(&map), Ok(mode), "macrocell {}", cell.id.0);
                assert_eq!(polarity_field.decode(&map), Ok(polarity), "macrocell {}", cell.id.0);
            }
        }
    }
}

#[test]
fn input_only_is_supported_but_not_through_the_mode_field() {
    // Measured: `ioin14` … `ioin23` leave the undriven macrocell reading
    // S0 = 0, S1 = 1 — combinational, active low — which is
    // indistinguishable from a combinational active-low *output*. The
    // architecture region is exactly two fuses per macrocell and both
    // are accounted for as polarity and mode, so no spare state exists
    // to encode "input only".
    //
    // What differs in those designs is the output-enable row, left
    // entirely intact. That the row is the mechanism is an INFERENCE —
    // it is the only remaining control in the fuse map — not a
    // measurement, and SPEC.md §7.4's output-enable experiments are
    // what would settle it.
    //
    // So the capability is advertised and the mode field refuses to
    // encode it. A field that silently substituted a combinational
    // output would drive a pin the design meant to leave floating.
    let cells = macrocells().expect("valid");
    for cell in &cells {
        assert!(cell.supports(MacrocellMode::InputOnly), "macrocell {}", cell.id.0);
        assert!(cell.supports(MacrocellMode::Registered));
        assert!(cell.supports(MacrocellMode::Combinational));

        let mode = cell.mode_field.as_ref().expect("mode");
        let mut map = erased_gal();
        assert!(
            mode.encode(&mut map, MacrocellMode::InputOnly).is_err(),
            "macrocell {} must refuse to encode InputOnly",
            cell.id.0
        );
        assert_eq!(mode.values().count(), 2, "the field encodes two modes, not three");
    }
}

#[test]
fn every_macrocell_uses_the_one_global_clock() {
    // The device has a single clock pin and no per-macrocell clock
    // select to encode: the architecture region is two fuses per cell
    // and both are spoken for.
    let cells = macrocells().expect("valid");
    for cell in &cells {
        assert_eq!(cell.fixed_clock, Some(GLOBAL_CLOCK), "macrocell {}", cell.id.0);
    }
}
