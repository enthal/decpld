//! The ATF22V10C's fuse regions, for each of its three JEDEC footprints.

use decpld_device::{FuseMutability, FuseRegion, FuseRegions, RegionError};

/// Fuses in the AND array.
///
/// Evidence: 132 rows × 44 columns. Rows and columns are
/// `OpenSourceCrossChecked` against Galette `af52987`
/// (`src/chips.rs:79-90`) and GALasm `c376d56`
/// (`src/galasm.h:71`), and every column position in that range was
/// measured directly — see `targets/evidence/atf22v10-fuse-map.md`,
/// "Columns: signal sources".
pub(crate) const ARRAY_FUSES: u32 = 5808;

/// First fuse of the interleaved S0/S1 architecture block.
///
/// Evidence: GALasm `XOR22V10 5808` (`src/galasm.h:55`), Galette's
/// writer emitting the interleaved pair immediately after the array
/// (`src/writer.rs:198-207`), and measured directly — a combinational
/// active-high output on pin 23 sets exactly 5808 and 5809
/// (experiment `arch-comb-high`).
pub(crate) const ARCHITECTURE_START: u32 = 5808;

/// Two fuses per macrocell, ten macrocells.
///
/// Evidence: experiments `arch-comb-high`, `arch-comb-low`,
/// `arch-reg-high`, `arch-reg-low` establish S0 = polarity and
/// S1 = mode; `mc14`, `mc17`, `mc20`, `fb22` establish one pair per
/// macrocell across the range.
pub(crate) const ARCHITECTURE_FUSES: u32 = 20;

/// First fuse of the 64-bit user signature.
///
/// Evidence, three independent directions:
/// - GALasm `SIG22V10 5828` (`src/galasm.h:57`);
/// - the datasheet's Table 10-1 gives PAL mode as 5828 fuses, i.e. the
///   device *without* a signature, so the signature must begin there;
/// - measured: compiling under `p22v10` yields `QF5828` with no bits set
///   above the architecture block (experiment `mode-pal`).
pub(crate) const SIGNATURE_START: u32 = 5828;

/// Evidence: datasheet §5, "There are 64-bits of programmable memory
/// that are always available to the user, even if the device is
/// secured." Measured to carry CUPL's `PartNo` as ASCII, MSB-first
/// (experiments `sig-partno-41`, `sig-partno-5A`).
pub(crate) const SIGNATURE_FUSES: u32 = 64;

/// The power-down enable.
///
/// Evidence: compiling one design under `g22v10cp` produces output
/// identical to the `g22v10` output plus this single fuse set
/// (experiments `mode-powerdown`, `arch-comb-high`). The datasheet's
/// Table 10-1 gives power-down mode as 5893 fuses; this is the 5893rd.
pub(crate) const POWER_DOWN_FUSE: u32 = 5892;

/// Which JEDEC footprint a file uses.
///
/// The ATF22V10C has three, not one. Evidence: datasheet Table 10-1
/// "Compiler Mode Selection", confirmed by compiling one design under
/// each of WinCUPL's three device types (`mode-pal`, `arch-comb-high`,
/// `mode-powerdown`).
///
/// `jed inspect` reads files it did not write, so all three are
/// accepted. Which one a *file* uses is decided by its `QF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Footprint {
    /// 5828 fuses: array and architecture bits, no signature.
    /// WinCUPL device type `P22V10`.
    Pal,
    /// 5892 fuses: adds the 64-bit user signature. WinCUPL `G22V10`,
    /// and what the default device type produces.
    Gal,
    /// 5893 fuses: adds the power-down enable. WinCUPL `G22V10CP`.
    PowerDown,
}

impl Footprint {
    /// Every footprint, ascending by fuse count.
    pub const ALL: [Footprint; 3] = [Footprint::Pal, Footprint::Gal, Footprint::PowerDown];

    /// The `QF` value a file using this footprint declares.
    #[must_use]
    pub fn fuse_count(self) -> u32 {
        match self {
            Footprint::Pal => ARRAY_FUSES + ARCHITECTURE_FUSES,
            Footprint::Gal => ARRAY_FUSES + ARCHITECTURE_FUSES + SIGNATURE_FUSES,
            Footprint::PowerDown => ARRAY_FUSES + ARCHITECTURE_FUSES + SIGNATURE_FUSES + 1,
        }
    }

    /// The WinCUPL device type that produces this footprint, for oracle
    /// work and for explaining a file to a user who arrived from CUPL.
    #[must_use]
    pub fn wincupl_device_type(self) -> &'static str {
        match self {
            Footprint::Pal => "p22v10",
            Footprint::Gal => "g22v10",
            Footprint::PowerDown => "g22v10cp",
        }
    }

    /// Identify a footprint from a file's declared fuse count.
    pub fn from_fuse_count(count: u32) -> Result<Self, FootprintError> {
        Self::ALL
            .into_iter()
            .find(|footprint| footprint.fuse_count() == count)
            .ok_or(FootprintError::UnknownFuseCount { count })
    }
}

/// A fuse count that is not one of the ATF22V10C's three footprints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FootprintError {
    #[error(
        "{count} is not an ATF22V10C fuse count; this device uses {} (PAL mode), \
         {} (GAL mode), or {} (power-down mode)",
        Footprint::Pal.fuse_count(),
        Footprint::Gal.fuse_count(),
        Footprint::PowerDown.fuse_count()
    )]
    UnknownFuseCount { count: u32 },
}

/// The fuse regions of one footprint.
///
/// Erased state is `true` throughout the programmable area: an erased
/// GAL-family cell is a broken link, so an erased device has every link
/// blown and every product term is the empty AND, i.e. constantly true.
/// The security fuse erases clear, which `FuseRegions` enforces.
pub fn regions_for(footprint: Footprint) -> Result<FuseRegions, RegionError> {
    let mut regions = vec![
        FuseRegion {
            name: "and-array",
            range: 0..ARRAY_FUSES,
            erased_value: true,
            mutability: FuseMutability::Programmable,
        },
        FuseRegion {
            name: "architecture",
            range: ARCHITECTURE_START..ARCHITECTURE_START + ARCHITECTURE_FUSES,
            erased_value: true,
            mutability: FuseMutability::Programmable,
        },
    ];

    // Matched explicitly rather than by `footprint >= Footprint::Gal`.
    // The derived ordering is declaration order, which happens to agree
    // with fuse count today — so a variant inserted in the wrong place
    // would silently change which footprints carry a signature, and the
    // `derive` says nothing about the order being load-bearing.
    if matches!(footprint, Footprint::Gal | Footprint::PowerDown) {
        regions.push(FuseRegion {
            name: "user-signature",
            range: SIGNATURE_START..SIGNATURE_START + SIGNATURE_FUSES,
            erased_value: true,
            mutability: FuseMutability::UserSignature,
        });
    }

    if matches!(footprint, Footprint::PowerDown) {
        regions.push(FuseRegion {
            name: "power-down",
            range: POWER_DOWN_FUSE..POWER_DOWN_FUSE + 1,
            erased_value: true,
            mutability: FuseMutability::Programmable,
        });
    }

    // NOTE: the security fuse is deliberately absent.
    //
    // The ATF22V10C has one — the datasheet's §6 says so — but it is not
    // inside `QF`. It is the JEDEC `G` field, which `decpld-jedec`
    // already models separately, and every footprint's fuse count is
    // fully accounted for without it (5808 + 20 [+ 64] [+ 1]). Adding a
    // region for it here would have to invent a fuse index, and there is
    // none to cite.
    FuseRegions::new(footprint.fuse_count(), regions)
}
