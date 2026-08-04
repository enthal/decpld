//! Classifying a device's fuses. SPEC.md §4.2.
//!
//! A fuse number on its own means nothing. What makes it meaningful is
//! which *region* it falls in — array cell, architecture bit, signature,
//! reserved — and that classification is the device layer's job. Nothing
//! above this layer may name a fuse at all.

use std::ops::Range;

/// A fuse's index within one device. SPEC.md §4.2.
///
/// A newtype rather than a bare `u32` because a `u32` that is sometimes
/// a fuse index and sometimes a pin number is exactly the bug class this
/// project cannot afford (CLAUDE.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FuseId(pub u32);

impl std::fmt::Display for FuseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fuse {}", self.0)
    }
}

/// What a region's fuses may be used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuseMutability {
    /// Ordinary design data: the AND array, architecture bits.
    Programmable,

    /// Must hold exactly this value. A change is a **hard error**, never
    /// a warning (SPEC.md §5.32) — a reserved fuse programmed wrongly is
    /// undefined behaviour in silicon, and there is no diagnostic on a
    /// device that misbehaves in a circuit.
    Reserved(bool),

    /// The user electronic signature. Free for the user, and on the
    /// ATF22V10C readable even after the security fuse is set.
    UserSignature,

    /// The security fuse. Clear by default, and setting it requires two
    /// explicit CLI flags because it permanently prevents reading the
    /// device back (CLAUDE.md → Safety).
    Security,
}

/// One classified span of a device's fuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuseRegion {
    pub name: &'static str,
    pub range: Range<u32>,
    /// The state an erased device holds here. For most GAL-family parts
    /// this is `true`: an erased cell is a broken link.
    pub erased_value: bool,
    pub mutability: FuseMutability,
}

impl FuseRegion {
    #[must_use]
    pub fn len(&self) -> u32 {
        self.range.end.saturating_sub(self.range.start)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn contains(&self, fuse: FuseId) -> bool {
        self.range.contains(&fuse.0)
    }
}

/// Why a proposed set of regions does not describe a device.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RegionError {
    #[error("region `{name}` is empty ({start}..{end}); a region with no fuses classifies nothing")]
    Empty { name: &'static str, start: u32, end: u32 },

    #[error("region `{name}` runs backwards ({start}..{end})")]
    Backwards { name: &'static str, start: u32, end: u32 },

    #[error(
        "regions `{first}` ({first_range:?}) and `{second}` ({second_range:?}) overlap; \
         a fuse cannot mean two things"
    )]
    Overlap {
        first: &'static str,
        first_range: Range<u32>,
        second: &'static str,
        second_range: Range<u32>,
    },

    #[error(
        "fuses {start}..{end} are not classified by any region; every fuse must be \
         accounted for, or the device model is silently incomplete"
    )]
    Unclassified { start: u32, end: u32 },

    #[error("region `{name}` ends at {end}, past the device's {count} fuses")]
    PastEnd { name: &'static str, end: u32, count: u32 },
}

/// A device's regions, checked to classify every fuse exactly once.
///
/// Construction is the only way to obtain one, so "every fuse is
/// classified" and "no fuse means two things" — two of SPEC.md §4.7's
/// required invariants — hold for every value of this type rather than
/// being asserted wherever someone remembers to. A device model that
/// forgot a region cannot be built at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuseRegions {
    regions: Vec<FuseRegion>,
    count: u32,
}

impl FuseRegions {
    /// Validate and take ownership of a device's regions.
    ///
    /// Order does not matter: the regions are sorted here, so a device
    /// definition can list them in whatever order reads best.
    pub fn new(count: u32, mut regions: Vec<FuseRegion>) -> Result<Self, RegionError> {
        for region in &regions {
            if region.range.start > region.range.end {
                return Err(RegionError::Backwards {
                    name: region.name,
                    start: region.range.start,
                    end: region.range.end,
                });
            }
            if region.is_empty() {
                return Err(RegionError::Empty {
                    name: region.name,
                    start: region.range.start,
                    end: region.range.end,
                });
            }
            if region.range.end > count {
                return Err(RegionError::PastEnd {
                    name: region.name,
                    end: region.range.end,
                    count,
                });
            }
        }

        regions.sort_by_key(|region| region.range.start);

        let mut covered = 0u32;
        for region in &regions {
            if region.range.start < covered {
                // Sorted by start, so the previous region is the one it
                // runs into.
                let previous = regions
                    .iter()
                    .find(|other| other.range.end > region.range.start && other.name != region.name)
                    .unwrap_or(region);
                return Err(RegionError::Overlap {
                    first: previous.name,
                    first_range: previous.range.clone(),
                    second: region.name,
                    second_range: region.range.clone(),
                });
            }
            if region.range.start > covered {
                return Err(RegionError::Unclassified { start: covered, end: region.range.start });
            }
            covered = region.range.end;
        }
        if covered != count {
            return Err(RegionError::Unclassified { start: covered, end: count });
        }

        Ok(Self { regions, count })
    }

    /// The device's fuse count.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Every region, ascending by fuse number.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &FuseRegion> {
        self.regions.iter()
    }

    /// The region a fuse belongs to.
    ///
    /// Binary search rather than a scan: encoding walks every fuse of a
    /// device, and a linear lookup per fuse would make that quadratic.
    #[must_use]
    pub fn region_of(&self, fuse: FuseId) -> Option<&FuseRegion> {
        let index = self.regions.partition_point(|region| region.range.end <= fuse.0);
        self.regions.get(index).filter(|region| region.contains(fuse))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programmable(name: &'static str, range: Range<u32>) -> FuseRegion {
        FuseRegion { name, range, erased_value: true, mutability: FuseMutability::Programmable }
    }

    /// A miniature device: an array, one architecture bit, a signature.
    /// Shaped like the ATF22V10C's real layout so the tests exercise the
    /// arrangement the target actually has, at a size a human can check.
    fn small_device() -> FuseRegions {
        FuseRegions::new(
            16,
            vec![
                programmable("and-array", 0..8),
                programmable("architecture", 8..10),
                FuseRegion {
                    name: "signature",
                    range: 10..16,
                    erased_value: true,
                    mutability: FuseMutability::UserSignature,
                },
            ],
        )
        .expect("a well-formed device")
    }

    #[test]
    fn a_gap_between_regions_is_refused() {
        // "Every fuse is classified" (SPEC.md §4.7). A gap means some
        // fuse has no meaning, and a device model that cannot say what a
        // fuse does will encode it wrongly in silence.
        let error =
            FuseRegions::new(16, vec![programmable("low", 0..4), programmable("high", 8..16)])
                .expect_err("fuses 4..8 mean nothing");
        assert_eq!(error, RegionError::Unclassified { start: 4, end: 8 });
    }

    #[test]
    fn a_gap_at_the_end_is_refused() {
        // The likeliest way to get this wrong: extend a device's fuse
        // count and forget to extend the last region.
        let error = FuseRegions::new(16, vec![programmable("all", 0..12)])
            .expect_err("fuses 12..16 mean nothing");
        assert_eq!(error, RegionError::Unclassified { start: 12, end: 16 });
    }

    #[test]
    fn overlapping_regions_are_refused() {
        // A fuse that belongs to two regions means two encoders will
        // each believe they own it.
        let error = FuseRegions::new(16, vec![programmable("a", 0..10), programmable("b", 8..16)])
            .expect_err("fuses 8..10 belong to both");
        assert!(matches!(error, RegionError::Overlap { .. }), "{error:?}");
    }

    #[test]
    fn a_region_past_the_device_is_refused() {
        let error = FuseRegions::new(16, vec![programmable("too-big", 0..20)])
            .expect_err("the device has 16 fuses");
        assert_eq!(error, RegionError::PastEnd { name: "too-big", end: 20, count: 16 });
    }

    #[test]
    fn an_empty_region_is_refused() {
        // Classifying nothing is never intentional; it is a typo in a
        // range, and one that would leave a gap elsewhere.
        let error =
            FuseRegions::new(16, vec![programmable("nothing", 4..4), programmable("all", 0..16)])
                .expect_err("an empty region");
        assert_eq!(error, RegionError::Empty { name: "nothing", start: 4, end: 4 });
    }

    #[test]
    // The reversed range is the input under test: clippy is right that
    // it yields nothing, which is exactly the mistake `FuseRegions` has
    // to catch in a device definition.
    #[allow(clippy::reversed_empty_ranges)]
    fn a_backwards_region_is_refused() {
        let error = FuseRegions::new(16, vec![programmable("reversed", 10..4)])
            .expect_err("start after end");
        assert_eq!(error, RegionError::Backwards { name: "reversed", start: 10, end: 4 });
    }

    #[test]
    fn declaration_order_does_not_matter() {
        // CLAUDE.md: declaration order is never observable. A device
        // definition should be able to list its regions in whatever
        // order reads best.
        let ascending =
            FuseRegions::new(16, vec![programmable("a", 0..8), programmable("b", 8..16)])
                .expect("valid");
        let descending =
            FuseRegions::new(16, vec![programmable("b", 8..16), programmable("a", 0..8)])
                .expect("valid");
        assert_eq!(ascending, descending);
    }

    #[test]
    fn every_fuse_resolves_to_exactly_one_region() {
        // The invariant the type exists to guarantee, checked
        // exhaustively over the whole device rather than sampled.
        let device = small_device();
        for fuse in 0..device.count() {
            let region = device
                .region_of(FuseId(fuse))
                .unwrap_or_else(|| panic!("fuse {fuse} has no region"));
            assert!(region.contains(FuseId(fuse)));
            let matching = device.iter().filter(|r| r.contains(FuseId(fuse))).count();
            assert_eq!(matching, 1, "fuse {fuse} is in {matching} regions");
        }
        assert_eq!(device.region_of(FuseId(device.count())), None, "one past the end");
    }

    #[test]
    fn a_region_is_found_at_both_of_its_edges() {
        // Boundary arithmetic is where a binary search goes wrong, and a
        // fuse landing in the neighbouring region is a mis-encoding that
        // nothing downstream would notice.
        let device = small_device();
        assert_eq!(device.region_of(FuseId(0)).unwrap().name, "and-array");
        assert_eq!(device.region_of(FuseId(7)).unwrap().name, "and-array");
        assert_eq!(device.region_of(FuseId(8)).unwrap().name, "architecture");
        assert_eq!(device.region_of(FuseId(9)).unwrap().name, "architecture");
        assert_eq!(device.region_of(FuseId(10)).unwrap().name, "signature");
        assert_eq!(device.region_of(FuseId(15)).unwrap().name, "signature");
    }
}
