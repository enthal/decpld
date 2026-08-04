//! The ATF22V10C's AND matrix, assembled from the measured geometry.
//!
//! No new device knowledge lives here. Every number comes from
//! [`crate::geometry`], which cites the experiment that measured it;
//! this module's job is to put those numbers into the shape
//! `decpld-device` encodes and decodes through.

use crate::geometry::{
    ASYNCHRONOUS_RESET_ROW, ArrayCell, Atf22v10Geometry, SYNCHRONOUS_PRESET_ROW,
};
use crate::geometry::{COLUMNS, MacrocellIndex, ROWS, SourceKind};
use decpld_device::{
    AndMatrixSpec, LiteralSource, MacrocellId, MatrixCellSpec, MatrixColumn, MatrixError,
    PhysicalSignalSource, ProductTermId, ProductTermRole, ProductTermSpec,
};
use decpld_logic::BoolInputId;

/// A product term is named by its array row.
///
/// The alternative — numbering terms 0..132 in some other order — would
/// add a mapping nobody needs and one more place for a transposition to
/// hide. A row *is* a product term on this device.
#[must_use]
pub fn product_term_of_row(row: u32) -> ProductTermId {
    ProductTermId(row)
}

/// A literal source is named by its array source index.
///
/// Same reasoning, and it matches the package map, where an
/// `InputResourceId` is also the source index. One number, not three
/// that must agree.
#[must_use]
pub fn bool_input_of_source(index: u8) -> BoolInputId {
    BoolInputId(u32::from(index))
}

/// A link is intact when its fuse is `0`.
///
/// Evidence: `jedec-3a` lines 344-348, "a zero, specifying a low
/// resistance link (a logical connection between two points); or a one,
/// specifying a high resistance link". The document's identity is its
/// sha256 in `targets/evidence/references.toml`.
///
/// Every experiment in `targets/evidence/atf22v10-fuse-map.md` reads
/// consistently with it — a single-literal design leaves exactly one
/// fuse at `0`, at the column measured for the pin driving it. That
/// corroboration is **not independent**: this project's reader and its
/// encoder share the convention, so a world where both are inverted
/// produces identical observations. Only hardware settles it, and
/// nothing here is `HardwareVerified`.
///
/// Level: `DatasheetSpecified` (SPEC.md §13.1) — one witness, the
/// standard itself.
///
/// This is the reverse of the reflex reading, and getting it backwards
/// inverts every product term on the device while still producing a
/// file with a valid checksum.
const CONNECTED: bool = false;
const DISCONNECTED: bool = true;

/// Build the AND matrix.
///
/// # Errors
///
/// If the geometry ever describes a matrix that violates
/// `AndMatrixSpec`'s invariants — a repeated fuse, two sources on one
/// column, a column past the end. It is returned rather than panicked
/// because the check is the point: this function is where the measured
/// tables meet the rules they are supposed to satisfy, and a device
/// model that fails them should say so, not abort.
pub fn and_matrix() -> Result<AndMatrixSpec, MatrixError> {
    let geometry = Atf22v10Geometry;

    // Rows, in array order. Every row is claimed by exactly one role:
    // the two device-wide control terms bracket the array, and the ten
    // macrocell blocks cover everything between.
    let mut rows = Vec::with_capacity(ROWS as usize);
    for row in 0..ROWS {
        let role = role_of_row(row).ok_or(MatrixError::RowHasNoResource { row })?;
        rows.push(ProductTermSpec { id: product_term_of_row(row), role });
    }

    let sources = geometry
        .sources()
        .map(|source| LiteralSource {
            id: bool_input_of_source(source.index),
            true_column: MatrixColumn(source.true_column()),
            complement_column: MatrixColumn(source.complement_column()),
            physical_source: match source.kind {
                SourceKind::Input(pin) => PhysicalSignalSource::Pin(pin),
                SourceKind::Feedback(macrocell) => {
                    PhysicalSignalSource::Feedback(macrocell_id(macrocell))
                }
            },
        })
        .collect();

    let mut cells = Vec::with_capacity(ROWS as usize);
    for row in 0..ROWS {
        let mut row_cells = Vec::with_capacity(COLUMNS as usize);
        for column in 0..COLUMNS {
            // `fuse_of` is the measured row-major addressing. If it ever
            // answered `None` inside these bounds the matrix would be
            // short a cell, and `AndMatrixSpec::new` would refuse it as
            // a ragged row rather than encode into a hole.
            let Some(fuse) = geometry.fuse_of(ArrayCell { row, column }) else {
                break;
            };
            row_cells.push(MatrixCellSpec {
                fuse,
                connected_value: CONNECTED,
                disconnected_value: DISCONNECTED,
            });
        }
        cells.push(row_cells);
    }

    AndMatrixSpec::new(rows, sources, cells)
}

/// The device-layer id for a macrocell.
///
/// `MacrocellIndex` and `MacrocellId` carry the same number: the
/// pin-ascending index this crate documents. The conversion exists so
/// the crate boundary is visible, not to renumber anything.
#[must_use]
pub fn macrocell_id(macrocell: MacrocellIndex) -> MacrocellId {
    MacrocellId(macrocell.0)
}

/// What owns a row.
///
/// Evidence: the row blocks of `crate::geometry`, measured one design
/// per macrocell (`mc14` … `mc23`), plus `global-ar-sp` for rows 0 and
/// 131. The first row of each block is that macrocell's output enable
/// and the rest are data terms.
/// `None` if no resource owns the row, which for this device cannot
/// happen: the ten blocks plus rows 0 and 131 cover 0..132 exactly, and
/// `row_blocks_partition_the_array_leaving_only_the_two_control_rows`
/// states it. Returning `None` rather than a plausible role means a
/// geometry change that broke the partition would be refused by
/// `and_matrix` instead of quietly relabelling a data row as the
/// device-wide reset — a build that fails beats one that is wrong.
fn role_of_row(row: u32) -> Option<ProductTermRole> {
    if row == ASYNCHRONOUS_RESET_ROW {
        return Some(ProductTermRole::AsynchronousReset);
    }
    if row == SYNCHRONOUS_PRESET_ROW {
        return Some(ProductTermRole::SynchronousPreset);
    }
    let geometry = Atf22v10Geometry;
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let macrocell = MacrocellIndex(index);
        let Some(block) = geometry.row_block(macrocell) else { continue };
        if block.output_enable_row == row {
            return Some(ProductTermRole::OutputEnable { macrocell: macrocell_id(macrocell) });
        }
        if block.data_rows.contains(&row) {
            return Some(ProductTermRole::Data { macrocell: macrocell_id(macrocell) });
        }
    }
    None
}
