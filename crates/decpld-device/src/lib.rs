//! Device-independent fuse maps and target descriptions. SPEC.md §4.
//!
//! This layer knows what a fuse *is* — a numbered cell with a state, a
//! classification, and rules about who may write it. It knows nothing
//! about JEDEC syntax, and nothing above it may name a fuse at all.
//!
//! ```text
//! language semantics    behaviour
//! decpld-logic          behaviour as Boolean functions
//! THIS LAYER            product terms, macrocells, fuses
//! decpld-jedec          bytes on disk
//! ```
//!
//! This crate depends on `decpld-logic`, and that direction is the
//! correct one. Dependencies flow downward: the layer that *maps*
//! behaviour to hardware must be able to name the behaviour it is
//! mapping, so a matrix column can say which Boolean input it carries.
//! The reverse would be the violation — `decpld-logic` contains no
//! fuse, pin, macrocell, or column, and must not.

mod and_matrix;
mod config;
mod evidence;
mod fuse_map;
mod meaning;
mod package;
mod region;

pub use and_matrix::{
    AndMatrixSpec, DecodeError, EncodeError, LiteralSource, MatrixCellSpec, MatrixColumn,
    MatrixError, PhysicalSignalSource, ProductTermId, ProductTermRole, ProductTermSpec, decode_row,
    disable_row, encode_cube, row_is_never_true,
};
pub use config::{
    ConfigField, ConfigFieldError, ConfigFieldId, FeedbackSource, LogicalOutputId, MacrocellConfig,
    MacrocellMode, MacrocellSpec, OutputPolarity, PhysicalDesign, PlacedCube,
};
pub use evidence::{CombinedEvidence, Evidence, EvidenceLevel};
pub use fuse_map::{FuseMap, FuseStatesError, FuseWriteError};
pub use meaning::{FuseMeaning, classify_fuse};
pub use package::{
    ClockResourceId, InputResourceId, MacrocellId, PackageError, PackageId, PackagePin,
    PackageSpec, PadId, PinNumber, PowerRail,
};
pub use region::{FuseId, FuseMutability, FuseRegion, FuseRegions, RegionError};
