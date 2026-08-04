//! Rendering a physical design as an inspection report. SPEC.md §6.3.
//!
//! "The compiler explains itself" (CLAUDE.md) is a feature, not debug
//! output: `jed inspect` must make a fuse map readable as macrocells and
//! equations. This crate is where a `PhysicalDesign` — which speaks in
//! macrocell ids, product-term rows, and Boolean input ids — becomes
//! something written in *pins*, which is the only vocabulary a person
//! holding the part shares with it.
//!
//! Nothing here knows about JEDEC syntax or about any particular
//! device. It consumes the device layer's types and produces text and
//! JSON, so a target this crate has never heard of reports the same way.

use decpld_device::{
    AndMatrixSpec, FeedbackSource, FuseMap, FuseMutability, MacrocellId, MacrocellMode,
    MacrocellSpec, OutputPolarity, PackageId, PackageSpec, PhysicalDesign, PhysicalSignalSource,
    PinNumber, PlacedCube, ProductTermId, ProductTermRole,
};
use decpld_logic::{BoolInputId, Cube, Polarity};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

/// Why a design could not be described.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    #[error(
        "the design is for package {design:?} and the report was given {supplied:?}; \
         pin numbers would be confidently wrong"
    )]
    PackageMismatch { design: PackageId, supplied: PackageId },

    #[error("{row} is not a row of this device's matrix")]
    UnknownRow { row: ProductTermId },

    #[error("no literal source carries input {input:?}, so it has no name on this part")]
    UnknownInput { input: BoolInputId },

    #[error("the design describes {macrocell}, which this device does not have")]
    UnknownMacrocell { macrocell: MacrocellId },

    #[error(
        "region `{name}` holds {bits} bits, which is not a whole number of bytes; \
         a signature cannot be reported without dropping the remainder"
    )]
    SignatureNotWholeBytes { name: &'static str, bits: u32 },
}

/// Where a literal's value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignalKind {
    /// A package pin driving the array directly.
    Input,
    /// A macrocell's output fed back in. It still arrives on a pin —
    /// the one that macrocell drives — which is why both kinds are
    /// named the same way and the difference is recorded rather than
    /// spelled into the name.
    Feedback,
}

/// A signal as a person reading a schematic would name it.
///
/// The pin is a plain number rather than a [`PinNumber`]: a report is a
/// presentation type, its JSON form is untyped, and carrying the newtype
/// here would mean spreading `serde` across the device layer to buy
/// nothing this crate can use it for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignalRef {
    pub name: String,
    pub pin: Option<u8>,
    pub kind: SignalKind,
}

/// One literal of a product term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LiteralRef {
    pub signal: SignalRef,
    pub complemented: bool,
}

/// One product term, rendered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EquationRef {
    pub row: u32,
    pub literals: Vec<LiteralRef>,
    /// The term in deCPLD's own operators, or `always` for a term with
    /// no literals — the empty AND, which is constantly **true**. A
    /// blank line there would show an always-enabled output as having
    /// no enable at all, which is the opposite of what it means.
    pub text: String,
    /// Whether the term holds some input at both polarities, and so can
    /// never fire.
    ///
    /// `Cube::canonical` preserves a contradiction deliberately, and
    /// such a term is as unsatisfiable as an absent one — but rendered
    /// as bare text it reads exactly like a term that works. Flagged
    /// rather than hidden: the two constantly-false spellings mean
    /// different things (`a & !a` is a mistake, an absent enable is an
    /// intent) and both need saying.
    pub never_true: bool,
}

/// A device-wide term: reset or preset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GlobalTermReport {
    pub role: String,
    pub equation: EquationRef,
}

/// One macrocell, as a user reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MacrocellReport {
    pub macrocell: u8,
    pub pin: Option<u8>,
    pub mode: &'static str,
    pub polarity: &'static str,
    pub feedback: &'static str,
    pub pad_enabled: bool,
    /// Absent when no term drives the pad — which is how this device
    /// family says "this pin is an input".
    pub output_enable: Option<EquationRef>,
    pub sum: Vec<EquationRef>,
    pub terms_used: usize,
    pub terms_available: usize,
}

/// The user signature region, as bytes and — where it is printable — as
/// text. WinCUPL writes the design's `PartNo` here, so a file somebody
/// else produced often says what it was called.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SignatureReport {
    pub bytes: Vec<u8>,
    /// `None` when the bytes are not printable ASCII. An erased
    /// signature is every bit blown, which is `0xFF` repeated and reads
    /// as nothing.
    pub text: Option<String>,
}

/// What the *file* said, as distinct from what the device holds.
///
/// SPEC.md §6.3 asks for checksums, and they are facts about a
/// transmission rather than about silicon — a `C` field can disagree
/// with the fuse data it claims to describe, and saying so is the point.
/// Kept as plain numbers so this crate still knows no JEDEC syntax.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceReport {
    pub design_specification: String,
    /// The `C` field as written, whether or not it was true.
    pub declared_fuse_checksum: Option<u16>,
    /// What the fuse data actually produces.
    pub computed_fuse_checksum: u16,
    pub transmission_checksum: Option<u16>,
    pub notes: Vec<String>,
    /// Identifiers of fields the reader retained but does not model.
    pub unmodelled_fields: Vec<String>,
}

impl SourceReport {
    /// Whether the file actually claims a fuse checksum.
    ///
    /// `C0000` means "not computed" — JEDEC 3A grants the transmission
    /// checksum that courtesy explicitly, and both GALasm and WinCUPL
    /// emit it for the fuse checksum, which is why this project's
    /// parser accepts it (`decpld-jedec`'s checksum check skips zero).
    /// A file carrying it has made no claim to be wrong about.
    #[must_use]
    pub fn declares_a_checksum(&self) -> bool {
        self.declared_fuse_checksum.is_some_and(|declared| declared != 0)
    }

    /// Whether the file's own checksum claim is true.
    ///
    /// Silence and `C0000` both agree vacuously. Treating the latter as
    /// a disagreement would accuse a valid file of corruption — and
    /// since the parser turns every *other* mismatch into an error,
    /// `C0000` is the only value that can reach a reader at all, so the
    /// rule is not a corner case but the whole behaviour.
    #[must_use]
    pub fn fuse_checksum_agrees(&self) -> bool {
        !self.declares_a_checksum()
            || self.declared_fuse_checksum == Some(self.computed_fuse_checksum)
    }
}

/// Everything `jed inspect` reports about one part. SPEC.md §6.3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InspectReport {
    pub device: String,
    pub package: String,
    pub fuse_count: u32,
    pub macrocells: Vec<MacrocellReport>,
    pub global_terms: Vec<GlobalTermReport>,
    pub user_signature: Option<SignatureReport>,
    pub security_fuse_set: bool,
    /// Present when the design came from a file rather than from a
    /// compiler's own encoder.
    pub source: Option<SourceReport>,
}

impl InspectReport {
    /// Describe a design.
    ///
    /// # Errors
    ///
    /// If the design is for a different package than the one supplied —
    /// pin numbers taken from the wrong package are confidently wrong,
    /// which is worse than none — or if it names a macrocell, a
    /// product-term row, or a Boolean input the device does not have.
    pub fn new(
        design: &PhysicalDesign,
        package: &PackageSpec,
        matrix: &AndMatrixSpec,
        specs: &[MacrocellSpec],
        fuses: &FuseMap,
    ) -> Result<Self, ReportError> {
        if design.package != package.id() {
            return Err(ReportError::PackageMismatch {
                design: design.package,
                supplied: package.id(),
            });
        }

        let names = SignalNames::new(matrix, package, specs);
        let specs_by_id: BTreeMap<MacrocellId, &MacrocellSpec> =
            specs.iter().map(|spec| (spec.id, spec)).collect();

        let mut macrocells = Vec::with_capacity(design.macrocells.len());
        for cell in &design.macrocells {
            let spec = specs_by_id
                .get(&cell.id)
                .copied()
                .ok_or(ReportError::UnknownMacrocell { macrocell: cell.id })?;
            let pin = spec.pad.and_then(|pad| package.pin_of_pad(pad)).map(|pin| pin.0);

            let sum = cell
                .data_terms
                .iter()
                .map(|placed| names.equation(matrix, placed))
                .collect::<Result<Vec<_>, _>>()?;
            let output_enable =
                cell.oe_term.as_ref().map(|placed| names.equation(matrix, placed)).transpose()?;

            macrocells.push(MacrocellReport {
                macrocell: cell.id.0,
                pin,
                mode: mode_name(cell.mode),
                polarity: polarity_name(cell.polarity),
                feedback: feedback_name(cell.feedback),
                pad_enabled: cell.pad_enabled(),
                output_enable,
                sum,
                terms_used: cell.data_terms.len(),
                terms_available: spec.data_term_capacity(),
            });
        }

        let mut global_terms = Vec::with_capacity(design.global_terms.len());
        for placed in &design.global_terms {
            let role =
                matrix.row(placed.row).ok_or(ReportError::UnknownRow { row: placed.row })?.role;
            global_terms.push(GlobalTermReport {
                role: role_name(role).to_owned(),
                equation: names.equation(matrix, placed)?,
            });
        }

        Ok(Self {
            device: design.device.to_owned(),
            package: package.name().to_owned(),
            fuse_count: fuses.len(),
            macrocells,
            global_terms,
            user_signature: read_signature(fuses)?,
            security_fuse_set: fuses
                .security_fuse()
                .and_then(|fuse| fuses.get(fuse))
                .unwrap_or(false),
            source: None,
        })
    }

    /// Record what the file this design was read from said.
    #[must_use]
    pub fn with_source(mut self, source: SourceReport) -> Self {
        self.source = Some(source);
        self
    }

    /// Add a readback lock the device model cannot see.
    ///
    /// Not every device holds the security bit inside its fuse count —
    /// the ATF22V10C's is the JEDEC `G` field, and there is no fuse
    /// index to cite for it (§4.7). Where the state arrives from
    /// outside the fuse vector it is recorded here, so a locked part is
    /// reported as locked rather than as clear because the region a
    /// generic reader looked for does not exist.
    ///
    /// **Additive, not an assignment.** A device that *does* keep the
    /// bit inside `QF`, read from a file that says nothing about it,
    /// would otherwise have a lock the fuses reported overwritten by
    /// the file's silence — and a part reported readable when it is
    /// locked is the one error that costs the user the chip. Evidence
    /// of a lock from either source is evidence of a lock.
    #[must_use]
    pub fn with_security_fuse(mut self, set: bool) -> Self {
        self.security_fuse_set |= set;
        self
    }

    /// The same report as JSON.
    ///
    /// # Errors
    ///
    /// If serialisation fails, which for this shape of data means the
    /// serialiser itself is broken rather than the data.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// The pin each Boolean input arrives on.
struct SignalNames {
    by_input: BTreeMap<BoolInputId, SignalRef>,
}

impl SignalNames {
    fn new(matrix: &AndMatrixSpec, package: &PackageSpec, specs: &[MacrocellSpec]) -> Self {
        // Through `MacrocellSpec::pad`, never by casting one id to the
        // other. `PadId` and `MacrocellId` are separate types because
        // they are separate facts, and a crate that claims to describe
        // a device it has never heard of cannot assume they agree —
        // the two would then disagree *within one report*, which says
        // a macrocell is on one pin and names its own feedback for
        // another.
        let pins_by_macrocell: BTreeMap<MacrocellId, PinNumber> = specs
            .iter()
            .filter_map(|spec| Some((spec.id, package.pin_of_pad(spec.pad?)?)))
            .collect();

        let by_input = matrix
            .sources()
            .iter()
            .map(|source| {
                let (pin, kind) = match source.physical_source {
                    PhysicalSignalSource::Pin(pin) => (Some(pin), SignalKind::Input),
                    PhysicalSignalSource::Feedback(macrocell) => {
                        (pins_by_macrocell.get(&macrocell).copied(), SignalKind::Feedback)
                    }
                };
                let pin = pin.map(|pin| pin.0);
                let name = match (pin, source.physical_source) {
                    (Some(pin), _) => format!("pin{pin}"),
                    // A feedback path from a macrocell with no bonded
                    // pad. Named for the macrocell, because there is no
                    // pin to name it for and inventing one would be a
                    // lie about where to put a probe.
                    (None, PhysicalSignalSource::Feedback(macrocell)) => {
                        format!("mc{}", macrocell.0)
                    }
                    (None, PhysicalSignalSource::Pin(number)) => format!("pin{}", number.0),
                };
                (source.id, SignalRef { name, pin, kind })
            })
            .collect();

        Self { by_input }
    }

    fn equation(
        &self,
        matrix: &AndMatrixSpec,
        placed: &PlacedCube,
    ) -> Result<EquationRef, ReportError> {
        if matrix.row(placed.row).is_none() {
            return Err(ReportError::UnknownRow { row: placed.row });
        }
        let cube = placed.cube.canonical();
        let literals = self.literals(&cube)?;
        Ok(EquationRef {
            row: placed.row.0,
            text: equation_text(&literals),
            never_true: cube.contradiction().is_some(),
            literals,
        })
    }

    fn literals(&self, cube: &Cube) -> Result<Vec<LiteralRef>, ReportError> {
        cube.literals
            .iter()
            .map(|literal| {
                let signal = self
                    .by_input
                    .get(&literal.input)
                    .cloned()
                    .ok_or(ReportError::UnknownInput { input: literal.input })?;
                Ok(LiteralRef { signal, complemented: literal.polarity == Polarity::Complement })
            })
            .collect()
    }
}

/// A term with no literals is the empty AND — constantly **true**.
const ALWAYS: &str = "always";

fn equation_text(literals: &[LiteralRef]) -> String {
    if literals.is_empty() {
        return ALWAYS.to_owned();
    }
    literals
        .iter()
        .map(|literal| {
            let bang = if literal.complemented { "!" } else { "" };
            format!("{bang}{}", literal.signal.name)
        })
        .collect::<Vec<_>>()
        .join(" & ")
}

fn mode_name(mode: MacrocellMode) -> &'static str {
    match mode {
        MacrocellMode::Combinational => "combinational",
        MacrocellMode::Registered => "registered",
        MacrocellMode::InputOnly => "input only",
    }
}

fn polarity_name(polarity: OutputPolarity) -> &'static str {
    match polarity {
        OutputPolarity::ActiveHigh => "active high",
        OutputPolarity::ActiveLow => "active low",
    }
}

fn feedback_name(feedback: FeedbackSource) -> &'static str {
    match feedback {
        FeedbackSource::Pin => "pin",
        FeedbackSource::Combinational => "combinational",
        FeedbackSource::Registered => "registered",
        FeedbackSource::None => "none",
    }
}

fn role_name(role: ProductTermRole) -> &'static str {
    match role {
        ProductTermRole::Data { .. } => "data",
        ProductTermRole::OutputEnable { .. } => "output enable",
        ProductTermRole::AsynchronousReset => "asynchronous reset",
        ProductTermRole::SynchronousPreset => "synchronous preset",
    }
}

/// Pack the user-signature region into bytes, most significant bit
/// first, and read them as ASCII where they are printable.
///
/// Bit order is measured for the ATF22V10C (experiments `sig-partno-41`
/// and `sig-partno-5A`) and assumed here for every device. That is a
/// device fact in a device-agnostic crate and is recorded as such: the
/// trigger to move it into [`decpld_device`] is the first target whose
/// signature is not MSB-first.
fn read_signature(fuses: &FuseMap) -> Result<Option<SignatureReport>, ReportError> {
    let Some(region) = fuses
        .regions()
        .iter()
        .find(|region| matches!(region.mutability, FuseMutability::UserSignature))
    else {
        return Ok(None);
    };

    let bits: Vec<bool> =
        region.range.clone().filter_map(|fuse| fuses.get(decpld_device::FuseId(fuse))).collect();

    // Refused rather than truncated. `chunks_exact` would drop a
    // remainder silently — a twenty-bit region reporting two bytes and
    // losing four — and a region shorter than a byte would produce no
    // bytes at all, whereupon "every byte is printable" holds vacuously
    // and the report prints an empty string for a region holding data.
    let bits_len = u32::try_from(bits.len()).unwrap_or(u32::MAX);
    if !bits.len().is_multiple_of(8) {
        return Err(ReportError::SignatureNotWholeBytes { name: region.name, bits: bits_len });
    }

    let bytes: Vec<u8> = bits
        .chunks_exact(8)
        .map(|chunk| chunk.iter().fold(0u8, |byte, bit| (byte << 1) | u8::from(*bit)))
        .collect();

    let printable = !bytes.is_empty() && bytes.iter().all(|byte| (0x20..=0x7E).contains(byte));
    let text = printable.then(|| bytes.iter().map(|byte| char::from(*byte)).collect::<String>());

    Ok(Some(SignatureReport { bytes, text }))
}

impl fmt::Display for InspectReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}  {}  {} fuses", self.device, self.package, self.fuse_count)?;

        for cell in &self.macrocells {
            writeln!(f)?;
            match cell.pin {
                Some(pin) => writeln!(f, "macrocell {} -> pin {pin}", cell.macrocell)?,
                None => writeln!(f, "macrocell {} (no bonded pin)", cell.macrocell)?,
            }
            writeln!(f, "  mode      {}", cell.mode)?;
            writeln!(f, "  polarity  {}", cell.polarity)?;
            writeln!(f, "  feedback  {}", cell.feedback)?;
            match &cell.output_enable {
                Some(enable) => {
                    writeln!(f, "  enable    {}{}", enable.text, never_true_marker(enable))?;
                }
                None => writeln!(f, "  enable    never (pad not driven)")?,
            }
            writeln!(f, "  terms     {} of {}", cell.terms_used, cell.terms_available)?;
            for (index, term) in cell.sum.iter().enumerate() {
                let joiner = if index == 0 { "  " } else { "| " };
                writeln!(f, "    {joiner}{}{}", term.text, never_true_marker(term))?;
            }
        }

        if !self.global_terms.is_empty() {
            writeln!(f, "\ndevice-wide terms")?;
            for term in &self.global_terms {
                writeln!(
                    f,
                    "  {:<20}{}{}",
                    term.role,
                    term.equation.text,
                    never_true_marker(&term.equation)
                )?;
            }
        }

        writeln!(f)?;
        match &self.user_signature {
            Some(signature) => {
                let hex: Vec<String> =
                    signature.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                match &signature.text {
                    Some(text) => writeln!(f, "signature  {} (\"{text}\")", hex.join(" "))?,
                    None => writeln!(f, "signature  {} (not printable)", hex.join(" "))?,
                }
            }
            None => writeln!(f, "signature  this device has none")?,
        }
        writeln!(f, "security   {}", if self.security_fuse_set { "SET" } else { "clear" })?;

        if let Some(source) = &self.source {
            writeln!(f, "\nfile")?;
            if !source.design_specification.trim().is_empty() {
                field(f, "header", &source.design_specification)?;
            }
            if source.fuse_checksum_agrees() {
                match source.declared_fuse_checksum {
                    // `C0000` is JEDEC's "not computed", not a claim
                    // that happens to be false — the parser accepts it
                    // for that reason, and calling it a disagreement
                    // would accuse a valid file of corruption.
                    Some(0) | None => writeln!(
                        f,
                        "  checksum  none declared, {:04X} computed",
                        source.computed_fuse_checksum
                    )?,
                    Some(declared) => writeln!(f, "  checksum  {declared:04X}")?,
                }
            } else {
                writeln!(
                    f,
                    "  checksum  {:04X} declared, {:04X} computed — DISAGREE",
                    source.declared_fuse_checksum.unwrap_or_default(),
                    source.computed_fuse_checksum
                )?;
            }
            if let Some(transmission) = source.transmission_checksum {
                writeln!(f, "  transmit  {transmission:04X}")?;
            }
            for note in &source.notes {
                field(f, "note", note)?;
            }
            if !source.unmodelled_fields.is_empty() {
                writeln!(f, "  retained  {}", source.unmodelled_fields.join(" "))?;
            }
        }
        Ok(())
    }
}

/// What follows a term that can never fire.
fn never_true_marker(equation: &EquationRef) -> &'static str {
    if equation.never_true { "   (never true)" } else { "" }
}

/// A labelled field whose value may run to several lines.
///
/// WinCUPL's header is always five or six — compiler banner, device,
/// library, date, name, part number — so printing it raw drops
/// unindented lines into the middle of an indented block. That is the
/// most common real input this report will ever see, not a corner case.
fn field(f: &mut fmt::Formatter<'_>, label: &str, value: &str) -> fmt::Result {
    const WIDTH: usize = 10;
    for (index, line) in value.trim_end().lines().enumerate() {
        if index == 0 {
            writeln!(f, "  {label:<WIDTH$}{}", line.trim())?;
        } else {
            writeln!(f, "  {:WIDTH$}{}", "", line.trim())?;
        }
    }
    Ok(())
}
