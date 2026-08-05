//! What established each of this device's mappings. SPEC.md §13.1.
//!
//! Every constant in this crate already cites its experiment in a
//! source comment. A comment cannot be checked, though: an experiment
//! can be renamed or deleted and the citation goes on reading
//! plausibly. This states the same citations as data, so a test can
//! require every named experiment to exist and every production
//! mapping to clear the threshold.
//!
//! It is deliberately not a second copy of the *fuse numbers*. Those
//! live once, in [`crate::geometry`] and [`crate::regions`]; a parallel
//! table of addresses would be exactly the drift this project cannot
//! afford. What is recorded here is how well each mapping is
//! established and by what.

use decpld_device::{Evidence, EvidenceLevel};

/// A device mapping, named for what it establishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mapping {
    /// Cell (row, column) is fuse 44·row + column.
    ArrayAddressing,
    /// Which column carries which signal, in which sense.
    ColumnMap,
    /// Which rows belong to which macrocell.
    RowBlocks,
    /// How many data terms each macrocell has, and what happens past it.
    Capacity,
    /// S0 is polarity, S1 is mode, pin-descending.
    ArchitectureBits,
    /// The enable row holds the output-enable term; all-intact is off.
    OutputEnable,
    /// Which pin does what on the DIP-24 package.
    PinRoles,
    /// The three JEDEC fuse counts.
    Footprints,
    /// 64 bits of user signature carrying CUPL's `PartNo`.
    UserSignature,
    /// A `0` in the array is an intact link.
    LinkConvention,
}

impl Mapping {
    /// Every mapping this crate claims.
    pub const ALL: [Mapping; 10] = [
        Mapping::ArrayAddressing,
        Mapping::ColumnMap,
        Mapping::RowBlocks,
        Mapping::Capacity,
        Mapping::ArchitectureBits,
        Mapping::OutputEnable,
        Mapping::PinRoles,
        Mapping::Footprints,
        Mapping::UserSignature,
        Mapping::LinkConvention,
    ];

    /// What established it.
    ///
    /// The levels are the ones the evidence document argues for, not a
    /// flattering summary of them. `LinkConvention` is the one to read
    /// twice: every experiment is *consistent* with a `0` meaning
    /// connected, and none of them is independent corroboration,
    /// because this project's reader and its encoder share the
    /// convention — a world where both are inverted produces identical
    /// observations. Only hardware settles it, and nothing here is
    /// `HardwareVerified`.
    #[must_use]
    pub fn evidence(self) -> Evidence {
        use EvidenceLevel::{DatasheetSpecified, DifferentiallyVerified, OpenSourceCrossChecked};
        match self {
            // Measured in absolute addresses, then found to agree with
            // two independent implementations.
            Mapping::ArrayAddressing => Evidence::new(
                OpenSourceCrossChecked,
                &["in1", "in2", "in3", "in4", "nc13", "mc14", "mc19", "global-ar-sp"],
            ),
            Mapping::ColumnMap => Evidence::new(
                DifferentiallyVerified,
                &[
                    "in1", "in2", "in3", "in11", "in13", "nc2", "nc3", "nc11", "nc13", "nfb22",
                    "nfb18", "nfb14", "fb14", "fb22", "fb23",
                ],
            ),
            Mapping::RowBlocks => Evidence::new(
                OpenSourceCrossChecked,
                &["in2", "fb22", "mc14", "mc19", "cap23-8", "cap19-16", "cap14-8"],
            ),
            // Three of ten blocks filled, and the term past each
            // refused. The other seven sizes are cross-checked only.
            Mapping::Capacity => Evidence::new(
                OpenSourceCrossChecked,
                &["cap23-8", "cap23-9", "cap19-16", "cap19-17", "cap14-8", "cap14-9"],
            ),
            Mapping::ArchitectureBits => Evidence::new(
                DifferentiallyVerified,
                &[
                    "arch-comb-high",
                    "arch-comb-low",
                    "arch-reg-high",
                    "arch-reg-low",
                    "mc14",
                    "mc19",
                    "mc23",
                ],
            ),
            Mapping::OutputEnable => Evidence::new(
                DifferentiallyVerified,
                &["oe-always", "oe-var", "oe-var-not", "oe-never", "oe-bidir"],
            ),
            // Roles the datasheet states and the compiler's refusals
            // corroborate.
            Mapping::PinRoles => Evidence::new(
                DifferentiallyVerified,
                &["clk-shared", "pwr12", "pwr24", "marker-inert", "ioin14", "ioin23"],
            ),
            Mapping::Footprints => {
                Evidence::new(DifferentiallyVerified, &["mode-pal", "mode-powerdown"])
            }
            Mapping::UserSignature => {
                Evidence::new(DifferentiallyVerified, &["sig-partno-41", "sig-partno-5A"])
            }
            // One witness: JEDEC 3A itself. Every experiment agrees
            // with it and none corroborates it.
            Mapping::LinkConvention => Evidence::new(DatasheetSpecified, &["jedec-3a"]),
        }
    }
}
