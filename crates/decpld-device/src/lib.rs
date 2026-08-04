//! Device-independent fuse maps and target descriptions. SPEC.md §4.
//!
//! This layer knows what a fuse *is* — a numbered cell with a state, a
//! classification, and rules about who may write it. It knows nothing
//! about JEDEC syntax, and nothing above it may name a fuse at all.
//!
//! ```text
//! language semantics    behaviour
//! target-independent IR behaviour, optimised
//! THIS LAYER            product terms, macrocells, fuses
//! decpld-jedec          bytes on disk
//! ```

mod fuse_map;
mod package;
mod region;

pub use fuse_map::{FuseMap, FuseWriteError};
pub use package::{
    ClockResourceId, InputResourceId, PackageError, PackageId, PackagePin, PackageSpec, PadId,
    PinNumber, PowerRail,
};
pub use region::{FuseId, FuseMutability, FuseRegion, FuseRegions, RegionError};
