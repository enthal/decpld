//! The ATF22V10C's AND matrix, checked against the oracle's own output.
//!
//! The device-layer tests in `decpld-device` check the *rules* over a
//! synthetic matrix. These check that this device's matrix is the one
//! the hardware actually has, by encoding a cube and comparing the
//! resulting fuses with what WinCUPL produced for the same design.

use decpld_atf22v10::*;
use decpld_device::{
    FuseId, FuseMap, MacrocellId, ProductTermId, ProductTermRole, decode_row, encode_cube,
};
use decpld_logic::{BoolInputId, Cube, Literal, Polarity};

const G: Atf22v10Geometry = Atf22v10Geometry;

fn erased_gal() -> FuseMap {
    FuseMap::erased(regions_for(Footprint::Gal).expect("a valid layout"))
}

/// The literal for a pin at a polarity, whichever kind of pin it is.
fn literal(pin: u8, polarity: Polarity) -> Literal {
    let source = G.source_of_pin(decpld_device::PinNumber(pin)).expect("a signal pin");
    Literal::new(bool_input_of_source(source.index), polarity)
}

#[test]
fn the_matrix_is_well_formed() {
    // `AndMatrixSpec::new` enforces SPEC.md §4.7's invariants — every
    // cell on its own fuse, no column carrying two sources, no ragged
    // row. Building it at all is the assertion; this states the shape
    // that came out.
    let matrix = and_matrix().expect("the measured geometry describes a valid matrix");
    assert_eq!(matrix.rows().len(), ROWS as usize);
    assert_eq!(matrix.columns(), COLUMNS);
    assert_eq!(matrix.sources().len(), 22);
}

#[test]
fn every_cell_sits_on_the_fuse_the_addressing_says() {
    // The matrix is assembled from `fuse_of`, so this checks the
    // assembly rather than the addressing — that no row was built
    // shifted, reversed, or one short.
    let matrix = and_matrix().expect("valid");
    for row in 0..ROWS {
        let cells = matrix.row_cells(ProductTermId(row)).expect("every row is present");
        assert_eq!(cells.len(), COLUMNS as usize, "row {row}");
        for (column, cell) in cells.iter().enumerate() {
            let column = u32::try_from(column).expect("column fits");
            assert_eq!(
                cell.fuse,
                G.fuse_of(ArrayCell { row, column }).expect("in range"),
                "row {row} column {column}"
            );
            // 0 is an intact link (JEDEC 3A), which is the reverse of
            // the reflex reading and inverts every term if flipped.
            assert!(!cell.connected_value, "row {row} column {column}");
            assert!(cell.disconnected_value, "row {row} column {column}");
        }
    }
}

#[test]
fn every_row_belongs_to_the_resource_that_owns_it() {
    // SPEC.md §4.7: "every product term belongs to the expected
    // resource". Rows 0 and 131 are device-wide; each block's first row
    // is that macrocell's output enable and the rest are data terms.
    let matrix = and_matrix().expect("valid");
    let mut seen = vec![None; ROWS as usize];

    seen[ASYNCHRONOUS_RESET_ROW as usize] = Some(ProductTermRole::AsynchronousReset);
    seen[SYNCHRONOUS_PRESET_ROW as usize] = Some(ProductTermRole::SynchronousPreset);
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let block = G.row_block(MacrocellIndex(index)).expect("a measured block");
        let macrocell = MacrocellId(index);
        seen[block.output_enable_row as usize] = Some(ProductTermRole::OutputEnable { macrocell });
        for row in block.data_rows.clone() {
            seen[row as usize] = Some(ProductTermRole::Data { macrocell });
        }
    }

    for row in 0..ROWS {
        let spec = matrix.row(ProductTermId(row)).expect("every row is present");
        assert_eq!(Some(spec.role), seen[row as usize], "row {row}");
    }

    // The counts the datasheet states: ten output enables, 8..16 data
    // terms each, and exactly two device-wide terms.
    let data =
        matrix.rows().iter().filter(|r| matches!(r.role, ProductTermRole::Data { .. })).count();
    let enables = matrix
        .rows()
        .iter()
        .filter(|r| matches!(r.role, ProductTermRole::OutputEnable { .. }))
        .count();
    assert_eq!(enables, 10);
    assert_eq!(data, 120, "8+10+12+14+16+16+14+12+10+8");
    assert_eq!(data + enables + 2, ROWS as usize);
}

#[test]
fn encoding_in2_reproduces_the_array_fuses_wincupl_produced() {
    // The first end-to-end agreement between deCPLD's encoder and the
    // oracle, on the same design.
    //
    // SCOPE: the AND ARRAY only, fuses 0..5808. WinCUPL's output for
    // this design also writes the architecture pair (5808..5828) and
    // the signature (5828..5892); nothing here encodes or checks those,
    // because `MacrocellSpec` and `ConfigField` do not exist yet.
    // Agreement on the whole file is a later claim than this one.
    //
    // Experiment `in2` is `PIN 2 = i0; PIN 23 = o0; o0 = i0;`. WinCUPL's
    // output blows 44–91 and 93–131, leaving fuse 92 intact and every
    // other array fuse at its erased value. That is two product terms:
    // pin 23's output-enable row (1) with no literals, and its first
    // data row (2) holding pin 2's true literal.
    //
    // Encoding those two cubes must produce exactly those fuses. This
    // is what a wrong column map, a wrong row block, a wrong addressing
    // stride, or an inverted connected-value would each break, and none
    // of them would be visible in a fuse checksum.
    let matrix = and_matrix().expect("valid");
    let mut map = erased_gal();

    let macrocell = G.macrocell_of_pin(decpld_device::PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");

    encode_cube(&mut map, &matrix, ProductTermId(block.output_enable_row), &Cube::always())
        .expect("the output enable is always on");
    let first_data_row = block.data_rows.start;
    encode_cube(
        &mut map,
        &matrix,
        ProductTermId(first_data_row),
        &Cube::new([literal(2, Polarity::True)]),
    )
    .expect("pin 2 is routable");

    // Exactly one intact fuse in 44..=131, at 92.
    let intact: Vec<u32> = (44..132).filter(|&fuse| map.get(FuseId(fuse)) == Some(false)).collect();
    assert_eq!(intact, [92], "in2 leaves exactly fuse 92 intact");
    for fuse in 44..132 {
        assert_eq!(map.get(FuseId(fuse)), Some(fuse != 92), "fuse {fuse}");
    }

    // Nothing outside those two rows was written.
    for fuse in 0..44 {
        assert!(!map.is_written(FuseId(fuse)), "row 0 fuse {fuse}");
    }
    for fuse in 132..5808 {
        assert!(!map.is_written(FuseId(fuse)), "fuse {fuse}");
    }
}

#[test]
fn encoding_global_ar_sp_reproduces_the_array_fuses_wincupl_produced() {
    // Array only, as above: this design's architecture pair reads
    // S0 = 1, S1 = 0 in the oracle's output and is not encoded here.
    //
    // Experiment `global-ar-sp` adds `o0.ar = rst` on pin 3 and
    // `o0.sp = pre` on pin 4. WinCUPL leaves fuses 8, 92 and 5776
    // intact, blowing 0–7, 9–91, 93–131, 5764–5775 and 5777–5807.
    //
    // Rows 0 and 131 are the device-wide control terms, and they are the
    // furthest-apart rows in the array — the pair a stride error moves
    // most.
    let matrix = and_matrix().expect("valid");
    let mut map = erased_gal();

    let macrocell = G.macrocell_of_pin(decpld_device::PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    encode_cube(&mut map, &matrix, ProductTermId(block.output_enable_row), &Cube::always())
        .expect("always on");
    encode_cube(
        &mut map,
        &matrix,
        ProductTermId(block.data_rows.start),
        &Cube::new([literal(2, Polarity::True)]),
    )
    .expect("pin 2");
    encode_cube(
        &mut map,
        &matrix,
        ProductTermId(ASYNCHRONOUS_RESET_ROW),
        &Cube::new([literal(3, Polarity::True)]),
    )
    .expect("pin 3");
    encode_cube(
        &mut map,
        &matrix,
        ProductTermId(SYNCHRONOUS_PRESET_ROW),
        &Cube::new([literal(4, Polarity::True)]),
    )
    .expect("pin 4");

    let intact: Vec<u32> = (0..5808).filter(|&fuse| map.get(FuseId(fuse)) == Some(false)).collect();
    assert_eq!(intact, [8, 92, 5776]);

    let written_extent: Vec<u32> = (0..5808).filter(|&fuse| map.is_written(FuseId(fuse))).collect();
    let expected: Vec<u32> = (0..132).chain(5764..5808).collect();
    assert_eq!(written_extent, expected, "four terms, 176 fuses");
}

#[test]
fn an_io_pin_used_as_an_input_encodes_through_its_feedback_column() {
    // Experiment `ioin22`: pin 22 undriven, feeding pin 23. WinCUPL
    // leaves fuse 94 intact — row 2, column 6, pin 22's feedback
    // column. The encoder must reach the same fuse from `source_of_pin`,
    // which is what makes an undriven I/O pin usable as a literal.
    let matrix = and_matrix().expect("valid");
    let mut map = erased_gal();
    let block =
        G.row_block(G.macrocell_of_pin(decpld_device::PinNumber(23)).expect("I/O")).expect("block");

    encode_cube(&mut map, &matrix, ProductTermId(block.output_enable_row), &Cube::always())
        .expect("always on");
    encode_cube(
        &mut map,
        &matrix,
        ProductTermId(block.data_rows.start),
        &Cube::new([literal(22, Polarity::True)]),
    )
    .expect("pin 22 feedback is routable");

    let intact: Vec<u32> = (44..132).filter(|&fuse| map.get(FuseId(fuse)) == Some(false)).collect();
    assert_eq!(intact, [94], "ioin22 leaves exactly fuse 94 intact");
}

#[test]
fn every_row_round_trips_a_representative_cube() {
    // SPEC.md §4.7: "every legal configuration round-trips through
    // encode/decode". Sampled per row rather than per cube — 132 rows
    // times every cube is not a test, it is a build — but every row is
    // covered, because a row whose cells were assembled wrong is
    // exactly what a spot check on row 2 would miss.
    let matrix = and_matrix().expect("valid");
    for row in 0..ROWS {
        let mut map = erased_gal();
        // A cube touching both ends of the column range and both
        // senses, so a reversed or truncated row cannot pass.
        let cube = Cube::new([
            literal(1, Polarity::True),
            literal(13, Polarity::Complement),
            literal(14, Polarity::True),
        ]);
        encode_cube(&mut map, &matrix, ProductTermId(row), &cube)
            .unwrap_or_else(|e| panic!("row {row}: {e:?}"));
        assert_eq!(
            decode_row(&map, &matrix, ProductTermId(row)),
            Ok(cube.canonical()),
            "row {row}"
        );
    }
}

#[test]
fn every_source_round_trips_at_both_polarities() {
    // The other axis: one row, every literal the device can express.
    // Together with the per-row test above, every cell of the matrix is
    // reached by some case.
    let matrix = and_matrix().expect("valid");
    for source in G.sources() {
        for polarity in [Polarity::True, Polarity::Complement] {
            let cube = Cube::new([Literal::new(bool_input_of_source(source.index), polarity)]);
            let mut map = erased_gal();
            encode_cube(&mut map, &matrix, ProductTermId(2), &cube)
                .unwrap_or_else(|e| panic!("source {} {polarity:?}: {e:?}", source.index));
            assert_eq!(
                decode_row(&map, &matrix, ProductTermId(2)),
                Ok(cube.canonical()),
                "source {} {polarity:?}",
                source.index
            );

            // And it landed on the column the geometry measured.
            let column = if polarity == Polarity::True {
                source.true_column()
            } else {
                source.complement_column()
            };
            let fuse = G.fuse_of(ArrayCell { row: 2, column }).expect("in range");
            assert_eq!(map.get(fuse), Some(false), "source {} {polarity:?}", source.index);
        }
    }
}

#[test]
fn the_package_and_the_matrix_name_the_same_source_for_a_pin() {
    // The package gives an input pin an `InputResourceId`; the matrix
    // gives it a `BoolInputId`. Both are documented as "the array
    // source index" — one number, not two that must be kept equal.
    //
    // Nothing checked that. Renumbering the matrix's inputs uniformly
    // passed the whole suite, because every test reached them through
    // the same helper: consistent with itself, and silently disagreeing
    // with the package map a fitter would route from.
    let matrix = and_matrix().expect("valid");
    let package = dip24();

    let mut checked = 0;
    for (pin, role) in package.pins() {
        if let Some(input) = role.dedicated_input() {
            // Constructed from the PACKAGE's number, not through
            // `bool_input_of_source`. Routing both sides through the
            // same helper is what let a uniform renumbering of the
            // matrix pass while silently disagreeing with the package.
            let source = matrix
                .source(BoolInputId(u32::from(input.0)))
                .unwrap_or_else(|| panic!("pin {} input {} has no matrix source", pin.0, input.0));
            assert_eq!(
                source.physical_source,
                decpld_device::PhysicalSignalSource::Pin(pin),
                "pin {} input resource {}",
                pin.0,
                input.0
            );
            checked += 1;
        }
        if let Some(pad) = role.pad() {
            // The other direction: a pad's macrocell must be the one the
            // matrix says feeds that column back.
            let feedback = G.source_of_pin(pin).expect("an I/O pin reaches the array");
            let source = matrix
                .source(BoolInputId(u32::from(feedback.index)))
                .unwrap_or_else(|| panic!("pin {} has no matrix source", pin.0));
            assert_eq!(
                source.physical_source,
                decpld_device::PhysicalSignalSource::Feedback(MacrocellId(pad.0)),
                "pin {}",
                pin.0
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 22, "twelve input pins and ten pads");
}
