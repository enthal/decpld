//! A device's fuse states, and who wrote them. SPEC.md §4.2.

use crate::region::{FuseId, FuseMutability, FuseRegions};
use std::collections::BTreeMap;

/// Why a fuse could not be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FuseWriteError {
    #[error("{fuse} is out of range: this device has {count} fuses")]
    OutOfRange { fuse: FuseId, count: u32 },

    /// Two encoders disagreed about one fuse.
    ///
    /// Detected rather than resolved, because there is no correct way to
    /// resolve it: last-writer-wins would silently pick one encoder's
    /// intent over another's, and the result is a device that behaves
    /// like neither design (CLAUDE.md → `FuseMap` tracks writes).
    #[error(
        "{fuse} in region `{region}` was already written as {existing} and something \
         else now wants {attempted}; two encoders disagree about one fuse"
    )]
    Conflict { fuse: FuseId, region: &'static str, existing: bool, attempted: bool },

    /// A reserved fuse is a **hard error**, never a warning
    /// (SPEC.md §13.2).
    #[error(
        "{fuse} is reserved in region `{region}` and must stay {required}; \
         changing it is undefined behaviour in silicon"
    )]
    Reserved { fuse: FuseId, region: &'static str, required: bool },

    /// The security fuse has its own gate, so it cannot be set by an
    /// ordinary encoder reaching for a fuse index.
    #[error(
        "{fuse} is the security fuse and is not writable through the ordinary path; \
         it requires both --security-fuse and --acknowledge-readback-lock"
    )]
    Security { fuse: FuseId },

    /// Asked to engage a permanent readback lock on a device that has
    /// no such fuse.
    ///
    /// Reported rather than silently succeeding: the caller asked for
    /// something irreversible, the device cannot do it, and answering
    /// "done" is the wrong end of "prefer a rejected build to a wrong
    /// one" on the one operation that cannot be undone.
    #[error("this device has no security fuse, so it cannot be locked against readback")]
    NoSecurityFuse,
}

/// Why a fuse vector could not be read as this device's.
///
/// Distinct from [`FuseWriteError`], which is about an *encoder*
/// claiming a fuse. These are about a vector that already exists —
/// typically one a JEDEC file carried — failing to describe this part.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FuseStatesError {
    #[error("this device has {expected} fuses; the vector has {actual}")]
    WrongLength { expected: u32, actual: usize },

    /// A reserved fuse is a **hard error**, never a warning
    /// (SPEC.md §13.2).
    #[error(
        "{fuse} is reserved in region `{region}` and must be {required}, but the vector \
         has it {found}; a device in that state is not one the manufacturer defines"
    )]
    Reserved { fuse: FuseId, region: &'static str, required: bool, found: bool },
}

/// Every fuse of one device, and whether anything has claimed it.
///
/// The `written` half is not bookkeeping: it is what turns "two encoders
/// fought over this fuse" from an invisible averaging into a reported
/// error. A wrong fuse produces a chip that misbehaves in a circuit and
/// a user who debugs their *hardware* for a week, so the cost of
/// detecting the conflict is trivially worth paying.
///
/// SPEC.md §4.2 types both halves as `BitVec`. `Vec<bool>` is used
/// instead — recorded there as a deliberate deviation — because the
/// device layer has no packing requirement. `decpld-jedec` packs because
/// the fuse checksum is *defined* over packed words; here packing would
/// buy one byte per fuse on a part with under six thousand of them, at
/// the cost of bit arithmetic in the one place this project can least
/// afford an off-by-one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuseMap {
    bits: Vec<bool>,
    written: Vec<bool>,
    regions: FuseRegions,
}

impl FuseMap {
    /// An erased device: every fuse at its region's erased value.
    ///
    /// Reserved fuses are set to their required value and marked
    /// written, so they are already correct and any later attempt to
    /// change one is refused rather than merely noticed at the end.
    #[must_use]
    pub fn erased(regions: FuseRegions) -> Self {
        let count = regions.count() as usize;
        let mut bits = vec![false; count];
        let mut written = vec![false; count];

        for region in regions.iter() {
            for fuse in region.range.clone() {
                let index = fuse as usize;
                match region.mutability {
                    FuseMutability::Reserved(required) => {
                        bits[index] = required;
                        written[index] = true;
                    }
                    _ => bits[index] = region.erased_value,
                }
            }
        }

        Self { bits, written, regions }
    }

    /// A device whose fuses somebody else already decided.
    ///
    /// The counterpart to [`Self::erased`], and the way a fuse vector
    /// from a file this compiler did not write becomes something the
    /// device layer can decode. Every fuse is marked **written**,
    /// because the file stated all of them: leaving them unclaimed
    /// would let an encoder later overwrite a value the file gave
    /// without [`FuseWriteError::Conflict`] ever firing, which is the
    /// whole reason the `written` half exists.
    ///
    /// The security fuse is accepted here even though
    /// [`Self::set_security_fuse`] gates it, because that gate is about
    /// the *act* of locking a part. A file that already carries the bit
    /// is reporting a part somebody else locked, and refusing to read
    /// it would leave the case a user most needs explained the one case
    /// that cannot be.
    ///
    /// # Errors
    ///
    /// If the vector's length is not this device's fuse count — the
    /// count is a file's claim about which part it is for, and padding
    /// or truncating would silently decode a foreign file as this
    /// device — or if it disagrees with a reserved fuse.
    pub fn from_states(
        regions: FuseRegions,
        states: impl IntoIterator<Item = bool>,
    ) -> Result<Self, FuseStatesError> {
        let bits: Vec<bool> = states.into_iter().collect();
        let expected = regions.count();
        if bits.len() != expected as usize {
            return Err(FuseStatesError::WrongLength { expected, actual: bits.len() });
        }

        for region in regions.iter() {
            let FuseMutability::Reserved(required) = region.mutability else {
                continue;
            };
            for fuse in region.range.clone() {
                // Indexed through `get`, but a miss is a REFUSAL rather
                // than a pass. The length check above plus
                // `FuseRegions`' own `PastEnd` rule make it
                // unreachable; defaulting to "compliant" would fail
                // open if it ever became reachable, which is the wrong
                // end of "prefer a rejected build to a wrong one".
                let found = bits.get(fuse as usize).copied().unwrap_or(!required);
                if found != required {
                    return Err(FuseStatesError::Reserved {
                        fuse: FuseId(fuse),
                        region: region.name,
                        required,
                        found,
                    });
                }
            }
        }

        let written = vec![true; bits.len()];
        Ok(Self { bits, written, regions })
    }

    #[must_use]
    pub fn regions(&self) -> &FuseRegions {
        &self.regions
    }

    #[must_use]
    pub fn len(&self) -> u32 {
        self.regions.count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The state of one fuse, or `None` if it does not exist.
    #[must_use]
    pub fn get(&self, fuse: FuseId) -> Option<bool> {
        self.bits.get(fuse.0 as usize).copied()
    }

    /// Whether anything has claimed this fuse.
    #[must_use]
    pub fn is_written(&self, fuse: FuseId) -> bool {
        self.written.get(fuse.0 as usize).copied().unwrap_or(false)
    }

    /// Claim a fuse for a design.
    ///
    /// Writing the same value twice is allowed: two encoders agreeing is
    /// not a conflict, and forbidding it would make the order encoders
    /// run in observable. Writing a *different* value is refused.
    pub fn set(&mut self, fuse: FuseId, value: bool) -> Result<(), FuseWriteError> {
        self.check_writable(fuse, value)?;
        let index = fuse.0 as usize;
        self.bits[index] = value;
        self.written[index] = true;
        Ok(())
    }

    /// Apply several writes, or none of them.
    ///
    /// Every write is validated — range, mutability, and conflict, both
    /// against the map and against earlier entries in the same batch —
    /// before any is applied. A partially applied write is the worst
    /// available outcome: the map is then neither the old design nor
    /// the new one, and nothing downstream can tell, because a fuse
    /// checksum over a half-written map is a perfectly valid checksum
    /// over the wrong thing.
    ///
    /// The same rule as `decpld-jedec`'s fuse-list application, for the
    /// same reason.
    pub fn set_all(
        &mut self,
        writes: impl IntoIterator<Item = (FuseId, bool)>,
    ) -> Result<(), FuseWriteError> {
        let mut pending: BTreeMap<FuseId, bool> = BTreeMap::new();
        for (fuse, value) in writes {
            self.check_writable(fuse, value)?;
            if let Some(&earlier) = pending.get(&fuse)
                && earlier != value
            {
                // `check_writable` resolved the region a line ago, so
                // this lookup cannot fail; naming the region is the
                // difference between a diagnostic a user can act on and
                // one that says a fuse number.
                let region = self.regions.region_of(fuse).map_or("unclassified", |r| r.name);
                return Err(FuseWriteError::Conflict {
                    fuse,
                    region,
                    existing: earlier,
                    attempted: value,
                });
            }
            pending.insert(fuse, value);
        }

        for (fuse, value) in pending {
            let index = fuse.0 as usize;
            self.bits[index] = value;
            self.written[index] = true;
        }
        Ok(())
    }

    /// Whether [`Self::set`] would accept this write, without doing it.
    fn check_writable(&self, fuse: FuseId, value: bool) -> Result<(), FuseWriteError> {
        let region = self
            .regions
            .region_of(fuse)
            .ok_or(FuseWriteError::OutOfRange { fuse, count: self.regions.count() })?;

        match region.mutability {
            FuseMutability::Reserved(required) => {
                return Err(FuseWriteError::Reserved { fuse, region: region.name, required });
            }
            FuseMutability::Security => return Err(FuseWriteError::Security { fuse }),
            FuseMutability::Programmable | FuseMutability::UserSignature => {}
        }

        let index = fuse.0 as usize;
        if self.written[index] && self.bits[index] != value {
            return Err(FuseWriteError::Conflict {
                fuse,
                region: region.name,
                existing: self.bits[index],
                attempted: value,
            });
        }
        Ok(())
    }

    /// Set the security fuse.
    ///
    /// A separate method from [`Self::set`] so that reaching it is a
    /// deliberate act rather than the result of an encoder iterating
    /// over a range. CLAUDE.md requires two explicit CLI flags to get
    /// here; this is the last gate before the bit itself.
    pub fn set_security_fuse(
        &mut self,
        acknowledged_readback_lock: bool,
    ) -> Result<(), FuseWriteError> {
        let Some(fuse) = self.security_fuse() else {
            return Err(FuseWriteError::NoSecurityFuse);
        };
        if !acknowledged_readback_lock {
            return Err(FuseWriteError::Security { fuse });
        }
        let index = fuse.0 as usize;
        self.bits[index] = true;
        self.written[index] = true;
        Ok(())
    }

    /// The device's security fuse, if it has one inside the fuse map.
    ///
    /// Exactly one fuse: `FuseRegions` rejects a security region that
    /// spans more, so this cannot silently lock part of one. Half-locking
    /// is the worst of the three outcomes — neither readable nor
    /// protected, with the remainder reported as unclaimed.
    #[must_use]
    pub fn security_fuse(&self) -> Option<FuseId> {
        self.regions
            .iter()
            .find(|region| region.mutability == FuseMutability::Security)
            .map(|region| FuseId(region.range.start))
    }

    /// Every fuse state in ascending order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = bool> + '_ {
        self.bits.iter().copied()
    }

    /// Fuses no encoder claimed.
    ///
    /// Not an error — an erased fuse is a legitimate state — but a fit
    /// report that cannot say which fuses were left alone cannot explain
    /// what it did.
    pub fn unwritten(&self) -> impl Iterator<Item = FuseId> + '_ {
        self.written
            .iter()
            .enumerate()
            .filter(|(_, written)| !**written)
            .map(|(index, _)| FuseId(index as u32))
    }
}

// ---------------------------------------------------------------------
// Loading a fuse vector somebody else produced.
// ---------------------------------------------------------------------

#[cfg(test)]
mod from_states_tests {
    use super::*;
    use crate::region::FuseRegion;

    /// One region of each mutability, so every rule has somewhere to
    /// apply — the same shape `tests::device` uses.
    fn device() -> FuseRegions {
        FuseRegions::new(
            16,
            vec![
                FuseRegion {
                    name: "array",
                    range: 0..8,
                    erased_value: true,
                    mutability: FuseMutability::Programmable,
                },
                FuseRegion {
                    name: "reserved",
                    range: 8..10,
                    erased_value: true,
                    mutability: FuseMutability::Reserved(false),
                },
                FuseRegion {
                    name: "signature",
                    range: 10..15,
                    erased_value: true,
                    mutability: FuseMutability::UserSignature,
                },
                FuseRegion {
                    name: "security",
                    range: 15..16,
                    erased_value: false,
                    mutability: FuseMutability::Security,
                },
            ],
        )
        .expect("a valid device")
    }

    #[test]
    fn a_vector_loads_fuse_for_fuse() {
        let states = [
            true, false, true, true, false, false, true, false, // array
            false, false, // reserved, at its required value
            true, true, false, true, false, // signature
            false, // security
        ];
        let map = FuseMap::from_states(device(), states).expect("a well-sized vector");
        assert_eq!(map.len(), 16);
        for (index, state) in states.iter().enumerate() {
            let fuse = FuseId(u32::try_from(index).expect("sixteen fits"));
            assert_eq!(map.get(fuse), Some(*state), "{fuse}");
        }
    }

    #[test]
    fn every_fuse_of_a_loaded_vector_is_marked_written() {
        // The file stated all of them. Leaving them unwritten would let
        // an encoder later overwrite a fuse the file gave a value to
        // without the conflict being detected, which is the whole
        // reason `written` exists.
        let map = FuseMap::from_states(device(), [false; 16]).expect("well sized");
        for fuse in 0..16 {
            assert!(map.is_written(FuseId(fuse)), "fuse {fuse}");
        }
        assert_eq!(map.unwritten().count(), 0);
    }

    #[test]
    fn a_vector_of_the_wrong_length_is_refused_naming_both_counts() {
        // The count is the file's claim about which device it is for.
        // Padding or truncating would decode a foreign file as this
        // device, which is exactly the confusion `jed inspect` exists
        // to resolve.
        assert_eq!(
            FuseMap::from_states(device(), [false; 15]),
            Err(FuseStatesError::WrongLength { expected: 16, actual: 15 })
        );
        assert_eq!(
            FuseMap::from_states(device(), [false; 17]),
            Err(FuseStatesError::WrongLength { expected: 16, actual: 17 })
        );
    }

    #[test]
    fn a_vector_disagreeing_with_a_reserved_fuse_is_refused() {
        // A reserved fuse is a hard error, never a warning
        // (SPEC.md §13.2). A file that changes one describes a device
        // in a state the manufacturer does not define, and describing
        // it back as a valid design would lend it credibility it has
        // not got.
        let mut states = [false; 16];
        states[9] = true;
        assert_eq!(
            FuseMap::from_states(device(), states),
            Err(FuseStatesError::Reserved {
                fuse: FuseId(9),
                region: "reserved",
                required: false,
                found: true,
            })
        );
    }

    #[test]
    fn a_loaded_vector_holds_the_same_states_the_map_it_came_from_did() {
        // The bridge `jed inspect` runs on: a map's own states, taken
        // out and put back, describe the same device. Only `written`
        // differs, and deliberately — an erased map has claimed only
        // its reserved fuses, a loaded one has a stated value for every
        // fuse.
        let mut original = FuseMap::erased(device());
        original.set(FuseId(3), false).expect("programmable");
        original.set(FuseId(11), false).expect("signature");

        let states: Vec<bool> = original.iter().collect();
        let loaded = FuseMap::from_states(device(), states).expect("well sized");
        for fuse in 0..16 {
            assert_eq!(loaded.get(FuseId(fuse)), original.get(FuseId(fuse)), "fuse {fuse}");
        }
        assert!(!original.is_written(FuseId(0)));
        assert!(loaded.is_written(FuseId(0)));
    }

    #[test]
    fn the_security_fuse_may_be_set_in_a_loaded_vector() {
        // `set_security_fuse` gates the *act* of locking a part. A file
        // that already carries `G1` is reporting a part somebody else
        // locked, and refusing to read it would leave the one case
        // where a user most needs to be told undescribable.
        let mut states = [false; 16];
        states[15] = true;
        let map = FuseMap::from_states(device(), states).expect("well sized");
        assert_eq!(map.get(FuseId(15)), Some(true));
        assert_eq!(map.security_fuse(), Some(FuseId(15)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::FuseRegion;

    /// A device with one region of each mutability, so every rule has
    /// somewhere to apply.
    fn device() -> FuseRegions {
        FuseRegions::new(
            16,
            vec![
                FuseRegion {
                    name: "array",
                    range: 0..8,
                    erased_value: true,
                    mutability: FuseMutability::Programmable,
                },
                FuseRegion {
                    name: "reserved",
                    range: 8..10,
                    erased_value: true,
                    mutability: FuseMutability::Reserved(false),
                },
                FuseRegion {
                    name: "signature",
                    range: 10..15,
                    erased_value: true,
                    mutability: FuseMutability::UserSignature,
                },
                FuseRegion {
                    name: "security",
                    range: 15..16,
                    erased_value: false,
                    mutability: FuseMutability::Security,
                },
            ],
        )
        .expect("a well-formed device")
    }

    #[test]
    fn an_erased_device_holds_each_regions_erased_value() {
        let map = FuseMap::erased(device());
        assert_eq!(map.get(FuseId(0)), Some(true), "the array erases to 1");
        assert_eq!(map.get(FuseId(15)), Some(false), "security erases to 0");
        assert_eq!(map.get(FuseId(16)), None, "one past the end");
    }

    #[test]
    fn the_security_fuse_is_clear_by_default() {
        // CLAUDE.md → Safety. Setting it permanently prevents reading
        // the device back, so nothing may arrive at it by accident.
        let map = FuseMap::erased(device());
        assert_eq!(map.get(FuseId(15)), Some(false));
    }

    #[test]
    fn two_encoders_disagreeing_about_one_fuse_is_an_error() {
        // The reason `written` exists. Averaging or last-writer-wins
        // gives a device that behaves like neither design, with nothing
        // to indicate it happened.
        let mut map = FuseMap::erased(device());
        map.set(FuseId(3), true).expect("first write");
        let error = map.set(FuseId(3), false).expect_err("a second, different write");
        assert_eq!(
            error,
            FuseWriteError::Conflict {
                fuse: FuseId(3),
                region: "array",
                existing: true,
                attempted: false
            }
        );
        assert_eq!(map.get(FuseId(3)), Some(true), "the first write must stand");
    }

    #[test]
    fn two_encoders_agreeing_is_not_a_conflict() {
        // Forbidding this would make the order encoders run in
        // observable, which is a different bug.
        let mut map = FuseMap::erased(device());
        map.set(FuseId(3), true).expect("first");
        map.set(FuseId(3), true).expect("agreeing second");
        assert_eq!(map.get(FuseId(3)), Some(true));
    }

    #[test]
    fn writing_a_reserved_fuse_is_a_hard_error() {
        // SPEC.md §13.2: a reserved fuse change is an error, never a
        // warning. There is no diagnostic on a device that misbehaves
        // in a circuit.
        let mut map = FuseMap::erased(device());
        for value in [true, false] {
            let error = map.set(FuseId(8), value).expect_err("reserved");
            assert_eq!(
                error,
                FuseWriteError::Reserved { fuse: FuseId(8), region: "reserved", required: false }
            );
        }
    }

    #[test]
    fn a_reserved_fuse_starts_at_its_required_value_and_counts_as_written() {
        // It is already correct, so nothing needs to write it — and
        // marking it written is what makes an unwritten-fuse report mean
        // "nobody claimed this" rather than "this is reserved".
        let map = FuseMap::erased(device());
        assert_eq!(map.get(FuseId(8)), Some(false), "the required value, not the erased one");
        assert!(map.is_written(FuseId(8)));
    }

    #[test]
    fn the_security_fuse_is_not_reachable_through_the_ordinary_write_path() {
        // An encoder iterating a range must not be able to arrive at it.
        let mut map = FuseMap::erased(device());
        assert_eq!(
            map.set(FuseId(15), true).expect_err("not this way"),
            FuseWriteError::Security { fuse: FuseId(15) }
        );
        assert_eq!(map.get(FuseId(15)), Some(false), "and it stays clear");
    }

    #[test]
    fn setting_the_security_fuse_requires_acknowledging_the_readback_lock() {
        let mut map = FuseMap::erased(device());
        assert!(map.set_security_fuse(false).is_err(), "without acknowledgement");
        assert_eq!(map.get(FuseId(15)), Some(false));

        map.set_security_fuse(true).expect("with acknowledgement");
        assert_eq!(map.get(FuseId(15)), Some(true));
    }

    #[test]
    fn the_signature_is_writable_like_ordinary_data() {
        // It is the user's to use; the datasheet says it stays readable
        // even on a secured device.
        let mut map = FuseMap::erased(device());
        map.set(FuseId(10), false).expect("the signature is the user's");
        assert_eq!(map.get(FuseId(10)), Some(false));
    }

    #[test]
    fn an_out_of_range_write_is_refused_not_ignored() {
        let mut map = FuseMap::erased(device());
        assert_eq!(
            map.set(FuseId(16), true).expect_err("past the end"),
            FuseWriteError::OutOfRange { fuse: FuseId(16), count: 16 }
        );
    }

    #[test]
    fn unwritten_reports_exactly_what_nobody_claimed() {
        let mut map = FuseMap::erased(device());
        map.set(FuseId(0), false).expect("write");
        let unwritten: Vec<u32> = map.unwritten().map(|f| f.0).collect();
        // 0 was written; 8 and 9 are reserved and count as written; 15
        // is the security fuse, which nothing has set.
        assert!(!unwritten.contains(&0));
        assert!(!unwritten.contains(&8) && !unwritten.contains(&9));
        assert!(unwritten.contains(&1) && unwritten.contains(&15));
    }

    #[test]
    fn a_failed_write_leaves_the_fuse_untouched() {
        // A refusal must not be a partial application — the same rule
        // the JEDEC layer's fuse lists follow.
        let mut map = FuseMap::erased(device());
        let before: Vec<bool> = map.iter().collect();
        let _ = map.set(FuseId(8), true);
        let _ = map.set(FuseId(15), true);
        let _ = map.set(FuseId(99), true);
        assert_eq!(map.iter().collect::<Vec<_>>(), before);
    }

    #[test]
    fn set_all_applies_every_write_or_none_of_them() {
        // The all-or-none rule, stated directly. Nothing exercised it:
        // replacing `set_all`'s body with a plain `for … { self.set(…)? }`
        // loop passed the entire workspace suite, because its only
        // caller resolves every failure before reaching it.
        //
        // A half-applied batch is the outcome this method exists to
        // prevent — the map is then neither the old design nor the new
        // one, and a fuse checksum over it is a valid checksum over the
        // wrong thing.
        let mut map = FuseMap::erased(device());
        map.set(FuseId(3), false).expect("a first writer claims fuse 3");
        let before: Vec<bool> = map.iter().collect();

        // Fuse 0 is writable; fuse 3 now conflicts. A sequential
        // implementation commits fuse 0 before discovering fuse 3.
        let error = map
            .set_all([(FuseId(0), false), (FuseId(3), true)])
            .expect_err("the batch must be refused");
        assert!(matches!(error, FuseWriteError::Conflict { fuse: FuseId(3), .. }), "{error:?}");

        assert_eq!(map.iter().collect::<Vec<_>>(), before, "a refused batch wrote something");
        assert!(!map.is_written(FuseId(0)), "fuse 0 was claimed by a refused batch");
    }

    #[test]
    fn set_all_refuses_a_batch_that_disagrees_with_itself() {
        // Conflict detection *within* the batch, which the map's
        // written-bits cannot catch because neither write has landed.
        // Two encoders disagreeing about one fuse must be detected
        // whether they run in sequence or arrive together.
        let mut map = FuseMap::erased(device());
        let error = map
            .set_all([(FuseId(0), false), (FuseId(1), false), (FuseId(0), true)])
            .expect_err("a batch cannot hold one fuse at two values");
        assert_eq!(
            error,
            FuseWriteError::Conflict {
                fuse: FuseId(0),
                region: "array",
                existing: false,
                attempted: true,
            }
        );
        assert!(!map.is_written(FuseId(1)), "an unrelated write in the batch was applied");
    }

    #[test]
    fn set_all_accepts_a_batch_that_repeats_a_fuse_with_one_value() {
        // Agreement is not disagreement, in a batch as in sequence.
        let mut map = FuseMap::erased(device());
        map.set_all([(FuseId(0), false), (FuseId(0), false)]).expect("agreeing writes");
        assert_eq!(map.get(FuseId(0)), Some(false));
        assert!(map.is_written(FuseId(0)));
    }

    #[test]
    fn set_all_refuses_reserved_and_out_of_range_fuses_without_writing() {
        let mut map = FuseMap::erased(device());
        let before: Vec<bool> = map.iter().collect();
        for batch in [
            vec![(FuseId(0), false), (FuseId(8), false)],
            vec![(FuseId(0), false), (FuseId(9999), false)],
        ] {
            assert!(map.set_all(batch).is_err());
            assert_eq!(map.iter().collect::<Vec<_>>(), before);
            assert!(!map.is_written(FuseId(0)));
        }
    }
}
