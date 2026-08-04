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
    MacrocellSpec, OutputPolarity, PackageId, PackagePin, PackageSpec, PhysicalDesign,
    PhysicalSignalSource, PinNumber, PlacedCube, ProductTermId, ProductTermRole,
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

        let names = SignalNames::new(matrix, package);
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
            user_signature: read_signature(fuses),
            security_fuse_set: fuses
                .security_fuse()
                .and_then(|fuse| fuses.get(fuse))
                .unwrap_or(false),
        })
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
    fn new(matrix: &AndMatrixSpec, package: &PackageSpec) -> Self {
        let pins_by_macrocell: BTreeMap<MacrocellId, PinNumber> = package
            .pins()
            .filter_map(|(pin, role)| match role {
                PackagePin::Pad(pad) => Some((pad, pin)),
                _ => None,
            })
            // A pad id and a macrocell id are the same number by
            // construction in every model here; the device layer keeps
            // them as separate types so a swap is a compile error.
            .map(|(pad, pin)| (MacrocellId(pad.0), pin))
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
        let literals = self.literals(&placed.cube)?;
        Ok(EquationRef { row: placed.row.0, text: equation_text(&literals), literals })
    }

    fn literals(&self, cube: &Cube) -> Result<Vec<LiteralRef>, ReportError> {
        cube.canonical()
            .literals
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
fn read_signature(fuses: &FuseMap) -> Option<SignatureReport> {
    let region = fuses
        .regions()
        .iter()
        .find(|region| matches!(region.mutability, FuseMutability::UserSignature))?;

    let bits: Vec<bool> =
        region.range.clone().filter_map(|fuse| fuses.get(decpld_device::FuseId(fuse))).collect();

    let bytes: Vec<u8> = bits
        .chunks_exact(8)
        .map(|chunk| chunk.iter().fold(0u8, |byte, bit| (byte << 1) | u8::from(*bit)))
        .collect();

    let text = bytes
        .iter()
        .all(|byte| (0x20..=0x7E).contains(byte))
        .then(|| bytes.iter().map(|byte| char::from(*byte)).collect::<String>());

    Some(SignatureReport { bytes, text })
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
                Some(enable) => writeln!(f, "  enable    {}", enable.text)?,
                None => writeln!(f, "  enable    never (pad not driven)")?,
            }
            writeln!(f, "  terms     {} of {}", cell.terms_used, cell.terms_available)?;
            for (index, term) in cell.sum.iter().enumerate() {
                let joiner = if index == 0 { "  " } else { "| " };
                writeln!(f, "    {joiner}{}", term.text)?;
            }
        }

        if !self.global_terms.is_empty() {
            writeln!(f, "\ndevice-wide terms")?;
            for term in &self.global_terms {
                writeln!(f, "  {:<20}{}", term.role, term.equation.text)?;
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
        writeln!(f, "security   {}", if self.security_fuse_set { "SET" } else { "clear" })
    }
}
