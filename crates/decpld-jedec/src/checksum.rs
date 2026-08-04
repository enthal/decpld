//! The transmission checksum.

/// The value that disables transmission-checksum verification.
///
/// JEDEC 3A, "Disabling the Transmission Checksum": receivers "should
/// always accept a dummy value of `0000` as a valid checksum". Some
/// systems cannot control their line endings, so the real sum is
/// unpredictable and this escape hatch exists.
pub const DUMMY_TRANSMISSION_CHECKSUM: u16 = 0;

/// The 16-bit sum of every byte given.
///
/// Per JEDEC 3A this covers "all ASCII characters transmitted between
/// and including the STX and ETX" — both delimiters are counted, and so
/// is every space and line ending, which is why a writer cannot
/// reformat whitespace without recomputing this.
///
/// Wraps at 65,536. The standard says "modulo 65,535"; its own worked
/// example and every implementation say otherwise. See the erratum
/// recorded against `jedec-3a` in `targets/evidence/references.toml`.
///
/// The standard also says "the parity bit is excluded", which refers to
/// 7-bit serial links where the eighth bit carried parity. Modern files
/// are 8-bit clean, so a byte at or above `0x80` is real data — masking
/// it off would corrupt the checksum of any file with a non-ASCII note.
#[must_use]
pub fn transmission_checksum(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |acc, &byte| acc.wrapping_add(u16::from(byte)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The complete worked example from JEDEC 3A, "Computing the
    /// Transmission Checksum", transcribed byte for byte:
    ///
    /// ```text
    ///    <STX>TEST*<CR><LF>       02+54+45+53+54+2A+0D+0A = 0183
    ///    QF0384*<CR><LF>       51+46+30+33+38+34+2A+0D+0A = 01A7
    ///    F0*  <CR><LF>               46+30+2A+20+20+0D+0A = 00F7
    ///    L10 101*<CR><LF>   4C+31+30+20+31+30+31+2A+0D+0A = 01A0
    ///    <ETX>                                         03 = 0003
    ///                                                       ----
    ///                                                       05C4
    /// ```
    ///
    /// Note the two spaces after `F0*` — they are load-bearing (0x20
    /// twice), and dropping them changes the answer. Transcription
    /// errors in a golden fixture are the quiet kind of wrong, so the
    /// per-line subtotals below are checked individually too.
    const WORKED_EXAMPLE: &[u8] = b"\x02TEST*\r\nQF0384*\r\nF0*  \r\nL10 101*\r\n\x03";

    #[test]
    fn transmission_checksum_matches_the_standards_worked_example() {
        assert_eq!(transmission_checksum(WORKED_EXAMPLE), 0x05C4);
    }

    #[test]
    fn each_line_of_the_worked_example_matches_its_stated_subtotal() {
        // Checking the subtotals separately means a mis-transcribed
        // fixture names the line it is wrong on, instead of only
        // failing the grand total.
        let cases: [(&[u8], u16); 5] = [
            (b"\x02TEST*\r\n", 0x0183),
            (b"QF0384*\r\n", 0x01A7),
            (b"F0*  \r\n", 0x00F7),
            (b"L10 101*\r\n", 0x01A0),
            (b"\x03", 0x0003),
        ];
        let mut total: u16 = 0;
        for (line, expected) in cases {
            assert_eq!(transmission_checksum(line), expected, "line {line:?}");
            total = total.wrapping_add(expected);
        }
        assert_eq!(total, 0x05C4, "subtotals must add to the stated grand total");
    }

    #[test]
    fn the_checksum_spans_stx_through_etx_inclusive() {
        // "the 16 bit sum of all ASCII characters transmitted between
        // and including the STX and ETX" — both delimiters count.
        assert_eq!(transmission_checksum(b"\x02"), 0x02);
        assert_eq!(transmission_checksum(b"\x03"), 0x03);
        assert_eq!(transmission_checksum(b"\x02\x03"), 0x05);
    }

    #[test]
    fn the_checksum_wraps_at_sixteen_bits() {
        // See the erratum note in fuses.rs: the text says modulo 65,535,
        // the worked examples and every implementation say 65,536.
        // 258 bytes of 0xFF = 65790 = 0x100FE -> 0x00FE.
        let data = vec![0xFFu8; 258];
        assert_eq!(transmission_checksum(&data), 0x00FE);
    }

    #[test]
    fn the_parity_bit_is_not_masked_off() {
        // The standard says "the parity bit is excluded", which refers
        // to 7-bit serial transmission where the 8th bit was parity.
        // Modern files are 8-bit clean and a byte >= 0x80 is real data
        // (a UTF-8 note field, say). Masking it off would corrupt the
        // checksum of any file containing one.
        assert_eq!(transmission_checksum(&[0x80]), 0x80);
    }

    #[test]
    fn empty_input_sums_to_zero() {
        assert_eq!(transmission_checksum(b""), 0);
    }

    #[test]
    fn the_dummy_checksum_is_zero() {
        // JEDEC 3A, "Disabling the Transmission Checksum": receivers
        // "should always accept a dummy value of 0000".
        assert_eq!(DUMMY_TRANSMISSION_CHECKSUM, 0);
    }
}
