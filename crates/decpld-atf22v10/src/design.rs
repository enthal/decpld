//! Whole-design encode and decode for the ATF22V10C.
//!
//! The pieces are already measured and tested: the AND matrix places
//! product terms, and the macrocells' configuration fields place mode
//! and polarity. This assembles them into a device, and it is where
//! SPEC.md §4.7's "every legal configuration round-trips" is finally a
//! statement about a whole part rather than a row or a field.

use crate::geometry::SYNCHRONOUS_PRESET_ROW;
use crate::geometry::{ASYNCHRONOUS_RESET_ROW, ROWS};
use crate::macrocells::{MacrocellError, macrocells};
use crate::matrix::and_matrix;
use crate::packages::DIP24;
use crate::regions::{Footprint, FootprintError, regions_for};
use decpld_device::{
    AndMatrixSpec, ConfigFieldError, DecodeError, EncodeError, FeedbackSource, FuseMap,
    MacrocellConfig, MacrocellId, MacrocellMode, MacrocellSpec, MatrixError, OutputPolarity,
    PhysicalDesign, PlacedCube, ProductTermId, ProductTermRole, RegionError, decode_row,
    disable_row, encode_cube, row_is_never_true,
};
use std::collections::{BTreeMap, BTreeSet};

/// The device id this crate models.
pub const DEVICE: &str = "ATF22V10C";

/// Why a whole design could not be encoded or decoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DesignError {
    #[error("{0}")]
    Regions(#[from] RegionError),
    #[error("{0}")]
    Matrix(#[from] MatrixError),
    #[error("{0}")]
    Macrocell(#[from] MacrocellError),
    #[error("{0}")]
    Field(#[from] ConfigFieldError),
    #[error("{0}")]
    Encode(#[from] EncodeError),
    #[error("{0}")]
    Decode(#[from] DecodeError),
    #[error("{0}")]
    Footprint(#[from] FootprintError),
    #[error(
        "{macrocell} has no {row} in that role; a term may only be placed in a row its own \
         macrocell owns, as a data term or as the output enable"
    )]
    TermNotOwned { macrocell: MacrocellId, row: ProductTermId },
    #[error("{macrocell} places two terms in {row}")]
    RowUsedTwice { macrocell: MacrocellId, row: ProductTermId },
    #[error("{row} is not a device-wide term; only the reset and preset rows may be global")]
    NotAGlobalTerm { row: ProductTermId },
    #[error("{row} is placed as both a device-wide term and a macrocell term")]
    GlobalRowUsedTwice { row: ProductTermId },
    #[error("the design describes {macrocell}, which this device does not have")]
    NoSuchMacrocell { macrocell: MacrocellId },
    #[error(
        "the design does not describe {macrocell}; every macrocell must be configured, because \
         an unconfigured one leaves its product terms erased, and an erased term is constantly \
         true"
    )]
    MacrocellMissing { macrocell: MacrocellId },
    #[error("the design describes {macrocell} twice")]
    MacrocellListedTwice { macrocell: MacrocellId },
    #[error("{macrocell} cannot take feedback from {feedback:?} on this device")]
    FeedbackNotSupported { macrocell: MacrocellId, feedback: FeedbackSource },
    #[error("this is an {expected} model; the design is for {device}")]
    WrongDevice { device: &'static str, expected: &'static str },
}

/// A design with every macrocell present and nothing configured.
///
/// The starting point for building one by hand, and the shape
/// [`decode_design`] fills in. Modes and polarities take the values an
/// erased part holds so that a blank design and an erased device agree
/// about a macrocell nobody touched.
///
/// # Errors
///
/// If the measured tables ever describe a macrocell that violates
/// [`decpld_device::MacrocellSpec`]'s rules — the same check every
/// other entry point in this module makes.
pub fn blank_design() -> Result<PhysicalDesign, DesignError> {
    let specs = macrocells()?;
    Ok(PhysicalDesign {
        device: DEVICE,
        package: DIP24,
        macrocells: specs
            .iter()
            .map(|spec| MacrocellConfig {
                id: spec.id,
                assigned_signal: None,
                // An erased architecture pair reads 1, 1.
                mode: MacrocellMode::Combinational,
                polarity: OutputPolarity::ActiveHigh,
                feedback: FeedbackSource::Pin,
                data_terms: Vec::new(),
                oe_term: None,
            })
            .collect(),
        global_terms: Vec::new(),
    })
}

/// Turn a design into fuses, in the given JEDEC footprint.
///
/// **Every row is written, including the ones the design does not use.**
/// The two "empty" states of this device are opposites and it matters
/// which one an unused row holds:
///
/// - all links *blown* — the state an erased part is in — is a product
///   term with no literals, the empty AND, constantly **true**;
/// - all links *intact* is every literal at both polarities, constantly
///   **false**.
///
/// A sum of products ORs its terms, so leaving an unused row in the
/// erased state would contribute a constantly-true term and drive the
/// output permanently high. Unused rows are therefore written to the
/// never-true state, which is exactly what an unused row looks like in
/// the oracle's own output.
///
/// That is also why the design must describe **every** macrocell rather
/// than only the ones a fitter placed: the rows to turn off are decided
/// by the device, not by the design, and a design that quietly omitted
/// a macrocell would leave ten pins' worth of that decision unmade.
pub fn encode_design(
    design: &PhysicalDesign,
    footprint: Footprint,
) -> Result<FuseMap, DesignError> {
    if design.device != DEVICE {
        return Err(DesignError::WrongDevice { device: design.device, expected: DEVICE });
    }

    let mut map = FuseMap::erased(regions_for(footprint)?);
    let matrix = and_matrix()?;
    let specs = macrocells()?;

    // Index the design by macrocell, refusing a repeat rather than
    // letting two configurations for one set of rows meet at the fuse
    // layer — where the complaint would name a fuse and neither the
    // macrocell nor the duplication.
    let mut configs_by_macrocell: BTreeMap<MacrocellId, &MacrocellConfig> = BTreeMap::new();
    for cell in &design.macrocells {
        if !specs.iter().any(|spec| spec.id == cell.id) {
            return Err(DesignError::NoSuchMacrocell { macrocell: cell.id });
        }
        if configs_by_macrocell.insert(cell.id, cell).is_some() {
            return Err(DesignError::MacrocellListedTwice { macrocell: cell.id });
        }
    }

    // Every row the design placed a term in, so the device-wide rows
    // can be checked against it. A global term landing in a macrocell's
    // block would otherwise be caught only as a fuse conflict.
    let mut placed_rows: BTreeSet<ProductTermId> = BTreeSet::new();

    for spec in &specs {
        let cell = configs_by_macrocell
            .get(&spec.id)
            .copied()
            .ok_or(DesignError::MacrocellMissing { macrocell: spec.id })?;
        encode_macrocell(&mut map, &matrix, spec, cell, &mut placed_rows)?;
    }

    for placed in &design.global_terms {
        let role = matrix.row(placed.row).map(|row| row.role);
        if !matches!(
            role,
            Some(ProductTermRole::AsynchronousReset | ProductTermRole::SynchronousPreset)
        ) {
            return Err(DesignError::NotAGlobalTerm { row: placed.row });
        }
        if !placed_rows.insert(placed.row) {
            return Err(DesignError::GlobalRowUsedTwice { row: placed.row });
        }
        encode_cube(&mut map, &matrix, placed.row, &placed.cube)?;
    }

    // Same for the two device-wide rows.
    for row in [ASYNCHRONOUS_RESET_ROW, SYNCHRONOUS_PRESET_ROW] {
        let id = ProductTermId(row);
        if !placed_rows.contains(&id) {
            disable_row(&mut map, &matrix, id)?;
        }
    }

    Ok(map)
}

/// Write one macrocell's terms and architecture bits.
fn encode_macrocell(
    map: &mut FuseMap,
    matrix: &AndMatrixSpec,
    spec: &MacrocellSpec,
    cell: &MacrocellConfig,
    placed_rows: &mut BTreeSet<ProductTermId>,
) -> Result<(), DesignError> {
    if !spec.feedback_modes.contains(&cell.feedback) {
        return Err(DesignError::FeedbackNotSupported {
            macrocell: spec.id,
            feedback: cell.feedback,
        });
    }

    // A term may only go where its macrocell owns a row **in that
    // role**. Ownership alone is not enough: a data term written into
    // the enable row would silently become the pin's tri-state control,
    // and decoding would report it back as the enable — a round-trip
    // that agrees with itself about the wrong thing.
    let mut used: BTreeSet<ProductTermId> = BTreeSet::new();
    let data_rows: BTreeSet<ProductTermId> = spec.data_terms.iter().copied().collect();
    for placed in &cell.data_terms {
        if !data_rows.contains(&placed.row) {
            return Err(DesignError::TermNotOwned { macrocell: spec.id, row: placed.row });
        }
        if !used.insert(placed.row) {
            return Err(DesignError::RowUsedTwice { macrocell: spec.id, row: placed.row });
        }
        encode_cube(map, matrix, placed.row, &placed.cube)?;
    }

    if let Some(placed) = &cell.oe_term {
        if spec.oe_term != Some(placed.row) {
            return Err(DesignError::TermNotOwned { macrocell: spec.id, row: placed.row });
        }
        used.insert(placed.row);
        encode_cube(map, matrix, placed.row, &placed.cube)?;
    }

    // Every row this macrocell owns that the design did not place a
    // term in is turned off, so it contributes nothing to the sum.
    // An untouched row would contribute a constantly-TRUE term
    // instead, because that is what an erased row is.
    //
    // A macrocell with no output-enable term is one whose pad is not
    // driven, and turning that row off is how this device says so.
    for &row in spec.data_terms.iter().chain(spec.oe_term.iter()) {
        if !used.contains(&row) {
            disable_row(map, matrix, row)?;
        }
    }
    placed_rows.extend(used);

    if let Some(field) = &spec.mode_field {
        field.encode(map, cell.mode)?;
    }
    if let Some(field) = &spec.polarity_field {
        field.encode(map, cell.polarity)?;
    }
    Ok(())
}

/// Read a whole design back out of a fuse map.
///
/// Total for any map with one of this device's three fuse counts,
/// because `jed inspect` reads files this compiler did not write. A row
/// nobody programmed decodes to the cube its fuses describe — on an
/// erased part, no literals at all, which is constantly true — rather
/// than being silently omitted.
///
/// A map of any other size is refused by fuse count rather than
/// half-decoded until a configuration field runs off the end: the
/// resulting complaint would name a fuse address, when what the user
/// needs to hear is that the file is for a different part.
pub fn decode_design(map: &FuseMap) -> Result<PhysicalDesign, DesignError> {
    // The footprint is an assertion about the map, not a constant: it
    // is what makes the `device` and `package` this function returns a
    // claim it has checked rather than one it has assumed.
    let _footprint = Footprint::from_fuse_count(map.len())?;

    let matrix = and_matrix()?;
    let specs = macrocells()?;

    let mut cells = Vec::with_capacity(specs.len());
    for spec in &specs {
        let mode = match &spec.mode_field {
            Some(field) => field.decode(map)?,
            None => MacrocellMode::Combinational,
        };
        let polarity = match &spec.polarity_field {
            Some(field) => field.decode(map)?,
            None => OutputPolarity::ActiveHigh,
        };

        let mut data_terms = Vec::new();
        for &row in &spec.data_terms {
            let cube = decode_row(map, &matrix, row)?;
            // A row that can never be true contributes nothing to the
            // sum. Reporting it would bury the design's real equations
            // under eight to sixteen unsatisfiable terms per macrocell.
            if !row_is_never_true(&cube) {
                data_terms.push(PlacedCube { row, cube });
            }
        }

        // An enable that can never be true is a pad that is never
        // driven, and it is reported as an absent term rather than as a
        // present one nobody can satisfy — the same rule the data terms
        // follow, so `oe_term` means the same thing in both directions.
        let oe_term = match spec.oe_term {
            Some(row) => {
                let cube = decode_row(map, &matrix, row)?;
                (!row_is_never_true(&cube)).then_some(PlacedCube { row, cube })
            }
            None => None,
        };

        cells.push(MacrocellConfig {
            id: spec.id,
            assigned_signal: None,
            mode,
            polarity,
            // No fuse on this device selects a feedback path, and
            // `macrocells()` advertises exactly one. Encoding refuses
            // any other value, so this is a report of what the part
            // does rather than a default standing in for a measurement.
            feedback: FeedbackSource::Pin,
            data_terms,
            oe_term,
        });
    }

    // The two rows no macrocell owns.
    let mut global_terms = Vec::new();
    for row in [ASYNCHRONOUS_RESET_ROW, SYNCHRONOUS_PRESET_ROW] {
        debug_assert!(row < ROWS);
        let id = ProductTermId(row);
        let cube = decode_row(map, &matrix, id)?;
        if !row_is_never_true(&cube) {
            global_terms.push(PlacedCube { row: id, cube });
        }
    }

    Ok(PhysicalDesign { device: DEVICE, package: DIP24, macrocells: cells, global_terms })
}
