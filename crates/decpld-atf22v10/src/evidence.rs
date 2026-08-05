//! What established each of this device's mappings. SPEC.md §13.1.
//!
//! Every constant in this crate already cites its experiment in a
//! source comment. A comment cannot be checked, though: an experiment
//! can be renamed or deleted and the citation goes on reading
//! plausibly. This states the same citations as data, so a test can
//! require every named experiment to exist, every document reference to
//! be registered, and every production mapping to clear the threshold.
//!
//! It is deliberately not a second copy of the *fuse numbers*. Those
//! live once, in [`crate::geometry`] and [`crate::regions`]; a parallel
//! table of addresses would be exactly the drift this project cannot
//! afford. What is recorded here is how well each mapping is
//! established and by what.
//!
//! **The document is the argument; this is a projection of it.** Each
//! mapping names the sections of `targets/evidence/atf22v10-fuse-map.md`
//! it comes from, and a test requires the experiments cited here to be
//! exactly the ones those sections name. Levels and citations otherwise
//! live in two places at once, and the pair drifts silently — which is
//! the same failure as a stale comment, one storey up.

use decpld_device::Evidence;
use decpld_device::EvidenceLevel::{
    DatasheetSpecified, DifferentiallyVerified, OpenSourceCrossChecked,
};

/// Declare the mappings, their evidence, and where each is argued for.
///
/// One declaration, so a variant cannot exist without an entry in
/// [`Mapping::ALL`] and evidence to go with it. Written out as three
/// separate items — the enum, the array, the match — a new variant
/// compiles with the array unchanged, and a mapping absent from `ALL` is
/// a mapping no test ever looks at.
macro_rules! mappings {
    ($(
        $(#[$attr:meta])*
        $name:ident {
            evidence: $evidence:expr,
            document_sections: [$($section:literal),* $(,)?] $(,)?
        }
    )+) => {
        /// A device mapping, named for what it establishes.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Mapping {
            $( $(#[$attr])* $name, )+
        }

        impl Mapping {
            /// Every mapping this crate claims.
            pub const ALL: &'static [Mapping] = &[ $( Mapping::$name, )+ ];

            /// What established it.
            #[must_use]
            pub fn evidence(self) -> Evidence {
                match self { $( Mapping::$name => $evidence, )+ }
            }

            /// The headings of `targets/evidence/atf22v10-fuse-map.md`
            /// that argue for it.
            ///
            /// Empty for a mapping no experiment touched, which then
            /// may cite no experiment at all.
            #[must_use]
            pub fn document_sections(self) -> &'static [&'static str] {
                match self { $( Mapping::$name => &[$($section),*], )+ }
            }
        }
    };
}

mappings! {
    /// Cell (row, column) is fuse 44·row + column.
    ///
    /// Measured in absolute addresses across six product terms, then
    /// found to agree with two independent implementations.
    ArrayAddressing {
        evidence: Evidence::established(
            OpenSourceCrossChecked,
            &[
                "in1", "in2", "in3", "in4", "nc13", "mc14", "mc19", "global-ar-sp",
                "galette", "galasm",
            ],
        ),
        document_sections: ["Fuse addressing: the array is row-major, stride 44"],
    }

    /// Which column carries which signal, in which sense.
    ///
    /// WinCUPL is the only witness. Both sweeps are cited in full
    /// because the input and feedback column maps run in opposite
    /// directions, so the ends do not imply the middle.
    ColumnMap {
        evidence: Evidence::established(
            DifferentiallyVerified,
            &[
                "in1", "in2", "in3", "in4", "in5", "in6", "in7", "in8", "in9", "in10", "in11",
                "in13", "nc2", "nc3", "nc11", "nc13", "nfb14", "nfb18", "nfb22", "fb14", "fb15",
                "fb16", "fb17", "fb18", "fb19", "fb20", "fb21", "fb22", "fb23",
            ],
        ),
        document_sections: ["Columns: true and complement", "Columns: signal sources"],
    }

    /// Which rows belong to which macrocell, and which two belong to
    /// the device.
    ///
    /// The measured row-start table turns out identical to Galette's
    /// `OLMC_ROWS_22V10`, which makes that a cross-check rather than the
    /// source it once was.
    ///
    /// The `cap*` designs belong here and not only to capacity: filling
    /// a block is what measured which rows it owns, and the refusal
    /// past the end is what shows the next row belongs to the next
    /// macrocell's enable rather than being spare. `geometry.rs`'s
    /// `row_block` cites them by name for exactly that.
    ///
    /// GALasm is deliberately **not** cited. It fixes the row count at
    /// 132; the block table is Galette's, and claiming a second
    /// implementation for it would be the overcounting this type exists
    /// to prevent.
    RowBlocks {
        evidence: Evidence::established(
            OpenSourceCrossChecked,
            &[
                "in2", "fb22", "arch-comb-high", "global-ar-sp", "mc14", "mc15", "mc16", "mc17",
                "mc18", "mc19", "mc20", "mc21", "mc22", "mc23", "cap23-8", "cap23-9", "cap19-16",
                "cap19-17", "cap14-8", "cap14-9", "galette",
            ],
        ),
        document_sections: [
            "Rows: macrocell blocks",
            "S0/S1 pair order is reversed relative to the row blocks",
            "Rows 0 and 131: the device-wide control terms",
            "Capacity",
        ],
    }

    /// How many data terms pins 14, 19 and 23 have, and what happens
    /// past each.
    ///
    /// Three blocks filled and the term past each refused — both
    /// eight-term ends and the widest block in the middle — agreeing
    /// with Galette's `OLMC_SIZE_22V10` and the datasheet's "8 TO 16
    /// PRODUCT TERMS".
    CapacityMeasured {
        evidence: Evidence::established(
            OpenSourceCrossChecked,
            &[
                "cap23-8", "cap23-9", "cap19-16", "cap19-17", "cap14-8", "cap14-9",
                "galette", "atf22v10c-datasheet",
            ],
        ),
        document_sections: ["Capacity"],
    }

    /// How many data terms the other seven macrocells have.
    ///
    /// Two documents agreeing and no experiment. Not
    /// `OpenSourceCrossChecked`: the ladder is a total order, so that
    /// rung asserts the differential beneath it, and no design has ever
    /// filled these seven blocks. Held apart from
    /// [`Mapping::CapacityMeasured`] rather than averaged into it,
    /// because "measured at both ends, assumed in the middle" is the
    /// shape this device has already been described wrongly in once.
    CapacityCrossChecked {
        evidence: Evidence::established(
            DatasheetSpecified,
            &["galette", "atf22v10c-datasheet"],
        ),
        document_sections: [],
    }

    /// S0 is polarity, S1 is mode, pin-descending.
    ///
    /// WinCUPL-only, and deliberately so: cross-checking the open-source
    /// implementations surfaced the reversed pair order as a discrepancy
    /// and could not settle it. Measured for all ten macrocells, because
    /// the two orderings run in opposite directions and an interpolation
    /// would look right at both ends.
    ArchitectureBits {
        evidence: Evidence::established(
            DifferentiallyVerified,
            &[
                "arch-comb-high", "arch-comb-low", "arch-reg-high", "arch-reg-low", "fb22",
                "mc14", "mc15", "mc16", "mc17", "mc18", "mc19", "mc20", "mc21", "mc22", "mc23",
            ],
        ),
        document_sections: [
            "Architecture bits S0 and S1",
            "S0/S1 pair order is reversed relative to the row blocks",
        ],
    }

    /// The enable row holds the output-enable term; all-intact is off.
    ///
    /// `in2` is the baseline: it writes no `.oe` at all, and every
    /// finding here is a difference from it.
    OutputEnable {
        evidence: Evidence::established(
            DifferentiallyVerified,
            &["in2", "oe-always", "oe-var", "oe-var-not", "oe-never", "oe-bidir"],
        ),
        document_sections: ["Output enable"],
    }

    /// Which pin does what on the DIP-24 package.
    ///
    /// `DatasheetSpecified`, not `DifferentiallyVerified`, and the
    /// distinction is the point: no fuse experiment can observe what a
    /// pin is bonded to. The refusals show that WinCUPL will not route a
    /// signal to pins 12 and 24; that one is ground and the other is the
    /// supply is a single-document claim, and this entry spans both
    /// halves, so it takes the weaker.
    PinRoles {
        evidence: Evidence::established(
            DatasheetSpecified,
            &[
                "atf22v10c-datasheet", "clk-shared", "pwr12", "pwr24", "marker-inert", "in2",
                "ioin14", "ioin15", "ioin16", "ioin17", "ioin18", "ioin19", "ioin20", "ioin21",
                "ioin22", "ioin23",
            ],
        ),
        document_sections: ["Pin roles: the DIP-24 package"],
    }

    /// The three JEDEC fuse counts, and the power-down fuse.
    Footprints {
        evidence: Evidence::established(
            DifferentiallyVerified,
            &["mode-pal", "mode-powerdown", "arch-comb-high", "atf22v10c-datasheet"],
        ),
        document_sections: ["The three JEDEC footprints, and the power-down fuse"],
    }

    /// 64 bits of user signature carrying CUPL's `PartNo`.
    ///
    /// `arch-comb-high` is one of the three data points, not background:
    /// it is the design that holds `PartNo 00`.
    UserSignature {
        evidence: Evidence::established(
            DifferentiallyVerified,
            &["arch-comb-high", "sig-partno-41", "sig-partno-5A"],
        ),
        document_sections: ["The user signature carries CUPL's PartNo"],
    }

    /// A `0` in the array is an intact link.
    ///
    /// One witness: JEDEC 3A itself, lines 344-348, registered with that
    /// locator in `references.toml`. Every experiment is *consistent*
    /// with it and none corroborates it, because this project's reader
    /// and its encoder share the convention — a world where both are
    /// inverted produces identical observations. Only hardware settles
    /// it, which is why no experiment is cited here at all.
    LinkConvention {
        evidence: Evidence::established(DatasheetSpecified, &["jedec-3a"]),
        document_sections: [],
    }
}
