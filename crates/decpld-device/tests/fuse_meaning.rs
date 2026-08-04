//! What a fuse *means*, for SPEC.md §7.5's delta classification.
//!
//! `decpld-jedec` reports that fuse 52 changed; it cannot say what fuse
//! 52 does, and must not learn. This is the device layer answering —
//! and it answers for **any** target, because everything it consults is
//! already in the matrix, the macrocells, and the regions. A target
//! that supplies those gets classification without writing any.
//!
//! The device here is invented, two macrocells and three sources, so
//! each case shows which rule it tests.

use decpld_device::{
    AndMatrixSpec, ClockResourceId, ConfigField, ConfigFieldId, FeedbackSource, FuseId,
    FuseMeaning, FuseMutability, FuseRegion, FuseRegions, LiteralSource, MacrocellId,
    MacrocellMode, MacrocellSpec, MatrixCellSpec, MatrixColumn, OutputPolarity, PadId,
    PhysicalSignalSource, PinNumber, ProductTermId, ProductTermRole, ProductTermSpec,
    classify_fuse,
};
use decpld_logic::{BoolInputId, Polarity};

const A: BoolInputId = BoolInputId(0);
const B: BoolInputId = BoolInputId(1);
const C: BoolInputId = BoolInputId(2);

const ROWS: u32 = 4;
const COLUMNS: u32 = 6;
const ARRAY: u32 = ROWS * COLUMNS; // 24
const CONFIG: u32 = 4; // 24..28, two fuses per macrocell
const SIGNATURE: u32 = 16; // 28..44
const SECURITY: u32 = 1; // 44

/// Row 0 is macrocell 0's enable, row 1 its data; row 2 is macrocell 1's
/// data, row 3 the device-wide reset.
fn matrix() -> AndMatrixSpec {
    let rows = vec![
        ProductTermSpec {
            id: ProductTermId(0),
            role: ProductTermRole::OutputEnable { macrocell: MacrocellId(0) },
        },
        ProductTermSpec {
            id: ProductTermId(1),
            role: ProductTermRole::Data { macrocell: MacrocellId(0) },
        },
        ProductTermSpec {
            id: ProductTermId(2),
            role: ProductTermRole::Data { macrocell: MacrocellId(1) },
        },
        ProductTermSpec { id: ProductTermId(3), role: ProductTermRole::AsynchronousReset },
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
            physical_source: PhysicalSignalSource::Feedback(MacrocellId(1)),
        },
    ];
    let cells = (0..ROWS)
        .map(|row| {
            (0..COLUMNS)
                .map(|column| MatrixCellSpec {
                    fuse: FuseId(row * COLUMNS + column),
                    connected_value: false,
                    disconnected_value: true,
                })
                .collect()
        })
        .collect();
    AndMatrixSpec::new(rows, sources, cells).expect("a valid matrix")
}

fn specs() -> Vec<MacrocellSpec> {
    vec![spec(0, vec![1], Some(0), ARRAY), spec(1, vec![2], None, ARRAY + 2)]
}

fn spec(id: u8, data: Vec<u32>, oe: Option<u32>, config_base: u32) -> MacrocellSpec {
    let polarity = ConfigField::new(
        ConfigFieldId(u16::from(id) * 2),
        "polarity",
        vec![FuseId(config_base)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false])],
    )
    .expect("a valid field");
    let mode = ConfigField::new(
        ConfigFieldId(u16::from(id) * 2 + 1),
        "mode",
        vec![FuseId(config_base + 1)],
        [(MacrocellMode::Combinational, vec![true]), (MacrocellMode::Registered, vec![false])],
    )
    .expect("a valid field");

    MacrocellSpec {
        id: MacrocellId(id),
        pad: Some(PadId(id)),
        data_terms: data.into_iter().map(ProductTermId).collect(),
        oe_term: oe.map(ProductTermId),
        supports_registered: true,
        supports_combinational: true,
        supports_input_only: true,
        feedback_modes: vec![FeedbackSource::Pin],
        mode_field: Some(mode),
        polarity_field: Some(polarity),
        fixed_clock: Some(ClockResourceId(0)),
    }
}

fn regions() -> FuseRegions {
    FuseRegions::new(
        ARRAY + CONFIG + SIGNATURE + SECURITY,
        vec![
            FuseRegion {
                name: "array",
                range: 0..ARRAY,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "architecture",
                range: ARRAY..ARRAY + CONFIG,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "user-signature",
                range: ARRAY + CONFIG..ARRAY + CONFIG + SIGNATURE,
                erased_value: true,
                mutability: FuseMutability::UserSignature,
            },
            FuseRegion {
                name: "security",
                range: ARRAY + CONFIG + SIGNATURE..ARRAY + CONFIG + SIGNATURE + SECURITY,
                erased_value: false,
                mutability: FuseMutability::Security,
            },
        ],
    )
    .expect("a valid layout")
}

fn meaning(fuse: u32) -> FuseMeaning {
    classify_fuse(FuseId(fuse), &matrix(), &specs(), &regions())
}

#[test]
fn a_matrix_fuse_names_its_row_role_signal_and_sense() {
    // The classification that matters: "fuse 8 changed" tells a person
    // reverse-engineering a device nothing, while "macrocell 0's data
    // term in row 1 gained pin 2" is the finding itself.
    //
    // Fuse 8 = row 1, column 2 = source B true. Row 1 is macrocell 0's
    // data term.
    assert_eq!(
        meaning(8),
        FuseMeaning::MatrixCell {
            row: ProductTermId(1),
            role: ProductTermRole::Data { macrocell: MacrocellId(0) },
            input: B,
            polarity: Polarity::True,
            source: PhysicalSignalSource::Pin(PinNumber(2)),
        }
    );

    // The column one higher is the same signal complemented — the
    // distinction a differential experiment lives or dies on.
    assert_eq!(
        meaning(9),
        FuseMeaning::MatrixCell {
            row: ProductTermId(1),
            role: ProductTermRole::Data { macrocell: MacrocellId(0) },
            input: B,
            polarity: Polarity::Complement,
            source: PhysicalSignalSource::Pin(PinNumber(2)),
        }
    );
}

#[test]
fn an_output_enable_fuse_is_distinguished_from_a_data_fuse() {
    // Same column, different row. An oracle diff that reported both as
    // "matrix" would lose the one fact a `.oe` experiment exists to
    // establish.
    assert!(matches!(
        meaning(4),
        FuseMeaning::MatrixCell { role: ProductTermRole::OutputEnable { .. }, .. }
    ));
    assert!(matches!(
        meaning(10),
        FuseMeaning::MatrixCell { role: ProductTermRole::Data { .. }, .. }
    ));
}

#[test]
fn a_device_wide_row_is_named_by_its_resource_not_its_macrocell() {
    // Row 3 belongs to no macrocell.
    assert_eq!(
        meaning(18),
        FuseMeaning::MatrixCell {
            row: ProductTermId(3),
            role: ProductTermRole::AsynchronousReset,
            input: A,
            polarity: Polarity::True,
            source: PhysicalSignalSource::Pin(PinNumber(1)),
        }
    );
}

#[test]
fn a_feedback_column_reports_the_macrocell_it_comes_from() {
    // Column 4 is source C, which is macrocell 1's feedback. A reader
    // needs to know it is feedback rather than a pin, because that is
    // what makes a column map re-derivable.
    assert_eq!(
        meaning(4 + COLUMNS),
        FuseMeaning::MatrixCell {
            row: ProductTermId(1),
            role: ProductTermRole::Data { macrocell: MacrocellId(0) },
            input: C,
            polarity: Polarity::True,
            source: PhysicalSignalSource::Feedback(MacrocellId(1)),
        }
    );
}

#[test]
fn a_configuration_fuse_names_its_macrocell_and_its_field() {
    // "Fuse 24 changed" versus "macrocell 0's polarity changed". The
    // second is what a `arch-*` experiment reports.
    assert_eq!(
        meaning(ARRAY),
        FuseMeaning::ConfigField {
            macrocell: Some(MacrocellId(0)),
            field: "polarity",
            id: ConfigFieldId(0),
            bit: 0,
        }
    );
    assert_eq!(
        meaning(ARRAY + 1),
        FuseMeaning::ConfigField {
            macrocell: Some(MacrocellId(0)),
            field: "mode",
            id: ConfigFieldId(1),
            bit: 0,
        }
    );
    assert_eq!(
        meaning(ARRAY + 2),
        FuseMeaning::ConfigField {
            macrocell: Some(MacrocellId(1)),
            field: "polarity",
            id: ConfigFieldId(2),
            bit: 0,
        }
    );
}

#[test]
fn a_signature_fuse_names_its_byte_and_bit() {
    // A signature delta is almost never interesting on its own — it is
    // the design's name leaking into the fuse map — so it must be
    // recognisable at a glance and not mistaken for logic.
    assert_eq!(
        meaning(ARRAY + CONFIG),
        FuseMeaning::UserSignature { region: "user-signature", byte: 0, bit: 0 }
    );
    assert_eq!(
        meaning(ARRAY + CONFIG + 7),
        FuseMeaning::UserSignature { region: "user-signature", byte: 0, bit: 7 }
    );
    assert_eq!(
        meaning(ARRAY + CONFIG + 8),
        FuseMeaning::UserSignature { region: "user-signature", byte: 1, bit: 0 }
    );
}

#[test]
fn the_security_fuse_is_called_out_by_name() {
    // The one delta that is irreversible on hardware.
    assert_eq!(meaning(ARRAY + CONFIG + SIGNATURE), FuseMeaning::SecurityFuse);
}

#[test]
fn a_fuse_no_model_accounts_for_is_unknown_rather_than_guessed() {
    // Past the end of the device. `jed inspect` and `oracle diff` both
    // read files this compiler did not write, and a classification that
    // invented a meaning for an address the model does not have would
    // be exactly the unevidenced guess this project forbids.
    assert_eq!(meaning(999), FuseMeaning::Unknown { fuse: FuseId(999) });
}

#[test]
fn every_fuse_of_the_device_classifies_as_something() {
    // SPEC.md §4.7 requires every fuse to be classified. This is that
    // invariant read through the classifier: no fuse inside the map may
    // come back `Unknown`, so a region added without a matrix, field,
    // or mutability to explain it is caught here rather than in a
    // report a user is reading.
    for fuse in 0..regions().count() {
        assert!(
            !matches!(meaning(fuse), FuseMeaning::Unknown { .. }),
            "fuse {fuse} has no meaning"
        );
    }
}

#[test]
fn a_matrix_fuse_is_never_reported_as_something_else() {
    // Classification order is not arbitrary: the matrix is consulted
    // first because a region can only say "programmable", while the
    // matrix knows which literal a cell carries. A device whose regions
    // called the whole array "architecture" would still classify every
    // array fuse as a matrix cell.
    for fuse in 0..ARRAY {
        assert!(
            matches!(meaning(fuse), FuseMeaning::MatrixCell { .. }),
            "fuse {fuse} is an array cell"
        );
    }
    for fuse in ARRAY..ARRAY + CONFIG {
        assert!(
            matches!(meaning(fuse), FuseMeaning::ConfigField { .. }),
            "fuse {fuse} is a configuration bit"
        );
    }
}

#[test]
fn categories_separate_the_things_an_experiment_varies_one_at_a_time() {
    // SPEC.md §7.5 asks for a classification, which is coarser than the
    // full description and is what makes a batch of deltas readable.
    // The distinctions that matter are the ones a differential
    // experiment is designed around: a data term is not an enable term,
    // and polarity is not mode.
    assert_eq!(meaning(8).category(), "matrix-data");
    assert_eq!(meaning(4).category(), "matrix-output-enable");
    assert_eq!(meaning(18).category(), "matrix-asynchronous-reset");
    assert_eq!(meaning(ARRAY).category(), "polarity");
    assert_eq!(meaning(ARRAY + 1).category(), "mode");
    assert_eq!(meaning(ARRAY + CONFIG).category(), "user-signature");
    assert_eq!(meaning(ARRAY + CONFIG + SIGNATURE).category(), "security");
    assert_eq!(meaning(999).category(), "unknown");
}

#[test]
fn a_description_names_the_resource_the_signal_and_the_sense() {
    // The whole value of `oracle diff --device` over `jed diff`: a
    // person reverse-engineering a fuse map needs the sentence, not the
    // address.
    assert_eq!(meaning(9).to_string(), "macrocell 0 data term in product term 1: literal !pin 2");
    assert_eq!(
        meaning(4).to_string(),
        "macrocell 0 output-enable term in product term 0: literal macrocell 1 feedback"
    );
    assert_eq!(meaning(ARRAY + 1).to_string(), "macrocell 0 mode bit 0");
    assert_eq!(meaning(ARRAY + CONFIG + 9).to_string(), "user-signature byte 1 bit 1");
    assert_eq!(meaning(ARRAY + CONFIG + SIGNATURE).to_string(), "the security fuse");
    assert_eq!(meaning(999).to_string(), "fuse 999, which this device does not have");
}

#[test]
fn a_programmable_fuse_no_matrix_cell_or_field_claims_is_other() {
    // The real case is the ATF22V10C's power-down fuse: programmable,
    // inside `QF`, and claimed by no matrix cell and no configuration
    // field. It must classify as something rather than fall through to
    // `Unknown`, which is reserved for addresses outside the model.
    let regions = FuseRegions::new(
        ARRAY + CONFIG + 1,
        vec![
            FuseRegion {
                name: "array",
                range: 0..ARRAY,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "architecture",
                range: ARRAY..ARRAY + CONFIG,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "power-down",
                range: ARRAY + CONFIG..ARRAY + CONFIG + 1,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
        ],
    )
    .expect("a valid layout");

    let meaning = classify_fuse(FuseId(ARRAY + CONFIG), &matrix(), &specs(), &regions);
    assert_eq!(
        meaning,
        FuseMeaning::Other { region: "power-down", mutability: FuseMutability::Programmable }
    );
    assert_eq!(meaning.category(), "other");
    assert_eq!(meaning.to_string(), "region `power-down`");
}

#[test]
fn a_fuse_claimed_by_both_a_matrix_cell_and_a_field_reads_as_the_matrix_cell() {
    // The ordering `classify_fuse` documents, pinned. A region can only
    // say "programmable"; the matrix knows which literal of which
    // product term a cell carries. Nothing forbids a target from
    // pointing a configuration field at an array fuse, and the finer
    // answer has to win — a device model that did so is broken, but a
    // classifier that reported "polarity bit" for an array cell would
    // hide which.
    let mut specs = specs();
    specs[0].polarity_field = Some(
        ConfigField::new(
            ConfigFieldId(99),
            "polarity",
            vec![FuseId(8)], // row 1, column 2 — an array cell
            [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false])],
        )
        .expect("a valid field"),
    );

    let meaning = classify_fuse(FuseId(8), &matrix(), &specs, &regions());
    assert!(
        matches!(meaning, FuseMeaning::MatrixCell { .. }),
        "the matrix is consulted first, got {meaning:?}"
    );
}
