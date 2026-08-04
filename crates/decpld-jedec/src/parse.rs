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

/// How tolerant the parser is of what it finds. SPEC.md §6.2.
///
/// The three modes differ in exactly one dimension — what happens to a
/// field deCPLD does not model — because that is the only decision where
/// reasonable callers genuinely disagree. A validator wants to be told;
/// a rewriter must not lose anything; a converter may not care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParserMode {
    /// The file must be conformant, and the transmission checksum must
    /// be present. For asking "is this file actually conformant?", which
    /// is a different question from "can I read it?".
    ///
    /// Conformance is a three-way distinction, not a two-way one:
    ///
    /// - identifiers JEDEC 3A **defines** are accepted, whether or not
    ///   deCPLD models them;
    /// - identifiers it **reserves** are accepted silently, because the
    ///   standard tells receiving equipment to ignore them;
    /// - anything else — which, since those two sets partition A-Z, means
    ///   a field not starting with an upper-case letter — is rejected.
    ///
    /// So strict mode rejects less than "only what deCPLD understands"
    /// would, and that is deliberate: it answers a question about the
    /// file, not about this implementation.
    Strict,

    /// Accept what real tools emit. Fields deCPLD does not model are
    /// **discarded**, with a warning naming each one.
    ///
    /// Use only when the output is a fresh artifact rather than a
    /// rewrite of the input — discarding a device's test vectors is
    /// silent data loss if anyone expected them to survive.
    Compatible,

    /// As `Compatible`, but unmodelled fields are **retained verbatim**
    /// so the file can be written back without losing anything.
    ///
    /// The default, because "no silent data loss" beats tidiness, and
    /// because `decpld jed canonicalize` would otherwise quietly delete
    /// the test vectors of every file it touched.
    #[default]
    PreserveUnknown,
}

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
    parse_with_mode(text, file, ParserMode::default())
}

/// Parse `text` under an explicit [`ParserMode`].
pub fn parse_with_mode(
    text: &str,
    file: FileId,
    mode: ParserMode,
) -> Result<Parsed, DiagnosticBundle> {
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

        // Report anything outside `<field character>` here, where the
        // offset is still known. The writer refuses such content, but by
        // then it is a `String` with no position, so the only symptom was
        // a `canonicalize` failure naming a category and not a place.
        // The two checks answer different questions — "is this file
        // conformant?" and "can I write it back?" — and share one
        // predicate so they cannot disagree about which files are which.
        for (offset, ch) in raw.char_indices() {
            if crate::is_field_character(ch) {
                continue;
            }
            let at_char = TextRange::new(
                body_start + (cursor + offset) as u32,
                body_start + (cursor + offset + ch.len_utf8()) as u32,
            );
            let message = format!("U+{:04X} is not a JEDEC 3A field character", ch as u32);
            let diagnostic = if mode == ParserMode::Strict {
                Diagnostic::error(codes::INVALID_FIELD_CHARACTER, message)
            } else {
                Diagnostic::warning(codes::INVALID_FIELD_CHARACTER, message)
            };
            diagnostics.push(
                diagnostic
                    .with_label(Label::primary(at(at_char), "cannot appear inside a field"))
                    .with_note(
                        "JEDEC 3A permits 0x20-0x29, 0x2B-0x7E, carriage return and line feed",
                    )
                    .with_note("deCPLD cannot write this file back out unchanged"),
            );
        }

        if design_specification.is_none() {
            // The header is the first field and has no identifier
            // (JEDEC 3A, General Field Syntax).
            design_specification = Some(raw.to_owned());
        } else {
            let trimmed = raw.trim_start();
            let lead = raw.len() - trimmed.len();
            let (identifier, field_body) = split_identifier(trimmed);

            // Recorded even when the identifier is empty. Dropping those
            // silently deleted the field from a rewrite in the mode whose
            // whole purpose is losing nothing, and left the only
            // remaining "not in the standard" case unreachable and
            // unreported (issue #24).
            fields.push(RawField {
                identifier,
                body: field_body,
                body_offset: body_start + cursor as u32 + lead as u32 + identifier.len() as u32,
                span: range,
            });
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

    // JEDEC 3A: `<fuse information> ::= [<default state>] <fuse list>
    // {<fuse list>} [<fuse checksum>]` — at most ONE `F`, and it
    // precedes the fuse lists.
    //
    // Both rules are enforced because the vector is built from `F`
    // before any `L` is applied, so a second or late `F` would
    // retroactively become the base for fuse lists that were written
    // against a different default. `\x02h*QF8*F0*L0 1*F1*\x030000`
    // silently produced 11111111 where a sequential reader gets
    // 10000000 — a wrong fuse vector with no diagnostic, which is the
    // one outcome this crate exists to prevent.
    let mut default_fuse: Option<bool> = None;
    let mut default_state_span: Option<TextRange> = None;
    let first_fuse_list = fields.iter().find(|f| f.identifier == "L").map(|f| f.span);
    for field in &fields {
        if field.identifier != "F" {
            continue;
        }
        let restated = match field.body.trim() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        if let Some(previous) = default_state_span {
            // Repetition and contradiction are different facts, and only
            // one of them makes the file unreadable. `F0*F0*` has exactly
            // one meaning — refusing it would reject a file whose intent
            // is not in doubt, in modes documented as accepting what real
            // tools emit. `F0*F1*` has no meaning at all.
            diagnostics.push(if restated == default_fuse {
                Diagnostic::warning(codes::DUPLICATE_DEFAULT_STATE, "more than one F field")
                    .with_label(Label::primary(at(field.span), "repeated here"))
                    .with_label(Label::secondary(at(previous), "first declared here"))
                    .with_note("JEDEC 3A allows at most one default-state field")
                    .with_note("both state the same default, so the file still has one meaning")
            } else {
                Diagnostic::error(
                    codes::CONTRADICTORY_DEFAULT_STATE,
                    "F fields disagree about the default fuse state",
                )
                .with_label(Label::primary(at(field.span), "declared here"))
                .with_label(Label::secondary(at(previous), "but declared differently here"))
                .with_note("the default state governs every fuse no L field mentions, so this changes the fuse vector")
            });
        }
        // Checked independently of the duplicate rule, not as its
        // `else`: a file can break both at once, and each names a
        // different thing the author got wrong.
        if let Some(list) = first_fuse_list
            && field.span.start > list.start
        {
            diagnostics.push(
                Diagnostic::error(
                    codes::DEFAULT_STATE_AFTER_FUSE_LIST,
                    "F field appears after an L field",
                )
                .with_label(Label::primary(at(field.span), "default state declared here"))
                .with_label(Label::secondary(at(list), "but this fuse list came first"))
                .with_note("JEDEC 3A places the default state before the fuse lists; after them, which fuses it governs depends on reading order"),
            );
        }
        default_state_span = Some(field.span);
        match field.body.trim() {
            "0" => default_fuse = Some(false),
            "1" => default_fuse = Some(true),
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
    let Some((count, fuse_count_span)) = fuse_count else {
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

    // `QF` alone drives allocation, so an unbounded value turns a
    // 19-byte file into hundreds of megabytes (CLAUDE.md's fuzz rule:
    // malformed input must never allocate unbounded). The ceiling is
    // device-independent by design — this crate knows nothing about
    // devices — and sits about a thousandfold above the ATF22V10's
    // ~5900 fuses, so no real part comes close.
    const MAX_FUSES: u32 = 8_000_000;
    if count > MAX_FUSES {
        diagnostics.push(
            Diagnostic::error(
                codes::FUSE_COUNT_TOO_LARGE,
                format!(
                    "QF declares {count} fuses, more than the {MAX_FUSES} deCPLD will allocate"
                ),
            )
            .with_note("this is a guard against malformed input, not a device limit"),
        );
        return Err(diagnostics);
    }

    let mut fuses = FuseVector::new(count, default_fuse.unwrap_or(false));
    let mut covered = FuseCoverage::new(count, default_fuse.is_none());
    let mut notes = Vec::new();
    let mut security = None;
    let mut security_span: Option<TextRange> = None;
    // A repeated `C` deliberately has no rule of its own: the checksum is
    // recomputed on every write, and a `C` disagreeing with the fuse data
    // already fails FUSE_CHECKSUM_MISMATCH below. Unlike `F` and `G`, a
    // duplicate cannot change what the file *means*.
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
                if apply_fuse_list(field, &mut fuses, &mut covered, &mut diagnostics, file).is_err()
                {
                    // Nothing further to do — a rejected field has
                    // already pushed its own diagnostics, and `parse`
                    // returns `Err` if any of them is an error. Written
                    // out rather than `let _ =` so the contract stays
                    // visible, and asserted so a future diagnostic
                    // downgraded to a warning cannot silently produce a
                    // half-read fuse vector.
                    debug_assert!(
                        diagnostics.has_errors(),
                        "a rejected fuse list must have reported why"
                    );
                }
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
            "G" => {
                let state = match field.body.trim() {
                    "0" => Some(false),
                    "1" => Some(true),
                    other => {
                        diagnostics.push(
                            Diagnostic::error(
                                codes::INVALID_SECURITY_FIELD,
                                format!("security fuse must be 0 or 1, found `{other}`"),
                            )
                            .with_label(Label::primary(at(field.span), "expected `G0*` or `G1*`")),
                        );
                        None
                    }
                };
                // The same rule as `F`, and it matters more here. The
                // security fuse is irreversible, and CLAUDE.md makes
                // setting it require two explicit CLI flags — so
                // resolving `G0*G1*` by last-writer-wins would infer
                // "permanently lock this part" from a file that cannot
                // make up its mind, walking straight around that gate.
                match (security, state, security_span) {
                    (Some(previous), Some(new), Some(first)) if previous != new => diagnostics
                        .push(
                            Diagnostic::error(
                                codes::CONTRADICTORY_SECURITY_FIELD,
                                "G fields disagree about the security fuse",
                            )
                            .with_label(Label::primary(at(field.span), "declared here"))
                            .with_label(Label::secondary(at(first), "but declared differently here"))
                            .with_note(
                                "the security fuse permanently prevents reading the device back, so this is never guessed",
                            ),
                        ),
                    (Some(_), Some(_), Some(first)) => diagnostics.push(
                        Diagnostic::warning(codes::DUPLICATE_SECURITY_FIELD, "more than one G field")
                            .with_label(Label::primary(at(field.span), "repeated here"))
                            .with_label(Label::secondary(at(first), "first declared here"))
                            .with_note("both state the same value, so the file still has one meaning"),
                    ),
                    _ => {}
                }
                if let Some(new) = state {
                    security = Some(new);
                    security_span.get_or_insert(field.span);
                }
            }
            other => {
                // Three outcomes, not two. A field can be legal JEDEC
                // deCPLD has no use for (say so: nothing), reserved by
                // the standard (say nothing — it tells receivers to
                // ignore these), or not a field identifier at all (say
                // so loudly).
                if classify(other) == IdentifierClass::NotInStandard {
                    let message = if other.is_empty() {
                        "field does not begin with an identifier".to_owned()
                    } else {
                        format!("`{other}` is not a JEDEC 3A field identifier")
                    };
                    let diagnostic = if mode == ParserMode::Strict {
                        Diagnostic::error(codes::UNKNOWN_FIELD, message)
                    } else {
                        Diagnostic::warning(codes::UNKNOWN_FIELD, message)
                    };
                    diagnostics.push(
                        diagnostic
                            .with_label(Label::primary(at(field.span), "unrecognised field"))
                            .with_note(
                                "JEDEC 3A field identifiers are single letters, optionally followed by subfield characters",
                            ),
                    );
                }

                match mode {
                    // Discarding is a choice, so it is announced. A
                    // field that vanished without a word would be
                    // indistinguishable from one that was never there.
                    ParserMode::Compatible => diagnostics.push(
                        Diagnostic::warning(
                            codes::FIELD_DISCARDED,
                            if other.is_empty() {
                                "field with no identifier discarded: deCPLD does not model it"
                                    .to_owned()
                            } else {
                                format!("`{other}` field discarded: deCPLD does not model it")
                            },
                        )
                        .with_label(Label::primary(at(field.span), "dropped"))
                        .with_note("parse in preserve-unknown mode to retain it"),
                    ),
                    ParserMode::Strict | ParserMode::PreserveUnknown => {
                        unknown_fields.push(JedecField {
                            // Normalised here rather than on the way
                            // out, so the model holds one
                            // representation. `QP 20*` and `QP20*` mean
                            // the same thing; if the difference survived
                            // into the model, a file would differ from
                            // its own canonicalisation.
                            identifier: other.to_owned(),
                            body: field.body.trim().to_owned(),
                            span: at(field.span),
                        });
                    }
                }
            }
        }
    }

    // Value fields must precede programming and testing fields (JEDEC
    // 3A, Value Fields). Hoisting here rather than only on the way out
    // means the model holds one canonical order, so a file and its own
    // canonicalisation compare equal. A stable sort keeps the relative
    // order within each group, which matters: test vectors are a
    // sequence, and V0002 arriving before V0001 would be a real change.
    unknown_fields.sort_by_key(|field| !field.is_value_field());

    // ---- Fuse coverage ----
    //
    // JEDEC 3A line 376: "If no F field is specified, all fuse states
    // must be defined." Without this the parser zero-filled whatever the
    // L fields did not reach and said nothing, inventing states that the
    // file never gave — and `write` then emitted the `F0*` that made the
    // invention look deliberate.
    if default_state_span.is_none()
        && let Some(first) = covered.first_unstated()
    {
        let missing = covered.unstated_count();
        diagnostics.push(
            Diagnostic::error(
                codes::INCOMPLETE_FUSE_COVERAGE,
                format!(
                    "no F field, so every fuse state must be given, but {missing} are not \
                     (first is fuse {first})"
                ),
            )
            .with_label(Label::primary(at(fuse_count_span), "this many fuses were declared"))
            .with_note(
                "JEDEC 3A: \"If no F field is specified, all fuse states must be defined \
                 after the QF field …\"",
            )
            .with_note("add an F0 or F1 field, or state the remaining fuses in an L field"),
        );
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
    if digits.len() < 4 && mode == ParserMode::Strict {
        // `<format> ::= <STX> {<field>} <ETX> <xmit checksum>` — the
        // checksum is part of the grammar. Files stored on disk rather
        // than sent down a serial line routinely omit it, so only strict
        // mode insists.
        diagnostics.push(
            Diagnostic::error(
                codes::MISSING_TRANSMISSION_CHECKSUM,
                "no transmission checksum after ETX",
            )
            .with_note("JEDEC 3A requires four hex digits after ETX; 0000 disables the check"),
        );
    }
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
            design_specification: design_specification.unwrap_or_default(),
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

/// Apply one `L<number> <states>` field, all or nothing.
///
/// States are collected into a scratch list and committed to `fuses`
/// only once the whole field is known good. Writing as it went left the
/// live vector half-updated on any fault; that was safe only because
/// every early return also pushed an error and `parse` gates on
/// `has_errors()` — an invariant held by two distant pieces of code and
/// enforced by neither. Downgrading one of those diagnostics to a
/// warning, or adding a `--best-effort` mode, would have shipped a
/// half-applied fuse vector with nothing to catch it.
///
/// Returns `Err(())` when nothing was applied, so a caller cannot
/// mistake a rejected field for an accepted one. CLAUDE.md: make wrong
/// states unrepresentable rather than guarding against them.
fn apply_fuse_list(
    field: &RawField<'_>,
    fuses: &mut FuseVector,
    covered: &mut FuseCoverage,
    diagnostics: &mut DiagnosticBundle,
    file: FileId,
) -> Result<(), ()> {
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
        return Err(());
    };
    let (number, states) = trimmed.split_at(separator);
    let states_offset = field.body_offset + lead + separator as u32;

    let Some(start) = parse_number(number) else {
        diagnostics.push(
            Diagnostic::error(codes::INVALID_NUMBER, "L field has no fuse number").with_label(
                Label::primary(Span::new(file, field.span), "expected `L<number> <states>*`"),
            ),
        );
        return Err(());
    };

    let mut pending: Vec<(u32, bool)> = Vec::new();
    let mut fuse = start;
    let mut sound = true;

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
        // Range FIRST, before the character is interpreted at all.
        //
        // Order is load-bearing, not stylistic. Checking it second meant
        // the bad-character branch advanced `fuse` with nothing bounding
        // it — and `parse_number` puts no ceiling on an L field's fuse
        // number, only `QF` is capped — so `L4294967295 X*` overflowed:
        // a panic in debug, a silent wrap in release. Checking first
        // makes `fuse` unable to exceed the device on any path, so the
        // increments below cannot overflow.
        //
        // Unlike a bad character this does not recover: every state
        // after it is out of range too, so continuing would emit one
        // diagnostic per remaining character, all saying the same thing.
        if !fuses.contains(fuse) {
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
            return Err(());
        }

        match ch {
            '0' => pending.push((fuse, false)),
            '1' => pending.push((fuse, true)),
            other => {
                // Reported and skipped rather than abandoning the field.
                // The module promises one run reports as much as
                // possible; stopping here made a file with three bad
                // characters take three runs to fix.
                diagnostics.push(
                    Diagnostic::error(
                        codes::INVALID_FUSE_STATE,
                        format!("fuse state must be 0 or 1, found `{other}`"),
                    )
                    .with_label(Label::primary(Span::new(file, at_char), "not a fuse state")),
                );
                sound = false;
            }
        }
        fuse += 1;
    }

    if !sound {
        return Err(());
    }
    for (fuse, state) in pending {
        // Cannot fail: every fuse was range-checked above, against the
        // same vector, which does not change in between.
        let _ = fuses.set(fuse, state);
        // Recorded here, with the commit, so a rejected field cannot
        // count towards coverage — it wrote nothing.
        covered.state(fuse);
    }
    Ok(())
}

/// The `Q` subfields JEDEC 3A defines (lines 308-316). Listed separately
/// from the split table below because their second letter is part of the
/// identifier: the standard writes `QF1024` as identifier `QF`, body
/// `1024`.
const TWO_LETTER_IDENTIFIERS: [&str; 3] = ["QF", "QP", "QV"];

/// Identifiers deCPLD splits after a single letter.
///
/// This is a statement about **naming**, not about conformance — it
/// decides where the identifier ends and the body begins, so that
/// `N some note` is a note rather than a field called `Nsome`.
///
/// Keeping it apart from [`classify`]'s tables is the point. Reusing one
/// table for both jobs is what dropped `T` and `Q` from the standard's
/// identifier set: a letter had to earn its place by being splittable,
/// and `Q` is not (its subfields take two letters), so `Q` fell out of
/// the conformance question too.
const SPLIT_AT_ONE_LETTER: [char; 13] =
    ['N', 'F', 'L', 'C', 'G', 'X', 'V', 'P', 'D', 'A', 'R', 'S', 'T'];

/// What JEDEC 3A says about an identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentifierClass {
    /// In `<field identifier>` (lines 219-221). Structurally legal,
    /// whether or not deCPLD models it.
    Defined,
    /// In `<reserved identifier>` (lines 223-225). Lines 234-236:
    /// "Receiving equipment should ignore fields starting with reserved
    /// identifiers."
    Reserved,
    /// Not an identifier at all. Because the two tables above partition
    /// A-Z exactly, this means the field did not begin with a letter.
    NotInStandard,
}

/// Classify a field identifier against JEDEC 3A.
///
/// Evidence: `jedec-3a` (sha256 `9207f92b…` in
/// `targets/evidence/references.toml`), `<field identifier>` at lines
/// 219-221 and `<reserved identifier>` at lines 223-225. The prose table
/// at lines 239-251 agrees. The two sets are disjoint and cover A-Z, which
/// `identifier_tables::the_two_tables_partition_the_alphabet` asserts —
/// the check that would have caught the original transcription.
///
/// Classification is by the **first** character, because line 230 permits
/// multi-character identifiers as subfields ("A1", "A$", "AB3"). So `QX`
/// is a subfield of the defined `Q`, not an invented identifier.
fn classify(identifier: &str) -> IdentifierClass {
    match identifier.chars().next() {
        Some('A' | 'C' | 'D' | 'F' | 'G' | 'L' | 'N' | 'P' | 'Q' | 'R' | 'S' | 'T' | 'V' | 'X') => {
            IdentifierClass::Defined
        }
        Some('B' | 'E' | 'H' | 'I' | 'J' | 'K' | 'M' | 'O' | 'U' | 'W' | 'Y' | 'Z') => {
            IdentifierClass::Reserved
        }
        _ => IdentifierClass::NotInStandard,
    }
}

/// Split a field into its identifier and body.
///
/// Matched against the standard's identifier table rather than by taking
/// the leading run of letters. "Leading letters" looks right until a
/// field body starts with one: `QFxyz` would be read as an unknown field
/// called `QFxyz` instead of a malformed fuse count, and `N some note`
/// would swallow the note. An unrecognised identifier still yields its
/// leading letters, so genuinely unknown fields keep a usable name.
pub(crate) fn split_identifier(field: &str) -> (&str, &str) {
    for candidate in TWO_LETTER_IDENTIFIERS {
        if let Some(body) = field.strip_prefix(candidate) {
            return (&field[..candidate.len()], body);
        }
    }
    match field.chars().next() {
        Some(first) if SPLIT_AT_ONE_LETTER.contains(&first) => field.split_at(first.len_utf8()),
        Some(first) if first.is_ascii_alphabetic() => {
            let len = field.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(field.len());
            field.split_at(len)
        }
        // No leading letter, so there is no identifier to find. The
        // whole field becomes the body and keeps an empty identifier,
        // rather than being discarded: `classify` calls this
        // `NotInStandard` and it is reported, but a rewrite must still
        // emit it unchanged (issue #24).
        _ => ("", field),
    }
}

/// Which fuses an `L` field has explicitly stated.
///
/// Deliberately separate from [`FuseVector`] rather than a flag on it.
/// Coverage is a fact about how a *file* was written, not about a
/// device's fuse states, and a vector carrying it would have to exclude
/// it from `PartialEq` by hand — otherwise a parsed vector would compare
/// unequal to an identical constructed one, quietly breaking `diff` and
/// every round-trip property.
struct FuseCoverage {
    /// Empty when coverage is not required, which is the common case.
    stated: Vec<bool>,
}

impl FuseCoverage {
    /// Track coverage only when it will actually be read.
    ///
    /// A file *with* an `F` field can never be short of coverage, so
    /// allocating for it is pure waste — a byte per fuse, eight times the
    /// `FuseVector` it shadows, and 8 MB at the `MAX_FUSES` ceiling that
    /// exists precisely to stop malformed input allocating unbounded.
    fn new(count: u32, required: bool) -> Self {
        Self { stated: if required { vec![false; count as usize] } else { Vec::new() } }
    }

    fn state(&mut self, fuse: u32) {
        // Callers range-check against the same vector before committing,
        // so this cannot be out of range when tracking is on. Asserted
        // rather than silently ignored: the failure direction is safe
        // (an under-count rejects a good file, never accepts a bad one),
        // but a silent miss would still be a bug worth catching.
        debug_assert!(
            self.stated.is_empty() || (fuse as usize) < self.stated.len(),
            "fuse {fuse} is outside the coverage map"
        );
        if let Some(slot) = self.stated.get_mut(fuse as usize) {
            *slot = true;
        }
    }

    /// The lowest fuse no `L` field mentioned, if any.
    fn first_unstated(&self) -> Option<u32> {
        self.stated.iter().position(|stated| !stated).map(|index| index as u32)
    }

    fn unstated_count(&self) -> usize {
        self.stated.iter().filter(|stated| !**stated).count()
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
        parse_err_with(text, ParserMode::default())
    }

    fn parse_err_with(text: &str, mode: ParserMode) -> Vec<u16> {
        match parse_with_mode(text, FILE, mode) {
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
        assert_eq!(parsed.file.design_specification, "minimal");
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
        assert_eq!(parsed.file.design_specification, "");
    }

    #[test]
    fn the_design_specification_may_span_lines() {
        // JEDEC 3A's example header is three lines of free text before
        // the terminating asterisk.
        let text = "\x02File for PLD 12S8\r\n6809 memory decode\r\nJoe Engineer*QF8*F0*L0 00000000*\x030000";
        let parsed = parse_ok(text);
        let header = parsed.file.design_specification;
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
        assert_eq!(parsed.file.default_fuse, Some(true));
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

    // ---- Parser modes ----

    /// A file carrying a `QP` pin count and a `V` test vector — both
    /// legal JEDEC that deCPLD does not model, a `Z` field that JEDEC 3A
    /// *reserves*, and a `123` field that is not a field at all.
    ///
    /// All three categories in one fixture, because the interesting
    /// behaviour is precisely that they are treated differently.
    const WITH_EXTRA_FIELDS: &str = "\x02h*QF8*F0*QP20*V0001 XXXX*Z custom*123*\x030000";

    #[test]
    fn preserve_unknown_retains_every_unmodelled_field() {
        let parsed =
            parse_with_mode(WITH_EXTRA_FIELDS, FILE, ParserMode::PreserveUnknown).expect("parses");
        let kept: Vec<&str> =
            parsed.file.unknown_fields.iter().map(|f| f.identifier.as_str()).collect();
        // The empty identifier is the `123` field: nothing to name it
        // by, but it must still survive a rewrite.
        assert_eq!(kept, ["QP", "V", "Z", ""]);
    }

    #[test]
    fn preserve_unknown_is_the_default() {
        let default = parse(WITH_EXTRA_FIELDS, FILE).expect("parses");
        let explicit =
            parse_with_mode(WITH_EXTRA_FIELDS, FILE, ParserMode::PreserveUnknown).expect("parses");
        assert_eq!(default.file, explicit.file);
    }

    #[test]
    fn only_non_standard_identifiers_are_warned_about() {
        // QP and V are legal JEDEC that deCPLD has no use for; that is
        // not the user's problem and must not produce noise.
        //
        // `Z` produces none either, and that is the part worth stating:
        // JEDEC 3A lines 234-236 tell receiving equipment to *ignore*
        // fields with reserved identifiers, and real Atmel-toolchain
        // files carry `J` and `U`. Warning about them made deCPLD noisy
        // about conformant files.
        //
        // `123` has no identifier at all, which is worth saying.
        let parsed = parse(WITH_EXTRA_FIELDS, FILE).expect("parses");
        let warned: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::UNKNOWN_FIELD)
            .map(|d| d.message.clone())
            .collect();
        assert_eq!(warned.len(), 1, "expected exactly one warning, got {warned:#?}");
        assert!(warned[0].contains("does not begin with an identifier"), "{}", warned[0]);
    }

    #[test]
    fn compatible_mode_discards_unmodelled_fields_but_says_so() {
        // Dropping data silently would make a discarded field
        // indistinguishable from one that was never there.
        let parsed =
            parse_with_mode(WITH_EXTRA_FIELDS, FILE, ParserMode::Compatible).expect("parses");
        assert!(parsed.file.unknown_fields.is_empty());
        let discarded =
            parsed.diagnostics.iter().filter(|d| d.code == codes::FIELD_DISCARDED).count();
        assert_eq!(discarded, 4, "each dropped field is announced");
    }

    #[test]
    fn strict_mode_rejects_a_non_standard_identifier() {
        let codes = parse_err_with(WITH_EXTRA_FIELDS, ParserMode::Strict);
        assert!(codes.contains(&codes::UNKNOWN_FIELD.as_u16()));
    }

    #[test]
    fn strict_mode_accepts_standard_fields_it_does_not_model() {
        // QP and V are conformant JEDEC. Strict means "conformant", not
        // "only what deCPLD understands".
        let parsed =
            parse_with_mode("\x02h*QF8*F0*QP20*V0001 XXXX*\x030000", FILE, ParserMode::Strict)
                .expect("QP and V are legal JEDEC");
        assert_eq!(parsed.file.unknown_fields.len(), 2);
    }

    #[test]
    fn strict_mode_requires_a_transmission_checksum() {
        let codes = parse_err_with("\x02h*QF8*F0*\x03", ParserMode::Strict);
        assert!(codes.contains(&codes::MISSING_TRANSMISSION_CHECKSUM.as_u16()));
        // The lenient modes accept it: files kept on disk rather than
        // sent down a serial line routinely stop at ETX.
        assert!(parse_with_mode("\x02h*QF8*F0*\x03", FILE, ParserMode::Compatible).is_ok());
    }

    // ---- Line endings and whitespace ----

    #[test]
    fn line_ending_style_does_not_change_the_result() {
        // JEDEC files come from DOS tooling and arrive with CRLF, LF, or
        // (from very old tools) bare CR. All three describe one device.
        let lf = "\x02h*\nQF16*\nF0*\nL0 1010000000000000*\n\x030000";
        let crlf = lf.replace('\n', "\r\n");
        let cr = lf.replace('\n', "\r");

        let a = parse_ok(lf);
        let b = parse_ok(&crlf);
        let c = parse_ok(&cr);
        assert_eq!(a.file.fuses, b.file.fuses);
        assert_eq!(a.file.fuses, c.file.fuses);
    }

    #[test]
    fn generous_whitespace_between_fields_is_accepted() {
        let parsed =
            parse_ok("\x02h*\r\n\r\n   QF16*  \r\n\tF0*\r\n  L0 1010000000000000*\r\n\x030000");
        assert_eq!(parsed.file.fuses.len(), 16);
        assert!(parsed.file.fuses.get(0).unwrap());
        assert!(parsed.file.fuses.get(2).unwrap());
    }

    #[test]
    fn a_terminator_at_the_start_of_the_next_line_is_accepted() {
        // Galette writes each field's `*` at the start of the following
        // line rather than immediately after the field text. It is
        // conformant — the `*` still terminates the preceding field —
        // but it looks alien enough that a parser written only against
        // the standard's tidy examples might reject it.
        let parsed = parse_ok("\x02header\n*F0\n*QF16\n*L0 1010000000000000\n*\x030000");
        assert_eq!(parsed.file.fuses.len(), 16);
        assert!(parsed.file.fuses.get(0).unwrap());
    }

    #[test]
    fn hex_digits_accept_either_case() {
        // Galette writes `C403e` — uppercase identifier, lowercase
        // digits — while WinCUPL writes both uppercase. Identifiers
        // themselves are uppercase in JEDEC 3A's table and no tool emits
        // them otherwise, so a lowercase `c` is deliberately NOT
        // accepted: loosening the grammar past what any implementation
        // produces buys nothing and blurs what a `C` field is.
        let lower = parse_ok("\x02h*QF8*F0*L0 11111111*C00ff*\x030000");
        let upper = parse_ok("\x02h*QF8*F0*L0 11111111*C00FF*\x030000");
        assert_eq!(lower.file.fuse_checksum, Some(0x00FF));
        assert_eq!(upper.file.fuse_checksum, Some(0x00FF));
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

/// The identifier tables, transcribed from the pinned JEDEC 3A copy.
///
/// Evidence: `jedec-3a` (sha256 `9207f92b…`, recorded in
/// `targets/evidence/references.toml`), re-fetched and re-hashed
/// 2026-08-03. Both tables come from the BNF, which is the normative
/// statement; the prose table at lines 239-251 agrees with it.
#[cfg(test)]
mod identifier_tables {
    use super::*;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    /// JEDEC 3A lines 219-221, `<field identifier>`.
    const DEFINED: [char; 14] =
        ['A', 'C', 'D', 'F', 'G', 'L', 'N', 'P', 'Q', 'R', 'S', 'T', 'V', 'X'];

    /// JEDEC 3A lines 223-225, `<reserved identifier>`.
    const RESERVED: [char; 12] = ['B', 'E', 'H', 'I', 'J', 'K', 'M', 'O', 'U', 'W', 'Y', 'Z'];

    #[test]
    fn the_two_tables_partition_the_alphabet() {
        // The strongest check available on a hand transcription, and the
        // one that would have caught the original defect: 14 + 12 = 26,
        // with no letter in both and no letter in neither. The old table
        // had twelve entries and no notion of "reserved" at all, so it
        // could not have satisfied this.
        let mut seen: Vec<char> = DEFINED.iter().chain(RESERVED.iter()).copied().collect();
        seen.sort_unstable();
        let alphabet: Vec<char> = ('A'..='Z').collect();
        assert_eq!(seen, alphabet, "the tables must cover A-Z exactly once");
    }

    #[test]
    fn classify_agrees_with_the_transcribed_tables() {
        // The partition test above checks the transcription against
        // itself, which proves the transcription is self-consistent and
        // nothing about the code. `classify` carries its own match arms,
        // so without this the two could drift and only the partition
        // would still hold.
        //
        // The other tests in this module reach `classify` through
        // `parse`, which is the behavioural check; this is the direct
        // one, and it covers the non-letter case the others cannot reach
        // by iterating letters.
        for letter in DEFINED {
            assert_eq!(classify(&letter.to_string()), IdentifierClass::Defined, "{letter}");
        }
        for letter in RESERVED {
            assert_eq!(classify(&letter.to_string()), IdentifierClass::Reserved, "{letter}");
        }
        for not_a_letter in ["", "1", "1abc", " ", "$", "*", "\u{e9}"] {
            assert_eq!(
                classify(not_a_letter),
                IdentifierClass::NotInStandard,
                "{not_a_letter:?} does not begin with a letter"
            );
        }
        // Lower case is not the standard's alphabet either; no tool
        // emits it, and accepting it would silently widen the table.
        for lower in 'a'..='z' {
            assert_eq!(
                classify(&lower.to_string()),
                IdentifierClass::NotInStandard,
                "lower-case {lower}"
            );
        }
    }

    #[test]
    fn every_defined_identifier_is_accepted_in_strict_mode() {
        // `T` (test cycles, line 245) was missing from the old table, so
        // a conformant file using it was rejected as "not a JEDEC 3A
        // field identifier". `Q` was missing too, which is why a `Q`
        // subfield outside QF/QP/QV was mis-reported.
        // A well-formed body per identifier. The bodies matter: this
        // test is about the *identifier* being recognised, so a body
        // that trips a field-specific rule would fail for the wrong
        // reason and prove nothing.
        let field = |letter: char| -> String {
            match letter {
                'C' => "C0000".to_owned(),   // four hex digits, 0 = "not computed"
                'Q' => "QX5".to_owned(),     // a subfield; QF is already in the file
                'L' => "L4 1111".to_owned(), // within the declared 8 fuses
                'N' => "N a note".to_owned(),
                'P' => "P1 2 3".to_owned(),
                'T' => "T4".to_owned(), // test cycles — absent from the old table
                'V' => "V0001 XXXX".to_owned(),
                other => format!("{other}0"),
            }
        };

        for letter in DEFINED {
            let body = field(letter);
            // The field under test goes BEFORE the fuse list: an `F`
            // after an `L` is rejected for its position, which would
            // fail this test for a reason that has nothing to do with
            // identifier classification.
            let text = format!("\x02h*QF8*F0*{body}*L0 11110000*\x030000");
            let parsed = parse_with_mode(&text, FILE, ParserMode::Strict);
            assert!(
                parsed.is_ok(),
                "`{body}` is conformant JEDEC 3A and strict mode must accept it: {:?}",
                parsed.err().map(|b| b.iter().map(|d| d.headline()).collect::<Vec<_>>())
            );
        }
    }

    #[test]
    fn a_reserved_identifier_is_ignored_rather_than_diagnosed() {
        // JEDEC 3A lines 234-236: "Reserved identifiers currently have
        // no function and are reserved for future use. Receiving
        // equipment should ignore fields starting with reserved
        // identifiers."
        //
        // Real Atmel-toolchain files carry `J` and `U`, so diagnosing
        // these made `validate --strictness strict` reject conformant files.
        for letter in RESERVED {
            let text = format!("\x02h*QF8*F0*L0 11110000*{letter}1*\x030000");
            let parsed = parse_with_mode(&text, FILE, ParserMode::Strict)
                .unwrap_or_else(|_| panic!("`{letter}` is reserved, not invalid"));
            let noise: Vec<String> =
                parsed.diagnostics.iter().map(|d| d.headline().to_string()).collect();
            assert!(noise.is_empty(), "`{letter}` must be ignored silently, got {noise:?}");
        }
    }

    #[test]
    fn a_reserved_field_still_survives_a_rewrite() {
        // "Ignore" means "raise no diagnostic", not "discard". Dropping
        // a vendor's `J` field is the same data loss as dropping test
        // vectors, in the mode whose purpose is losing nothing.
        let text = "\x02h*QF8*F0*L0 11110000*J vendor data*\x030000";
        let file = parse(text, FILE).expect("parses").file;
        assert_eq!(file.unknown_fields.len(), 1, "the reserved field must be retained");
        let written = crate::write(&file, crate::WriterStyle::Canonical).expect("writes");
        assert!(written.contains("J vendor data*"), "{written}");
    }

    #[test]
    fn a_q_subfield_outside_the_three_defined_ones_is_structurally_legal() {
        // Line 230: "Multiple character identifiers can be used to
        // create subfields (that is, "A1", "A$", or "AB3")." Only F, P
        // and V are *defined* Q subfields (lines 308-316), but the
        // standard nowhere forbids others, so deCPLD does not invent a
        // rejection it cannot cite.
        let parsed = parse_with_mode("\x02h*QF8*F0*QX5*\x030000", FILE, ParserMode::Strict)
            .expect("QX is a subfield, not an invented identifier");
        // The subfield letter belongs to the identifier, the way the
        // standard writes `QF1024` as identifier QF and body 1024.
        assert_eq!(parsed.file.unknown_fields[0].identifier, "QX");
        assert_eq!(parsed.file.unknown_fields[0].body, "5");
    }

    #[test]
    fn a_field_whose_identifier_is_not_a_letter_is_reported_and_kept() {
        // Issue #24, found while designing this change. Because the two
        // tables partition A-Z, a non-letter identifier is the ONLY way
        // left to be outside the standard — so this path is now the sole
        // home of E3040, and it used to drop the field on the floor.
        let text = "\x02h*QF8*F0*L0 11110000*123*\x030000";

        let parsed = parse(text, FILE).expect("recoverable in the lenient default");
        assert_eq!(parsed.file.unknown_fields.len(), 1, "the field must not vanish");
        let written = crate::write(&parsed.file, crate::WriterStyle::Canonical).expect("writes");
        assert!(written.contains("123*"), "must survive a rewrite: {written}");

        // And it is genuinely non-conformant, so strict mode says so.
        let strict = parse_with_mode(text, FILE, ParserMode::Strict);
        assert!(strict.is_err(), "a field with no identifier is not conformant JEDEC");
    }
}

/// `L` fields apply all-or-nothing, and report everything wrong with
/// them. Issue #21.
#[cfg(test)]
mod fuse_list_atomicity {
    use super::*;
    use crate::codes;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    #[test]
    fn a_bad_state_leaves_no_earlier_fuse_written() {
        // `apply_fuse_list` used to return on the first bad character
        // with the fuses before it already written into the live vector.
        // It was safe only because every early return also pushed an
        // error and `parse` gates on `has_errors()` — nothing in the
        // types enforced it, and the function returned `()`, so a caller
        // could not be made to notice.
        //
        // Made structural: writes go to a scratch list and are committed
        // only if the whole field is good.
        let bundle = parse("\x02h*QF8*F0*L0 111X0000*\x030000", FILE)
            .expect_err("a bad fuse state is an error");
        assert!(bundle.iter().any(|d| d.code == codes::INVALID_FUSE_STATE));
    }

    #[test]
    fn every_bad_state_in_a_field_is_reported_not_just_the_first() {
        // The module promises "parsing continues past recoverable
        // problems so that one run reports as much as possible", and
        // then abandoned the field at the first bad character — so a
        // file with three faults took three runs to fix.
        let bundle =
            parse("\x02h*QF8*F0*L0 1X1Y0Z00*\x030000", FILE).expect_err("bad states are errors");
        let bad = bundle.iter().filter(|d| d.code == codes::INVALID_FUSE_STATE).count();
        assert_eq!(bad, 3, "expected one diagnostic per bad character");
    }

    #[test]
    fn each_reported_state_names_its_own_character() {
        let bundle = parse("\x02h*QF8*F0*L0 1X1Y0000*\x030000", FILE).expect_err("errors");
        let messages: Vec<String> = bundle
            .iter()
            .filter(|d| d.code == codes::INVALID_FUSE_STATE)
            .map(|d| d.message.clone())
            .collect();
        assert!(messages.iter().any(|m| m.contains('X')), "{messages:?}");
        assert!(messages.iter().any(|m| m.contains('Y')), "{messages:?}");
    }

    #[test]
    fn an_out_of_range_fuse_still_stops_the_field() {
        // Running off the end is different from a bad character: every
        // state after it is also out of range, so reporting each one
        // would be a wall of noise saying one thing.
        let bundle = parse("\x02h*QF8*F0*L6 1111*\x030000", FILE).expect_err("out of range");
        let out = bundle.iter().filter(|d| d.code == codes::FUSE_OUT_OF_RANGE).count();
        assert_eq!(out, 1, "one diagnostic, not one per overflowing state");
    }

    #[test]
    fn a_good_field_after_a_bad_one_is_still_read() {
        // Atomicity is per field, not per file: abandoning everything
        // after the first bad field would report less, not more.
        let bundle = parse("\x02h*QF16*F0*L0 111X*L8 11111111*\x030000", FILE).expect_err("errors");
        assert!(bundle.iter().any(|d| d.code == codes::INVALID_FUSE_STATE));
        // The second field is well formed and must not have produced a
        // diagnostic of its own.
        assert_eq!(bundle.iter().filter(|d| d.code == codes::INVALID_FUSE_STATE).count(), 1);
    }
}

#[cfg(test)]
mod review_findings {
    use super::*;
    use crate::codes;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    fn codes_of(text: &str) -> Vec<u16> {
        match parse(text, FILE) {
            Ok(parsed) => panic!("expected rejection, got {} fuses", parsed.file.fuses.len()),
            Err(bundle) => bundle.iter().map(|d| d.code.as_u16()).collect(),
        }
    }

    #[test]
    fn a_late_default_state_field_is_rejected_for_being_late() {
        // Found by review with this exact input. It parsed with ZERO
        // diagnostics and produced 11111111, where a reader honouring
        // the F0 that actually precedes the L gets 10000000 — a wrong
        // fuse vector, silently, which is the outcome this crate exists
        // to prevent.
        //
        // Two separate faults here, and the file commits both: the F is
        // a repeat *and* it follows an L. Asserting the ordering code is
        // what this test is named for; the second-review tests below
        // cover repetition on its own.
        assert!(
            codes_of("\x02h*QF8*F0*L0 1*F1*\x030000")
                .contains(&codes::DEFAULT_STATE_AFTER_FUSE_LIST.as_u16())
        );
    }

    #[test]
    fn a_lone_default_state_after_a_fuse_list_is_rejected() {
        // No repetition to hide behind: one F field, in the wrong place.
        assert!(
            codes_of("\x02h*QF8*L0 1*F1*\x030000")
                .contains(&codes::DEFAULT_STATE_AFTER_FUSE_LIST.as_u16())
        );
    }

    #[test]
    fn two_security_fields_that_disagree_are_rejected() {
        // Found by the second review round. This parsed with ZERO
        // diagnostics and resolved to `Some(true)` by last-writer-wins:
        // an internally contradictory file silently deciding to
        // permanently lock the part.
        //
        // The `F` fix in the previous round argued that a field whose
        // meaning depends on reading order is "the one outcome this
        // crate exists to prevent". `G` is the same shape and carries
        // more weight — CLAUDE.md gives the security fuse a two-flag
        // confirmation at the CLI precisely because it is irreversible,
        // and inferring it from a self-contradictory file walks around
        // that gate entirely.
        //
        // `write` cannot catch this: it emits one `G1*`, which reparses
        // to exactly what the parser decided.
        assert!(
            codes_of("\x02h*QF8*F0*G0*G1*\x030000")
                .contains(&codes::CONTRADICTORY_SECURITY_FIELD.as_u16())
        );
    }

    #[test]
    fn two_security_fields_that_agree_are_a_warning_not_an_error() {
        // Redundancy and contradiction are different facts. `G1*G1*` is
        // a grammar violation with exactly one possible meaning, so
        // refusing it would reject a file whose intent is not in doubt
        // — and the lenient modes are documented as accepting what real
        // tools emit.
        let parsed = parse(FILE_WITH_REPEATED_SECURITY, FILE).expect("one unambiguous meaning");
        assert_eq!(parsed.file.security, Some(true));
        let codes: Vec<u16> = parsed.diagnostics.iter().map(|d| d.code.as_u16()).collect();
        assert!(codes.contains(&codes::DUPLICATE_SECURITY_FIELD.as_u16()), "{codes:?}");
    }

    const FILE_WITH_REPEATED_SECURITY: &str = "\x02h*QF8*F0*G1*G1*\x030000";

    #[test]
    fn two_default_state_fields_that_agree_are_a_warning_not_an_error() {
        // The same distinction, applied to the field the previous round
        // hardened. `F0*F0*` cannot mean two things.
        let parsed = parse("\x02h*QF8*F0*F0*L0 1*\x030000", FILE).expect("one meaning");
        assert_eq!(parsed.file.default_fuse, Some(false));
        let codes: Vec<u16> = parsed.diagnostics.iter().map(|d| d.code.as_u16()).collect();
        assert!(codes.contains(&codes::DUPLICATE_DEFAULT_STATE.as_u16()), "{codes:?}");
    }

    #[test]
    fn two_default_state_fields_that_disagree_are_rejected() {
        assert!(
            codes_of("\x02h*QF8*F0*F1*L0 1*\x030000")
                .contains(&codes::CONTRADICTORY_DEFAULT_STATE.as_u16())
        );
    }

    #[test]
    fn an_absurd_fuse_count_is_refused_rather_than_allocated() {
        // A 19-byte file allocated 50 MB; QF4294967295 allocated 512 MB,
        // and writing it would have collected a 4 GB Vec<bool>.
        assert!(
            codes_of("\x02h*QF4294967295*F0*\x030000")
                .contains(&codes::FUSE_COUNT_TOO_LARGE.as_u16())
        );
        // A real device is nowhere near the ceiling.
        assert!(parse("\x02h*QF5892*F0*\x030000", FILE).is_ok());
    }
}

#[cfg(test)]
mod second_review_findings {
    use super::*;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    #[test]
    fn a_huge_fuse_number_with_a_bad_state_does_not_overflow() {
        // Found by review, and introduced by the atomicity change: the
        // bad-character branch incremented `fuse` unconditionally, where
        // previously it returned. `parse_number` caps nothing — only QF
        // is bounded — so an L field naming u32::MAX panicked in debug
        // and wrapped silently in release.
        //
        // CLAUDE.md's fuzz rule: malformed input must never panic.
        let text = "\x02h*QF8*F0*L4294967295 X*\x030000";
        let bundle = parse(text, FILE).expect_err("out of range");
        assert!(bundle.iter().any(|d| d.code == codes::FUSE_OUT_OF_RANGE), "{bundle:?}");
    }

    #[test]
    fn the_fuse_number_ceiling_is_the_device_not_the_integer_type() {
        // Neighbouring values, so an off-by-one in the guard shows up.
        for number in [u32::MAX, u32::MAX - 1, 9, 8] {
            let text = format!("\x02h*QF8*F0*L{number} 1*\x030000");
            assert!(parse(&text, FILE).is_err(), "fuse {number} is beyond an 8-fuse device");
        }
        assert!(parse("\x02h*QF8*F0*L7 1*\x030000", FILE).is_ok(), "fuse 7 is the last one");
    }

    #[test]
    fn a_character_the_writer_cannot_encode_is_reported_where_it_is() {
        // Found by review: `parse` accepted a tab in a note and `write`
        // refused it, so the only way to learn was a failed
        // `canonicalize` naming a category ("a note") and no position.
        //
        // The parser knows the offset, so it says so. Reported here as
        // well as refused there, because the two answer different
        // questions: "is this file conformant?" and "can I write this
        // back?".
        let text = "\x02h*QF8*F0*L0 11110000*N ta\tb*\x030000";

        let parsed = parse(text, FILE).expect("a tab is recoverable in the lenient default");
        let found: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter(|d| d.code == codes::INVALID_FIELD_CHARACTER)
            .collect();
        assert_eq!(found.len(), 1, "{:?}", parsed.diagnostics);
        assert!(found[0].message.contains("U+0009"), "{}", found[0].message);
        // And it points at the tab rather than at the field.
        let span = found[0].labels.first().expect("a label").span;
        assert_eq!(&text[span.range.start as usize..span.range.end as usize], "\t");

        // Non-conformant, so strict mode refuses it.
        assert!(parse_with_mode(text, FILE, ParserMode::Strict).is_err());
    }

    #[test]
    fn the_framing_characters_are_not_reported_as_field_characters() {
        // STX and ETX are framing, not field content, and CR/LF are in
        // the class. A file full of all four must stay silent, or every
        // real JEDEC file would warn.
        let text = "\x02h*\r\nQF8*\r\nF0*\r\nL0 11110000*\r\n\x030000";
        let parsed = parse(text, FILE).expect("parses");
        assert!(
            !parsed.diagnostics.iter().any(|d| d.code == codes::INVALID_FIELD_CHARACTER),
            "{:?}",
            parsed.diagnostics
        );
    }

    #[test]
    fn a_rejected_fuse_list_writes_nothing_into_the_vector() {
        // The atomicity claim itself, which the previous tests asserted
        // only indirectly via diagnostics — they passed against the
        // pre-change tree, so #21's headline was untested.
        let field = RawField {
            identifier: "L",
            body: "0 111X0000",
            body_offset: 0,
            span: TextRange::new(0, 10),
        };
        let mut fuses = FuseVector::new(8, false);
        let mut diagnostics = DiagnosticBundle::new();

        let mut covered = FuseCoverage::new(8, true);
        let outcome = apply_fuse_list(&field, &mut fuses, &mut covered, &mut diagnostics, FILE);

        assert!(outcome.is_err(), "a field with a bad state must be rejected");
        // And it counts towards coverage as much as it wrote: nothing.
        // Moving `covered.state(...)` up into the validation loop would
        // still pass every other test on this branch, so the invariant
        // the source comment claims needs stating here.
        assert_eq!(
            covered.first_unstated(),
            Some(0),
            "a rejected field must not count towards coverage"
        );
        for fuse in 0..8 {
            assert_eq!(
                fuses.get(fuse),
                Some(false),
                "fuse {fuse} was written despite the field being rejected"
            );
        }
    }

    #[test]
    fn an_accepted_fuse_list_writes_all_of_it() {
        // The other half: atomicity must not mean "never commits".
        let field = RawField {
            identifier: "L",
            body: "0 1011",
            body_offset: 0,
            span: TextRange::new(0, 6),
        };
        let mut fuses = FuseVector::new(8, false);
        let mut diagnostics = DiagnosticBundle::new();

        let mut covered = FuseCoverage::new(8, true);
        assert!(apply_fuse_list(&field, &mut fuses, &mut covered, &mut diagnostics, FILE).is_ok());
        assert_eq!(covered.first_unstated(), Some(4), "fuses 0..3 were stated, 4.. were not");
        let states: Vec<bool> = fuses.iter().collect();
        assert_eq!(states, [true, false, true, true, false, false, false, false]);
    }
}

/// A file with no `F` field. Issue #20.
///
/// Evidence: `jedec-3a` line 376 — "If no F field is specified, all fuse
/// states must be defined". (The rest of that sentence, "after the QF
/// field and before the first L field", is incoherent: fuse states are
/// defined *by* the L fields. The clause relied on here is unambiguous;
/// the placement clause is recorded as a defect in references.toml.)
#[cfg(test)]
mod missing_default_state {
    use super::*;
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    #[test]
    fn a_file_with_no_f_field_and_incomplete_coverage_is_refused() {
        // Before: this parsed clean and invented twelve fuses.
        //
        //   \x02h*QF16*L0 1010*\x030000
        //     -> 1010000000000000, default_fuse = false, diagnostics = []
        //
        // `write` then emitted `F0*`, converting "every state must be
        // explicit" into "unlisted means 0" — a change of meaning
        // produced by the command whose job is to preserve meaning.
        let bundle = parse("\x02h*QF16*L0 1010*\x030000", FILE)
            .expect_err("twelve fuses have no stated value");
        assert!(
            bundle.iter().any(|d| d.code == codes::INCOMPLETE_FUSE_COVERAGE),
            "{:?}",
            bundle.iter().map(|d| d.headline()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_diagnostic_says_how_many_and_where_the_first_gap_is() {
        // "Some fuses are undefined" is useless on a 5892-fuse device.
        let bundle = parse("\x02h*QF16*L0 1010*\x030000", FILE).expect_err("incomplete");
        let message = bundle
            .iter()
            .find(|d| d.code == codes::INCOMPLETE_FUSE_COVERAGE)
            .expect("the diagnostic")
            .message
            .clone();
        // Asserted as whole phrases. `contains('4')` would have passed on
        // the "1010" in the input echo, on a fuse count of 4, or on any
        // stray digit — a test that cannot fail for the right reason is
        // not a test.
        assert!(message.contains("12 are not"), "must count the gap: {message}");
        assert!(message.contains("first is fuse 4"), "must locate it: {message}");
    }

    #[test]
    fn a_file_with_no_f_field_but_complete_coverage_is_accepted() {
        // Perfectly legal, and the reason this cannot simply reject
        // every file without an F field.
        let parsed = parse("\x02h*QF8*L0 10110001*\x030000", FILE)
            .expect("every fuse is stated, so no default is needed");
        assert_eq!(parsed.file.default_fuse, None, "silence is not F0");
        let states: Vec<bool> = parsed.file.fuses.iter().collect();
        assert_eq!(states, [true, false, true, true, false, false, false, true]);
    }

    #[test]
    fn coverage_may_be_spread_across_several_l_fields() {
        let parsed = parse("\x02h*QF8*L4 0001*L0 1011*\x030000", FILE)
            .expect("order does not matter, only completeness");
        assert_eq!(parsed.file.default_fuse, None);
    }

    #[test]
    fn a_repeated_fuse_does_not_count_as_covering_another() {
        // `L0 1111` twice covers fuses 0..3 and nothing else, however
        // many times it is said.
        let bundle = parse("\x02h*QF8*L0 1111*L0 0000*\x030000", FILE)
            .expect_err("fuses 4..7 are still unstated");
        assert!(bundle.iter().any(|d| d.code == codes::INCOMPLETE_FUSE_COVERAGE));
    }

    #[test]
    fn an_f_field_means_coverage_is_not_required() {
        // The F field exists precisely to make the unlisted fuses
        // meaningful, so it switches the requirement off.
        let parsed = parse("\x02h*QF16*F0*L0 1010*\x030000", FILE).expect("F0 covers the rest");
        assert_eq!(parsed.file.default_fuse, Some(false));
    }

    #[test]
    fn silence_and_f0_are_different_files() {
        // The same argument the crate already makes about `G`: `None` is
        // silence, `Some(false)` is an instruction. If these compared
        // equal, `jed diff` would call a rewrite that invented a default
        // "no change".
        let silent = parse("\x02h*QF8*L0 00000000*\x030000", FILE).expect("complete").file;
        let stated = parse("\x02h*QF8*F0*L0 00000000*\x030000", FILE).expect("parses").file;
        assert!(!silent.describes_same_device_as(&stated));
        assert_eq!(crate::diff(&silent, &stated).default_fuse, Some((None, Some(false))));
    }

    #[test]
    fn a_file_with_no_f_field_round_trips_without_growing_one() {
        // The bug's second half: `write` emitted `F0*` regardless, so a
        // file that said "every state is explicit" came back saying
        // "unlisted means 0".
        for style in [crate::WriterStyle::Canonical, crate::WriterStyle::Compact] {
            let original = parse("\x02h*QF8*L0 10110001*\x030000", FILE).expect("parses").file;
            let written = crate::write(&original, style).expect("writes");
            assert!(!written.contains("F0*"), "invented a default in {style:?}:\n{written}");
            assert!(!written.contains("F1*"), "invented a default in {style:?}:\n{written}");

            let reparsed = parse(&written, FILE).expect("reparses").file;
            assert!(original.describes_same_device_as(&reparsed), "{style:?}:\n{written}");
            assert_eq!(reparsed.default_fuse, None);
        }
    }

    #[test]
    fn compact_style_states_every_fuse_when_there_is_no_default() {
        // Compact writes only fuses differing from the default. With no
        // default there is nothing to differ from, so it must state them
        // all — inventing an F field to compress against would change
        // what the file says.
        let file = parse("\x02h*QF8*L0 10110001*\x030000", FILE).expect("parses").file;
        let compact = crate::write(&file, crate::WriterStyle::Compact).expect("writes");
        let reparsed = parse(&compact, FILE).expect("reparses").file;
        assert!(file.describes_same_device_as(&reparsed), "{compact}");
    }
}
