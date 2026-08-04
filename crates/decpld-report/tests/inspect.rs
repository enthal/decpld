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

fn source_report() -> decpld_report::SourceReport {
    decpld_report::SourceReport {
        design_specification: " a two-macrocell part ".to_owned(),
        declared_fuse_checksum: Some(0x1234),
        computed_fuse_checksum: 0x1234,
        transmission_checksum: Some(0xABCD),
        notes: vec!["written by hand".to_owned()],
        unmodelled_fields: vec!["QP".to_owned(), "J".to_owned()],
    }
}

#[test]
fn a_checksum_a_file_declared_is_reported_beside_the_one_its_data_produces() {
    // A `C` field can disagree with the fuse data it claims to
    // describe. Reporting only one of the two would let a corrupt file
    // look correct, whichever one was chosen.
    let mut source = source_report();
    assert!(source.fuse_checksum_agrees());

    // A non-zero value that is not the computed one. Zero would NOT do
    // here: JEDEC gives it the meaning "not computed", so it is the one
    // wrong-looking value that is not a wrong claim.
    source.declared_fuse_checksum = Some(0x1235);
    assert!(!source.fuse_checksum_agrees());

    // A file that declared none makes no claim to be wrong about.
    source.declared_fuse_checksum = None;
    assert!(source.fuse_checksum_agrees());
}

#[test]
fn a_readback_lock_outside_the_fuse_vector_is_still_reported() {
    // The ATF22V10C's security bit is the JEDEC `G` field and has no
    // fuse index inside `QF` (SPEC.md §4.7). A report that looked only
    // for a security region would call a locked part clear.
    let report = report();
    assert!(!report.security_fuse_set, "this fixture's own region is clear");
    assert!(report.with_security_fuse(true).security_fuse_set);
}

#[test]
fn the_text_report_shows_what_the_file_claimed() {
    insta::assert_snapshot!(report().with_source(source_report()).to_string());
}

#[test]
fn a_disagreeing_checksum_is_called_out_in_the_text_report() {
    let mut source = source_report();
    source.declared_fuse_checksum = Some(0x1235);
    insta::assert_snapshot!(report().with_source(source).to_string());
}

#[test]
fn a_zero_checksum_reads_as_none_declared_in_the_text_report() {
    let mut source = source_report();
    source.declared_fuse_checksum = Some(0x0000);
    insta::assert_snapshot!(report().with_source(source).to_string());
}

#[test]
fn a_zero_checksum_is_not_computed_rather_than_wrong() {
    // JEDEC 3A grants `C0000` the meaning "not computed", and this
    // project's own parser accepts it for exactly that reason — both
    // GALasm and WinCUPL emit it. Treating it as a disagreement accuses
    // a valid file of corruption.
    //
    // It is also the ONLY value that can reach a disagreement through
    // the CLI: the parser turns any other mismatch into an error, so
    // without this rule every "DISAGREE" the command ever printed would
    // be a false alarm.
    let mut source = source_report();
    source.declared_fuse_checksum = Some(0x0000);
    source.computed_fuse_checksum = 0xDD2F;
    assert!(source.fuse_checksum_agrees(), "C0000 makes no claim to be wrong about");
    assert!(!source.declares_a_checksum());

    source.declared_fuse_checksum = Some(0xDD2E);
    assert!(!source.fuse_checksum_agrees(), "a non-zero wrong checksum is a disagreement");
    assert!(source.declares_a_checksum());
}

#[test]
fn a_signature_holding_a_space_is_still_printable() {
    // 0x20 is inside JEDEC's own field-character class and is what a
    // short `PartNo` is padded with, so a signature reading "Hi      "
    // must render as text. An off-by-one at the bottom of the printable
    // range would silently hide the common case.
    let mut map = FuseMap::erased(regions());
    // 'H' = 0x48, ' ' = 0x20.
    for (index, bit) in [0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0].into_iter().enumerate() {
        let fuse = decpld_device::FuseId(ARRAY + u32::try_from(index).expect("sixteen fits"));
        map.set(fuse, bit == 1).expect("a signature fuse");
    }
    let report =
        InspectReport::new(&design(), &package(), &matrix(), &specs(), &map).expect("describable");
    let signature = report.user_signature.expect("a signature region");
    assert_eq!(signature.bytes, vec![0x48, 0x20]);
    assert_eq!(signature.text.as_deref(), Some("H "));
}

#[test]
fn a_signature_region_that_is_not_whole_bytes_is_refused() {
    // Packing would otherwise drop the remainder silently: a 20-bit
    // region would report two bytes and lose four bits, and a region
    // shorter than a byte would report NO bytes — whereupon "every byte
    // is printable" is vacuously true and the report prints an empty
    // string for a region that holds data.
    let regions = FuseRegions::new(
        ARRAY + 20,
        vec![
            FuseRegion {
                name: "array",
                range: 0..ARRAY,
                erased_value: true,
                mutability: FuseMutability::Programmable,
            },
            FuseRegion {
                name: "user-signature",
                range: ARRAY..ARRAY + 20,
                erased_value: true,
                mutability: FuseMutability::UserSignature,
            },
        ],
    )
    .expect("a valid layout");
    let map = FuseMap::erased(regions);
    assert_eq!(
        InspectReport::new(&design(), &package(), &matrix(), &specs(), &map)
            .expect_err("twenty bits is not a whole number of bytes"),
        ReportError::SignatureNotWholeBytes { name: "user-signature", bits: 20 }
    );
}

#[test]
fn a_macrocell_whose_pad_is_not_its_own_number_is_still_named_correctly() {
    // `PadId` and `MacrocellId` are separate types because they are
    // separate facts: `MacrocellSpec::pad` is what connects them, and
    // deriving a pin by casting one id to the other is a device
    // assumption living in a device-agnostic crate.
    //
    // Concretely, with the two pads swapped: macrocell 0 drives pin 4,
    // so a literal fed back from macrocell 0 must read `pin4`. Casting
    // the raw number instead names it `pin3` — and the very same report
    // says three lines earlier that macrocell 0 is on pin 4.
    let mut specs = specs();
    specs[0].pad = Some(PadId(1));
    specs[1].pad = Some(PadId(0));

    let report =
        InspectReport::new(&design(), &package(), &matrix(), &specs, &FuseMap::erased(regions()))
            .expect("describable");

    assert_eq!(report.macrocells[0].pin, Some(4), "macrocell 0 now drives pin 4");
    assert_eq!(report.macrocells[1].pin, Some(3));
    // The device-wide term names macrocell 0's feedback, which now
    // arrives on pin 4.
    assert_eq!(report.global_terms[0].equation.text, "!pin1 & pin4");
    // And macrocell 0's own second term names macrocell 1's feedback.
    assert_eq!(report.macrocells[0].sum[1].text, "pin3");
}

#[test]
fn a_lock_reported_by_the_device_is_not_cleared_by_a_file_that_is_silent() {
    // `with_security_fuse` supplies a state the fuse vector cannot
    // carry; it must not overwrite one the vector *can*. A device whose
    // security bit is inside `QF`, read from a file with no `G` field,
    // would otherwise be reported as readable — the one error that
    // costs the user the chip.
    let mut map = FuseMap::erased(regions());
    map.set_security_fuse(true).expect("this device has one");
    let report =
        InspectReport::new(&design(), &package(), &matrix(), &specs(), &map).expect("describable");
    assert!(report.security_fuse_set);
    assert!(report.with_security_fuse(false).security_fuse_set, "silence does not unlock");

    // And the other direction still works: a device with no security
    // region takes the state entirely from outside.
    let report = report_of(&FuseMap::erased(regions()));
    assert!(!report.security_fuse_set);
    assert!(report_of(&FuseMap::erased(regions())).with_security_fuse(true).security_fuse_set);
}

fn report_of(map: &FuseMap) -> InspectReport {
    InspectReport::new(&design(), &package(), &matrix(), &specs(), map).expect("describable")
}

#[test]
fn a_contradictory_term_is_marked_never_true_rather_than_read_as_logic() {
    // `Cube::canonical` deliberately preserves a contradiction, and a
    // term holding an input at both polarities is constantly false —
    // just as unsatisfiable as an absent one. Rendered as bare text it
    // is indistinguishable from a term that fires, which is precisely
    // the confusion between the two empty states this report exists to
    // prevent.
    let mut design = design();
    design.macrocells[0].data_terms[0].cube = Cube::new([t(IN1), c(IN1)]);
    let report =
        InspectReport::new(&design, &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
            .expect("describable");
    let term = &report.macrocells[0].sum[0];
    assert!(term.never_true, "a & !a can never fire");
    assert_eq!(term.text, "pin1 & !pin1");
    // A satisfiable term is not marked.
    assert!(!report.macrocells[0].sum[1].never_true);
    // Nor is the empty AND, which is the opposite state entirely.
    assert!(!report.macrocells[0].output_enable.as_ref().expect("always").never_true);
}

#[test]
fn a_cube_built_out_of_order_renders_the_same_as_one_built_in_order() {
    // The report canonicalises rather than trusting its input's order.
    // Everything reaching it today comes from `decode_row`, which
    // already does — but a fitter's own pre-encode design will not, and
    // determinism (SPEC.md §13.2) is not something to discover later.
    let ordered = {
        let mut d = design();
        d.macrocells[0].data_terms[0].cube = Cube::new([t(IN1), c(IN2), t(FB1)]);
        InspectReport::new(&d, &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
            .expect("ok")
    };
    let shuffled = {
        let mut d = design();
        d.macrocells[0].data_terms[0].cube = Cube::new([t(FB1), t(IN1), c(IN2), t(IN1)]);
        InspectReport::new(&d, &package(), &matrix(), &specs(), &FuseMap::erased(regions()))
            .expect("ok")
    };
    assert_eq!(ordered.macrocells[0].sum[0], shuffled.macrocells[0].sum[0]);
    assert_eq!(ordered.macrocells[0].sum[0].text, "pin1 & !pin2 & pin4");
}

#[test]
fn a_multi_line_header_does_not_break_the_two_column_layout() {
    // WinCUPL's header is always several lines — compiler banner,
    // device, library, date, name, part number. Printing it raw puts
    // five unindented lines in the middle of an indented block, which
    // is the single most common real input this report will ever see.
    let mut source = source_report();
    source.design_specification =
        "CUPL(WM) 5.0a\nDevice g22v10 Library DLIB-h-40-1\nName counter\n".to_owned();
    source.notes = vec!["note line one\nsecond line".to_owned()];
    insta::assert_snapshot!(report().with_source(source).to_string());
}

#[test]
fn the_text_report_shows_the_transmission_checksum() {
    // SPEC.md §6.3 asks for checksums, plural. The transmission
    // checksum reached JSON and not the text report.
    let text = report().with_source(source_report()).to_string();
    assert!(text.contains("ABCD"), "{text}");
}
