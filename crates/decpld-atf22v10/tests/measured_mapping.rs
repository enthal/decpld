//! The measured ATF22V10C mapping, asserted against the numbers the
//! oracle produced.
//!
//! Every expectation here is transcribed from
//! `targets/evidence/atf22v10-fuse-map.md`, which cites the experiment
//! that produced it. If the oracle is later shown wrong, these are the
//! tests that trusted it and this is where to look.

use decpld_atf22v10::*;
use decpld_device::{FuseId, FuseMap, FuseMutability, PinNumber};

const G: Atf22v10Geometry = Atf22v10Geometry;

#[test]
fn input_pins_land_on_the_measured_columns() {
    // Experiments in1..in11, in13. Output fixed on pin 23, input swept;
    // exactly one column stays intact and that column is the answer.
    let expected: [(u8, u32); 12] = [
        (1, 0),
        (2, 4),
        (3, 8),
        (4, 12),
        (5, 16),
        (6, 20),
        (7, 24),
        (8, 28),
        (9, 32),
        (10, 36),
        (11, 40),
        (13, 42),
    ];
    for (pin, column) in expected {
        let source = G
            .sources()
            .find(|s| s.kind == SourceKind::Input(PinNumber(pin)))
            .unwrap_or_else(|| panic!("pin {pin} has no source"));
        assert_eq!(source.true_column(), column, "pin {pin}");
        assert_eq!(source.complement_column(), column + 1, "pin {pin} complement");
    }
}

#[test]
fn macrocell_feedback_lands_on_the_measured_columns() {
    // Experiments fb14..fb23. Pin n drives pin 23, so pin 23's data
    // term names pin n's feedback column.
    let expected: [(u8, u32); 10] = [
        (23, 2),
        (22, 6),
        (21, 10),
        (20, 14),
        (19, 18),
        (18, 22),
        (17, 26),
        (16, 30),
        (15, 34),
        (14, 38),
    ];
    for (pin, column) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let source = G
            .sources()
            .find(|s| s.kind == SourceKind::Feedback(macrocell))
            .unwrap_or_else(|| panic!("pin {pin} feedback has no source"));
        assert_eq!(source.true_column(), column, "pin {pin} feedback");
    }
}

#[test]
fn every_column_is_claimed_exactly_once() {
    // 22 sources x 2 senses = 44 columns, none shared and none missing.
    // A source silently sharing a column with another is a mapping
    // error that still produces a plausible file.
    let mut seen = vec![0u32; COLUMNS as usize];
    for source in G.sources() {
        seen[source.true_column() as usize] += 1;
        seen[source.complement_column() as usize] += 1;
    }
    assert_eq!(G.sources().count(), 22, "22 signal sources");
    for (column, count) in seen.iter().enumerate() {
        assert_eq!(*count, 1, "column {column} is claimed {count} times");
    }
}

#[test]
fn source_21_is_an_input_not_feedback() {
    // The exception that breaks "odd means feedback". It holds for all
    // ten feedbacks and still gets this one of eleven odd sources
    // wrong — extrapolating from pin 22 would have put a wrong column
    // in the model.
    let source = G.source(21).expect("source 21 exists");
    assert_eq!(source.kind, SourceKind::Input(PinNumber(13)));
    assert_eq!(source.true_column(), 42);
}

#[test]
fn row_blocks_start_where_they_were_measured() {
    // Every macrocell measured individually.
    // All ten macrocells, one design each (mc14..mc23). Five of these
    // were previously interpolated from Galette's table; interpolating
    // between measured points is still not measuring, and this device
    // has two fuse orderings running opposite ways — exactly the shape
    // where an interpolation looks right and is wrong in the middle.
    let expected: [(u8, u32); 10] = [
        (14, 122),
        (15, 111),
        (16, 98),
        (17, 83),
        (18, 66),
        (19, 49),
        (20, 34),
        (21, 21),
        (22, 10),
        (23, 1),
    ];
    for (pin, first_row) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a block");
        assert_eq!(block.output_enable_row, first_row, "pin {pin}");
    }
}

#[test]
fn architecture_pairs_run_opposite_to_row_blocks() {
    // The reversal, stated as the thing it is: j = 9 - i. The single
    // most likely place for this mapping to be silently transposed,
    // measured for every macrocell rather than at its ends.
    let expected: [(u8, u32); 10] = [
        (23, 5808),
        (22, 5810),
        (21, 5812),
        (20, 5814),
        (19, 5816),
        (18, 5818),
        (17, 5820),
        (16, 5822),
        (15, 5824),
        (14, 5826),
    ];
    for (pin, s0) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let pair = G.architecture_pair(macrocell).expect("a pair");
        assert_eq!(pair.polarity.0, s0, "pin {pin} S0");
        assert_eq!(pair.mode.0, s0 + 1, "pin {pin} S1");
    }

    // And the orderings genuinely disagree: pin 23 has the lowest rows
    // AND the lowest architecture pair, while carrying the HIGHEST
    // macrocell index. Two of the three run one way and one the other.
    let pin23 = G.macrocell_of_pin(PinNumber(23)).unwrap();
    let pin14 = G.macrocell_of_pin(PinNumber(14)).unwrap();
    assert!(
        G.row_block(pin23).unwrap().output_enable_row
            < G.row_block(pin14).unwrap().output_enable_row
    );
    assert!(
        G.architecture_pair(pin23).unwrap().polarity.0
            < G.architecture_pair(pin14).unwrap().polarity.0
    );
    assert!(pin23 > pin14, "and the macrocell indices run the other way");
}

#[test]
fn row_blocks_partition_the_array_leaving_only_the_two_control_rows() {
    // Blocks must not overlap and must account for 130 of the 132 rows.
    // The two left over are the device-wide control terms, both
    // measured (experiment global-ar-sp) rather than inferred from
    // Galette's print order.
    let mut used = vec![false; ROWS as usize];
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let block = G.row_block(MacrocellIndex(index)).expect("a block");
        for row in std::iter::once(block.output_enable_row).chain(block.data_rows.clone()) {
            assert!(!used[row as usize], "row {row} belongs to two macrocells");
            used[row as usize] = true;
        }
        let data = block.data_rows.end - block.data_rows.start;
        assert!((8..=16).contains(&data), "macrocell {index} has {data} data terms");
    }
    let unused: Vec<usize> =
        used.iter().enumerate().filter(|(_, u)| !**u).map(|(r, _)| r).collect();
    assert_eq!(
        unused,
        vec![ASYNCHRONOUS_RESET_ROW as usize, SYNCHRONOUS_PRESET_ROW as usize],
        "only the two device-wide control rows sit outside the macrocell blocks"
    );
}

#[test]
fn the_three_footprints_have_the_measured_fuse_counts() {
    // Experiments mode-pal, arch-comb-high, mode-powerdown: one design
    // compiled under each WinCUPL device type.
    assert_eq!(Footprint::Pal.fuse_count(), 5828);
    assert_eq!(Footprint::Gal.fuse_count(), 5892);
    assert_eq!(Footprint::PowerDown.fuse_count(), 5893);

    for footprint in Footprint::ALL {
        assert_eq!(Footprint::from_fuse_count(footprint.fuse_count()), Ok(footprint));
    }
    assert!(Footprint::from_fuse_count(2194).is_err(), "a GAL16V8 count is not ours");
}

#[test]
fn an_unknown_fuse_count_names_the_three_that_are_accepted() {
    // A user arriving with the wrong device's file should be told what
    // this device does accept, not merely that theirs is wrong.
    let message = Footprint::from_fuse_count(5000).unwrap_err().to_string();
    for expected in ["5828", "5892", "5893"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn every_footprint_classifies_every_fuse() {
    // FuseRegions enforces the partition, so this asserts each
    // footprint's regions are constructible — that the boundaries add up.
    for footprint in Footprint::ALL {
        let regions = regions_for(footprint).unwrap_or_else(|e| panic!("{footprint:?}: {e}"));
        assert_eq!(regions.count(), footprint.fuse_count(), "{footprint:?}");
    }
}

#[test]
fn pal_mode_has_no_signature_and_power_down_mode_has_the_extra_fuse() {
    // Measured: mode-pal yields QF5828 with nothing set above the
    // architecture block, and mode-powerdown is the GAL output plus
    // fuse 5892.
    let pal = regions_for(Footprint::Pal).unwrap();
    assert!(pal.iter().all(|r| r.mutability != FuseMutability::UserSignature));

    let gal = regions_for(Footprint::Gal).unwrap();
    let signature = gal
        .iter()
        .find(|r| r.mutability == FuseMutability::UserSignature)
        .expect("GAL mode has a signature");
    assert_eq!(signature.range, 5828..5892);

    let pd = regions_for(Footprint::PowerDown).unwrap();
    let power_down = pd.iter().find(|r| r.name == "power-down").expect("the extra fuse");
    assert_eq!(power_down.range, 5892..5893);
}

#[test]
fn the_security_fuse_is_not_in_the_fuse_map() {
    // The datasheet says the device has one, but it is the JEDEC `G`
    // field rather than a numbered fuse: every footprint's count is
    // fully accounted for without it. Modelling it here would mean
    // inventing an index with nothing to cite.
    for footprint in Footprint::ALL {
        let regions = regions_for(footprint).unwrap();
        assert!(
            regions.iter().all(|r| r.mutability != FuseMutability::Security),
            "{footprint:?} must not invent a security fuse index"
        );
        let mut map = FuseMap::erased(regions);
        assert!(map.set_security_fuse(true).is_err());
    }
}

#[test]
fn an_erased_device_has_every_array_link_blown() {
    // An erased GAL cell is a broken link, so every product term is the
    // empty AND — constantly true. Getting this backwards would invert
    // the meaning of every unprogrammed row.
    let map = FuseMap::erased(regions_for(Footprint::Gal).unwrap());
    for fuse in [0, 100, 5807] {
        assert_eq!(map.get(FuseId(fuse)), Some(true), "fuse {fuse}");
    }
}

#[test]
fn feedback_complements_are_adjacent_too() {
    // The evidence document measures nfb22/nfb18/nfb14 precisely because
    // establishing the complement relation on inputs and generalising to
    // feedback is an assumption rather than a measurement. Asserting it
    // only for inputs would leave the model's stronger claim untested.
    for pin in [22, 18, 14] {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let source = G
            .sources()
            .find(|s| s.kind == SourceKind::Feedback(macrocell))
            .expect("a feedback source");
        assert_eq!(source.complement_column(), source.true_column() + 1, "pin {pin}");
    }
}

#[test]
fn out_of_range_lookups_return_none_rather_than_panicking() {
    // Index arithmetic is this crate's named hazard, and these are the
    // paths a fitter will reach with a computed index.
    for index in 22..=255u8 {
        assert!(G.source(index).is_none(), "source {index}");
    }
    for pin in [0u8, 12, 13, 24, 255] {
        assert!(G.macrocell_of_pin(PinNumber(pin)).is_none(), "pin {pin} is not an I/O pin");
    }
    for index in [Atf22v10Geometry::MACROCELLS, 200, 255] {
        assert!(G.row_block(MacrocellIndex(index)).is_none(), "macrocell {index}");
        assert!(G.architecture_pair(MacrocellIndex(index)).is_none(), "macrocell {index}");
        assert!(G.macrocell_pin(MacrocellIndex(index)).is_none(), "macrocell {index}");
    }
}

#[test]
fn the_oracle_put_its_literals_where_row_major_addressing_says() {
    // The array's fuse addressing was, until this test, the one thing
    // the crate relied on and never stated: every experiment was read as
    // "row r, column c" by dividing the fuse address by 44, so the
    // row/column view could not be evidence for the formula that
    // produced it.
    //
    // These expectations are absolute fuse addresses, read straight out
    // of oracle output with no decomposition applied. The reasoning is
    // in `targets/evidence/atf22v10-fuse-map.md`, "Fuse addressing".
    //
    // Fuse 92 is the ANCHOR, not an agreement: it is where `in2`'s own
    // literal lands, and column 4 was read off that very address, so
    // "92 = 44*2 + 4" is an identity. The evidence is the other four,
    // which place a literal for a pin whose column is already known
    // into a DIFFERENT product term and find it at an address
    // differing by an exact multiple of 44:
    //
    //   pin 3   96 (in3) vs 8 (global-ar-sp AR)      diff 88   = 2 * 44
    //   pin 4   100 (in4) vs 5776 (global-ar-sp SP)  diff 5676 = 129 * 44
    //   pin 2   92 (in2) vs 2204 (mc19)              diff 2112 = 48 * 44
    //   pin 2   92 (in2) vs 5416 (mc14)              diff 5324 = 121 * 44
    let expected: [(u32, u32, u32, &str); 5] = [
        // The asynchronous-reset row, driven from pin 3 (column 8).
        (0, 8, 8, "global-ar-sp"),
        // Pin 23's first data row, driven from pin 2 (column 4).
        (2, 4, 92, "global-ar-sp"),
        // The synchronous-preset row, driven from pin 4 (column 12).
        (131, 12, 5776, "global-ar-sp"),
        // Pin 14's first data row — the far end of the array.
        (123, 4, 5416, "mc14"),
        // Pin 19's first data row — the middle.
        (50, 4, 2204, "mc19"),
    ];
    for (row, column, fuse, experiment) in expected {
        assert_eq!(
            G.fuse_of(ArrayCell { row, column }),
            Some(FuseId(fuse)),
            "row {row} column {column} ({experiment})"
        );
        assert_eq!(
            G.cell_of(FuseId(fuse)),
            Some(ArrayCell { row, column }),
            "fuse {fuse} ({experiment})"
        );
    }
}

#[test]
fn every_row_block_starts_on_the_fuse_the_oracle_wrote_first() {
    // The row numbers are measured; this checks that turning one into a
    // fuse address reproduces the address the oracle actually wrote, for
    // the three macrocells where a blown span pins it exactly.
    for (pin, first_fuse, experiment) in
        [(23u8, 44u32, "global-ar-sp"), (19, 2156, "mc19"), (14, 5368, "mc14")]
    {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a measured block");
        assert_eq!(
            G.fuse_of(ArrayCell { row: block.output_enable_row, column: 0 }),
            Some(FuseId(first_fuse)),
            "pin {pin} ({experiment})"
        );
    }
}

#[test]
fn addressing_round_trips_over_the_whole_array() {
    // Exhaustive rather than sampled: 5808 fuses is cheap, and a
    // boundary that folds two cells onto one fuse is invisible in a spot
    // check. This is the invariant SPEC.md §4.7 states as "matrix cells
    // map one-to-one to fuses", asserted in both directions.
    let mut seen = vec![false; (ROWS * COLUMNS) as usize];
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let cell = ArrayCell { row, column };
            let fuse = G.fuse_of(cell).unwrap_or_else(|| panic!("{cell:?} has no fuse"));
            assert!(fuse.0 < ROWS * COLUMNS, "{cell:?} escaped the array at {fuse:?}");
            assert!(!seen[fuse.0 as usize], "{cell:?} collided at {fuse:?}");
            seen[fuse.0 as usize] = true;
            assert_eq!(G.cell_of(fuse), Some(cell));
        }
    }
    assert!(seen.iter().all(|hit| *hit), "some array fuse belongs to no cell");
}

#[test]
fn addresses_outside_the_array_are_rejected_rather_than_wrapped() {
    // A fitter reaches these with computed indices, and the failure to
    // avoid is a row past the end quietly aliasing onto the next
    // region's fuses — which for row 132 would be the architecture bits.
    for cell in [
        ArrayCell { row: ROWS, column: 0 },
        ArrayCell { row: 0, column: COLUMNS },
        ArrayCell { row: ROWS, column: COLUMNS },
        ArrayCell { row: u32::MAX, column: u32::MAX },
    ] {
        assert!(G.fuse_of(cell).is_none(), "{cell:?}");
    }
    for fuse in [ROWS * COLUMNS, 5828, 5892, u32::MAX] {
        assert!(G.cell_of(FuseId(fuse)).is_none(), "fuse {fuse} is not in the array");
    }
}

#[test]
fn the_evidence_documents_summary_table_is_arithmetically_right() {
    // `targets/evidence/atf22v10-fuse-map.md` ends with a summary
    // collecting every measurement into fuse order: each macrocell
    // block as a fuse range, the column map, the architecture pairs,
    // the signature bytes. Those ranges are arithmetic over the
    // measured row and column numbers, not further measurements.
    //
    // A summary table nobody checks is where a transcription error
    // lives indefinitely, read as authoritative precisely because it is
    // convenient. So the numbers below are transcribed from the
    // document and re-derived from the model here, and the two must
    // agree.
    let blocks: [(u8, u32, u32); 10] = [
        (23, 44, 439),
        (22, 440, 923),
        (21, 924, 1495),
        (20, 1496, 2155),
        (19, 2156, 2903),
        (18, 2904, 3651),
        (17, 3652, 4311),
        (16, 4312, 4883),
        (15, 4884, 5367),
        (14, 5368, 5763),
    ];
    for (pin, first, last) in blocks {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a measured block");
        assert_eq!(
            G.fuse_of(ArrayCell { row: block.output_enable_row, column: 0 }),
            Some(FuseId(first)),
            "pin {pin} block start"
        );
        assert_eq!(
            G.fuse_of(ArrayCell {
                row: block.last_data_row().expect("a block with data terms"),
                column: COLUMNS - 1,
            }),
            Some(FuseId(last)),
            "pin {pin} block end"
        );
    }

    // The column table, transcribed the same way. Input pins and
    // feedback sources alternate for the first 20 columns and then stop
    // alternating, so a cell copied one position out of line reads as
    // perfectly ordinary.
    let columns: [(u32, Option<u8>, Option<u8>); 22] = [
        (0, Some(1), None),
        (2, None, Some(23)),
        (4, Some(2), None),
        (6, None, Some(22)),
        (8, Some(3), None),
        (10, None, Some(21)),
        (12, Some(4), None),
        (14, None, Some(20)),
        (16, Some(5), None),
        (18, None, Some(19)),
        (20, Some(6), None),
        (22, None, Some(18)),
        (24, Some(7), None),
        (26, None, Some(17)),
        (28, Some(8), None),
        (30, None, Some(16)),
        (32, Some(9), None),
        (34, None, Some(15)),
        (36, Some(10), None),
        (38, None, Some(14)),
        (40, Some(11), None),
        (42, Some(13), None),
    ];
    for (column, input_pin, feedback_pin) in columns {
        let source = G
            .sources()
            .find(|s| s.true_column() == column)
            .unwrap_or_else(|| panic!("no source at column {column}"));
        let expected = match (input_pin, feedback_pin) {
            (Some(pin), None) => SourceKind::Input(PinNumber(pin)),
            (None, Some(pin)) => {
                SourceKind::Feedback(G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin"))
            }
            _ => panic!("column {column} claims to be both or neither"),
        };
        assert_eq!(source.kind, expected, "column {column}");
    }

    // The signature byte table: byte b is fuses 5828+8b ..= 5835+8b.
    // The only summary cell with nothing else pinning it, and the eight
    // ranges must tile the signature region exactly — a byte boundary
    // off by one silently rewrites the PartNo text.
    let signature: [(u32, u32, u32); 8] = [
        (0, 5828, 5835),
        (1, 5836, 5843),
        (2, 5844, 5851),
        (3, 5852, 5859),
        (4, 5860, 5867),
        (5, 5868, 5875),
        (6, 5876, 5883),
        (7, 5884, 5891),
    ];
    let region = regions_for(Footprint::Gal).expect("a valid layout");
    let signature_region = region
        .iter()
        .find(|r| r.mutability == FuseMutability::UserSignature)
        .expect("GAL mode has a signature");
    let mut covered =
        vec![false; (signature_region.range.end - signature_region.range.start) as usize];
    for (byte, first, last) in signature {
        assert_eq!(signature_region.range.start + byte * 8, first, "byte {byte} start");
        assert_eq!(first + 7, last, "byte {byte} is eight fuses");
        for fuse in first..=last {
            let offset = (fuse - signature_region.range.start) as usize;
            assert!(!covered[offset], "fuse {fuse} is in two bytes");
            covered[offset] = true;
        }
    }
    assert!(covered.iter().all(|hit| *hit), "the eight bytes do not tile the signature region");

    // The two device-wide control rows bracket the array.
    for (row, first, last) in
        [(ASYNCHRONOUS_RESET_ROW, 0, 43), (SYNCHRONOUS_PRESET_ROW, 5764, 5807)]
    {
        assert_eq!(G.fuse_of(ArrayCell { row, column: 0 }), Some(FuseId(first)));
        assert_eq!(G.fuse_of(ArrayCell { row, column: COLUMNS - 1 }), Some(FuseId(last)));
    }
}

#[test]
fn a_block_with_no_data_terms_has_no_last_data_row() {
    // `RowBlock` is public and constructible, so the empty case is
    // reachable by a caller before it is reachable by this device. The
    // point of the accessor is that it answers `None` rather than a
    // wrapped `u32::MAX`, which `fuse_of` would then reject — two guards
    // where one would do, but this is the one that cannot be forgotten
    // at a call site.
    let empty = RowBlock { output_enable_row: 7, data_rows: 8..8 };
    assert_eq!(empty.last_data_row(), None);

    let one = RowBlock { output_enable_row: 7, data_rows: 8..9 };
    assert_eq!(one.last_data_row(), Some(8));

    // Every real block has data terms.
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let block = G.row_block(MacrocellIndex(index)).expect("a measured block");
        assert_eq!(block.last_data_row(), Some(block.data_rows.end - 1), "macrocell {index}");
    }
}
