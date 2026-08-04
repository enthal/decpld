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
use crate::regions::{Footprint, regions_for};
use decpld_device::{
    ConfigFieldError, DecodeError, EncodeError, FeedbackSource, FuseMap, MacrocellConfig,
    MacrocellMode, MacrocellSpec, MatrixError, OutputPolarity, PhysicalDesign, PlacedCube,
    ProductTermId, ProductTermRole, RegionError, decode_row, disable_row, encode_cube,
    row_is_never_true,
};

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
    #[error(
        "macrocell {macrocell} has no product term {row:?}; a term may only be placed in a row \
         its own macrocell owns"
    )]
    TermNotOwned { macrocell: u8, row: ProductTermId },
    #[error("{row:?} is not a device-wide term; only the reset and preset rows may be global")]
    NotAGlobalTerm { row: ProductTermId },
    #[error("the design describes macrocell {macrocell}, which this device does not have")]
    NoSuchMacrocell { macrocell: u8 },
}

/// A design with every macrocell present and nothing configured.
///
/// The starting point for building one by hand, and the shape
/// [`decode_design`] fills in. Modes and polarities take the values an
/// erased part holds so that a blank design and an erased device agree
/// about a macrocell nobody touched.
pub fn blank_design(footprint: Footprint) -> Result<PhysicalDesign, DesignError> {
    let _ = regions_for(footprint)?;
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
                pad_enabled: false,
            })
            .collect(),
        global_terms: Vec::new(),
    })
}

/// Turn a design into fuses.
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
pub fn encode_design(design: &PhysicalDesign) -> Result<FuseMap, DesignError> {
    let footprint = Footprint::Gal;
    let mut map = FuseMap::erased(regions_for(footprint)?);
    let matrix = and_matrix()?;
    let specs = macrocells()?;

    for cell in &design.macrocells {
        let spec = specs
            .iter()
            .find(|spec| spec.id == cell.id)
            .ok_or(DesignError::NoSuchMacrocell { macrocell: cell.id.0 })?;

        // A term may only go where its macrocell owns a row. Without
        // this an output's equation could land on another output's pin,
        // and every layer below would encode it faithfully.
        for placed in cell.data_terms.iter().chain(cell.oe_term.iter()) {
            let owned = spec.data_terms.contains(&placed.row) || spec.oe_term == Some(placed.row);
            if !owned {
                return Err(DesignError::TermNotOwned { macrocell: cell.id.0, row: placed.row });
            }
            encode_cube(&mut map, &matrix, placed.row, &placed.cube)?;
        }

        // Every row this macrocell owns that the design did not place a
        // term in is turned off, so it contributes nothing to the sum.
        // An untouched row would contribute a constantly-TRUE term
        // instead, because that is what an erased row is.
        //
        // A macrocell with no output-enable term is one whose pad is not
        // driven, and turning that row off is how this device says so.
        for &row in spec.data_terms.iter().chain(spec.oe_term.iter()) {
            let placed = cell.data_terms.iter().chain(cell.oe_term.iter()).any(|p| p.row == row);
            if !placed {
                disable_row(&mut map, &matrix, row)?;
            }
        }

        encode_macrocell_fields(&mut map, spec, cell)?;
    }

    for placed in &design.global_terms {
        let role = matrix.row(placed.row).map(|row| row.role);
        if !matches!(
            role,
            Some(ProductTermRole::AsynchronousReset | ProductTermRole::SynchronousPreset)
        ) {
            return Err(DesignError::NotAGlobalTerm { row: placed.row });
        }
        encode_cube(&mut map, &matrix, placed.row, &placed.cube)?;
    }

    // Same for the two device-wide rows.
    for row in [ASYNCHRONOUS_RESET_ROW, SYNCHRONOUS_PRESET_ROW] {
        let id = ProductTermId(row);
        if !design.global_terms.iter().any(|placed| placed.row == id) {
            disable_row(&mut map, &matrix, id)?;
        }
    }

    Ok(map)
}

fn encode_macrocell_fields(
    map: &mut FuseMap,
    spec: &MacrocellSpec,
    cell: &MacrocellConfig,
) -> Result<(), DesignError> {
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
/// Total for any map of the right size, because `jed inspect` reads
/// files this compiler did not write. A row nobody programmed decodes
/// to the cube its fuses describe — on an erased part, every literal at
/// both polarities, which is constantly false — rather than being
/// silently omitted.
pub fn decode_design(map: &FuseMap) -> Result<PhysicalDesign, DesignError> {
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
        let pad_enabled = oe_term.is_some();

        cells.push(MacrocellConfig {
            id: spec.id,
            assigned_signal: None,
            mode,
            polarity,
            feedback: FeedbackSource::Pin,
            data_terms,
            oe_term,
            pad_enabled,
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
