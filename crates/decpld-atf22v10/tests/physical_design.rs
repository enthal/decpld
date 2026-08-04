//! Whole-design encode and decode for the ATF22V10C.
//!
//! The per-row and per-field round-trips are checked elsewhere. This is
//! the one SPEC.md §4.7 states as "every legal configuration
//! round-trips through encode/decode" at the level a user cares about:
//! a whole part, read back into macrocells and equations.

use decpld_atf22v10::*;
use decpld_device::{
    FeedbackSource, FuseId, FuseMap, MacrocellId, MacrocellMode, OutputPolarity, PhysicalDesign,
    PinNumber, PlacedCube, ProductTermId,
};
use decpld_logic::{Cube, Literal, Polarity};

const G: Atf22v10Geometry = Atf22v10Geometry;

fn literal(pin: u8, polarity: Polarity) -> Literal {
    let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
    Literal::new(bool_input_of_source(source.index), polarity)
}

/// The macrocell driving `pin`, and its measured row block.
fn cell_of_pin(
    design: &mut PhysicalDesign,
    pin: u8,
) -> (&mut decpld_device::MacrocellConfig, RowBlock) {
    let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|c| c.id == MacrocellId(macrocell.0))
        .expect("every macrocell is present");
    (cell, block)
}

/// The `in2` design: pin 2 drives pin 23, combinational, active high.
fn in2_design() -> PhysicalDesign {
    let mut design = blank_design().expect("a blank design");
    let (cell, block) = cell_of_pin(&mut design, 23);

    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
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
    let design = blank_design().expect("a blank design");
    assert_eq!(design.macrocells.len(), 10);
    assert_eq!(design.package, DIP24);
    for cell in &design.macrocells {
        assert!(cell.data_terms.is_empty(), "macrocell {}", cell.id.0);
        assert_eq!(cell.oe_term, None);
        assert!(!cell.pad_enabled());
        assert_eq!(cell.assigned_signal, None);
    }
    assert!(design.global_terms.is_empty());
}

/// The blown fuses of a map, as inclusive runs — the form the evidence
/// document records oracle output in.
fn blown_runs(fuses: &FuseMap, range: std::ops::Range<u32>) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for fuse in range {
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
    let fuses = encode_design(&in2_design(), Footprint::Gal).expect("encodable");
    assert_eq!(blown_runs(&fuses, 0..5808), [(44, 91), (93, 131)]);

    // And the architecture bits, which the matrix tests could not reach:
    // `arch-comb-high` on pin 23 sets 5808 and 5809.
    assert_eq!(fuses.get(FuseId(5808)), Some(true), "S0: active high");
    assert_eq!(fuses.get(FuseId(5809)), Some(true), "S1: combinational");
}

#[test]
fn every_architecture_pair_is_written_including_the_nine_unused_ones() {
    // The array tests stop at 5808, so without this the commit's claim
    // to encode "the architecture pair too" would rest on one pair in
    // ten. Every pair is written, and the nine cells the design does
    // not use take the blank design's values (combinational, active
    // high) rather than being left to whatever the caller's map held.
    //
    // Note this does NOT match WinCUPL, which writes two different
    // things depending on how a macrocell is unused. The `oe-*` runs
    // measure both: a cell the design never mentions reads S0 clear,
    // S1 clear (5810-5827 are all intact in every one of them), while
    // `ioin14` … `ioin23` leave a cell used only as an *input* at S0
    // clear, S1 set. deCPLD writes 1, 1 for every unused cell, which is
    // neither.
    //
    // The difference is inert: a macrocell whose output-enable row is
    // off is tri-stated at either polarity, and the pin is undriven
    // either way. It is recorded here rather than silently tolerated,
    // because the whole-array comparisons above stop short of this
    // region and a reader would otherwise assume they covered it.
    let fuses = encode_design(&in2_design(), Footprint::Gal).expect("encodable");
    for fuse in 5808..5828 {
        assert!(fuses.is_written(FuseId(fuse)), "architecture fuse {fuse} left unwritten");
        assert_eq!(fuses.get(FuseId(fuse)), Some(true), "fuse {fuse}");
    }
}

#[test]
fn a_design_round_trips_through_fuses() {
    // SPEC.md §4.7. Encode a design, decode the fuses, and compare with
    // the design that went in — the invariant the compile driver will
    // rest on, at the level where a mistake becomes a wrong chip.
    let design = in2_design();
    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    let decoded = decode_design(&fuses).expect("decodable");
    assert_eq!(decoded, design);
}

/// A design that uses more than one term, more than one macrocell, and
/// rows that are not the first of their block.
///
/// Every other design in this file has exactly one data term, in
/// `data_rows.start`, in one macrocell, with mode and polarity equal to
/// the blank defaults everywhere else — which leaves an encoder free to
/// drop every term after the first, ignore the row a term names, or
/// skip the architecture fields of any macrocell with no data terms,
/// and still pass.
fn multi_term_design() -> PhysicalDesign {
    let mut design = blank_design().expect("a blank design");

    // Pin 23: three terms, on the first, an interior, and the last row
    // of its block; registered and active low, both the opposite of the
    // blank defaults.
    let (cell, block) = cell_of_pin(&mut design, 23);
    cell.mode = MacrocellMode::Registered;
    cell.polarity = OutputPolarity::ActiveLow;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = vec![
        PlacedCube {
            row: ProductTermId(block.data_rows.start),
            cube: Cube::new([literal(2, Polarity::True)]),
        },
        PlacedCube {
            row: ProductTermId(block.data_rows.start + 1),
            cube: Cube::new([literal(3, Polarity::Complement), literal(4, Polarity::True)]),
        },
        PlacedCube {
            row: ProductTermId(block.last_data_row().expect("a non-empty block")),
            cube: Cube::new([literal(5, Polarity::True)]),
        },
    ];

    // Pin 18, in the middle of the array, configured differently again:
    // combinational and active high, two terms.
    let (cell, block) = cell_of_pin(&mut design, 18);
    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term = Some(PlacedCube {
        row: ProductTermId(block.output_enable_row),
        cube: Cube::new([literal(6, Polarity::True)]),
    });
    cell.data_terms = vec![
        PlacedCube {
            row: ProductTermId(block.data_rows.start + 2),
            cube: Cube::new([literal(7, Polarity::True), literal(8, Polarity::Complement)]),
        },
        PlacedCube {
            row: ProductTermId(block.last_data_row().expect("a non-empty block")),
            cube: Cube::new([literal(9, Polarity::Complement)]),
        },
    ];

    // Pin 14, registered and active high — a third distinct pair, so a
    // mode or polarity written to the wrong macrocell shows up.
    let (cell, block) = cell_of_pin(&mut design, 14);
    cell.mode = MacrocellMode::Registered;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start + 1),
        cube: Cube::new([literal(11, Polarity::True)]),
    }];

    design
}

#[test]
fn a_design_with_several_terms_across_several_macrocells_round_trips() {
    let design = multi_term_design();
    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    let decoded = decode_design(&fuses).expect("decodable");
    assert_eq!(decoded, design);
}

#[test]
fn every_term_lands_in_the_row_it_named() {
    // The round-trip above compares whole designs, so an encoder that
    // wrote a term to the wrong row *and* a decoder that read it back
    // from the same wrong row would agree with each other. This checks
    // the absolute fuse addresses instead: each cube's literal must
    // appear in its own row's 44 fuses and nowhere else.
    let design = multi_term_design();
    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");

    // Every one of the 44 columns carries a source — 22 signals at two
    // polarities — so a row's intact links are exactly its literals.
    assert_eq!(G.sources().count() * 2, COLUMNS as usize);

    for cell in &design.macrocells {
        for placed in cell.data_terms.iter().chain(cell.oe_term.iter()) {
            let start = placed.row.0 * COLUMNS;
            let intact: Vec<u32> = (start..start + COLUMNS)
                .filter(|fuse| fuses.get(FuseId(*fuse)) == Some(false))
                .collect();
            let mut expected: Vec<u32> = placed
                .cube
                .literals
                .iter()
                .map(|literal| {
                    let source = literal.input.0;
                    let column = match literal.polarity {
                        Polarity::True => source * 2,
                        Polarity::Complement => source * 2 + 1,
                    };
                    start + column
                })
                .collect();
            expected.sort_unstable();
            assert_eq!(intact, expected, "macrocell {} term in row {}", cell.id.0, placed.row.0);
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

    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    // The `global-ar-sp` measurement, whole-array:
    //
    //     blown 0-7, 9-91, 93-131, 5764-5775, 5777-5807
    //
    // Rows 0 and 131 are now written, so the array's two ends carry
    // terms and everything between rows 3 and 130 is off.
    assert_eq!(
        blown_runs(&fuses, 0..5808),
        [(0, 7), (9, 91), (93, 131), (5764, 5775), (5777, 5807)]
    );

    assert_eq!(decode_design(&fuses).expect("decodable"), design);
}

#[test]
fn a_disabled_pad_decodes_as_disabled() {
    // An output-enable term that is constantly false leaves the pin
    // undriven, which is how this device reaches "input only". Decoding
    // must report that rather than an enabled output whose enable
    // happens to be unsatisfiable.
    //
    // An absent output-enable term IS the disabled pad. Expressing it as
    // a present-but-contradictory cube would be refused by `encode_cube`,
    // and rightly so: `a & !a` written by a designer is a mistake worth
    // reporting, while a pad nobody drives is an intent worth encoding.
    let mut design = blank_design().expect("a blank design");
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let (cell, _) = cell_of_pin(&mut design, 23);
    cell.oe_term = None;

    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    let decoded = decode_design(&fuses).expect("decodable");
    let decoded_cell =
        decoded.macrocells.iter().find(|c| c.id == MacrocellId(macrocell.0)).expect("present");
    assert!(!decoded_cell.pad_enabled(), "a never-true enable leaves the pad off");
}

#[test]
fn a_design_placing_a_term_in_another_macrocells_row_is_refused() {
    // Rows are owned. Writing pin 23's logic into pin 22's block would
    // put one output's equation on another's pin, and nothing later in
    // the pipeline would notice.
    let mut design = in2_design();
    let intruder = G.macrocell_of_pin(PinNumber(22)).expect("an I/O pin");
    let stolen = G.row_block(intruder).expect("a block").data_rows.start;
    let (cell, _) = cell_of_pin(&mut design, 23);
    cell.data_terms[0].row = ProductTermId(stolen);

    // The exact error matters. Without the ownership check this still
    // fails — the neighbouring macrocell turns that row off, and the two
    // writes conflict — so `is_err()` alone passes whether or not the
    // rule exists, and would go on passing if it were deleted.
    let error =
        encode_design(&design, Footprint::Gal).expect_err("a term outside the macrocell's rows");
    assert_eq!(
        error,
        DesignError::TermNotOwned { macrocell: MacrocellId(9), row: ProductTermId(stolen) },
        "the refusal must name the ownership violation, not a downstream fuse conflict"
    );
}

#[test]
fn a_data_term_in_the_macrocells_own_output_enable_row_is_refused() {
    // Ownership is not enough: a row belongs to a macrocell *in a
    // role*. A data term written into the enable row would silently
    // turn the design's logic into the pin's tri-state control, and
    // decoding reports it back as the enable — a round-trip that agrees
    // with itself about the wrong thing.
    let mut design = blank_design().expect("a blank design");
    let (cell, block) = cell_of_pin(&mut design, 23);
    let oe_row = ProductTermId(block.output_enable_row);
    cell.oe_term = None;
    cell.data_terms =
        vec![PlacedCube { row: oe_row, cube: Cube::new([literal(2, Polarity::True)]) }];

    let error = encode_design(&design, Footprint::Gal)
        .expect_err("a data term may not occupy the enable row");
    assert_eq!(error, DesignError::TermNotOwned { macrocell: MacrocellId(9), row: oe_row });
}

#[test]
fn an_output_enable_term_in_a_data_row_is_refused() {
    // The mirror image, so the role check cannot be satisfied by a
    // predicate that happens to accept everything in one direction.
    let mut design = blank_design().expect("a blank design");
    let (cell, block) = cell_of_pin(&mut design, 23);
    let data_row = ProductTermId(block.data_rows.start);
    cell.oe_term = Some(PlacedCube { row: data_row, cube: Cube::always() });

    let error = encode_design(&design, Footprint::Gal)
        .expect_err("an enable term may not occupy a data row");
    assert_eq!(error, DesignError::TermNotOwned { macrocell: MacrocellId(9), row: data_row });
}

#[test]
fn a_design_that_omits_a_macrocell_is_refused() {
    // The corruption this encoder exists to prevent, arriving from the
    // other side. A macrocell the design does not mention has rows
    // nobody turns off, and an untouched row on this device is
    // constantly TRUE — so an incomplete design silently encodes to a
    // part with pins permanently high. Guessing a configuration for the
    // missing cell would be worse: the caller would never learn that
    // the design it handed over was not the design that was programmed.
    let mut design = in2_design();
    design.macrocells.retain(|cell| cell.id != MacrocellId(0));

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("an incomplete design"),
        DesignError::MacrocellMissing { macrocell: MacrocellId(0) }
    );

    // And the degenerate case: no macrocells at all, which under an
    // encoder that iterated the design's own list would produce a part
    // with every one of its 130 rows erased and ten pins driven high.
    let empty = PhysicalDesign {
        device: DEVICE,
        package: DIP24,
        macrocells: Vec::new(),
        global_terms: Vec::new(),
    };
    assert_eq!(
        encode_design(&empty, Footprint::Gal).expect_err("an empty design"),
        DesignError::MacrocellMissing { macrocell: MacrocellId(0) }
    );
}

#[test]
fn a_design_listing_one_macrocell_twice_is_refused_by_name() {
    // Two entries for one macrocell means two configurations for one
    // set of rows. Left to the fuse layer this surfaces as "two
    // encoders disagree about fuse 88", which names neither the
    // macrocell nor the duplication.
    let mut design = in2_design();
    let duplicate = design.macrocells[3].clone();
    design.macrocells.push(duplicate);

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("a macrocell listed twice"),
        DesignError::MacrocellListedTwice { macrocell: MacrocellId(3) }
    );
}

#[test]
fn a_design_naming_a_macrocell_this_device_does_not_have_is_refused() {
    let mut design = in2_design();
    let mut extra = design.macrocells[0].clone();
    extra.id = MacrocellId(10);
    design.macrocells.push(extra);

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("eleven macrocells"),
        DesignError::NoSuchMacrocell { macrocell: MacrocellId(10) }
    );
}

#[test]
fn a_row_used_twice_within_one_macrocell_is_refused_by_name() {
    // The same row named by two terms is a design error, not a fuse
    // conflict, and it stays one even when the two cubes are identical
    // — where the fuse layer sees two agreeing writes and permits them.
    let mut design = in2_design();
    let (cell, _) = cell_of_pin(&mut design, 23);
    let repeated = cell.data_terms[0].clone();
    cell.data_terms.push(repeated);

    let row = design.macrocells[9].data_terms[0].row;
    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("one row, two terms"),
        DesignError::RowUsedTwice { macrocell: MacrocellId(9), row }
    );
}

#[test]
fn a_feedback_source_this_device_cannot_encode_is_refused() {
    // `feedback` is part of the design and no fuse encodes it on this
    // part, so an unsupported value must be refused rather than written
    // nowhere and decoded back as `Pin` — a round-trip that would
    // report success on a design the chip does not implement.
    let mut design = in2_design();
    let (cell, _) = cell_of_pin(&mut design, 23);
    cell.feedback = FeedbackSource::Registered;

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("an unencodable feedback path"),
        DesignError::FeedbackNotSupported {
            macrocell: MacrocellId(9),
            feedback: FeedbackSource::Registered
        }
    );
}

#[test]
fn a_design_for_another_device_is_refused() {
    // `PhysicalDesign` names its device, and encoding one for an
    // ATF16V8 into an ATF22V10 fuse map would produce a plausible file
    // for the wrong part.
    let mut design = in2_design();
    design.device = "ATF16V8";
    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("a foreign design"),
        DesignError::WrongDevice { device: "ATF16V8", expected: DEVICE }
    );
}

#[test]
fn the_footprint_decides_the_size_of_the_encoded_map() {
    // A parameter that is validated and then discarded reads as if it
    // were honoured. Each footprint must produce its own fuse count —
    // and the array and architecture regions, which all three share,
    // must be identical across them.
    for footprint in Footprint::ALL {
        let fuses = encode_design(&in2_design(), footprint).expect("encodable");
        assert_eq!(fuses.len(), footprint.fuse_count(), "{footprint:?}");
        assert_eq!(
            blown_runs(&fuses, 0..5828),
            [(44, 91), (93, 131), (5808, 5827)],
            "{footprint:?}"
        );
    }
}

#[test]
fn decoding_a_map_that_is_not_this_device_is_refused_by_fuse_count() {
    // `jed inspect` reads files this compiler did not write, and a
    // 2194-fuse ATF16V8 file must be turned away as the wrong part
    // rather than partly decoded until some field runs off the end.
    let fuses = FuseMap::erased(regions_for(Footprint::Gal).expect("valid"));
    assert!(decode_design(&fuses).is_ok());

    let foreign = FuseMap::erased(
        decpld_device::FuseRegions::new(
            2194,
            vec![decpld_device::FuseRegion {
                name: "array",
                range: 0..2194,
                erased_value: true,
                mutability: decpld_device::FuseMutability::Programmable,
            }],
        )
        .expect("valid"),
    );
    assert_eq!(
        decode_design(&foreign).expect_err("not an ATF22V10C fuse count"),
        DesignError::Footprint(FootprintError::UnknownFuseCount { count: 2194 })
    );
}

#[test]
fn decoding_an_erased_device_reports_ten_permanently_enabled_pads() {
    // The state a part arrives in, and the fact this whole encoder
    // rests on: an erased ATF22V10C is all links BLOWN, so every
    // product term holds no literals at all — the empty AND, constantly
    // TRUE. Every output-enable row is therefore permanently asserted
    // and every pin is driven, which is the opposite of the intuition
    // that "erased" means "does nothing".
    let fuses = FuseMap::erased(regions_for(Footprint::Gal).expect("a valid layout"));
    let decoded = decode_design(&fuses).expect("an erased part is describable");
    assert_eq!(decoded.macrocells.len(), 10);
    for cell in &decoded.macrocells {
        assert!(cell.pad_enabled(), "an erased OE row is all-blown, i.e. permanently enabled");
        assert_eq!(cell.oe_term.as_ref().map(|term| term.cube.clone()), Some(Cube::always()));
        // Every data row is constantly true as well, so the decoded
        // design reports all of them rather than hiding a row that can
        // fire. The count is the macrocell's full capacity.
        let block = G.row_block(MacrocellIndex(cell.id.0)).expect("a block");
        assert_eq!(cell.data_terms.len(), block.data_rows.len(), "macrocell {}", cell.id.0);
        for term in &cell.data_terms {
            assert_eq!(term.cube, Cube::always());
        }
    }
}

#[test]
fn a_device_wide_row_placed_twice_is_refused_by_name() {
    // The reset and preset rows belong to no macrocell, so nothing in
    // the per-macrocell checks sees them. Two terms for one of them is
    // the same design error as two terms for one data row, and gets the
    // same kind of answer.
    let mut design = in2_design();
    design.global_terms = vec![
        PlacedCube {
            row: ProductTermId(ASYNCHRONOUS_RESET_ROW),
            cube: Cube::new([literal(3, Polarity::True)]),
        },
        PlacedCube {
            row: ProductTermId(ASYNCHRONOUS_RESET_ROW),
            cube: Cube::new([literal(4, Polarity::True)]),
        },
    ];

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("one global row, two terms"),
        DesignError::GlobalRowUsedTwice { row: ProductTermId(ASYNCHRONOUS_RESET_ROW) }
    );
}

#[test]
fn a_macrocell_term_placed_in_a_device_wide_row_is_refused() {
    // And the reverse: row 0 belongs to no macrocell, so placing a data
    // term there must fail as an ownership violation rather than as a
    // fuse conflict with the reset row's own disabling write.
    let mut design = in2_design();
    let (cell, _) = cell_of_pin(&mut design, 23);
    cell.data_terms[0].row = ProductTermId(ASYNCHRONOUS_RESET_ROW);

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("a data term in the reset row"),
        DesignError::TermNotOwned {
            macrocell: MacrocellId(9),
            row: ProductTermId(ASYNCHRONOUS_RESET_ROW)
        }
    );
}

#[test]
fn a_global_term_in_a_macrocells_row_is_refused_as_not_global() {
    let mut design = in2_design();
    let block = G.row_block(MacrocellIndex(0)).expect("a block");
    design.global_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start),
        cube: Cube::new([literal(3, Polarity::True)]),
    }];

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("a macrocell row is not global"),
        DesignError::NotAGlobalTerm { row: ProductTermId(block.data_rows.start) }
    );
}

#[test]
fn refusals_name_the_design_object_in_words_not_rust_syntax() {
    // SPEC.md §8.3 and CLAUDE.md: a diagnostic names the object it is
    // about. `ProductTermId(2)` in a user-facing message is a leak of
    // Rust's `Debug` formatting, not an explanation.
    let error = DesignError::TermNotOwned { macrocell: MacrocellId(9), row: ProductTermId(2) };
    let text = error.to_string();
    assert!(text.contains("macrocell 9"), "{text}");
    assert!(text.contains("product term 2"), "{text}");
    assert!(!text.contains("ProductTermId"), "{text}");
    assert!(!text.contains("MacrocellId"), "{text}");
}
