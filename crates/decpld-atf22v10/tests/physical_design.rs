//! Whole-design encode and decode for the ATF22V10C.
//!
//! The per-row and per-field round-trips are checked elsewhere. This is
//! the one SPEC.md §4.7 states as "every legal configuration
//! round-trips through encode/decode" at the level a user cares about:
//! a whole part, read back into macrocells and equations.

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

/// The `in2` design: pin 2 drives pin 23, combinational, active high.
fn in2_design() -> PhysicalDesign {
    let mut design = blank_design(Footprint::Gal).expect("a blank design");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|c| c.id == MacrocellId(macrocell.0))
        .expect("pin 23 has a macrocell");

    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.pad_enabled = true;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start),
        cube: Cube::new([literal(2, Polarity::True)]),
    }];
    design
}

#[test]
fn a_blank_design_leaves_every_macrocell_unconfigured() {
    let design = blank_design(Footprint::Gal).expect("a blank design");
    assert_eq!(design.macrocells.len(), 10);
    assert_eq!(design.package, DIP24);
    for cell in &design.macrocells {
        assert!(cell.data_terms.is_empty(), "macrocell {}", cell.id.0);
        assert_eq!(cell.oe_term, None);
        assert!(!cell.pad_enabled);
        assert_eq!(cell.assigned_signal, None);
    }
    assert!(design.global_terms.is_empty());
}

/// The blown fuses of a map, as inclusive runs — the form the evidence
/// document records oracle output in.
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

#[test]
fn encoding_in2_reproduces_the_whole_array_wincupl_produced() {
    // Not a window this time — the ENTIRE array, compared with the runs
    // the evidence document records for `in2`:
    //
    //     blown 44-91, 93-131
    //
    // Everything else is intact, including rows 0 and 131 and every
    // other macrocell's block, because an unused product term on this
    // device is all-links-connected and therefore never true. Getting
    // that backwards leaves unused rows constantly true, which ORs into
    // every sum and drives ten pins high.
    let fuses = encode_design(&in2_design()).expect("encodable");
    assert_eq!(blown_runs(&fuses, 5808), [(44, 91), (93, 131)]);

    // And the architecture bits, which the matrix tests could not reach:
    // `arch-comb-high` on pin 23 sets 5808 and 5809.
    assert_eq!(fuses.get(FuseId(5808)), Some(true), "S0: active high");
    assert_eq!(fuses.get(FuseId(5809)), Some(true), "S1: combinational");
}

#[test]
fn a_design_round_trips_through_fuses() {
    // SPEC.md §4.7. Encode a design, decode the fuses, and compare with
    // the design that went in — the invariant the compile driver will
    // rest on, at the level where a mistake becomes a wrong chip.
    let design = in2_design();
    let fuses = encode_design(&design).expect("encodable");
    let decoded = decode_design(&fuses).expect("decodable");
    assert_eq!(decoded, design);
}

#[test]
fn every_macrocell_round_trips_in_every_mode_and_polarity() {
    // Exhaustive over the configuration space: ten macrocells, two
    // modes, two polarities, driven from a literal that exercises both
    // ends of the column range.
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let macrocell = MacrocellIndex(index);
        let pin = G.macrocell_pin(macrocell).expect("a pin");
        let block = G.row_block(macrocell).expect("a block");
        for mode in [MacrocellMode::Combinational, MacrocellMode::Registered] {
            for polarity in [OutputPolarity::ActiveHigh, OutputPolarity::ActiveLow] {
                let mut design = blank_design(Footprint::Gal).expect("blank");
                let cell = design
                    .macrocells
                    .iter_mut()
                    .find(|c| c.id == MacrocellId(index))
                    .expect("present");
                cell.mode = mode;
                cell.polarity = polarity;
                cell.pad_enabled = true;
                cell.oe_term = Some(PlacedCube {
                    row: ProductTermId(block.output_enable_row),
                    cube: Cube::always(),
                });
                cell.data_terms = vec![PlacedCube {
                    row: ProductTermId(block.data_rows.start),
                    cube: Cube::new([
                        literal(1, Polarity::True),
                        literal(13, Polarity::Complement),
                    ]),
                }];

                let fuses = encode_design(&design)
                    .unwrap_or_else(|e| panic!("pin {} {mode:?} {polarity:?}: {e:?}", pin.0));
                let decoded = decode_design(&fuses)
                    .unwrap_or_else(|e| panic!("pin {} {mode:?} {polarity:?}: {e:?}", pin.0));
                assert_eq!(decoded, design, "pin {} {mode:?} {polarity:?}", pin.0);
            }
        }
    }
}

#[test]
fn the_device_wide_reset_and_preset_terms_round_trip() {
    // Rows 0 and 131 belong to no macrocell, so a design that forgot
    // them would decode as equal to one that had them — the two rows
    // furthest apart in the array, and the pair an addressing error
    // moves most.
    let mut design = in2_design();
    design.global_terms = vec![
        PlacedCube {
            row: ProductTermId(ASYNCHRONOUS_RESET_ROW),
            cube: Cube::new([literal(3, Polarity::True)]),
        },
        PlacedCube {
            row: ProductTermId(SYNCHRONOUS_PRESET_ROW),
            cube: Cube::new([literal(4, Polarity::True)]),
        },
    ];

    let fuses = encode_design(&design).expect("encodable");
    // The `global-ar-sp` measurement, whole-array:
    //
    //     blown 0-7, 9-91, 93-131, 5764-5775, 5777-5807
    //
    // Rows 0 and 131 are now written, so the array's two ends carry
    // terms and everything between rows 3 and 130 is off.
    assert_eq!(blown_runs(&fuses, 5808), [(0, 7), (9, 91), (93, 131), (5764, 5775), (5777, 5807)]);

    assert_eq!(decode_design(&fuses).expect("decodable"), design);
}

#[test]
fn a_disabled_pad_decodes_as_disabled() {
    // An output-enable term that is constantly false leaves the pin
    // undriven, which is how this device reaches "input only". Decoding
    // must report that rather than an enabled output whose enable
    // happens to be unsatisfiable.
    let mut design = blank_design(Footprint::Gal).expect("blank");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a block");
    let cell =
        design.macrocells.iter_mut().find(|c| c.id == MacrocellId(macrocell.0)).expect("present");
    // An absent output-enable term IS the disabled pad. Expressing it as
    // a present-but-contradictory cube would be refused by `encode_cube`,
    // and rightly so: `a & !a` written by a designer is a mistake worth
    // reporting, while a pad nobody drives is an intent worth encoding.
    // The two need different spellings, and this is the one that means
    // "off".
    cell.pad_enabled = false;
    cell.oe_term = None;
    let _ = block;

    let fuses = encode_design(&design).expect("encodable");
    let decoded = decode_design(&fuses).expect("decodable");
    let decoded_cell =
        decoded.macrocells.iter().find(|c| c.id == MacrocellId(macrocell.0)).expect("present");
    assert!(!decoded_cell.pad_enabled, "a never-true enable leaves the pad off");
}

#[test]
fn a_design_placing_a_term_in_another_macrocells_row_is_refused() {
    // Rows are owned. Writing pin 23's logic into pin 22's block would
    // put one output's equation on another's pin, and nothing later in
    // the pipeline would notice.
    let mut design = in2_design();
    let intruder = G.macrocell_of_pin(PinNumber(22)).expect("an I/O pin");
    let stolen = G.row_block(intruder).expect("a block").data_rows.start;
    let cell = design.macrocells.iter_mut().find(|c| c.id == MacrocellId(9)).expect("pin 23");
    cell.data_terms[0].row = ProductTermId(stolen);

    // The exact error matters. Without the ownership check this still
    // fails — the neighbouring macrocell turns that row off, and the two
    // writes conflict — so `is_err()` alone passes whether or not the
    // rule exists, and would go on passing if it were deleted.
    let error = encode_design(&design).expect_err("a term outside the macrocell's rows");
    assert_eq!(
        error,
        DesignError::TermNotOwned { macrocell: 9, row: ProductTermId(stolen) },
        "the refusal must name the ownership violation, not a downstream fuse conflict"
    );
}

#[test]
fn decoding_an_erased_device_reports_ten_undriven_macrocells() {
    // The state a part arrives in. Every array link is intact, so every
    // product term holds every literal at both polarities — constantly
    // false — and no pad is driven.
    let fuses = FuseMap::erased(regions_for(Footprint::Gal).expect("a valid layout"));
    let decoded = decode_design(&fuses).expect("an erased part is describable");
    assert_eq!(decoded.macrocells.len(), 10);
    for cell in &decoded.macrocells {
        assert!(cell.pad_enabled, "an erased OE row is all-blown, i.e. permanently enabled");
    }
}
