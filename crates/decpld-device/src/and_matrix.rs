//! The AND matrix: which fuse connects which literal to which product
//! term. SPEC.md §4.4 and §12.3.
//!
//! This is the layer where a Boolean function stops being a function.
//! Above it a cube is an AND of literals; here it is a set of links in
//! a row, and the mapping between the two is the thing a wrong fuse
//! corrupts.

use crate::{FuseId, FuseMap, FuseWriteError, MacrocellId, PinNumber};
use decpld_logic::{BoolInputId, Cube, Literal, Polarity};
use std::collections::{BTreeMap, BTreeSet};

/// A column of the AND matrix — one sense of one signal source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatrixColumn(pub u32);

/// A product term: one row of the AND matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductTermId(pub u32);

/// What physically drives a matrix column.
///
/// The device layer's answer to "where does this literal come from",
/// kept separate from [`BoolInputId`] because the Boolean layer must
/// not learn what a pin is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PhysicalSignalSource {
    /// A package pin driving the array directly.
    Pin(PinNumber),
    /// A macrocell's output fed back into the array.
    Feedback(MacrocellId),
}

/// What a product term is for.
///
/// SPEC.md §4.7 requires that "every product term belongs to the
/// expected resource". Naming the resource in the row itself makes that
/// checkable rather than a comment: a fitter that placed a data term in
/// an output-enable row would be asking for an output that is enabled
/// by its own logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProductTermRole {
    /// One term of a macrocell's sum.
    Data { macrocell: MacrocellId },
    /// A macrocell's output-enable term.
    OutputEnable { macrocell: MacrocellId },
    /// A device-wide asynchronous reset.
    AsynchronousReset,
    /// A device-wide synchronous preset.
    SynchronousPreset,
}

impl ProductTermRole {
    /// The macrocell this term belongs to, or `None` for a device-wide
    /// term.
    #[must_use]
    pub fn macrocell(self) -> Option<MacrocellId> {
        match self {
            ProductTermRole::Data { macrocell } | ProductTermRole::OutputEnable { macrocell } => {
                Some(macrocell)
            }
            ProductTermRole::AsynchronousReset | ProductTermRole::SynchronousPreset => None,
        }
    }
}

/// One row of the matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductTermSpec {
    pub id: ProductTermId,
    pub role: ProductTermRole,
}

/// One link: the fuse joining a column to a row, and what its two
/// states mean.
///
/// The two values are carried per cell rather than assumed globally
/// because "which value means connected" is a device fact. On the
/// GAL family a `0` is an intact link, so `connected_value` is `false`
/// — the opposite of the reflex reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixCellSpec {
    pub fuse: FuseId,
    pub connected_value: bool,
    pub disconnected_value: bool,
}

/// A signal available to the array, in both senses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiteralSource {
    pub id: BoolInputId,
    pub true_column: MatrixColumn,
    pub complement_column: MatrixColumn,
    pub physical_source: PhysicalSignalSource,
}

impl LiteralSource {
    /// The column carrying this source at `polarity`.
    #[must_use]
    pub fn column(self, polarity: Polarity) -> MatrixColumn {
        match polarity {
            Polarity::True => self.true_column,
            Polarity::Complement => self.complement_column,
        }
    }
}

/// The AND matrix, validated on construction.
///
/// SPEC.md §4.7's "matrix cells map one-to-one to fuses" holds for
/// every value of this type. A matrix that shared a fuse between two
/// cells could not be fixed by care at the call site: connecting one
/// literal would silently connect another, and the design that came out
/// would not be the design that went in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AndMatrixSpec {
    rows: Vec<ProductTermSpec>,
    sources: Vec<LiteralSource>,
    /// Indexed `[row position][column]`, not by `ProductTermId`.
    cells: Vec<Vec<MatrixCellSpec>>,
    row_positions_by_id: BTreeMap<ProductTermId, usize>,
    source_positions_by_id: BTreeMap<BoolInputId, usize>,
    columns: u32,
}

/// Why a matrix definition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MatrixError {
    #[error("a matrix must have at least one row and one column")]
    Empty,
    #[error("a matrix must have at least one literal source")]
    NoSources,
    #[error("column {column:?} carries no literal source")]
    ColumnHasNoSource { column: MatrixColumn },
    #[error("a row of {cells} cells is wider than a column index can address")]
    TooManyColumns { cells: usize },
    #[error("product term {row:?} is defined twice")]
    RowIdUsedTwice { row: ProductTermId },
    #[error("input {input:?} has two literal sources")]
    SourceIdUsedTwice { input: BoolInputId },
    #[error("{cells} rows of cells for {rows} product terms")]
    RowCountMismatch { cells: usize, rows: usize },
    #[error("product term {row:?} has {cells} cells but the matrix has {columns} columns")]
    RaggedRow { row: ProductTermId, cells: u32, columns: u32 },
    #[error("column {column:?} is outside a matrix of {columns} columns")]
    ColumnOutOfRange { column: MatrixColumn, columns: u32 },
    #[error("column {column:?} carries two literal sources")]
    ColumnUsedTwice { column: MatrixColumn },
    #[error("input {input:?} uses column {column:?} for both senses")]
    SenseColumnsCollide { input: BoolInputId, column: MatrixColumn },
    #[error("{fuse:?} appears in two matrix cells")]
    FuseUsedTwice { fuse: FuseId },
    #[error("{fuse:?} has the same value for connected and disconnected")]
    CellCannotDistinguish { fuse: FuseId },
    #[error("array row {row} belongs to no resource")]
    RowHasNoResource { row: u32 },
}

impl AndMatrixSpec {
    /// Build a matrix, checking everything SPEC.md §4.7 requires of one.
    pub fn new(
        rows: Vec<ProductTermSpec>,
        sources: Vec<LiteralSource>,
        cells: Vec<Vec<MatrixCellSpec>>,
    ) -> Result<Self, MatrixError> {
        if rows.is_empty() || cells.first().is_none_or(Vec::is_empty) {
            return Err(MatrixError::Empty);
        }
        if cells.len() != rows.len() {
            return Err(MatrixError::RowCountMismatch { cells: cells.len(), rows: rows.len() });
        }
        if sources.is_empty() {
            return Err(MatrixError::NoSources);
        }
        let columns = u32::try_from(cells[0].len())
            .map_err(|_| MatrixError::TooManyColumns { cells: cells[0].len() })?;

        let mut row_positions_by_id = BTreeMap::new();
        for (position, row) in rows.iter().enumerate() {
            if row_positions_by_id.insert(row.id, position).is_some() {
                return Err(MatrixError::RowIdUsedTwice { row: row.id });
            }
            let width = u32::try_from(cells[position].len())
                .map_err(|_| MatrixError::TooManyColumns { cells: cells[position].len() })?;
            if width != columns {
                return Err(MatrixError::RaggedRow { row: row.id, cells: width, columns });
            }
        }

        // One fuse per cell, across the whole matrix rather than per
        // row: the invariant is about the device, and a fuse shared
        // between two *rows* is the same corruption as one shared
        // within a row.
        let mut seen_fuses = BTreeSet::new();
        for row_cells in &cells {
            for cell in row_cells {
                if cell.connected_value == cell.disconnected_value {
                    return Err(MatrixError::CellCannotDistinguish { fuse: cell.fuse });
                }
                if !seen_fuses.insert(cell.fuse) {
                    return Err(MatrixError::FuseUsedTwice { fuse: cell.fuse });
                }
            }
        }

        let mut source_positions_by_id = BTreeMap::new();
        let mut seen_columns = BTreeSet::new();
        for (position, source) in sources.iter().enumerate() {
            if source_positions_by_id.insert(source.id, position).is_some() {
                return Err(MatrixError::SourceIdUsedTwice { input: source.id });
            }
            if source.true_column == source.complement_column {
                return Err(MatrixError::SenseColumnsCollide {
                    input: source.id,
                    column: source.true_column,
                });
            }
            for column in [source.true_column, source.complement_column] {
                if column.0 >= columns {
                    return Err(MatrixError::ColumnOutOfRange { column, columns });
                }
                if !seen_columns.insert(column) {
                    return Err(MatrixError::ColumnUsedTwice { column });
                }
            }
        }

        // Every column must carry a source. A column nothing can drive
        // is dead silicon: `encode_cube` blows it on every row and
        // `decode_row` never reads it, so a literal routed there would
        // vanish with no layer noticing. If a device ever genuinely has
        // spare columns, this error is where that gets decided
        // deliberately rather than discovered later.
        for column in 0..columns {
            if !seen_columns.contains(&MatrixColumn(column)) {
                return Err(MatrixError::ColumnHasNoSource { column: MatrixColumn(column) });
            }
        }

        Ok(Self { rows, sources, cells, row_positions_by_id, source_positions_by_id, columns })
    }

    #[must_use]
    pub fn rows(&self) -> &[ProductTermSpec] {
        &self.rows
    }

    #[must_use]
    pub fn sources(&self) -> &[LiteralSource] {
        &self.sources
    }

    #[must_use]
    pub fn columns(&self) -> u32 {
        self.columns
    }

    /// The row with this id.
    #[must_use]
    pub fn row(&self, row: ProductTermId) -> Option<ProductTermSpec> {
        self.row_positions_by_id.get(&row).map(|&position| self.rows[position])
    }

    /// Every cell of a row, ascending by column.
    #[must_use]
    pub fn row_cells(&self, row: ProductTermId) -> Option<&[MatrixCellSpec]> {
        self.row_positions_by_id.get(&row).map(|&position| self.cells[position].as_slice())
    }

    /// The literal source for an input.
    #[must_use]
    pub fn source(&self, input: BoolInputId) -> Option<LiteralSource> {
        self.source_positions_by_id.get(&input).map(|&position| self.sources[position])
    }

    /// The cell where a literal meets a row.
    #[must_use]
    pub fn cell_for_literal(&self, row: ProductTermId, literal: Literal) -> Option<MatrixCellSpec> {
        let cells = self.row_cells(row)?;
        let column = self.source(literal.input)?.column(literal.polarity);
        cells.get(column.0 as usize).copied()
    }
}

/// Why a cube could not be encoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    #[error("product term {row:?} is not in this matrix")]
    UnknownRow { row: ProductTermId },
    #[error("product term {row:?}: cube holds input {input:?} at both polarities")]
    ContradictoryCube { row: ProductTermId, input: BoolInputId },
    #[error("product term {row:?}: input {input:?} has no column in this matrix")]
    UnroutableLiteral { row: ProductTermId, input: BoolInputId },
    #[error("product term {row:?}: {source}")]
    Fuse { row: ProductTermId, source: FuseWriteError },
    #[error("product term {row:?}: {source}")]
    Decode { row: ProductTermId, source: DecodeError },
    #[error(
        "product term {row:?}: encoded fuses decode to a different cube \
         (requested {requested:?}, decoded {decoded:?})"
    )]
    RoundTrip { row: ProductTermId, requested: Cube, decoded: Cube },
}

/// Why a row could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("product term {row:?} is not in this matrix")]
    UnknownRow { row: ProductTermId },
    #[error("product term {row:?}: {fuse:?} is outside the fuse map")]
    FuseOutOfRange { row: ProductTermId, fuse: FuseId },
}

/// Decode a row from a function supplying each cell's state.
///
/// Shared by [`decode_row`], which reads a committed `FuseMap`, and by
/// [`encode_cube`], which reads the values it is *about* to write. One
/// definition, so a check performed before committing and an inspection
/// performed after cannot disagree.
///
/// `None` means a cell's state was unavailable. For a validated
/// [`AndMatrixSpec`] the column lookup cannot fail — rows are not
/// ragged and every column has a source — so the only real cause is a
/// fuse outside the map.
fn decode_cells(
    matrix: &AndMatrixSpec,
    cells: &[MatrixCellSpec],
    mut state_of: impl FnMut(FuseId) -> Option<bool>,
) -> Option<Cube> {
    let mut literals = Vec::new();
    for source in matrix.sources() {
        for polarity in [Polarity::True, Polarity::Complement] {
            let cell = cells.get(source.column(polarity).0 as usize)?;
            if state_of(cell.fuse)? == cell.connected_value {
                literals.push(Literal::new(source.id, polarity));
            }
        }
    }
    Some(Cube::new(literals).canonical())
}

/// Encode a cube into a product-term row, verifying before it commits.
///
/// SPEC.md §12.3, with §4.4's "and then decodes the row to verify it"
/// built in rather than left to the caller, so a caller cannot do the
/// first without the second (CLAUDE.md → *Make wrong states
/// unrepresentable*).
///
/// **Nothing is written unless everything can be.** Contradiction,
/// routability, and the round-trip are all checked against values that
/// have not been committed; the single `set_all` is the last thing this
/// function does. A half-encoded row is the worst outcome available —
/// neither the old design nor the new one, with a fuse checksum that
/// would not blink — so no failure path may leave one. Verifying after
/// the write would make that guarantee depend on the check never
/// failing, which is not a guarantee at all.
///
/// The comparison is between *canonical* cubes. A row is a set of
/// connections with no order to preserve, so requiring the decoded
/// literals to arrive in the order they were written would fail on
/// correct output.
///
/// # What this round-trip does and does not catch
///
/// It re-reads the values through the *same* [`AndMatrixSpec`], so it
/// catches a regression in this encoding path or in `FuseMap`.
///
/// It **cannot** catch a wrong device table. A mistaken column, fuse
/// number, or row block is applied identically in both directions, so
/// the comparison passes and the fuses are confidently wrong.
///
/// CLAUDE.md's *encode, then decode, then compare* is a statement about
/// the whole compile driver (SPEC.md §12.1): generate fuses, decode them
/// into a physical design, and compare with the design that was
/// intended. This local check is **not** a substitute, and reading it as
/// one would be the more dangerous mistake precisely because it uses the
/// same words.
///
/// It follows that [`EncodeError::RoundTrip`] cannot fire for any input
/// against a validated matrix, and that **no test demonstrates the
/// verify-before-commit ordering** — moving the `set_all` above the
/// comparison changes no observable behaviour while the comparison
/// never fails. The ordering is kept because it is free and because it
/// is what makes the atomicity guarantee structural rather than
/// contingent: if a future change relaxes a matrix invariant and makes
/// the check reachable, a version that committed first would leave the
/// half-encoded row this function promises cannot exist.
pub fn encode_cube(
    map: &mut FuseMap,
    matrix: &AndMatrixSpec,
    row: ProductTermId,
    cube: &Cube,
) -> Result<(), EncodeError> {
    let cells = matrix.row_cells(row).ok_or(EncodeError::UnknownRow { row })?;

    // Canonical first, so two callers that build the same literal set in
    // different orders get the same diagnostic rather than whichever
    // contradiction their `Vec` order happened to reach first.
    let cube = cube.canonical();

    if let Some(input) = cube.contradiction() {
        return Err(EncodeError::ContradictoryCube { row, input });
    }

    // Resolve every literal before deciding any fuse value.
    let mut connected = BTreeSet::new();
    for &literal in &cube.literals {
        let cell = matrix
            .cell_for_literal(row, literal)
            .ok_or(EncodeError::UnroutableLiteral { row, input: literal.input })?;
        connected.insert(cell.fuse);
    }

    // Every cell of the row gets its final state in ONE write. Writing
    // the disconnected state to the whole row and then connecting the
    // literals would make the row fight its own first pass: the fuse
    // map tracks writes, so the second write to a literal's cell is a
    // conflict with the first — an encoder disagreeing with itself.
    //
    // The whole row is written, not just the connected cells, so the
    // result depends on the cube alone rather than on whatever the row
    // held before.
    let writes: Vec<(FuseId, bool)> = cells
        .iter()
        .map(|cell| {
            let value = if connected.contains(&cell.fuse) {
                cell.connected_value
            } else {
                cell.disconnected_value
            };
            (cell.fuse, value)
        })
        .collect();

    // Decode what is about to be written, compare, and only then
    // commit.
    let pending: BTreeMap<FuseId, bool> = writes.iter().copied().collect();
    let decoded = decode_cells(matrix, cells, |fuse| pending.get(&fuse).copied()).ok_or(
        EncodeError::Decode {
            row,
            source: DecodeError::FuseOutOfRange { row, fuse: FuseId(u32::MAX) },
        },
    )?;
    if decoded != cube {
        return Err(EncodeError::RoundTrip { row, requested: cube, decoded });
    }

    map.set_all(writes).map_err(|source| EncodeError::Fuse { row, source })
}

/// Read a product-term row back out of a fuse map.
///
/// Total over any fuse vector, because `jed inspect` reads files this
/// compiler did not write. A row holding an input at both polarities is
/// physically meaningful — constantly false — so it decodes to a cube
/// that *is* a contradiction rather than to an error or, worse, to one
/// literal with the other quietly dropped.
///
/// The result is **canonical** — sorted and de-duplicated — so a decoded
/// row compares equal to the cube that produced it whatever order either
/// was built in. Source order alone would not guarantee that: nothing
/// requires a matrix to declare its sources in ascending
/// [`BoolInputId`].
pub fn decode_row(
    map: &FuseMap,
    matrix: &AndMatrixSpec,
    row: ProductTermId,
) -> Result<Cube, DecodeError> {
    let cells = matrix.row_cells(row).ok_or(DecodeError::UnknownRow { row })?;

    decode_cells(matrix, cells, |fuse| map.get(fuse)).ok_or_else(|| {
        // Naming the offending fuse costs a second pass and is the
        // difference between a diagnostic a user can act on and one that
        // says only that a row failed.
        let fuse = cells
            .iter()
            .map(|cell| cell.fuse)
            .find(|&fuse| map.get(fuse).is_none())
            .unwrap_or(FuseId(u32::MAX));
        DecodeError::FuseOutOfRange { row, fuse }
    })
}
