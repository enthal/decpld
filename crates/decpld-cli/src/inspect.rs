//! Turning a parsed JEDEC file into an inspection report. SPEC.md §6.3.
//!
//! This is the seam where three layers meet, and it lives here rather
//! than in any of them: `decpld-jedec` transfers numbered fuse states
//! and must not know what a macrocell is, `decpld-atf22v10` describes a
//! device and must not know JEDEC syntax, and `decpld-report` renders a
//! design and must not know either. Somebody has to join them, and the
//! joint belongs above all three.
//!
//! Everything here is a pure function of a parsed file, so it is tested
//! without spawning a process (CLAUDE.md: logic testable outside the CLI
//! must not live inside a CLI function).

use decpld_atf22v10::{
    DesignError, Footprint, FootprintError, and_matrix, decode_design, dip24, macrocells,
};
use decpld_device::{FuseMap, FuseStatesError, PackageSpec};
use decpld_jedec::JedecFile;
use decpld_report::{InspectReport, ReportError, SourceReport};

/// Which device a file is to be read as.
///
/// A file does not say. `QF` narrows it — 5892 fuses is not an
/// ATF16V8 — but several parts share a fuse count across families, so
/// the user names the part and deCPLD checks the count agrees rather
/// than guessing from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Atf22v10c,
}

impl Device {
    fn package(self) -> PackageSpec {
        match self {
            Device::Atf22v10c => dip24(),
        }
    }
}

/// Why a file could not be described as a device.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InspectError {
    #[error("{0}")]
    Footprint(#[from] FootprintError),
    #[error("{0}")]
    FuseStates(#[from] FuseStatesError),
    #[error("{0}")]
    Design(#[from] DesignError),
    #[error("{0}")]
    Report(#[from] ReportError),
    #[error("{0}")]
    Device(String),
    #[error(
        "the file describes a {package_in_file} part and --package {requested} was asserted; \
         the assertion is checked, never applied"
    )]
    PackageMismatch { package_in_file: &'static str, requested: String },
}

/// Describe a parsed JEDEC file as a device.
///
/// `package` is a **checked assertion**, never an override (CLAUDE.md:
/// the source is authoritative for the target). A file's fuse count
/// already decides its footprint, and a package name that disagrees with
/// what the device model says is a mistake worth reporting rather than a
/// request to relabel the output.
///
/// # Errors
///
/// If the file's fuse count is not one this device has, if a reserved
/// fuse disagrees, if the fuses do not decode into a design, or if the
/// asserted package is not the device's.
pub fn inspect(
    file: &JedecFile,
    device: Device,
    package: Option<&str>,
) -> Result<InspectReport, InspectError> {
    let Device::Atf22v10c = device;

    // The file's `QF` decides the footprint. The device model refuses a
    // count it does not have, which is what turns "this is an ATF16V8
    // file" from a confusing half-decode into one sentence.
    let footprint = Footprint::from_fuse_count(file.fuses.len())?;
    let regions =
        decpld_atf22v10::regions_for(footprint).map_err(|e| InspectError::Device(e.to_string()))?;

    let map = FuseMap::from_states(regions, file.fuses.iter())?;
    let design = decode_design(&map)?;

    let package_spec = device.package();
    if let Some(requested) = package
        && !requested.eq_ignore_ascii_case(package_spec.name())
    {
        return Err(InspectError::PackageMismatch {
            package_in_file: package_spec.name(),
            requested: requested.to_owned(),
        });
    }

    let matrix = and_matrix().map_err(|e| InspectError::Device(e.to_string()))?;
    let specs = macrocells().map_err(|e| InspectError::Device(e.to_string()))?;

    let report = InspectReport::new(&design, &package_spec, &matrix, &specs, &map)?
        .with_source(source_report(file))
        // The ATF22V10C's security bit is the JEDEC `G` field and has
        // no fuse index inside `QF` (SPEC.md §4.7), so the fuse map
        // cannot see it. A file that carried no `G` says nothing, which
        // is not the same as saying the part is unlocked — but for a
        // report "clear" is the only honest rendering of "no evidence
        // of a lock", and the `G` field's own absence is visible in the
        // file section.
        .with_security_fuse(file.security.unwrap_or(false));

    Ok(report)
}

/// What the file itself claimed, for the report's file section.
fn source_report(file: &JedecFile) -> SourceReport {
    SourceReport {
        design_specification: file.design_specification.clone(),
        declared_fuse_checksum: file.fuse_checksum,
        computed_fuse_checksum: file.computed_fuse_checksum(),
        transmission_checksum: file.transmission_checksum,
        notes: file.notes.clone(),
        unmodelled_fields: file
            .unknown_fields
            .iter()
            .map(|field| field.identifier.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use decpld_atf22v10::{Atf22v10Geometry, blank_design, bool_input_of_source, encode_design};
    use decpld_device::{
        MacrocellId, MacrocellMode, OutputPolarity, PinNumber, PlacedCube, ProductTermId,
    };
    use decpld_jedec::FuseVector;
    use decpld_logic::{Cube, Literal, Polarity};

    const G: Atf22v10Geometry = Atf22v10Geometry;

    /// Pin 2 and pin 3 drive pin 23, registered and active low — a
    /// design with something to say in every field the report has.
    fn probe_file() -> JedecFile {
        let mut design = blank_design().expect("a blank design");
        let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a measured block");
        let literal = |pin: u8, polarity| {
            let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
            Literal::new(bool_input_of_source(source.index), polarity)
        };
        let cell = design
            .macrocells
            .iter_mut()
            .find(|cell| cell.id == MacrocellId(macrocell.0))
            .expect("present");
        cell.mode = MacrocellMode::Registered;
        cell.polarity = OutputPolarity::ActiveLow;
        cell.oe_term =
            Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
        cell.data_terms = vec![PlacedCube {
            row: ProductTermId(block.data_rows.start),
            cube: Cube::new([literal(2, Polarity::True), literal(3, Polarity::Complement)]),
        }];

        let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
        file_of(&fuses)
    }

    fn file_of(fuses: &FuseMap) -> JedecFile {
        let mut vector = FuseVector::new(fuses.len(), false);
        for (index, state) in fuses.iter().enumerate() {
            let index = u32::try_from(index).expect("under six thousand");
            vector.set(index, state).expect("in range");
        }
        JedecFile {
            design_specification: "probe\n".to_owned(),
            fuses: vector,
            default_fuse: Some(false),
            notes: Vec::new(),
            security: None,
            fuse_checksum: None,
            transmission_checksum: None,
            unknown_fields: Vec::new(),
        }
    }

    #[test]
    fn a_file_this_compiler_wrote_reads_back_as_the_design_that_went_in() {
        // The whole point of `jed inspect`: fuses in, equations out. If
        // this ever renders something other than the cube encoded above,
        // a user reading a file to find out what a chip does is being
        // told the wrong answer.
        let report = inspect(&probe_file(), Device::Atf22v10c, None).expect("describable");
        assert_eq!(report.device, "ATF22V10C");
        assert_eq!(report.package, "DIP24");
        assert_eq!(report.fuse_count, 5892);

        let driven: Vec<_> = report.macrocells.iter().filter(|cell| cell.pad_enabled).collect();
        assert_eq!(driven.len(), 1, "only pin 23 is driven");
        let cell = driven[0];
        assert_eq!(cell.pin, Some(23));
        assert_eq!(cell.mode, "registered");
        assert_eq!(cell.polarity, "active low");
        assert_eq!(cell.sum.len(), 1);
        assert_eq!(cell.sum[0].text, "pin2 & !pin3");
        assert_eq!(cell.output_enable.as_ref().map(|e| e.text.as_str()), Some("always"));
        assert!(report.global_terms.is_empty(), "no reset or preset in this design");
    }

    #[test]
    fn a_file_with_another_parts_fuse_count_is_refused_by_count() {
        // An ATF16V8 file is 2194 fuses. Reading it as an ATF22V10C
        // would decode nonsense that looks like a design.
        let mut file = probe_file();
        file.fuses = FuseVector::new(2194, false);
        assert_eq!(
            inspect(&file, Device::Atf22v10c, None).expect_err("not this part"),
            InspectError::Footprint(FootprintError::UnknownFuseCount { count: 2194 })
        );
    }

    #[test]
    fn every_footprint_this_device_has_is_readable() {
        // A file may arrive in PAL, GAL, or power-down form, and all
        // three are this part. Only the GAL one carries a signature.
        for footprint in Footprint::ALL {
            let design = blank_design().expect("blank");
            let fuses = encode_design(&design, footprint).expect("encodable");
            let file = file_of(&fuses);
            let report = inspect(&file, Device::Atf22v10c, None)
                .unwrap_or_else(|e| panic!("{footprint:?}: {e}"));
            assert_eq!(report.fuse_count, footprint.fuse_count(), "{footprint:?}");
            assert_eq!(
                report.user_signature.is_some(),
                !matches!(footprint, Footprint::Pal),
                "{footprint:?} signature"
            );
        }
    }

    #[test]
    fn the_package_argument_is_checked_and_never_applied() {
        // SPEC.md §8.2 and CLAUDE.md: the source is authoritative for
        // the target, and `--package` is an assertion. Relabelling the
        // report to whatever was asked for would make the flag a way to
        // print pin numbers for a package the file is not for.
        let file = probe_file();
        assert!(inspect(&file, Device::Atf22v10c, Some("DIP24")).is_ok());
        assert!(inspect(&file, Device::Atf22v10c, Some("dip24")).is_ok(), "case-insensitive");

        let error =
            inspect(&file, Device::Atf22v10c, Some("PLCC28")).expect_err("not this package");
        assert_eq!(
            error,
            InspectError::PackageMismatch { package_in_file: "DIP24", requested: "PLCC28".into() }
        );
    }

    #[test]
    fn a_locked_part_is_reported_as_locked() {
        // The ATF22V10C's security bit is the JEDEC `G` field and lies
        // outside `QF`, so nothing in the fuse map can see it. A report
        // that only consulted the map would tell a user a locked part is
        // readable — the one error that costs them the chip.
        let mut file = probe_file();
        file.security = Some(true);
        assert!(inspect(&file, Device::Atf22v10c, None).expect("describable").security_fuse_set);

        file.security = Some(false);
        assert!(!inspect(&file, Device::Atf22v10c, None).expect("describable").security_fuse_set);

        file.security = None;
        assert!(!inspect(&file, Device::Atf22v10c, None).expect("describable").security_fuse_set);
    }

    #[test]
    fn the_checksum_a_file_declared_is_reported_beside_the_computed_one() {
        let mut file = probe_file();
        let computed = file.computed_fuse_checksum();
        file.fuse_checksum = Some(computed);
        let source = inspect(&file, Device::Atf22v10c, None).expect("describable").source;
        assert!(source.as_ref().expect("a file section").fuse_checksum_agrees());

        file.fuse_checksum = Some(computed.wrapping_add(1));
        let source = inspect(&file, Device::Atf22v10c, None).expect("describable").source;
        let source = source.expect("a file section");
        assert!(!source.fuse_checksum_agrees());
        assert_eq!(source.computed_fuse_checksum, computed);
    }

    #[test]
    fn an_erased_part_reports_every_pin_driven_by_a_constantly_true_sum() {
        // What a blank chip actually does, which is the opposite of the
        // intuition that "erased" means "inert". Every link is blown, so
        // every term is the empty AND and every output-enable row is
        // permanently asserted.
        let regions = decpld_atf22v10::regions_for(Footprint::Gal).expect("valid");
        let file = file_of(&FuseMap::erased(regions));
        let report = inspect(&file, Device::Atf22v10c, None).expect("describable");
        assert_eq!(report.macrocells.len(), 10);
        for cell in &report.macrocells {
            assert!(cell.pad_enabled, "macrocell {}", cell.macrocell);
            assert_eq!(cell.output_enable.as_ref().map(|e| e.text.as_str()), Some("always"));
            assert_eq!(cell.terms_used, cell.terms_available);
        }
    }
}
