//! Properties over device region layouts. SPEC.md §4.7's required
//! invariants, stated rather than sampled.
//!
//! The examples in `region.rs` check the shapes a human thought of.
//! These check that the guarantee holds for *any* layout the type
//! accepts — which is what makes `FuseRegions` a proof rather than a
//! convention, since every device model in the project will be built
//! through it.

use decpld_device::{
    FuseId, FuseMap, FuseMutability, FuseRegion, FuseRegions, FuseWriteError, RegionError,
};
use proptest::prelude::*;

/// A layout that partitions `count` fuses, built by cutting the range at
/// arbitrary points so it is valid by construction.
///
/// At most one security region, one fuse wide and erasing clear —
/// because those are now construction-time requirements rather than
/// conventions, and a generator that ignored them would only ever test
/// the rejection path.
fn any_valid_layout() -> impl Strategy<Value = (u32, Vec<FuseRegion>)> {
    (
        1u32..200,
        proptest::collection::vec(0u32..200, 0..8),
        proptest::collection::vec(0usize..4, 1..9),
    )
        .prop_map(|(count, cuts, kinds)| {
            let mut boundaries: Vec<u32> =
                cuts.into_iter().map(|c| c % count).filter(|c| *c > 0).collect();
            boundaries.push(0);
            boundaries.push(count);
            boundaries.sort_unstable();
            boundaries.dedup();

            const NAMES: [&str; 9] = ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"];
            let mut security_placed = false;
            let regions = boundaries
                .windows(2)
                .enumerate()
                .map(|(index, pair)| {
                    let range = pair[0]..pair[1];
                    let wants_security = kinds.get(index).copied().unwrap_or(0) % 4 == 3;
                    // A security region must be exactly one fuse and
                    // there may be at most one, so it can only land on a
                    // single-fuse slice and only once.
                    let is_security =
                        wants_security && !security_placed && range.end - range.start == 1;
                    if is_security {
                        security_placed = true;
                    }
                    let mutability = if is_security {
                        FuseMutability::Security
                    } else {
                        match kinds.get(index).copied().unwrap_or(0) % 3 {
                            0 => FuseMutability::Programmable,
                            1 => FuseMutability::Reserved(index % 2 == 0),
                            _ => FuseMutability::UserSignature,
                        }
                    };
                    FuseRegion {
                        name: NAMES[index % NAMES.len()],
                        range,
                        // The security fuse is clear by default and a
                        // device model may not say otherwise.
                        erased_value: !is_security && index % 2 == 0,
                        mutability,
                    }
                })
                .collect();
            (count, regions)
        })
}

proptest! {
    #[test]
    fn a_valid_layout_classifies_every_fuse_exactly_once((count, regions) in any_valid_layout()) {
        // SPEC.md §4.7: "every fuse is classified". Stated over the
        // whole device rather than spot-checked, because a layout that
        // classifies 5891 of 5892 fuses is indistinguishable from a
        // correct one until the missing fuse is the one that matters.
        let device = FuseRegions::new(count, regions).expect("valid by construction");
        prop_assert_eq!(device.count(), count);

        for fuse in 0..count {
            let matching = device.iter().filter(|r| r.contains(FuseId(fuse))).count();
            prop_assert_eq!(matching, 1, "fuse {} is in {} regions", fuse, matching);
        }
        prop_assert!(device.region_of(FuseId(count)).is_none(), "one past the end");
    }

    #[test]
    fn region_of_agrees_with_a_linear_scan((count, regions) in any_valid_layout()) {
        // `region_of` is a binary search; this pins it to the definition.
        // Boundary arithmetic in a lookup is exactly where a fuse silently
        // lands in the neighbouring region.
        let device = FuseRegions::new(count, regions).expect("valid");
        for fuse in 0..count {
            let found = device.region_of(FuseId(fuse)).map(|r| r.name);
            let scanned = device.iter().find(|r| r.contains(FuseId(fuse))).map(|r| r.name);
            prop_assert_eq!(found, scanned, "fuse {}", fuse);
        }
    }

    #[test]
    fn perturbing_a_valid_layout_is_always_rejected(
        (count, regions) in any_valid_layout(),
        which in 0usize..64,
        how in 0usize..4,
    ) {
        // The rejection side, which nothing exercised: every invalid-layout
        // rule rested on exactly one example, one deleted assertion away
        // from being untested. Perturbing a *known-good* layout is how to
        // state "these rules hold generally" rather than "these four
        // inputs are refused".
        prop_assume!(!regions.is_empty());
        let target = which % regions.len();
        let mut broken = regions.clone();

        match how {
            // Drop a region, leaving its fuses unclassified.
            0 => { broken.remove(target); }
            // Extend one region over its neighbour.
            1 => broken[target].range.end += 1,
            // Collapse one to nothing.
            2 => broken[target].range.end = broken[target].range.start,
            // Run one backwards.
            _ => {
                let range = &mut broken[target].range;
                std::mem::swap(&mut range.start, &mut range.end);
            }
        }
        prop_assume!(broken != regions);

        prop_assert!(
            FuseRegions::new(count, broken.clone()).is_err(),
            "accepted a perturbed layout ({}): {:?}",
            how,
            broken
        );
    }

    #[test]
    fn a_security_region_that_erases_set_is_always_rejected(
        (count, regions) in any_valid_layout()
    ) {
        // SPEC.md §5.32 and CLAUDE.md → Safety. A device arriving
        // "erased" with the readback lock already engaged is unreadable
        // before anything has programmed it, and no device model may
        // declare that.
        let mut broken = regions.clone();
        let Some(security) =
            broken.iter_mut().find(|r| r.mutability == FuseMutability::Security)
        else {
            return Ok(());
        };
        security.erased_value = true;
        let name = security.name;

        prop_assert_eq!(
            FuseRegions::new(count, broken),
            Err(RegionError::SecurityErasesSet { name })
        );
    }

    #[test]
    fn an_erased_map_is_clear_where_it_matters_and_written_where_it_is_fixed(
        (count, regions) in any_valid_layout()
    ) {
        // Reserved fuses arrive already correct and already claimed;
        // the security fuse arrives clear whatever else is true;
        // everything else starts at its region's erased value, unclaimed.
        //
        // The previous version of this property put Security in the
        // catch-all arm and so *asserted* that a security fuse erases to
        // whatever the model declared — affirming the bug rather than
        // catching it.
        let device = FuseRegions::new(count, regions).expect("valid");
        let map = FuseMap::erased(device);

        for fuse in 0..count {
            let region = map.regions().region_of(FuseId(fuse)).expect("classified");
            match region.mutability {
                FuseMutability::Reserved(required) => {
                    prop_assert!(map.is_written(FuseId(fuse)));
                    prop_assert_eq!(map.get(FuseId(fuse)), Some(required));
                }
                FuseMutability::Security => {
                    prop_assert_eq!(
                        map.get(FuseId(fuse)),
                        Some(false),
                        "the security fuse is clear by default"
                    );
                }
                FuseMutability::Programmable | FuseMutability::UserSignature => {
                    prop_assert!(!map.is_written(FuseId(fuse)));
                    prop_assert_eq!(map.get(FuseId(fuse)), Some(region.erased_value));
                }
            }
        }
    }

    #[test]
    fn no_ordinary_write_can_reach_a_reserved_or_security_fuse(
        (count, regions) in any_valid_layout()
    ) {
        // The safety claim, over every layout rather than the one
        // example device: an encoder sweeping a fuse range must not be
        // able to arrive at either by accident.
        let device = FuseRegions::new(count, regions).expect("valid");
        let mut map = FuseMap::erased(device);
        let before: Vec<bool> = map.iter().collect();

        for fuse in 0..count {
            let mutability = map.regions().region_of(FuseId(fuse)).expect("classified").mutability;
            let guarded =
                matches!(mutability, FuseMutability::Reserved(_) | FuseMutability::Security);
            let opposite = !map.get(FuseId(fuse)).expect("in range");
            let outcome = map.set(FuseId(fuse), opposite);
            prop_assert_eq!(
                outcome.is_err(),
                guarded,
                "fuse {} guarded={} but write {:?}",
                fuse,
                guarded,
                outcome
            );
        }

        for fuse in 0..count {
            let mutability = map.regions().region_of(FuseId(fuse)).expect("classified").mutability;
            if matches!(mutability, FuseMutability::Reserved(_) | FuseMutability::Security) {
                prop_assert_eq!(map.get(FuseId(fuse)), Some(before[fuse as usize]));
            }
        }
    }

    #[test]
    fn locking_a_device_is_all_or_nothing((count, regions) in any_valid_layout()) {
        // Which fuse of a security region gets locked was constrained by
        // no test: moving `security_fuse()` from the region's start to
        // its end passed the whole suite. Now the region is one fuse
        // wide by construction, and this states the consequence — after
        // a successful lock, every security fuse is set and claimed.
        let device = FuseRegions::new(count, regions).expect("valid");
        let has_security =
            device.iter().any(|r| r.mutability == FuseMutability::Security);
        let mut map = FuseMap::erased(device);

        match map.set_security_fuse(true) {
            Ok(()) => {
                prop_assert!(has_security, "locked a device with no security fuse");
                for fuse in 0..count {
                    let region = map.regions().region_of(FuseId(fuse)).expect("classified");
                    if region.mutability == FuseMutability::Security {
                        prop_assert_eq!(map.get(FuseId(fuse)), Some(true), "fuse {}", fuse);
                        prop_assert!(map.is_written(FuseId(fuse)), "fuse {}", fuse);
                    }
                }
            }
            Err(error) => {
                prop_assert!(!has_security, "refused a lock on a device that has the fuse");
                prop_assert_eq!(error, FuseWriteError::NoSecurityFuse);
            }
        }
    }
}
