//! What a fuse means. SPEC.md §7.5.
//!
//! `decpld-jedec` can say that fuse 52 changed. It cannot say what fuse
//! 52 *does*, and must not learn — it has to stay usable against a
//! device it has never heard of. So the classification lives here, in
//! the layer that owns fuse semantics, and `decpld oracle diff --device`
//! consults it.
//!
//! **No new device knowledge is introduced.** Everything below is
//! already stated by an [`AndMatrixSpec`], a [`MacrocellSpec`], or a
//! [`FuseRegions`], which is why one implementation serves every
//! target: a device model that describes itself gets classification
//! without writing any.

use crate::and_matrix::{AndMatrixSpec, PhysicalSignalSource, ProductTermRole};
use crate::config::{ConfigFieldId, MacrocellSpec};
use crate::package::MacrocellId;
use crate::region::{FuseId, FuseMutability, FuseRegions};
use decpld_logic::{BoolInputId, Polarity};
use std::fmt;

/// What one fuse controls.
///
/// The vocabulary SPEC.md §7.5 asks a delta to be classified into. A
/// device-independent shape, because the question "what does this fuse
/// do" has the same *kinds* of answer on every PAL/GAL-family part even
/// though the answers differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuseMeaning {
    /// One link of the AND array: a literal in a product term.
    ///
    /// The most informative case and the one differential experiments
    /// live on. "Fuse 52 changed" is not a finding; "macrocell 9's
    /// output-enable term gained pin 3, true sense" is.
    MatrixCell {
        row: crate::and_matrix::ProductTermId,
        role: ProductTermRole,
        input: BoolInputId,
        polarity: Polarity,
        source: PhysicalSignalSource,
    },

    /// A bit of a macrocell's configuration — polarity, mode, and
    /// whatever else a target encodes as a [`crate::ConfigField`].
    ///
    /// `macrocell` is `None` for a device-wide field, which no target
    /// here has yet but the ATF16V8's global mode will be.
    ConfigField {
        macrocell: Option<MacrocellId>,
        field: &'static str,
        id: ConfigFieldId,
        /// Position within the field, in the field's own bit order.
        bit: u32,
    },

    /// A bit of the user signature, by byte and bit within the region.
    ///
    /// Almost never an interesting delta on its own — it is usually the
    /// design's name leaking into the fuse map — so it must be
    /// recognisable at a glance rather than mistaken for logic.
    UserSignature { region: &'static str, byte: u32, bit: u32 },

    /// The readback lock. The one delta that cannot be undone on
    /// hardware.
    SecurityFuse,

    /// A fuse the device model accounts for but this vocabulary has no
    /// name for — a reserved bit, or a programmable region no matrix
    /// cell or configuration field claims.
    Other { region: &'static str, mutability: FuseMutability },

    /// A fuse the device model does not have.
    ///
    /// Reported rather than guessed at. `oracle diff` and `jed inspect`
    /// both read files this compiler did not write, and inventing a
    /// meaning for an address outside the model would be exactly the
    /// unevidenced guess this project forbids.
    Unknown { fuse: FuseId },
}

/// Say what a fuse controls on this device.
///
/// Total: every fuse gets an answer, and one outside the model gets
/// [`FuseMeaning::Unknown`] rather than a plausible-looking guess.
///
/// The matrix is consulted **before** the regions, deliberately. A
/// region can only say "programmable"; the matrix knows which literal
/// of which product term a cell carries, and that is the whole content
/// of a matrix delta. Asking the coarser source first would throw the
/// finer answer away.
#[must_use]
pub fn classify_fuse(
    fuse: FuseId,
    matrix: &AndMatrixSpec,
    specs: &[MacrocellSpec],
    regions: &FuseRegions,
) -> FuseMeaning {
    if let Some(meaning) = matrix_meaning(fuse, matrix) {
        return meaning;
    }
    if let Some(meaning) = config_meaning(fuse, specs) {
        return meaning;
    }

    let Some(region) = regions.region_of(fuse) else {
        return FuseMeaning::Unknown { fuse };
    };

    match region.mutability {
        FuseMutability::Security => FuseMeaning::SecurityFuse,
        FuseMutability::UserSignature => {
            // Offset within the region, packed eight bits to a byte.
            // Which *end* of a byte comes first is a device fact and is
            // not decided here: this reports a position, and a reader
            // that needs the value asks the target.
            let offset = fuse.0.saturating_sub(region.range.start);
            FuseMeaning::UserSignature { region: region.name, byte: offset / 8, bit: offset % 8 }
        }
        mutability => FuseMeaning::Other { region: region.name, mutability },
    }
}

/// The array cell at this fuse, if any.
fn matrix_meaning(fuse: FuseId, matrix: &AndMatrixSpec) -> Option<FuseMeaning> {
    let (row, column) = matrix.position_of_fuse(fuse)?;
    let source = matrix
        .sources()
        .iter()
        .find(|source| source.true_column.0 == column || source.complement_column.0 == column)?;
    let polarity =
        if source.true_column.0 == column { Polarity::True } else { Polarity::Complement };

    Some(FuseMeaning::MatrixCell {
        row: row.id,
        role: row.role,
        input: source.id,
        polarity,
        source: source.physical_source,
    })
}

/// The configuration bit at this fuse, if any.
///
/// The macrocells are walked in order and each cell's polarity field
/// before its mode field, so a fuse claimed by two fields would resolve
/// to the same one on every run (SPEC.md §13.2). Nothing currently
/// forbids that overlap — `MacrocellSpec` does not cross-check its
/// fields — so the tie-break is stated rather than left to whichever
/// happens to be visited first.
fn config_meaning(fuse: FuseId, specs: &[MacrocellSpec]) -> Option<FuseMeaning> {
    for spec in specs {
        if let Some(field) = &spec.polarity_field
            && let Some(meaning) =
                field_meaning(field.bits(), field.name(), field.id(), spec.id, fuse)
        {
            return Some(meaning);
        }
        if let Some(field) = &spec.mode_field
            && let Some(meaning) =
                field_meaning(field.bits(), field.name(), field.id(), spec.id, fuse)
        {
            return Some(meaning);
        }
    }
    None
}

/// Taken as parts rather than as a `&ConfigField<T>` so one body serves
/// every value type a target encodes.
fn field_meaning(
    bits: &[FuseId],
    name: &'static str,
    id: ConfigFieldId,
    macrocell: MacrocellId,
    fuse: FuseId,
) -> Option<FuseMeaning> {
    let bit = bits.iter().position(|held| *held == fuse)?;
    Some(FuseMeaning::ConfigField {
        macrocell: Some(macrocell),
        field: name,
        id,
        bit: u32::try_from(bit).unwrap_or(u32::MAX),
    })
}

impl fmt::Display for FuseMeaning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FuseMeaning::MatrixCell { row, role, polarity, source, .. } => {
                let sense = match polarity {
                    Polarity::True => "",
                    Polarity::Complement => "!",
                };
                write!(f, "{role} {row}, literal {sense}{source}")
            }
            FuseMeaning::ConfigField { macrocell: Some(macrocell), field, bit, .. } => {
                write!(f, "{macrocell} {field} bit {bit}")
            }
            FuseMeaning::ConfigField { macrocell: None, field, bit, .. } => {
                write!(f, "device-wide {field} bit {bit}")
            }
            FuseMeaning::UserSignature { region, byte, bit } => {
                write!(f, "{region} byte {byte} bit {bit}")
            }
            FuseMeaning::SecurityFuse => write!(f, "the security fuse"),
            FuseMeaning::Other { region, .. } => write!(f, "region `{region}`"),
            FuseMeaning::Unknown { fuse } => write!(f, "{fuse}, which this device does not have"),
        }
    }
}

impl fmt::Display for PhysicalSignalSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PhysicalSignalSource::Pin(pin) => write!(f, "{pin}"),
            PhysicalSignalSource::Feedback(macrocell) => write!(f, "{macrocell} feedback"),
        }
    }
}

impl fmt::Display for ProductTermRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProductTermRole::Data { macrocell } => write!(f, "{macrocell} data term"),
            ProductTermRole::OutputEnable { macrocell } => {
                write!(f, "{macrocell} output-enable term")
            }
            ProductTermRole::AsynchronousReset => write!(f, "the asynchronous-reset term"),
            ProductTermRole::SynchronousPreset => write!(f, "the synchronous-preset term"),
        }
    }
}
