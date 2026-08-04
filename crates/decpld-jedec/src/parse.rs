//! Parsing a JEDEC file into a [`JedecFile`].
//!
//! Structure, per JEDEC 3A:
//!
//! ```text
//! <format> ::= <STX> {<field>} <ETX> <xmit checksum>
//! ```
//!
//! Text before STX and after the checksum is not part of the file — the
//! standard's own example shows "random text" on both sides, a relic of
//! sharing a serial line with a terminal.

use crate::codes;
use crate::model::{JedecField, JedecFile};
use crate::{DUMMY_TRANSMISSION_CHECKSUM, FuseVector, transmission_checksum};
use decpld_diagnostics::{Diagnostic, DiagnosticBundle, FileId, Label, Span, TextRange};

const STX: u8 = 0x02;
const ETX: u8 = 0x03;

/// A successful parse, possibly with warnings.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub file: JedecFile,
    pub diagnostics: DiagnosticBundle,
}

/// One `<identifier><body>*` field, located in the source.
struct RawField<'t> {
    /// The identifier, e.g. `QF`, `L`, `C`.
    identifier: &'t str,
    /// Everything between the identifier and the `*`.
    body: &'t str,
    /// Offset of `body` within the whole file.
    body_offset: u32,
    /// The whole field including its identifier and `*`.
    span: TextRange,
}

/// Parse `text` into a [`JedecFile`].
///
/// Returns `Err` with every diagnostic found when the file cannot be
/// trusted. Parsing continues past recoverable problems so that one run
/// reports as much as possible rather than one error at a time.
pub fn parse(text: &str, file: FileId) -> Result<Parsed, DiagnosticBundle> {
    let mut diagnostics = DiagnosticBundle::new();
    let bytes = text.as_bytes();

    let at = |range: TextRange| Span::new(file, range);

    // ---- Framing ----

    let Some(stx) = bytes.iter().position(|&b| b == STX) else {
        diagnostics.push(
            Diagnostic::error(codes::MISSING_STX, "no STX character: this is not a JEDEC file")
                .with_note("a JEDEC transmission begins with STX (0x02)"),
        );
        return Err(diagnostics);
    };

    let Some(etx) = bytes[stx..].iter().position(|&b| b == ETX).map(|i| i + stx) else {
        diagnostics.push(
            Diagnostic::error(codes::MISSING_ETX, "no ETX character: the file is truncated")
                .with_label(Label::primary(
                    at(TextRange::empty_at(bytes.len() as u32)),
                    "expected ETX (0x03) before here",
                ))
                .with_note("a JEDEC transmission ends with ETX followed by a 4-digit checksum"),
        );
        return Err(diagnostics);
    };

    // ---- Fields ----

    let body = &text[stx + 1..etx];
    let body_start = (stx + 1) as u32;

    let mut fields = Vec::new();
    let mut cursor = 0usize;
    let mut design_specification = None;

    while cursor < body.len() {
        let Some(star) = body[cursor..].find('*').map(|i| i + cursor) else {
            // Remaining text with no terminator. Trailing whitespace
            // before ETX is normal and not a field at all.
            if !body[cursor..].trim().is_empty() {
                let range =
                    TextRange::new(body_start + cursor as u32, body_start + body.len() as u32);
                diagnostics.push(
                    Diagnostic::error(codes::UNTERMINATED_FIELD, "field is missing its `*`")
                        .with_label(Label::primary(at(range), "this field never terminates"))
                        .with_note("every JEDEC field ends with an asterisk"),
                );
            }
            break;
        };

        let raw = &body[cursor..star];
        let range = TextRange::new(body_start + cursor as u32, body_start + star as u32 + 1);

        if design_specification.is_none() {
            // The header is the first field and has no identifier
            // (JEDEC 3A, General Field Syntax).
            design_specification = Some(raw.to_owned());
        } else {
            let trimmed = raw.trim_start();
            let lead = raw.len() - trimmed.len();
            let (identifier, field_body) = split_identifier(trimmed);

            if !identifier.is_empty() {
                fields.push(RawField {
                    identifier,
                    body: field_body,
                    body_offset: body_start + cursor as u32 + lead as u32 + identifier.len() as u32,
                    span: range,
                });
            }
        }

        cursor = star + 1;
    }

    // ---- Interpret fields ----
    //
    // Two passes: QF must be known before any L can be applied, and the
    // standard requires QF to come first anyway. Doing it in one pass
    // would mean either buffering every L or refusing files that are
    // perfectly legal.

    let mut fuse_count: Option<(u32, TextRange)> = None;
    for field in &fields {
        if field.identifier != "QF" {
            continue;
        }
        if let Some((_, previous)) = fuse_count {
            diagnostics.push(
                Diagnostic::error(codes::DUPLICATE_FUSE_COUNT, "more than one QF field")
                    .with_label(Label::primary(at(field.span), "repeated here"))
                    .with_label(Label::secondary(at(previous), "first declared here"))
                    .with_note("a file describes one device, which has one fuse count"),
            );
            continue;
        }
        match parse_number(field.body) {
            Some(count) => fuse_count = Some((count, field.span)),
            None => diagnostics.push(
                Diagnostic::error(codes::INVALID_NUMBER, "QF is not a decimal number")
                    .with_label(Label::primary(at(field.span), "expected `QF<number>*`")),
            ),
        }
    }

    let mut default_fuse = false;
    for field in &fields {
        if field.identifier != "F" {
            continue;
        }
        match field.body.trim() {
            "0" => default_fuse = false,
            "1" => default_fuse = true,
            other => diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_DEFAULT_STATE,
                    format!("default fuse state must be 0 or 1, found `{other}`"),
                )
                .with_label(Label::primary(at(field.span), "expected `F0*` or `F1*`")),
            ),
        }
    }

    let has_fuse_data = fields.iter().any(|f| f.identifier == "L");
    let Some((count, _)) = fuse_count else {
        if has_fuse_data {
            diagnostics
                .push(Diagnostic::error(codes::MISSING_FUSE_COUNT, "no QF field").with_note(
                "QF declares how many fuses the device has; L fields cannot be placed without it",
            ));
        } else {
            diagnostics.push(
                Diagnostic::error(codes::MISSING_FUSE_COUNT, "no QF field")
                    .with_note("every JEDEC file must declare its fuse count"),
            );
        }
        return Err(diagnostics);
    };

    let mut fuses = FuseVector::new(count, default_fuse);
    let mut notes = Vec::new();
    let mut security = None;
    let mut fuse_checksum = None;
    let mut unknown_fields = Vec::new();
    let mut seen_fuse_count = false;

    for field in &fields {
        match field.identifier {
            "QF" => seen_fuse_count = true,
            "F" => {}
            "L" => {
                if !seen_fuse_count {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::FUSE_LIST_BEFORE_FUSE_COUNT,
                            "L field appears before QF",
                        )
                        .with_label(Label::primary(at(field.span), "fuse list here"))
                        .with_note(
                            "JEDEC 3A requires value fields before programming fields, because the fuse count sizes the device",
                        ),
                    );
                    continue;
                }
                apply_fuse_list(field, &mut fuses, &mut diagnostics, file);
            }
            "C" => match parse_hex16(field.body.trim()) {
                Some(value) => fuse_checksum = Some(value),
                None => diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_CHECKSUM_FIELD,
                        "fuse checksum must be four hexadecimal digits",
                    )
                    .with_label(Label::primary(at(field.span), "expected `C<4 hex digits>*`")),
                ),
            },
            "N" => notes.push(field.body.trim().to_owned()),
            "G" => match field.body.trim() {
                "0" => security = Some(false),
                "1" => security = Some(true),
                other => diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_SECURITY_FIELD,
                        format!("security fuse must be 0 or 1, found `{other}`"),
                    )
                    .with_label(Label::primary(at(field.span), "expected `G0*` or `G1*`")),
                ),
            },
            _ => unknown_fields.push(JedecField {
                identifier: field.identifier.to_owned(),
                body: field.body.to_owned(),
                span: at(field.span),
            }),
        }
    }

    // ---- Checksums ----

    if let Some(declared) = fuse_checksum {
        let computed = fuses.checksum();
        // A zero C field means "not computed": both GALasm and WinCUPL
        // emit it, and rejecting it would fail files every other tool
        // accepts. The standard grants the same courtesy to the
        // transmission checksum explicitly.
        if declared != 0 && declared != computed {
            diagnostics.push(
                Diagnostic::error(
                    codes::FUSE_CHECKSUM_MISMATCH,
                    format!("fuse checksum mismatch: file says {declared:04X}, data gives {computed:04X}"),
                )
                .with_note("the file's fuse data does not match the checksum it carries; one of them is wrong"),
            );
        }
    }

    let mut transmission = None;
    let trailer = &text[etx + 1..];
    let digits: String = trailer.chars().take_while(char::is_ascii_hexdigit).collect();
    if digits.len() >= 4
        && let Some(declared) = parse_hex16(&digits[..4])
    {
        transmission = Some(declared);
        let computed = transmission_checksum(&bytes[stx..=etx]);
        if declared != DUMMY_TRANSMISSION_CHECKSUM && declared != computed {
            diagnostics.push(
                Diagnostic::error(
                    codes::TRANSMISSION_CHECKSUM_MISMATCH,
                    format!(
                        "transmission checksum mismatch: file says {declared:04X}, bytes give {computed:04X}"
                    ),
                )
                .with_note("0000 disables this check (JEDEC 3A, Disabling the Transmission Checksum)"),
            );
        }
    }

    if diagnostics.has_errors() {
        return Err(diagnostics);
    }

    Ok(Parsed {
        file: JedecFile {
            design_specification,
            fuses,
            default_fuse,
            notes,
            security,
            fuse_checksum,
            transmission_checksum: transmission,
            unknown_fields,
        },
        diagnostics,
    })
}

/// Apply one `L<number> <states>` field.
fn apply_fuse_list(
    field: &RawField<'_>,
    fuses: &mut FuseVector,
    diagnostics: &mut DiagnosticBundle,
    file: FileId,
) {
    let trimmed = field.body.trim_start();
    let lead = (field.body.len() - trimmed.len()) as u32;

    // JEDEC 3A, Fuse List Field: "A space and/or a carriage return must
    // separate the fuse number from the fuse states."
    //
    // This is enforced rather than guessed at, because without the
    // separator the field is genuinely ambiguous: fuse states are `0`
    // and `1`, which are also digits, so `L001111…` could be fuse 0 with
    // states `01111…`, or fuse 001111 with no states, or anything
    // between. Taking the longest digit run would parse such a file into
    // a confidently wrong fuse vector — the exact failure mode this
    // project cannot afford, since nothing downstream would notice.
    let Some(separator) = trimmed.find(char::is_whitespace) else {
        diagnostics.push(
            Diagnostic::error(
                codes::AMBIGUOUS_FUSE_LIST,
                "L field has no separator between the fuse number and the fuse states",
            )
            .with_label(Label::primary(
                Span::new(file, field.span),
                "cannot tell where the fuse number ends",
            ))
            .with_note("JEDEC 3A requires a space or carriage return after the fuse number")
            .with_note("fuse states are 0 and 1, which are also digits, so this cannot be guessed"),
        );
        return;
    };
    let (number, states) = trimmed.split_at(separator);
    let states_offset = field.body_offset + lead + separator as u32;

    let Some(start) = parse_number(number) else {
        diagnostics.push(
            Diagnostic::error(codes::INVALID_NUMBER, "L field has no fuse number").with_label(
                Label::primary(Span::new(file, field.span), "expected `L<number> <states>*`"),
            ),
        );
        return;
    };

    let mut fuse = start;
    for (offset, ch) in states.char_indices() {
        // Delimiters may appear anywhere among the states; the standard
        // shows the same data grouped three different ways.
        if ch.is_whitespace() {
            continue;
        }
        let at_char = TextRange::new(
            states_offset + offset as u32,
            states_offset + (offset + ch.len_utf8()) as u32,
        );
        let state = match ch {
            '0' => false,
            '1' => true,
            other => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_FUSE_STATE,
                        format!("fuse state must be 0 or 1, found `{other}`"),
                    )
                    .with_label(Label::primary(Span::new(file, at_char), "not a fuse state")),
                );
                return;
            }
        };
        if fuses.set(fuse, state).is_err() {
            diagnostics.push(
                Diagnostic::error(
                    codes::FUSE_OUT_OF_RANGE,
                    format!("fuse {fuse} is beyond the declared count of {}", fuses.len()),
                )
                .with_label(Label::primary(
                    Span::new(file, at_char),
                    "this state has nowhere to go",
                ))
                .with_note("QF declares the device's fuse count; an L field may not exceed it"),
            );
            return;
        }
        fuse += 1;
    }
}

/// The two-letter field identifiers from JEDEC 3A's identifier table.
const TWO_LETTER_IDENTIFIERS: [&str; 3] = ["QF", "QP", "QV"];

/// The one-letter field identifiers from JEDEC 3A's identifier table.
/// `D` is listed as obsolete but still appears in older files.
const ONE_LETTER_IDENTIFIERS: [char; 12] =
    ['N', 'F', 'L', 'C', 'G', 'X', 'V', 'P', 'D', 'A', 'R', 'S'];

/// Split a field into its identifier and body.
///
/// Matched against the standard's identifier table rather than by taking
/// the leading run of letters. "Leading letters" looks right until a
/// field body starts with one: `QFxyz` would be read as an unknown field
/// called `QFxyz` instead of a malformed fuse count, and `N some note`
/// would swallow the note. An unrecognised identifier still yields its
/// leading letters, so genuinely unknown fields keep a usable name.
fn split_identifier(field: &str) -> (&str, &str) {
    for candidate in TWO_LETTER_IDENTIFIERS {
        if let Some(body) = field.strip_prefix(candidate) {
            return (&field[..candidate.len()], body);
        }
    }
    match field.chars().next() {
        Some(first) if ONE_LETTER_IDENTIFIERS.contains(&first) => field.split_at(first.len_utf8()),
        Some(first) if first.is_ascii_alphabetic() => {
            let len = field.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(field.len());
            field.split_at(len)
        }
        _ => ("", field),
    }
}

/// A decimal number, allowing the leading zeros JEDEC 3A permits
/// (`L12` and `L0012` name the same fuse).
fn parse_number(text: &str) -> Option<u32> {
    let text = text.trim();
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Exactly four hexadecimal digits.
fn parse_hex16(text: &str) -> Option<u16> {
    (text.len() == 4 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u16::from_str_radix(text, 16).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codes;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    fn parse_ok(text: &str) -> Parsed {
        match parse(text, FILE) {
            Ok(parsed) => parsed,
            Err(bundle) => {
                let messages: Vec<_> = bundle.iter().map(Diagnostic::headline).collect();
                panic!("expected a successful parse, got: {messages:#?}");
            }
        }
    }

    fn parse_err(text: &str) -> Vec<u16> {
        match parse(text, FILE) {
            Ok(parsed) => {
                panic!("expected a failed parse, got a file with {} fuses", parsed.file.fuses.len())
            }
            Err(bundle) => bundle.iter().map(|d| d.code.as_u16()).collect(),
        }
    }

    /// A minimal well-formed file: STX, header, QF, F, one L, ETX, and a
    /// dummy transmission checksum.
    fn minimal() -> String {
        "\x02minimal*QF8*F0*L0 10100000*\x030000".to_owned()
    }

    // ---- Framing ----

    #[test]
    fn parses_a_minimal_file() {
        let parsed = parse_ok(&minimal());
        assert_eq!(parsed.file.fuses.len(), 8);
        assert_eq!(parsed.file.design_specification.as_deref(), Some("minimal"));
        assert!(parsed.file.fuses.get(0).unwrap());
        assert!(!parsed.file.fuses.get(1).unwrap());
        assert!(parsed.file.fuses.get(2).unwrap());
    }

    #[test]
    fn text_before_stx_is_ignored() {
        // JEDEC 3A's transmission-checksum example is explicit that
        // "random text" may precede the STX; it is not part of the file.
        let parsed = parse_ok(&format!("random text\r\n{}", minimal()));
        assert_eq!(parsed.file.fuses.len(), 8);
    }

    #[test]
    fn text_after_the_transmission_checksum_is_ignored() {
        let parsed = parse_ok(&format!("{} random trailing text", minimal()));
        assert_eq!(parsed.file.fuses.len(), 8);
    }

    #[test]
    fn a_missing_stx_is_an_error() {
        assert!(parse_err("QF8*F0*L0 10100000*\x030000").contains(&codes::MISSING_STX.as_u16()));
    }

    #[test]
    fn a_missing_etx_is_an_error() {
        assert!(parse_err("\x02header*QF8*F0*L0 10100000*").contains(&codes::MISSING_ETX.as_u16()));
    }

    #[test]
    fn an_unterminated_field_is_an_error() {
        // No `*` before ETX: the field runs off the end of the file.
        let codes = parse_err("\x02header*QF8*F0*L0 1010\x030000");
        assert!(codes.contains(&codes::UNTERMINATED_FIELD.as_u16()));
    }

    // ---- Design specification ----

    #[test]
    fn the_design_specification_may_be_empty() {
        let parsed = parse_ok("\x02*QF8*F0*L0 00000000*\x030000");
        assert_eq!(parsed.file.design_specification.as_deref(), Some(""));
    }

    #[test]
    fn the_design_specification_may_span_lines() {
        // JEDEC 3A's example header is three lines of free text before
        // the terminating asterisk.
        let text = "\x02File for PLD 12S8\r\n6809 memory decode\r\nJoe Engineer*QF8*F0*L0 00000000*\x030000";
        let parsed = parse_ok(text);
        let header = parsed.file.design_specification.unwrap();
        assert!(header.contains("Joe Engineer"), "got {header:?}");
        assert!(header.contains("6809 memory decode"));
    }

    // ---- QF ----

    #[test]
    fn a_missing_fuse_count_is_an_error() {
        assert!(
            parse_err("\x02h*F0*L0 1010*\x030000").contains(&codes::MISSING_FUSE_COUNT.as_u16())
        );
    }

    #[test]
    fn a_fuse_list_before_the_fuse_count_is_an_error() {
        // JEDEC 3A: "The value fields must occur before any device
        // programming or testing fields." Without QF there is no way to
        // know how large the vector is, so this cannot be recovered.
        let codes = parse_err("\x02h*F0*L0 1010*QF8*\x030000");
        assert!(codes.contains(&codes::FUSE_LIST_BEFORE_FUSE_COUNT.as_u16()));
    }

    #[test]
    fn a_repeated_fuse_count_is_an_error() {
        let codes = parse_err("\x02h*QF8*QF16*F0*\x030000");
        assert!(codes.contains(&codes::DUPLICATE_FUSE_COUNT.as_u16()));
    }

    #[test]
    fn a_non_numeric_fuse_count_is_an_error() {
        assert!(parse_err("\x02h*QFxyz*F0*\x030000").contains(&codes::INVALID_NUMBER.as_u16()));
    }

    // ---- F ----

    #[test]
    fn the_default_state_fills_unlisted_fuses() {
        let parsed = parse_ok("\x02h*QF16*F1*L0 00000000*\x030000");
        assert!(!parsed.file.fuses.get(0).unwrap(), "L field wins where it speaks");
        assert!(parsed.file.fuses.get(8).unwrap(), "F1 fills the rest");
        assert!(parsed.file.default_fuse);
    }

    #[test]
    fn an_invalid_default_state_is_an_error() {
        assert!(
            parse_err("\x02h*QF8*F2*\x030000").contains(&codes::INVALID_DEFAULT_STATE.as_u16())
        );
    }

    // ---- L ----

    #[test]
    fn fuse_numbers_may_have_leading_zeros() {
        // JEDEC 3A: "L12 and L0012 are the same".
        let parsed = parse_ok("\x02h*QF16*F0*L0012 1*\x030000");
        assert!(parsed.file.fuses.get(12).unwrap());
    }

    #[test]
    fn fuse_states_may_be_split_across_lines_and_spaces() {
        // JEDEC 3A shows all three of these forms as equivalent.
        let one_run = parse_ok("\x02h*QF32*F0*L0 11111011111111111111111111110111*\x030000");
        let split = parse_ok("\x02h*QF32*F0*L0\r\n1111101111111111\r\n1111111111110111*\x030000");
        let spaced = parse_ok("\x02h*QF32*F0*L0 11111011 11111111 11111111 11110111*\x030000");
        assert_eq!(one_run.file.fuses, split.file.fuses);
        assert_eq!(one_run.file.fuses, spaced.file.fuses);
    }

    #[test]
    fn separate_l_fields_address_separate_regions() {
        // The standard's third equivalent form: one L field per chunk.
        let parsed = parse_ok("\x02h*QF16*F0*L00 11111111*L08 00000000*\x030000");
        assert!(parsed.file.fuses.get(0).unwrap());
        assert!(parsed.file.fuses.get(7).unwrap());
        assert!(!parsed.file.fuses.get(8).unwrap());
    }

    #[test]
    fn a_later_fuse_list_overrides_an_earlier_one() {
        // JEDEC 3A, Fuse List Field: "the last state replaces all
        // previous states specified for that fuse. This allows a file to
        // be modified or patched by appending new fuse states."
        let parsed = parse_ok("\x02h*QF8*F0*L0 11111111*L0 00000000*\x030000");
        assert!(!parsed.file.fuses.get(0).unwrap());
        assert!(!parsed.file.fuses.get(7).unwrap());
    }

    #[test]
    fn a_fuse_list_without_a_separator_is_rejected_not_guessed() {
        // JEDEC 3A line 385: "A space and/or a carriage return must
        // separate the fuse number from the fuse states."
        //
        // Without it the field is genuinely ambiguous — states are 0 and
        // 1, which are also digits — so `L001111…` could be fuse 0 with
        // states 01111…, or fuse 001111 with none. Guessing would
        // produce a confidently wrong fuse vector that nothing
        // downstream would catch.
        //
        // The standard's own opening example prints its L fields this
        // way, but that is a rendering artifact of the document: it
        // contradicts the rule stated in its own body text.
        let codes = parse_err("\x02h*QF32*F0*L001111101111111111111111111111*\x030000");
        assert!(codes.contains(&codes::AMBIGUOUS_FUSE_LIST.as_u16()));
    }

    // NOT TESTED, deliberately: JEDEC 3A prints three "equivalent" forms
    // of one fuse list, and they are not equivalent as transcribed in
    // the copy recorded as `jedec-3a`. Measured state counts are 108,
    // 107, and four 27-state blocks placed on a 28-fuse stride. Whatever
    // the original said, this rendering has lost characters — the same
    // rot that dropped the mandatory separators from its opening
    // example.
    //
    // Turning a corrupt transcription into a golden test would encode a
    // wrong expected answer confidently, which is the failure mode the
    // tests-first rule exists to prevent. The behaviours those examples
    // were meant to demonstrate are covered by the three tests above
    // instead, on data whose correctness does not depend on the
    // document's typography.

    #[test]
    fn a_fuse_list_past_the_fuse_count_is_an_error() {
        let codes = parse_err("\x02h*QF8*F0*L0 1111111111*\x030000");
        assert!(codes.contains(&codes::FUSE_OUT_OF_RANGE.as_u16()));
    }

    #[test]
    fn a_non_binary_fuse_state_is_an_error() {
        assert!(
            parse_err("\x02h*QF8*F0*L0 1012*\x030000")
                .contains(&codes::INVALID_FUSE_STATE.as_u16())
        );
    }

    // ---- C ----

    #[test]
    fn a_matching_fuse_checksum_is_accepted() {
        // The standard's worked example, as a whole file this time.
        let text =
            "\x02h*QF500*F0*L0000 01001110 00001000 11110000 11111111 01010001*C021A*\x030000";
        let parsed = parse_ok(text);
        assert_eq!(parsed.file.fuse_checksum, Some(0x021A));
        assert!(parsed.diagnostics.is_empty(), "a correct checksum is not noteworthy");
    }

    #[test]
    fn a_mismatched_fuse_checksum_is_an_error() {
        let codes = parse_err("\x02h*QF8*F0*L0 10000000*CFFFF*\x030000");
        assert!(codes.contains(&codes::FUSE_CHECKSUM_MISMATCH.as_u16()));
    }

    #[test]
    fn a_zero_fuse_checksum_disables_the_check() {
        // GALasm and WinCUPL both emit C0000 to mean "not computed".
        // Treating it as a real value would reject files every other
        // tool accepts.
        let parsed = parse_ok("\x02h*QF8*F0*L0 10000000*C0000*\x030000");
        assert_eq!(parsed.file.fuse_checksum, Some(0));
    }

    #[test]
    fn the_last_fuse_checksum_wins() {
        // JEDEC 3A: "If multiple C fields are received only the last is
        // significant."
        let parsed = parse_ok("\x02h*QF8*F0*L0 10000000*CFFFF*C0001*\x030000");
        assert_eq!(parsed.file.fuse_checksum, Some(1));
    }

    #[test]
    fn a_malformed_checksum_field_is_an_error() {
        assert!(
            parse_err("\x02h*QF8*F0*CZZZZ*\x030000")
                .contains(&codes::INVALID_CHECKSUM_FIELD.as_u16())
        );
    }

    // ---- Transmission checksum ----

    #[test]
    fn a_dummy_transmission_checksum_is_accepted() {
        let parsed = parse_ok(&minimal());
        assert_eq!(parsed.file.transmission_checksum, Some(0));
    }

    #[test]
    fn a_correct_transmission_checksum_is_accepted() {
        let body = "\x02h*QF8*F0*L0 10000000*\x03";
        let sum = crate::transmission_checksum(body.as_bytes());
        let parsed = parse_ok(&format!("{body}{sum:04X}"));
        assert_eq!(parsed.file.transmission_checksum, Some(sum));
    }

    #[test]
    fn a_wrong_transmission_checksum_is_an_error() {
        let codes = parse_err("\x02h*QF8*F0*L0 10000000*\x03ABCD");
        assert!(codes.contains(&codes::TRANSMISSION_CHECKSUM_MISMATCH.as_u16()));
    }

    #[test]
    fn an_absent_transmission_checksum_is_allowed() {
        // Files stored on disk rather than sent down a serial line often
        // stop at ETX.
        let parsed = parse_ok("\x02h*QF8*F0*L0 10000000*\x03");
        assert_eq!(parsed.file.transmission_checksum, None);
    }

    // ---- N and G ----

    #[test]
    fn notes_are_collected_in_order() {
        let parsed = parse_ok("\x02h*QF8*N first note*F0*N second note*\x030000");
        assert_eq!(parsed.file.notes, ["first note", "second note"]);
    }

    #[test]
    fn the_security_fuse_is_recorded_but_defaults_to_absent() {
        assert_eq!(parse_ok(&minimal()).file.security, None);
        assert_eq!(parse_ok("\x02h*QF8*F0*G1*\x030000").file.security, Some(true));
        assert_eq!(parse_ok("\x02h*QF8*F0*G0*\x030000").file.security, Some(false));
    }

    #[test]
    fn an_invalid_security_field_is_an_error() {
        assert!(
            parse_err("\x02h*QF8*F0*G7*\x030000").contains(&codes::INVALID_SECURITY_FIELD.as_u16())
        );
    }

    // ---- Spans ----

    #[test]
    fn diagnostics_point_at_the_offending_field() {
        let text = "\x02h*QF8*F0*L0 1012*\x030000";
        let bundle = parse(text, FILE).expect_err("should fail");
        let diagnostic = bundle
            .iter()
            .find(|d| d.code == codes::INVALID_FUSE_STATE)
            .expect("invalid fuse state diagnostic");
        let span = diagnostic.primary_span().expect("a primary span");
        // The caret must land on the `2`, not on the field or the file.
        assert_eq!(&text[span.range], "2");
    }
}
