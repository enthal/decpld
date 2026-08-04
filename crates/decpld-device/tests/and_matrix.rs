//! Cube encoding and row decoding over a small synthetic matrix.
//!
//! SPEC.md §4.4 and §12.3. The device here is invented — two product
//! terms, three literal sources — because the point is the *rules*, and
//! a real part would drag in 5808 fuses of detail that hide which rule
//! each case is testing. The ATF22V10's own matrix is checked against
//! measurements in `decpld-atf22v10`.

use decpld_device::{
    AndMatrixSpec, EncodeError, FuseId, FuseMap, FuseMutability, FuseRegion, FuseRegions,
    LiteralSource, MacrocellId, MatrixCellSpec, MatrixColumn, MatrixError, PhysicalSignalSource,
    PinNumber, ProductTermId, ProductTermRole, ProductTermSpec, decode_row, encode_cube,
};
use decpld_logic::{BoolInputId, Cube, Literal, Polarity};

const A: BoolInputId = BoolInputId(0);
const B: BoolInputId = BoolInputId(1);
const C: BoolInputId = BoolInputId(2);

const ROWS: u32 = 2;
const COLUMNS: u32 = 6;

fn t(input: BoolInputId) -> Literal {
    Literal::new(input, Polarity::True)
}
fn c(input: BoolInputId) -> Literal {
    Literal::new(input, Polarity::Complement)
}

/// A 2 × 6 array: three sources, each with a true and a complement
/// column, laid out row-major exactly as a GAL is.
fn matrix() -> AndMatrixSpec {
    let rows = vec![
        ProductTermSpec {
            id: ProductTermId(0),
            role: ProductTermRole::Data { macrocell: MacrocellId(0) },
        },
        ProductTermSpec {
            id: ProductTermId(1),
            role: ProductTermRole::OutputEnable { macrocell: MacrocellId(0) },
        },
    ];
    let sources = vec![
        LiteralSource {
            id: A,
            true_column: MatrixColumn(0),
            complement_column: MatrixColumn(1),
            physical_source: PhysicalSignalSource::Pin(PinNumber(1)),
        },
        LiteralSource {
            id: B,
            true_column: MatrixColumn(2),
            complement_column: MatrixColumn(3),
            physical_source: PhysicalSignalSource::Pin(PinNumber(2)),
        },
        LiteralSource {
            id: C,
            true_column: MatrixColumn(4),
            complement_column: MatrixColumn(5),
            physical_source: PhysicalSignalSource::Feedback(MacrocellId(0)),
        },
    ];
    let cells = (0..ROWS)
        .map(|row| {
            (0..COLUMNS)
                .map(|column| MatrixCellSpec {
                    fuse: FuseId(row * COLUMNS + column),
                    // 0 is an intact link on this family, so connected
                    // is `false` and an erased device reads all-blown.
                    connected_value: false,
                    disconnected_value: true,
                })
                .collect()
        })
        .collect();
    AndMatrixSpec::new(rows, sources, cells).expect("a valid synthetic matrix")
}

fn erased() -> FuseMap {
    let regions = FuseRegions::new(
        ROWS * COLUMNS,
        vec![FuseRegion {
            name: "array",
            range: 0..ROWS * COLUMNS,
            erased_value: true,
            mutability: FuseMutability::Programmable,
        }],
    )
    .expect("valid");
    FuseMap::erased(regions)
}

#[test]
fn a_cube_encodes_and_decodes_back_to_itself() {
    let matrix = matrix();
    let mut map = erased();
    let cube = Cube::new([t(A), c(C)]);

    encode_cube(&mut map, &matrix, ProductTermId(0), &cube).expect("encodable");

    // A's true column (0) and C's complement column (5) are intact;
    // everything else in row 0 is blown.
    assert_eq!(map.get(FuseId(0)), Some(false), "A true connected");
    assert_eq!(map.get(FuseId(1)), Some(true), "A complement disconnected");
    assert_eq!(map.get(FuseId(5)), Some(false), "C complement connected");
    for fuse in [2, 3, 4] {
        assert_eq!(map.get(FuseId(fuse)), Some(true), "fuse {fuse}");
    }

    let decoded = decode_row(&map, &matrix, ProductTermId(0)).expect("decodable");
    assert_eq!(decoded, cube.canonical());
}

#[test]
fn encoding_one_row_leaves_every_other_row_alone() {
    // A product term is 44 fuses wide on the real part and a row's
    // encoder walks a contiguous range; running one fuse past the end
    // would silently rewrite the next term's first literal.
    let matrix = matrix();
    let mut map = erased();
    encode_cube(&mut map, &matrix, ProductTermId(0), &Cube::new([t(A)])).expect("encodable");

    for fuse in COLUMNS..ROWS * COLUMNS {
        assert_eq!(map.get(FuseId(fuse)), Some(true), "row 1 fuse {fuse} was touched");
        assert!(!map.is_written(FuseId(fuse)), "row 1 fuse {fuse} was claimed");
    }
}

#[test]
fn the_empty_cube_blows_every_link_in_its_row() {
    // The empty AND is constantly true, which on this hardware is a row
    // with no connections at all. Getting it backwards — leaving the
    // row intact — produces a constantly *false* term, and an output
    // enable encoded that way disables a pin the design meant to drive.
    let matrix = matrix();
    let mut map = erased();
    encode_cube(&mut map, &matrix, ProductTermId(1), &Cube::always()).expect("encodable");

    // Asserting the bits alone would pass on an untouched map, since the
    // erased value already matches; the claim is that the encoder wrote
    // them.
    for fuse in COLUMNS..ROWS * COLUMNS {
        assert_eq!(map.get(FuseId(fuse)), Some(true), "fuse {fuse}");
        assert!(map.is_written(FuseId(fuse)), "fuse {fuse} was never written");
    }
    assert_eq!(decode_row(&map, &matrix, ProductTermId(1)), Ok(Cube::always()));
}

#[test]
fn a_contradictory_cube_is_refused_and_writes_nothing() {
    // `a & !a` asks the array to connect an input's true and complement
    // columns in the same row. That is constantly false, and on this
    // hardware it is indistinguishable from a row nobody programmed —
    // so a design meaning "never" and a design nobody wrote produce the
    // same fuses. Refusing is the only honest answer.
    let matrix = matrix();
    let mut map = erased();
    let before: Vec<bool> = map.iter().collect();

    let error = encode_cube(&mut map, &matrix, ProductTermId(0), &Cube::new([t(A), t(B), c(A)]))
        .expect_err("a contradiction must be refused");
    assert_eq!(error, EncodeError::ContradictoryCube { row: ProductTermId(0), input: A });

    assert_eq!(map.iter().collect::<Vec<_>>(), before, "a refused encode must write nothing");
    assert!((0..ROWS * COLUMNS).all(|fuse| !map.is_written(FuseId(fuse))));
}

#[test]
fn a_literal_the_matrix_cannot_route_is_refused_and_writes_nothing() {
    let matrix = matrix();
    let mut map = erased();
    let before: Vec<bool> = map.iter().collect();
    let stranger = BoolInputId(99);

    let error = encode_cube(&mut map, &matrix, ProductTermId(0), &Cube::new([t(A), t(stranger)]))
        .expect_err("an unroutable literal must be refused");
    assert_eq!(error, EncodeError::UnroutableLiteral { row: ProductTermId(0), input: stranger });
    assert_eq!(map.iter().collect::<Vec<_>>(), before, "a refused encode must write nothing");
    assert!(
        (0..ROWS * COLUMNS).all(|fuse| !map.is_written(FuseId(fuse))),
        "a refused encode must claim nothing either -- the erased value and the \
         disconnected value are both `true`, so comparing bits alone cannot tell"
    );
}

#[test]
fn an_unknown_row_is_refused() {
    let matrix = matrix();
    let mut map = erased();
    let error = encode_cube(&mut map, &matrix, ProductTermId(7), &Cube::always())
        .expect_err("there is no row 7");
    assert_eq!(error, EncodeError::UnknownRow { row: ProductTermId(7) });
    assert!(decode_row(&map, &matrix, ProductTermId(7)).is_err());
}

#[test]
fn a_row_with_both_senses_connected_decodes_as_the_contradiction_it_is() {
    // Decoding must be total: `jed inspect` reads files this compiler
    // did not write, including ones a different tool produced. A row
    // holding an input at both polarities is physically meaningful —
    // constantly false — so it decodes to a cube that *is* a
    // contradiction rather than to an error or, worse, to one literal
    // with the other quietly dropped.
    let matrix = matrix();
    let mut map = erased();
    // Columns 0 and 1 are A's true and complement senses; connecting
    // both is what a row nobody sanity-checked can physically hold.
    // Written in one atomic batch, because writing a fuse twice with
    // different values is itself refused.
    map.set_all((0..COLUMNS).map(|fuse| (FuseId(fuse), fuse > 1))).expect("writable");
    map.set(FuseId(1), false).expect("writable");

    let decoded = decode_row(&map, &matrix, ProductTermId(0)).expect("decodable");
    assert_eq!(decoded, Cube::new([t(A), c(A)]));
    assert_eq!(decoded.contradiction(), Some(A));
}

#[test]
fn encoding_the_same_row_twice_with_different_cubes_is_a_write_conflict() {
    // `FuseMap` tracks writes so two encoders fighting over one fuse is
    // detected rather than averaged. Re-encoding a row is exactly that
    // fight, and the second encode must not win silently.
    let matrix = matrix();
    let mut map = erased();
    encode_cube(&mut map, &matrix, ProductTermId(0), &Cube::new([t(A)])).expect("first");
    let error = encode_cube(&mut map, &matrix, ProductTermId(0), &Cube::new([t(B)]))
        .expect_err("the second encode must not silently win");
    assert!(matches!(error, EncodeError::Fuse { .. }), "got {error:?}");

    // And the first cube is still there, whole. A refused second encode
    // that had already committed part of its row would leave a design
    // that is neither of the two.
    assert_eq!(decode_row(&map, &matrix, ProductTermId(0)), Ok(Cube::new([t(A)]).canonical()));
}

#[test]
fn encoding_the_same_row_twice_with_the_same_cube_is_fine() {
    // Two encoders agreeing is not a disagreement, and refusing it
    // would make the order they run in observable.
    let matrix = matrix();
    let mut map = erased();
    let cube = Cube::new([t(A), c(B)]);
    encode_cube(&mut map, &matrix, ProductTermId(0), &cube).expect("first");
    encode_cube(&mut map, &matrix, ProductTermId(0), &cube).expect("second, identical");
    assert_eq!(decode_row(&map, &matrix, ProductTermId(0)), Ok(cube.canonical()));
}

#[test]
fn every_cube_over_this_matrix_round_trips() {
    // Exhaustive over all 3^3 assignments of each input to
    // absent / true / complement. SPEC.md §4.7 requires that every
    // legal configuration round-trips; for a matrix this small that is
    // a statement rather than a sample.
    let matrix = matrix();
    let inputs = [A, B, C];
    for code in 0..27u32 {
        let mut literals = Vec::new();
        let mut digits = code;
        for input in inputs {
            match digits % 3 {
                1 => literals.push(t(input)),
                2 => literals.push(c(input)),
                _ => {}
            }
            digits /= 3;
        }
        let cube = Cube::new(literals);
        let mut map = erased();
        encode_cube(&mut map, &matrix, ProductTermId(0), &cube)
            .unwrap_or_else(|e| panic!("{cube:?}: {e:?}"));
        assert_eq!(decode_row(&map, &matrix, ProductTermId(0)), Ok(cube.canonical()), "{cube:?}");
    }
}

#[test]
fn a_matrix_with_no_sources_or_an_unreachable_column_is_refused() {
    // A column no source drives is dead silicon: every row blows it and
    // no decode reads it, so a literal routed there would vanish
    // silently.
    let good = matrix();
    assert_eq!(
        AndMatrixSpec::new(good.rows().to_vec(), Vec::new(), cells_of(&good)),
        Err(MatrixError::NoSources)
    );

    let mut sources = good.sources().to_vec();
    sources.pop();
    assert_eq!(
        AndMatrixSpec::new(good.rows().to_vec(), sources, cells_of(&good)),
        Err(MatrixError::ColumnHasNoSource { column: MatrixColumn(4) })
    );
}

#[test]
fn a_matrix_whose_cells_share_a_fuse_is_refused() {
    // SPEC.md §4.7: "matrix cells map one-to-one to fuses". Two cells
    // on one fuse means connecting one literal silently connects
    // another, which no amount of care at the call site can fix.
    let good = matrix();
    let mut cells: Vec<Vec<MatrixCellSpec>> =
        (0..ROWS).map(|row| good.row_cells(ProductTermId(row)).unwrap().to_vec()).collect();
    cells[1][0].fuse = FuseId(0);

    let error =
        AndMatrixSpec::new(good.rows().to_vec(), good.sources().to_vec(), cells).expect_err("dup");
    assert_eq!(error, MatrixError::FuseUsedTwice { fuse: FuseId(0) });
}

#[test]
fn a_matrix_with_two_sources_on_one_column_is_refused() {
    let good = matrix();
    let mut sources = good.sources().to_vec();
    sources[1].true_column = MatrixColumn(0);
    let error = AndMatrixSpec::new(good.rows().to_vec(), sources, cells_of(&good))
        .expect_err("two sources on one column");
    assert_eq!(error, MatrixError::ColumnUsedTwice { column: MatrixColumn(0) });
}

#[test]
fn a_source_whose_column_is_outside_the_array_is_refused() {
    let good = matrix();
    let mut sources = good.sources().to_vec();
    sources[2].complement_column = MatrixColumn(COLUMNS);
    let error = AndMatrixSpec::new(good.rows().to_vec(), sources, cells_of(&good))
        .expect_err("column past the end");
    assert_eq!(
        error,
        MatrixError::ColumnOutOfRange { column: MatrixColumn(COLUMNS), columns: COLUMNS }
    );
}

#[test]
fn a_source_using_one_column_for_both_senses_is_refused() {
    // True and complement on the same column would make `a` and `!a`
    // the same fuse, so a cube could never distinguish them and
    // `contradiction` would never fire.
    let good = matrix();
    let mut sources = good.sources().to_vec();
    sources[0].complement_column = sources[0].true_column;
    let error = AndMatrixSpec::new(good.rows().to_vec(), sources, cells_of(&good))
        .expect_err("one column for both senses");
    assert_eq!(error, MatrixError::SenseColumnsCollide { input: A, column: MatrixColumn(0) });
}

#[test]
fn a_ragged_or_mismatched_cell_grid_is_refused() {
    let good = matrix();

    let mut short = cells_of(&good);
    short.pop();
    assert!(AndMatrixSpec::new(good.rows().to_vec(), good.sources().to_vec(), short).is_err());

    let mut ragged = cells_of(&good);
    ragged[1].pop();
    assert_eq!(
        AndMatrixSpec::new(good.rows().to_vec(), good.sources().to_vec(), ragged),
        Err(MatrixError::RaggedRow { row: ProductTermId(1), cells: COLUMNS - 1, columns: COLUMNS })
    );
}

#[test]
fn a_cell_whose_two_states_are_the_same_value_is_refused() {
    // Connected and disconnected must differ, or the fuse says nothing
    // and every decode of it is a coin flip dressed as a measurement.
    let good = matrix();
    let mut cells = cells_of(&good);
    cells[0][0].disconnected_value = cells[0][0].connected_value;
    let error = AndMatrixSpec::new(good.rows().to_vec(), good.sources().to_vec(), cells)
        .expect_err("a cell that cannot express two states");
    assert_eq!(error, MatrixError::CellCannotDistinguish { fuse: FuseId(0) });
}

#[test]
fn duplicate_row_ids_are_refused() {
    let good = matrix();
    let rows = vec![good.rows()[0], good.rows()[0]];
    let error = AndMatrixSpec::new(rows, good.sources().to_vec(), cells_of(&good))
        .expect_err("two rows cannot share an id");
    assert_eq!(error, MatrixError::RowIdUsedTwice { row: ProductTermId(0) });
}

#[test]
fn duplicate_source_ids_are_refused() {
    let good = matrix();
    let mut sources = good.sources().to_vec();
    sources[1].id = A;
    let error = AndMatrixSpec::new(good.rows().to_vec(), sources, cells_of(&good))
        .expect_err("two sources cannot share an input");
    assert_eq!(error, MatrixError::SourceIdUsedTwice { input: A });
}

fn cells_of(matrix: &AndMatrixSpec) -> Vec<Vec<MatrixCellSpec>> {
    matrix.rows().iter().map(|row| matrix.row_cells(row.id).unwrap().to_vec()).collect()
}
