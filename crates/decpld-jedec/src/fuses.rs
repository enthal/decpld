//! The fuse vector and its checksum.

/// A fuse number outside the device's declared fuse count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FuseError {
    #[error("fuse {fuse} is out of range: this device has {count} fuses (0..{count})")]
    OutOfRange { fuse: u32, count: u32 },
}

/// The state of every fuse in one device.
///
/// Storage is JEDEC's own 8-bit word layout: fuse *N* is bit `N % 8` of
/// word `N / 8`, least-significant bit first (JEDEC 3A, "Fuse Checksum
/// Field"). Choosing the standard's layout as the in-memory layout means
/// the fuse checksum is literally the sum of the words — there is no
/// packing step that could be written backwards, which is the error this
/// project can least afford to make quietly.
///
/// Bits past `len` in the final partial word are held at zero so they
/// cannot contribute to the checksum. The standard's own example relies
/// on this: its 500-fuse device has a final word covering fuses 496..499
/// plus four positions that do not exist, and that word reads `00`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuseVector {
    words: Vec<u8>,
    len: u32,
}

impl FuseVector {
    /// A vector of `count` fuses, every one set to `default`.
    ///
    /// `default` is the `F` field: `F0*` clears, `F1*` sets.
    #[must_use]
    pub fn new(count: u32, default: bool) -> Self {
        let word_count = count.div_ceil(8) as usize;
        let mut words = vec![if default { 0xFF } else { 0x00 }; word_count];

        // Trim the non-existent high bits of the final partial word.
        let leftover = count % 8;
        if default
            && leftover != 0
            && let Some(last) = words.last_mut()
        {
            *last = (1u8 << leftover) - 1;
        }

        Self { words, len: count }
    }

    /// The number of fuses, i.e. the `QF` value.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The state of one fuse, or `None` if it does not exist.
    #[must_use]
    pub fn get(&self, fuse: u32) -> Option<bool> {
        (fuse < self.len).then(|| self.bit(fuse))
    }

    /// Set one fuse.
    ///
    /// Out of range is an error rather than a silent no-op: a file
    /// naming a fuse beyond `QF` is malformed, and quietly dropping the
    /// write would let a truncated design look like it compiled.
    ///
    /// Repeated writes are allowed and the last wins — JEDEC 3A, Fuse
    /// List Field, which is how patched files work.
    pub fn set(&mut self, fuse: u32, state: bool) -> Result<(), FuseError> {
        if fuse >= self.len {
            return Err(FuseError::OutOfRange { fuse, count: self.len });
        }
        let mask = 1u8 << (fuse % 8);
        let word = &mut self.words[(fuse / 8) as usize];
        if state {
            *word |= mask
        } else {
            *word &= !mask
        }
        Ok(())
    }

    /// The 16-bit fuse checksum, for the `C` field.
    ///
    /// Covers the entire device — fuse 0 through `QF - 1` — not merely
    /// the fuses an `L` field happened to mention (JEDEC 3A).
    ///
    /// Wraps at 65,536. The standard's text says "modulo 65,535", but
    /// its own worked examples and every extant implementation are
    /// ordinary 16-bit wraparound; recorded as a known erratum against
    /// `jedec-3a` in `targets/evidence/references.toml`.
    #[must_use]
    pub fn checksum(&self) -> u16 {
        self.words.iter().fold(0u16, |acc, &word| acc.wrapping_add(u16::from(word)))
    }

    /// Every fuse state in ascending fuse order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = bool> + '_ {
        (0..self.len).map(|fuse| self.bit(fuse))
    }

    /// Read a fuse without a bounds check. Callers have already
    /// established `fuse < self.len`.
    fn bit(&self, fuse: u32) -> bool {
        self.words[(fuse / 8) as usize] & (1u8 << (fuse % 8)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set fuses from a string of `0`/`1`, starting at `start`.
    /// Whitespace is ignored so test data can be grouped readably.
    fn set_from_bits(fuses: &mut FuseVector, start: u32, bits: &str) {
        for (offset, ch) in bits.chars().filter(|c| !c.is_whitespace()).enumerate() {
            fuses.set(start + offset as u32, ch == '1').expect("fuse in range");
        }
    }

    #[test]
    fn a_new_vector_holds_the_requested_count_all_clear() {
        let fuses = FuseVector::new(500, false);
        assert_eq!(fuses.len(), 500);
        assert!(!fuses.get(0).unwrap());
        assert!(!fuses.get(499).unwrap());
        assert_eq!(fuses.get(500), None, "fuse 500 does not exist in a 500-fuse device");
    }

    #[test]
    fn a_new_vector_can_default_to_set() {
        // `F1*` means every unlisted fuse is 1.
        let fuses = FuseVector::new(24, true);
        assert!(fuses.get(0).unwrap());
        assert!(fuses.get(23).unwrap());
    }

    #[test]
    fn setting_out_of_range_is_refused_not_ignored() {
        // A file naming a fuse beyond QF is malformed. Silently
        // dropping it would let a truncated design look like it
        // compiled.
        let mut fuses = FuseVector::new(8, false);
        assert!(fuses.set(7, true).is_ok());
        assert!(fuses.set(8, true).is_err());
    }

    #[test]
    fn later_writes_replace_earlier_ones() {
        // JEDEC 3A, Fuse List Field: "If the state for a fuse is
        // specified more than once, the last state replaces all
        // previous states." This is how patched files work.
        let mut fuses = FuseVector::new(8, false);
        fuses.set(3, true).unwrap();
        assert!(fuses.get(3).unwrap());
        fuses.set(3, false).unwrap();
        assert!(!fuses.get(3).unwrap());
    }

    // ---- Fuse checksum ----
    //
    // The worked example below is transcribed verbatim from JEDEC 3A,
    // "Fuse Checksum Field". It is the reason the algorithm did not have
    // to be taken on trust from any existing implementation.
    //
    //     QF500*
    //     F0* L0000 01001110 00001000 11110000 11111111 01010001*
    //     C021A*
    //
    // with the standard's own byte breakdown:
    //
    //     fuse 0000  0 1 1 1 0 0 1 0  ->  72     (MSB is fuse 7)
    //     fuse 0008  0 0 0 1 0 0 0 0  ->  10
    //     fuse 0016  0 0 0 0 1 1 1 1  ->  0F
    //     fuse 0024  1 1 1 1 1 1 1 1  ->  FF
    //     fuse 0032  1 0 0 0 1 0 1 0  ->  8A
    //     fuse 0040..0499             ->  00
    //                                    ----
    //                                    021A

    #[test]
    fn fuse_checksum_matches_the_standards_worked_example() {
        let mut fuses = FuseVector::new(500, false);
        set_from_bits(&mut fuses, 0, "01001110 00001000 11110000 11111111 01010001");
        assert_eq!(fuses.checksum(), 0x021A);
    }

    #[test]
    fn fuse_checksum_packs_words_lsb_first() {
        // The individual words from the standard's breakdown, checked
        // one at a time so a packing error names which word is wrong
        // rather than just producing a wrong total.
        let cases = [
            ("01001110", 0x72u16),
            ("00001000", 0x10),
            ("11110000", 0x0F),
            ("11111111", 0xFF),
            ("01010001", 0x8A),
        ];
        for (bits, expected) in cases {
            let mut fuses = FuseVector::new(8, false);
            set_from_bits(&mut fuses, 0, bits);
            assert_eq!(fuses.checksum(), expected, "packing {bits}");
        }
    }

    #[test]
    fn fuse_zero_is_the_least_significant_bit_of_word_zero() {
        // The single most consequential bit of the layout: get this
        // backwards and every checksum in the project is wrong.
        let mut fuses = FuseVector::new(8, false);
        fuses.set(0, true).unwrap();
        assert_eq!(fuses.checksum(), 0x01);

        let mut fuses = FuseVector::new(8, false);
        fuses.set(7, true).unwrap();
        assert_eq!(fuses.checksum(), 0x80);
    }

    #[test]
    fn checksum_covers_the_whole_device_including_unspecified_fuses() {
        // JEDEC 3A: "The checksum is for the entire device (fuse number
        // 0 to the maximum fuse number set by the QF field), not just
        // the fuse states sent."
        let all_set = FuseVector::new(16, true);
        assert_eq!(all_set.checksum(), 0xFF + 0xFF);
    }

    #[test]
    fn fuses_beyond_the_count_do_not_contribute_to_the_checksum() {
        // The standard's example device has 500 fuses, so word 62 holds
        // fuses 496..499 and four positions that do not exist. Those
        // must read as zero: the example's final word is 00.
        let fuses = FuseVector::new(500, true);
        // 62 whole words of 0xFF, then a final word with only the low
        // four bits populated (fuses 496..499) = 0x0F.
        let expected = (62u32 * 0xFF + 0x0F) as u16;
        assert_eq!(fuses.checksum(), expected);
    }

    #[test]
    fn checksum_wraps_at_sixteen_bits() {
        // JEDEC 3A says "modulo 65,535" for both checksums, but its own
        // worked examples are ordinary 16-bit wraparound (65,536), and
        // every extant implementation uses 65,536. Recorded as a known
        // erratum in targets/evidence/references.toml.
        //
        // 2064 fuses all set = 258 words of 0xFF = 65790 = 0x100FE,
        // which is 0x00FE in 16 bits but 0x00FF modulo 65535. This case
        // is precisely where the two readings diverge.
        let fuses = FuseVector::new(258 * 8, true);
        assert_eq!(fuses.checksum(), 0x00FE);
    }

    #[test]
    fn an_empty_device_has_a_zero_checksum() {
        assert_eq!(FuseVector::new(0, false).checksum(), 0);
        assert_eq!(FuseVector::new(0, true).checksum(), 0);
    }

    #[test]
    fn iterating_yields_every_fuse_in_order() {
        let mut fuses = FuseVector::new(10, false);
        fuses.set(0, true).unwrap();
        fuses.set(9, true).unwrap();
        let bits: Vec<bool> = fuses.iter().collect();
        assert_eq!(bits.len(), 10);
        assert!(bits[0]);
        assert!(bits[9]);
        assert!(!bits[5]);
    }
}
