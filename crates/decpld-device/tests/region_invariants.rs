//! Properties over device region layouts. SPEC.md §4.7's required
//! invariants, stated rather than sampled.
//!
//! The examples in `region.rs` check the shapes a human thought of.
//! These check that the guarantee holds for *any* layout the type
//! accepts — which is what makes `FuseRegions` a proof rather than a
//! convention, since every device model in the project will be built
//! through it.

use decpld_device::{FuseId, FuseMap, FuseMutability, FuseRegion, FuseRegions};
use proptest::prelude::*;

/// A layout that partitions `count` fuses, built by cutting the range at
/// arbitrary points so it is valid by construction.
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
            let regions = boundaries
                .windows(2)
                .enumerate()
                .map(|(index, pair)| FuseRegion {
                    name: NAMES[index % NAMES.len()],
                    range: pair[0]..pair[1],
                    erased_value: index % 2 == 0,
                    mutability: match kinds.get(index).copied().unwrap_or(0) % 4 {
                        0 => FuseMutability::Programmable,
                        1 => FuseMutability::Reserved(index % 2 == 0),
                        2 => FuseMutability::UserSignature,
                        _ => FuseMutability::Security,
                    },
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
            prop_assert!(device.region_of(FuseId(fuse)).is_some());
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
    fn an_erased_map_is_written_exactly_where_it_is_reserved(
        (count, regions) in any_valid_layout()
    ) {
        // Reserved fuses arrive already correct and already claimed;
        // everything else is unclaimed until an encoder says otherwise.
        // Getting this backwards would make an unwritten-fuse report
        // list every reserved fuse as unaccounted for.
        let device = FuseRegions::new(count, regions).expect("valid");
        let map = FuseMap::erased(device);

        for fuse in 0..count {
            let region = map.regions().region_of(FuseId(fuse)).expect("classified");
            match region.mutability {
                FuseMutability::Reserved(required) => {
                    prop_assert!(map.is_written(FuseId(fuse)));
                    prop_assert_eq!(map.get(FuseId(fuse)), Some(required));
                }
                _ => {
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
            let guarded = matches!(
                mutability,
                FuseMutability::Reserved(_) | FuseMutability::Security
            );
            // Try to flip it to the opposite of whatever it holds.
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

        // Every guarded fuse still holds what it started with.
        for fuse in 0..count {
            let mutability = map.regions().region_of(FuseId(fuse)).expect("classified").mutability;
            if matches!(mutability, FuseMutability::Reserved(_) | FuseMutability::Security) {
                prop_assert_eq!(map.get(FuseId(fuse)), Some(before[fuse as usize]));
            }
        }
    }
}
