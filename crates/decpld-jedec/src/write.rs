//! Writing a [`JedecFile`] back out.

use crate::model::JedecFile;
use crate::transmission_checksum;

/// How the output is laid out. SPEC.md §5.6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriterStyle {
    /// Every fuse stated explicitly, one field per line, 32 fuses per
    /// `L` field. Verbose, diffable, and never ambiguous about what a
    /// fuse is set to — the default, and what `decpld jed canonicalize`
    /// produces.
    #[default]
    Canonical,

    /// Only fuses differing from the `F` default, grouped into maximal
    /// runs. Much smaller for sparse designs, and closer to what real
    /// assemblers emit.
    Compact,
    //
    // NOT IMPLEMENTED: `WinCuplComparable`. SPEC.md lists it, and it
    // will be needed for byte-level oracle comparisons — but matching
    // WinCUPL's layout requires having WinCUPL's output to match, and
    // this project has none yet. Writing it now would mean inventing a
    // format from memory and calling it "WinCUPL-comparable", which is
    // exactly the unevidenced guess CLAUDE.md forbids. It lands in M1
    // with the oracle harness, against real captured files.
}

/// Why a file could not be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// Free text contained `*`, which terminates a JEDEC field.
    ///
    /// The format has no escape mechanism, so this cannot be encoded.
    /// Refusing is the only honest option: emitting it would produce a
    /// file that silently reads back as something else.
    #[error("{context} contains an asterisk, which would terminate the field early: {text:?}")]
    AsteriskInText { context: &'static str, text: String },

    /// The written file did not read back as the file it came from.
    ///
    /// Always a deCPLD bug rather than a user error, and reported rather
    /// than returned as output because a JEDEC file that quietly means
    /// something else is the worst artifact this crate can produce.
    #[error(
        "internal error: the written file does not read back as the same device. \
         This is a deCPLD bug; please report it."
    )]
    RoundTripFailed,
}

/// Join an unmodelled field's identifier to its body.
///
/// A space is inserted when the body begins with a letter, because the
/// parser recovers an unknown identifier by taking the leading
/// alphabetic run: `Z` + `custom` written as `Zcustom*` reads back as
/// identifier `Zcustom` with an empty body. That is silent corruption in
/// the mode whose entire purpose is losing nothing. `QP` + `20` still
/// writes as `QP20*`, the form the standard prints.
fn join_field(identifier: &str, body: &str) -> String {
    if body.starts_with(|c: char| c.is_ascii_alphabetic()) {
        format!("{identifier} {body}*\n")
    } else {
        format!("{identifier}{body}*\n")
    }
}

/// Serialise `file` in the given style.
///
/// The result always carries a correct transmission checksum and a
/// recomputed fuse checksum, so writing a file is also how it gets
/// repaired.
pub fn write(file: &JedecFile, style: WriterStyle) -> Result<String, WriteError> {
    let text = render(file, style)?;

    // Encode, then decode, then compare — CLAUDE.md's central rule,
    // applied to the writer itself rather than only to a test property.
    //
    // A test can only check the cases someone thought to generate, and
    // the identifier-joining bug above slipped through precisely because
    // the property never generated a body starting with a letter. This
    // check does not depend on anyone having imagined the failure. It
    // costs one extra parse of a file measured in kilobytes, and it is
    // what stands between `canonicalize` and quietly rewriting a user's
    // fuse map into a different one.
    //
    // What it does NOT prove: this is the writer checked against
    // deCPLD's own parser, so any assumption the two share passes
    // silently. It is self-consistency, not conformance to JEDEC 3A —
    // `render` below notes one such blind spot, where the parser's two
    // passes accept a field order the standard forbids. Conformance
    // comes from the fixtures and the oracle, not from here.
    // Compared against the file as the writer is *entitled* to emit it,
    // not against the caller's exact field order. Hoisting value fields
    // ahead of the programming fields is required by JEDEC 3A and is a
    // documented service to hand-built files — so treating it as a
    // corruption reported "internal error, please report it" for a
    // caller ordering its own Vec differently. Every other difference
    // still fails.
    let mut expected = file.clone();
    expected.unknown_fields.sort_by_key(|f| !f.is_value_field());

    match crate::parse(&text, decpld_diagnostics::FileId(0)) {
        Ok(parsed) if parsed.file.describes_same_device_as(&expected) => Ok(text),
        _ => Err(WriteError::RoundTripFailed),
    }
}

/// Serialise without verifying. Use [`write`].
fn render(file: &JedecFile, style: WriterStyle) -> Result<String, WriteError> {
    let mut out = String::from('\u{2}');

    // The design specification is the first field and has no identifier.
    let header = &file.design_specification;
    reject_asterisk("the design specification", header)?;
    out.push_str(header);
    out.push_str("*\n");

    out.push_str(&format!("QF{}*\n", file.fuses.len()));

    // JEDEC 3A, Value Fields: "The value fields must occur before any
    // device programming or testing fields in the data file." A retained
    // QP or QV must therefore be hoisted to sit beside QF, not left to
    // trail after the L and C fields in whatever order it arrived.
    //
    // deCPLD's own parser does two passes and would not notice, which is
    // precisely why this matters: the output is for other tools, and
    // `canonicalize` emitting non-conformant files would defeat the
    // command's whole purpose.
    // The parser already hoists these, so for a parsed file this is a
    // no-op. It is repeated here so the writer is correct on its own:
    // a `JedecFile` built by hand still produces a conformant file.
    let (value_fields, other_fields): (Vec<_>, Vec<_>) =
        file.unknown_fields.iter().partition(|f| f.is_value_field());
    for field in &value_fields {
        reject_asterisk("an unmodelled field", &field.body)?;
        out.push_str(&join_field(&field.identifier, &field.body));
    }

    out.push_str(if file.default_fuse { "F1*\n" } else { "F0*\n" });

    for note in &file.notes {
        reject_asterisk("a note", note)?;
        out.push_str(&format!("N {note}*\n"));
    }

    // Silence and an explicit instruction are different facts, so a
    // security bit that was never mentioned is not invented here.
    if let Some(secure) = file.security {
        out.push_str(if secure { "G1*\n" } else { "G0*\n" });
    }

    match style {
        WriterStyle::Canonical => write_all_fuses(&mut out, file),
        WriterStyle::Compact => write_differing_fuses(&mut out, file),
    }

    // Recomputed, never copied: a file declaring `C0000` ("not
    // computed") should come out of canonicalisation carrying a real
    // checksum, not inherit the input's failure to have one.
    out.push_str(&format!("C{:04X}*\n", file.fuses.checksum()));

    for field in &other_fields {
        reject_asterisk("an unmodelled field", &field.body)?;
        out.push_str(&join_field(&field.identifier, &field.body));
    }

    out.push('\u{3}');
    // The checksum covers STX through ETX inclusive, so it can only be
    // computed once everything before it exists.
    let checksum = transmission_checksum(out.as_bytes());
    out.push_str(&format!("{checksum:04X}\n"));

    Ok(out)
}

/// Every fuse, in fields of 32, on their own lines.
fn write_all_fuses(out: &mut String, file: &JedecFile) {
    const PER_FIELD: u32 = 32;
    let states: Vec<bool> = file.fuses.iter().collect();

    for start in (0..file.fuses.len()).step_by(PER_FIELD as usize) {
        let end = (start + PER_FIELD).min(file.fuses.len());
        // Zero-padded to four digits so fuse numbers align in a diff.
        out.push_str(&format!("L{start:04} "));
        for state in &states[start as usize..end as usize] {
            out.push(if *state { '1' } else { '0' });
        }
        out.push_str("*\n");
    }
}

/// Only runs that differ from the default state.
fn write_differing_fuses(out: &mut String, file: &JedecFile) {
    let states: Vec<bool> = file.fuses.iter().collect();
    let default = file.default_fuse;

    let mut index = 0usize;
    while index < states.len() {
        if states[index] == default {
            index += 1;
            continue;
        }
        let start = index;
        while index < states.len() && states[index] != default {
            index += 1;
        }
        out.push_str(&format!("L{start} "));
        for state in &states[start..index] {
            out.push(if *state { '1' } else { '0' });
        }
        out.push_str("*\n");
    }
}

fn reject_asterisk(context: &'static str, text: &str) -> Result<(), WriteError> {
    if text.contains('*') {
        return Err(WriteError::AsteriskInText { context, text: text.to_owned() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParserMode, parse, parse_with_mode};
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    fn round_trip(text: &str, style: WriterStyle) -> crate::JedecFile {
        let original = parse(text, FILE).expect("fixture parses").file;
        let written = write(&original, style).expect("writes");
        let reparsed = parse(&written, FILE)
            .unwrap_or_else(|bundle| {
                let messages: Vec<_> = bundle.iter().map(|d| d.headline()).collect();
                panic!("our own output did not parse: {messages:#?}\n---\n{written}");
            })
            .file;
        assert!(
            original.describes_same_device_as(&reparsed),
            "round trip changed the file\n---\n{written}\n---\nbefore: {original:#?}\nafter: {reparsed:#?}"
        );
        // Both checksums are regenerated rather than copied — see
        // `describes_same_device_as` — so assert they are *correct*
        // rather than *unchanged*. This is the check that would catch a
        // writer emitting a stale or absent checksum.
        assert_eq!(
            reparsed.fuse_checksum,
            Some(reparsed.computed_fuse_checksum()),
            "written file must carry a correct fuse checksum"
        );
        assert!(reparsed.transmission_checksum.is_some());
        reparsed
    }

    const SAMPLE: &str =
        "\x02a design*QF32*F0*L0 10110000111100001010101011110000*C0000*N a note*G0*\x030000";

    // ---- Round trip ----

    #[test]
    fn canonical_output_round_trips() {
        round_trip(SAMPLE, WriterStyle::Canonical);
    }

    #[test]
    fn compact_output_round_trips() {
        round_trip(SAMPLE, WriterStyle::Compact);
    }

    #[test]
    fn a_real_galette_file_round_trips() {
        // The strongest round-trip case available: a file this project
        // did not author, with 2194 fuses and a header that must survive
        // verbatim.
        let text =
            include_str!("../../../targets/fixtures/jedec/galette-gal16v8-combinatorial.jed");
        round_trip(text, WriterStyle::Canonical);
        round_trip(text, WriterStyle::Compact);
    }

    #[test]
    fn value_fields_are_written_before_programming_fields() {
        // JEDEC 3A, Value Fields: "The value fields must occur before any
        // device programming or testing fields in the data file." QP and
        // QV are value fields, so a retained one must be hoisted to sit
        // with QF rather than trailing after the L and C fields.
        //
        // deCPLD's own parser does two passes and would not notice, which
        // is exactly why this needs asserting: the file is for other
        // tools, and `canonicalize` producing non-conformant output would
        // defeat the point of the command.
        let text = "\x02h*QF16*QP20*F0*L0 1010000000000000*\x030000";
        let file = parse(text, FILE).expect("parses").file;
        let written = write(&file, WriterStyle::Canonical).expect("writes");

        let qp = written.find("QP20*").expect("QP retained");
        let first_l = written.find("L0").expect("an L field");
        let c = written.find("C0").expect("a C field");
        assert!(qp < first_l, "QP must precede the fuse list:\n{written}");
        assert!(qp < c, "QP must precede the checksum:\n{written}");
    }

    #[test]
    fn non_value_fields_stay_after_the_fuse_data() {
        // Test vectors are testing fields and belong after the
        // programming fields, so they must NOT be hoisted.
        let text = "\x02h*QF8*F0*V0001 XXXX*L0 11110000*\x030000";
        let file = parse(text, FILE).expect("parses").file;
        let written = write(&file, WriterStyle::Canonical).expect("writes");

        let v = written.find("V0001").expect("V retained");
        let first_l = written.find("L0").expect("an L field");
        assert!(v > first_l, "test vectors follow the fuse list:\n{written}");
    }

    #[test]
    fn an_unmodelled_field_with_a_spaced_body_round_trips() {
        // `QP 20*` and `QP20*` mean the same thing, and the model must
        // pick one representation at parse time. Normalising on the way
        // *out* instead would make a file differ from its own
        // canonicalisation — and, because the diff renderer trims for
        // display too, report it as `["QP20"] -> ["QP20"]`: a difference
        // with nothing visibly different, which is worse than either
        // answer alone.
        round_trip("\x02h*QF8*QP 20*F0*L0 11110000*\x030000", WriterStyle::Canonical);
    }

    #[test]
    fn unmodelled_field_bodies_are_normalised_at_parse_time() {
        let spaced = parse("\x02h*QF8*QP 20*F0*\x030000", FILE).expect("parses").file;
        let tight = parse("\x02h*QF8*QP20*F0*\x030000", FILE).expect("parses").file;
        assert_eq!(spaced.unknown_fields, tight.unknown_fields);
        assert!(spaced.describes_same_device_as(&tight));
    }

    #[test]
    fn unmodelled_fields_survive_a_round_trip() {
        // The whole point of preserve-unknown: a rewrite must not delete
        // the test vectors of a file it did not fully understand.
        let text = "\x02h*QF8*F0*QP20*V0001 XXXX*L0 11110000*\x030000";
        let reparsed = round_trip(text, WriterStyle::Canonical);
        let kept: Vec<&str> =
            reparsed.unknown_fields.iter().map(|f| f.identifier.as_str()).collect();
        assert_eq!(kept, ["QP", "V"]);
    }

    // ---- Checksums ----

    #[test]
    fn the_written_transmission_checksum_verifies() {
        // The checksum covers the bytes it is appended to, so it can
        // only be computed after everything else is emitted. Getting
        // this wrong produces a file that fails its own check — which is
        // why re-parsing in strict mode is the assertion.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        parse_with_mode(&written, FILE, ParserMode::Strict)
            .expect("our output must satisfy its own transmission checksum");
    }

    #[test]
    fn the_written_fuse_checksum_is_recomputed_not_copied() {
        // The input declares C0000 ("not computed"). A writer that
        // copied the declared value would propagate a file's failure to
        // carry a real checksum; canonicalising should fix it.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let expected = file.computed_fuse_checksum();
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(
            written.contains(&format!("C{expected:04X}")),
            "expected C{expected:04X} in:\n{written}"
        );
    }

    // ---- Style ----

    #[test]
    fn canonical_style_is_one_field_per_line() {
        let file = parse(SAMPLE, FILE).unwrap().file;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(written.contains("QF32*\n"), "got:\n{written}");
        assert!(written.contains("F0*\n"));
    }

    #[test]
    fn compact_style_emits_only_fuses_that_differ_from_the_default() {
        // 32 fuses, all default except one. Canonical states every fuse;
        // compact says only what is surprising.
        let text = "\x02h*QF32*F0*L7 1*\x030000";
        let file = parse(text, FILE).unwrap().file;

        let canonical = write(&file, WriterStyle::Canonical).unwrap();
        let compact = write(&file, WriterStyle::Compact).unwrap();

        assert!(compact.len() < canonical.len(), "compact should be smaller");
        assert!(canonical.contains("L0000 "), "canonical states every fuse from 0");
        assert!(compact.contains("L7 1*"), "compact names just the run: {compact}");
    }

    #[test]
    fn compact_output_of_an_all_default_device_has_no_fuse_list() {
        let text = "\x02h*QF32*F0*\x030000";
        let file = parse(text, FILE).unwrap().file;
        let compact = write(&file, WriterStyle::Compact).unwrap();
        assert!(!compact.contains("*L"), "nothing differs from the default: {compact}");
        // It must still round trip: the F field carries the whole story.
        round_trip(text, WriterStyle::Compact);
    }

    #[test]
    fn output_is_deterministic() {
        // SPEC.md §5.32: the same inputs must produce the same bytes.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let a = write(&file, WriterStyle::Canonical).unwrap();
        let b = write(&file, WriterStyle::Canonical).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn writing_is_idempotent_through_a_reparse() {
        // write -> parse -> write must reach a fixed point immediately,
        // or `decpld jed canonicalize` would churn files that are
        // already canonical.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let once = write(&file, WriterStyle::Canonical).unwrap();
        let twice = write(&parse(&once, FILE).unwrap().file, WriterStyle::Canonical).unwrap();
        assert_eq!(once, twice);
    }

    // ---- Refusals ----

    #[test]
    fn a_note_containing_an_asterisk_is_refused() {
        // JEDEC has no escape mechanism: an asterisk inside a note would
        // terminate the field early and silently corrupt the file.
        // Refusing beats emitting something that reads back differently.
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.notes.push("this * ends the field".to_owned());
        assert!(matches!(
            write(&file, WriterStyle::Canonical),
            Err(WriteError::AsteriskInText { .. })
        ));
    }

    #[test]
    fn a_header_containing_an_asterisk_is_refused() {
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.design_specification = "bad * header".to_owned();
        assert!(matches!(
            write(&file, WriterStyle::Canonical),
            Err(WriteError::AsteriskInText { .. })
        ));
    }

    #[test]
    fn an_empty_header_round_trips_as_an_empty_header() {
        // JEDEC cannot express "no header" — the header IS the first
        // field — so an empty one is the floor, and it must survive.
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.design_specification = String::new();
        let written = write(&file, WriterStyle::Canonical).unwrap();
        let reparsed = parse(&written, FILE).unwrap().file;
        assert_eq!(reparsed.design_specification, "");
    }

    #[test]
    fn the_security_bit_is_only_written_when_it_was_present() {
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.security = None;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(!written.contains("*G"), "silence must stay silence: {written}");

        file.security = Some(true);
        assert!(write(&file, WriterStyle::Canonical).unwrap().contains("G1*"));
    }
}

#[cfg(test)]
mod review_findings {
    use super::*;
    use crate::{JedecField, parse};
    use decpld_diagnostics::{FileId, Span, TextRange};

    const FILE: FileId = FileId(0);

    #[test]
    fn an_unmodelled_field_whose_body_starts_with_a_letter_round_trips() {
        // Found by review, using this exact input. `Z` + `custom` was
        // written `Zcustom*` and read back as identifier `Zcustom` with
        // an empty body — silent corruption in the DEFAULT mode, whose
        // stated purpose is losing nothing.
        let text = "\x02h*QF8*F0*L0 11110000*Z custom*\x030000";
        let original = parse(text, FILE).expect("parses").file;
        let written = write(&original, WriterStyle::Canonical).expect("writes");
        let reparsed = parse(&written, FILE).expect("reparses").file;

        assert!(original.describes_same_device_as(&reparsed), "written:\n{written}");
        assert_eq!(reparsed.unknown_fields[0].identifier, "Z");
        assert_eq!(reparsed.unknown_fields[0].body, "custom");
    }

    #[test]
    fn a_value_field_still_writes_without_a_separator() {
        // The fix must not disturb the form the standard prints.
        let file = parse("\x02h*QF8*QP20*F0*\x030000", FILE).expect("parses").file;
        assert!(write(&file, WriterStyle::Canonical).unwrap().contains("QP20*"));
    }

    #[test]
    fn a_hand_built_file_with_unhoisted_value_fields_still_writes() {
        // Found by the second review round. `render` deliberately hoists
        // value fields ahead of the programming fields, and says so:
        // "a `JedecFile` built by hand still produces a conformant file".
        // But the round-trip guard compared against the *unhoisted*
        // input, and `unknown_fields` is an ordered Vec — so the writer's
        // own documented service to hand-built files was reported as
        // `RoundTripFailed`: "internal error … please report it", for
        // nothing worse than field order.
        let mut file = parse("\x02h*QF8*F0*L0 11110000*\x030000", FILE).expect("parses").file;
        file.unknown_fields = vec![
            JedecField {
                identifier: "V".to_owned(),
                body: "0001 XXXX".to_owned(),
                span: Span::new(FILE, TextRange::empty_at(0)),
            },
            JedecField {
                identifier: "QP".to_owned(),
                body: "20".to_owned(),
                span: Span::new(FILE, TextRange::empty_at(0)),
            },
        ];

        let written = write(&file, WriterStyle::Canonical).expect("hoisting is not a corruption");
        let qp = written.find("QP20*").expect("QP retained");
        let v = written.find("V0001").expect("V retained");
        assert!(qp < v, "the value field must be hoisted:\n{written}");
    }

    #[test]
    fn write_refuses_rather_than_emitting_a_file_that_reads_back_differently() {
        // The structural guard: a note carrying whitespace the parser
        // would normalise away cannot be encoded faithfully, so `write`
        // now refuses instead of silently changing it. This check does
        // not depend on anyone having imagined the failure mode, which
        // is exactly why it is worth its cost.
        let mut file = parse("\x02h*QF8*F0*\x030000", FILE).expect("parses").file;
        file.notes.push("  padded  ".to_owned());
        assert_eq!(write(&file, WriterStyle::Canonical), Err(WriteError::RoundTripFailed));
    }
}
