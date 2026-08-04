//! Parsing real JEDEC files produced by another implementation.
//!
//! These fixtures come from [Galette](https://github.com/simon-frankau/galette),
//! an independent Rust GAL assembler (MIT licensed; see
//! `targets/fixtures/jedec/README.md` for attribution). They matter more
//! than any hand-written input, for two reasons.
//!
//! First, they are *shaped like real files* rather than like the
//! standard's tidy examples: Galette writes each field's terminating `*`
//! at the start of the following line, uses lowercase hex, and ends with
//! a bare `*` before ETX. A parser written only against the standard's
//! prose would plausibly reject all of that.
//!
//! Second, and more valuable: each file carries a `C` fuse checksum that
//! Galette computed with its own independent implementation. Agreeing
//! with it raises the checksum algorithm from "matches the standard's
//! worked example" to `OpenSourceCrossChecked` (SPEC.md §5.31) — two
//! implementations that never saw each other's code producing the same
//! number over 2194 real fuses.

use decpld_diagnostics::FileId;
use decpld_jedec::parse;

const COMBINATORIAL: &str =
    include_str!("../../../targets/fixtures/jedec/galette-gal16v8-combinatorial.jed");
const SECURITY_BIT: &str =
    include_str!("../../../targets/fixtures/jedec/galette-gal16v8-security-bit.jed");

#[test]
fn parses_a_real_galette_file() {
    let parsed = parse(COMBINATORIAL, FileId(0)).expect("galette output should parse");

    assert_eq!(parsed.file.fuses.len(), 2194, "QF2194");
    assert!(!parsed.file.default_fuse, "F0");
    assert_eq!(parsed.file.security, Some(false), "G0");

    let header = parsed.file.design_specification.expect("a header");
    assert!(header.contains("Galette"), "header should survive: {header:?}");
    assert!(header.contains("GAL16V8"));
}

#[test]
fn our_fuse_checksum_agrees_with_galettes() {
    // The cross-check that matters. Galette wrote `C403e` after computing
    // it from its own fuse data with its own code. If our independently
    // written checksum over the fuses we parsed from its L fields lands
    // on the same value, then both the checksum algorithm AND the L-field
    // placement are confirmed by a second implementation.
    //
    // Note this also validates fuse *placement*: a checksum agreeing over
    // 2194 fuses while any fuse sat at the wrong index would be a
    // coincidence worth about one chance in 65,536.
    let parsed = parse(COMBINATORIAL, FileId(0)).expect("galette output should parse");

    assert_eq!(parsed.file.fuse_checksum, Some(0x403E), "as written by galette");
    assert_eq!(
        parsed.file.computed_fuse_checksum(),
        0x403E,
        "our checksum over galette's fuse data must match what galette declared"
    );
}

#[test]
fn our_transmission_checksum_agrees_with_galettes() {
    // Likewise for the transmission checksum: `999d` is Galette's sum
    // over the STX..=ETX byte range. The parser verifies it during
    // parsing, so a successful parse already proves agreement — this
    // test states the fact explicitly so a regression names itself.
    let parsed = parse(COMBINATORIAL, FileId(0)).expect("galette output should parse");
    assert_eq!(parsed.file.transmission_checksum, Some(0x999D));
}

#[test]
fn the_security_bit_fixture_differs_only_in_g() {
    // Galette's two fixtures are the same design with G0 and G1. Reading
    // one as secured and the other as not — while their fuse data stays
    // identical — confirms the G field is not being confused with fuse
    // data, and that the security bit is nowhere in the fuse vector.
    let plain = parse(COMBINATORIAL, FileId(0)).expect("parses");
    let secured = parse(SECURITY_BIT, FileId(0)).expect("parses");

    assert_eq!(plain.file.security, Some(false));
    assert_eq!(secured.file.security, Some(true));
    assert_eq!(
        plain.file.fuses, secured.file.fuses,
        "the security bit must not disturb the fuse vector"
    );
}

#[test]
fn a_real_file_parses_without_complaint() {
    // Output from a working assembler should produce no diagnostics at
    // all. Warnings here would mean deCPLD is fussy about something real
    // tools legitimately do.
    let parsed = parse(COMBINATORIAL, FileId(0)).expect("parses");
    let messages: Vec<_> = parsed.diagnostics.iter().map(|d| d.headline()).collect();
    assert!(messages.is_empty(), "unexpected diagnostics: {messages:#?}");
}
