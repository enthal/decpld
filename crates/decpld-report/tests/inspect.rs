//! Rendering a physical design as an inspection report. SPEC.md §6.3.
//!
//! The device here is invented — two macrocells, four literal sources —
//! because the point is the *rendering rules*, and a real part would
//! drag in 5808 fuses of detail that hide which rule each case tests.
//! The ATF22V10's own report is checked end-to-end in `decpld-cli`.

use decpld_device::{
    AndMatrixSpec, ClockResourceId, FeedbackSource, FuseMap, FuseMutability, FuseRegion,
    FuseRegions, InputResourceId, LiteralSource, MacrocellConfig, MacrocellId, MacrocellMode,
    MacrocellSpec, MatrixCellSpec, MatrixColumn, OutputPolarity, PackageId, PackagePin,
    PackageSpec, PadId, PhysicalDesign, PhysicalSignalSource, PinNumber, PlacedCube, PowerRail,
    ProductTermId, ProductTermRole, ProductTermSpec,
};
use decpld_logic::{BoolInputId, Cube, Literal, Polarity};
use decpld_report::{InspectReport, ReportError, SignalKind};

const IN1: BoolInputId = BoolInputId(0);
const IN2: BoolInputId = BoolInputId(1);
const FB0: BoolInputId = BoolInputId(2);
const FB1: BoolInputId = BoolInputId(3);

const ROWS: u32 = 8;
const COLUMNS: u32 = 8;
const ARRAY: u32 = ROWS * COLUMNS;
const SIGNATURE: u32 = 16;

const PACKAGE: PackageId = PackageId(0);

fn t(input: BoolInputId) -> Literal {
    Literal::new(input, Polarity::True)
}
fn c(input: BoolInputId) -> Literal {
    Literal::new(input, Polarity::Complement)
}

/// Row layout: macrocell 0 takes rows 0 (enable) and 1–2 (data);
/// macrocell 1 takes 3 (enable) and 4–6; row 7 is device-wide.
fn matrix() -> AndMatrixSpec {
    let rows = vec![
        row(0, ProductTermRole::OutputEnable { macrocell: MacrocellId(0) }),
        row(1, ProductTermRole::Data { macrocell: MacrocellId(0) }),
        row(2, ProductTermRole::Data { macrocell: MacrocellId(0) }),
        row(3, ProductTermRole::OutputEnable { macrocell: MacrocellId(1) }),
        row(4, ProductTermRole::Data { macrocell: MacrocellId(1) }),
        row(5, ProductTermRole::Data { macrocell: MacrocellId(1) }),
        row(6, ProductTermRole::Data { macrocell: MacrocellId(1) }),
        row(7, ProductTermRole::AsynchronousReset),
    ];
    let sources = vec![
        source(IN1, 0, PhysicalSignalSource::Pin(PinNumber(1))),
        source(IN2, 1, PhysicalSignalSource::Pin(PinNumber(2))),
        source(FB0, 2, PhysicalSignalSource::Feedback(MacrocellId(0))),
        source(FB1, 3, PhysicalSignalSource::Feedback(MacrocellId(1))),
    ];
    let cells = (0..ROWS)
        .map(|r| {
            (0..COLUMNS)
                .map(|column| MatrixCellSpec {
                    fuse: decpld_device::FuseId(r * COLUMNS + column),
                    connected_value: false,
                    disconnected_value: true,
                })
                .collect()
        })
        .collect();
    AndMatrixSpec::new(rows, sources, cells).expect("a valid matrix")
}

fn row(id: u32, role: ProductTermRole) -> ProductTermSpec {
    ProductTermSpec { id: ProductTermId(id), role }
}

fn source(id: BoolInputId, index: u32, physical_source: PhysicalSignalSource) -> LiteralSource {
    LiteralSource {
        id,
        true_column: MatrixColumn(index * 2),
        complement_column: MatrixColumn(index * 2 + 1),
        physical_source,
    }
}

fn package() -> PackageSpec {
    PackageSpec::new(
        PACKAGE,
        "DIP8",
        [
            (PinNumber(1), PackagePin::DedicatedInput(InputResourceId(0))),
            (PinNumber(2), PackagePin::DedicatedInput(InputResourceId(1))),
            (PinNumber(3), PackagePin::Pad(PadId(0))),
            (PinNumber(4), PackagePin::Pad(PadId(1))),
            (PinNumber(5), PackagePin::Clock(ClockResourceId(0))),
            (PinNumber(6), PackagePin::NoConnect),
            (PinNumber(7), PackagePin::Power(PowerRail::Ground)),
            (PinNumber(8), PackagePin::Power(PowerRail::Supply)),
        ],
    )
    .expect("a valid package")
}

fn specs() -> Vec<MacrocellSpec> {
    vec![spec(0, vec![1, 2], 0), spec(1, vec![4, 5, 6], 3)]
}

fn spec(id: u8, data: Vec<u32>, oe: u32) -> MacrocellSpec {
    MacrocellSpec {
        id: MacrocellId(id),
        pad: Some(PadId(id)),
        data_terms: data.into_iter().map(ProductTermId).collect(),
        oe_term: Some(ProductTermId(oe)),
        supports_registered: true,
        supports_combinational: true,
        supports_input_only: true,
        feedback_modes: vec![FeedbackSource::Pin],
        mode_field: None,
        polarity_field: None,
        fixed_clock: Some(ClockResourceId(0)),
    }
}

fn regions() -> FuseRegions {
    FuseRegions::new(
        ARRAY + SIGNATURE + 1,
        vec![
            FuseRegion {
                name: "array",
                range: 0..ARRAY,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "user-signature",
                range: ARRAY..ARRAY + SIGNATURE,
                erased_value: true,
                mutability: FuseMutability::UserSignature,
            },
            FuseRegion {
                name: "security",
                range: ARRAY + SIGNATURE..ARRAY + SIGNATURE + 1,
                erased_value: false,
                mutability: FuseMutability::Security,
            },
        ],
    )
    .expect("a valid layout")
}

/// Macrocell 0 drives pin 3 from two terms; macrocell 1's pad is off.
fn design() -> PhysicalDesign {
    PhysicalDesign {
        device: "SYNTH2",
        package: PACKAGE,
        macrocells: vec![
            MacrocellConfig {
                id: MacrocellId(0),
                assigned_signal: None,
                mode: MacrocellMode::Registered,
                polarity: OutputPolarity::ActiveLow,
                feedback: FeedbackSource::Pin,
                data_terms: vec![
                    PlacedCube { row: ProductTermId(1), cube: Cube::new([t(IN1), c(IN2)]) },
                    PlacedCube { row: ProductTermId(2), cube: Cube::new([t(FB1)]) },
                ],
                oe_term: Some(PlacedCube { row: ProductTermId(0), cube: Cube::always() }),
            },
            MacrocellConfig {
                id: MacrocellId(1),
                assigned_signal: None,
                mode: MacrocellMode::Combinational,
                polarity: OutputPolarity::ActiveHigh,
                feedback: FeedbackSource::Pin,
                data_terms: Vec::new(),
                oe_term: None,
            },
        ],
        global_terms: vec![PlacedCube { row: ProductTermId(7), cube: Cube::new([c(IN1), t(FB0)]) }],
    }
}

fn report() -> InspectReport {
    InspectReport::new(&design(), &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
        .expect("a describable design")
}

#[test]
fn a_literal_is_named_for_the_pin_it_arrives_on() {
    // A user reads a report to find out which *pin* drives what. A
    // literal rendered as its internal input index would make them do
    // the mapping the report exists to do for them — and a feedback
    // path arrives on the pin its macrocell drives, so it is named the
    // same way, with the difference recorded rather than spelled.
    let report = report();
    let cell = &report.macrocells[0];
    let sum = &cell.sum;
    assert_eq!(sum[0].text, "pin1 & !pin2");
    assert_eq!(sum[1].text, "pin4");

    assert_eq!(sum[0].literals[0].signal.name, "pin1");
    assert_eq!(sum[0].literals[0].signal.kind, SignalKind::Input);
    assert_eq!(sum[0].literals[0].signal.pin, Some(1));
    assert!(!sum[0].literals[0].complemented);
    assert!(sum[0].literals[1].complemented);

    // Macrocell 1's feedback arrives on pin 4, the pin its pad is on.
    assert_eq!(sum[1].literals[0].signal.kind, SignalKind::Feedback);
    assert_eq!(sum[1].literals[0].signal.pin, Some(4));
}

#[test]
fn an_empty_product_term_reads_as_always_not_as_an_empty_line() {
    // The device's central subtlety, and the one a reader is most
    // likely to get backwards: a term with no literals is the empty
    // AND, constantly TRUE. Rendering it as blank would show an
    // always-enabled output as having no enable at all.
    let report = report();
    assert_eq!(
        report.macrocells[0].output_enable.as_ref().map(|e| e.text.as_str()),
        Some("always")
    );
}

#[test]
fn a_pad_with_no_enable_term_reads_as_never() {
    // The opposite state, and the one that means "this pin is an
    // input". `None` in the design is a pad nobody drives; the report
    // must say so in words rather than omit the line.
    let report = report();
    let cell = &report.macrocells[1];
    assert!(!cell.pad_enabled);
    assert_eq!(cell.output_enable, None);
    assert!(cell.sum.is_empty());
}

#[test]
fn unused_product_terms_are_counted_against_capacity() {
    // SPEC.md §6.3 asks for unused product terms. A count against
    // capacity is what tells a user whether a design that fits has room
    // to grow, and it is per macrocell because that is how the silicon
    // allocates them.
    let report = report();
    assert_eq!(report.macrocells[0].terms_used, 2);
    assert_eq!(report.macrocells[0].terms_available, 2);
    assert_eq!(report.macrocells[1].terms_used, 0);
    assert_eq!(report.macrocells[1].terms_available, 3);
}

#[test]
fn a_macrocell_is_reported_with_the_pin_its_pad_is_on() {
    let report = report();
    assert_eq!(report.macrocells[0].pin, Some(3));
    assert_eq!(report.macrocells[1].pin, Some(4));
    assert_eq!(report.macrocells[0].mode, "registered");
    assert_eq!(report.macrocells[0].polarity, "active low");
}

#[test]
fn device_wide_terms_are_reported_by_role_not_by_row_number() {
    // "Row 7" means nothing to a user; "asynchronous reset" is the
    // thing they can act on.
    let report = report();
    assert_eq!(report.global_terms.len(), 1);
    assert_eq!(report.global_terms[0].role, "asynchronous reset");
    assert_eq!(report.global_terms[0].equation.text, "!pin1 & pin3");
}

#[test]
fn an_erased_signature_and_a_clear_security_fuse_are_reported_as_such() {
    let report = report();
    assert!(!report.security_fuse_set);
    // Every signature bit blown is 0xFF 0xFF, which is not printable.
    assert_eq!(report.user_signature.as_ref().map(|s| s.bytes.clone()), Some(vec![0xFF, 0xFF]));
    assert_eq!(report.user_signature.as_ref().map(|s| s.text.clone()), Some(None));
}

#[test]
fn a_signature_holding_ascii_is_reported_as_text() {
    // WinCUPL writes the design's `PartNo` here, so a user inspecting a
    // file somebody else produced can often read what it was called.
    let mut map = FuseMap::erased(regions());
    // 'H' = 0x48, 'i' = 0x69, MSB first within each byte.
    for (index, bit) in [0, 1, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 1, 0, 0, 1].into_iter().enumerate() {
        let fuse = decpld_device::FuseId(ARRAY + u32::try_from(index).expect("sixteen fits"));
        map.set(fuse, bit == 1).expect("a signature fuse");
    }
    let report =
        InspectReport::new(&design(), &package(), &matrix(), &specs(), &map).expect("describable");
    let signature = report.user_signature.expect("a signature region");
    assert_eq!(signature.bytes, vec![0x48, 0x69]);
    assert_eq!(signature.text.as_deref(), Some("Hi"));
}

#[test]
fn a_set_security_fuse_is_reported() {
    let mut map = FuseMap::erased(regions());
    map.set_security_fuse(true).expect("this device has one");
    let report =
        InspectReport::new(&design(), &package(), &matrix(), &specs(), &map).expect("describable");
    assert!(report.security_fuse_set);
}

#[test]
fn a_design_whose_package_is_not_the_one_supplied_is_refused() {
    // The pin numbers in the report come from the package. Rendering a
    // design against the wrong one would print pin assignments that are
    // confidently wrong, which is worse than printing none.
    let mut design = design();
    design.package = PackageId(7);
    assert_eq!(
        InspectReport::new(&design, &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
            .expect_err("a mismatched package"),
        ReportError::PackageMismatch { design: PackageId(7), supplied: PACKAGE }
    );
}

#[test]
fn a_term_in_a_row_the_matrix_does_not_have_is_refused() {
    let mut design = design();
    design.macrocells[0].data_terms[0].row = ProductTermId(99);
    assert!(matches!(
        InspectReport::new(&design, &package(), &matrix(), &specs(), &FuseMap::erased(regions())),
        Err(ReportError::UnknownRow { row: ProductTermId(99) })
    ));
}

#[test]
fn a_literal_no_source_carries_is_refused_rather_than_named_by_index() {
    // A design referring to an input the matrix has no column for
    // cannot be rendered honestly. Falling back to "input 9" would
    // print a signal name that corresponds to nothing on the part.
    let mut design = design();
    design.macrocells[0].data_terms[0].cube = Cube::new([t(BoolInputId(9))]);
    assert_eq!(
        InspectReport::new(&design, &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
            .expect_err("an unroutable literal"),
        ReportError::UnknownInput { input: BoolInputId(9) }
    );
}

#[test]
fn the_text_report_reads_as_a_part() {
    insta::assert_snapshot!(report().to_string());
}

#[test]
fn the_json_report_carries_every_field_the_text_one_does() {
    let json = report().to_json().expect("serialisable");
    insta::assert_snapshot!(json);
}
