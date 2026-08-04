//! Configuration fields: a device option stored as fuses. SPEC.md §4.3.
//!
//! Only this layer knows whether a logical option is encoded by fuse
//! zero or one. Above it a macrocell is registered or combinational;
//! here that is a bit pattern, and which pattern is a measured fact.

use crate::{
    ClockResourceId, FuseId, FuseMap, FuseWriteError, MacrocellId, PackageId, PadId, ProductTermId,
};
use decpld_logic::Cube;
use std::collections::{BTreeMap, BTreeSet};

/// Identifies a configuration field within a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigFieldId(pub u16);

/// Whether an output drives its pin true or inverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputPolarity {
    ActiveHigh,
    ActiveLow,
}

/// What a macrocell does with its sum of products.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MacrocellMode {
    /// The sum drives the pin directly.
    Combinational,
    /// The sum is clocked into a flip-flop that drives the pin.
    Registered,
    /// The cell drives nothing; its pin is an input.
    ///
    /// Not every device encodes this in a configuration field. Where the
    /// output is instead held off by an always-false output-enable term,
    /// a `ConfigField<MacrocellMode>` will not contain this variant and
    /// [`ConfigField::encode`] refuses it — which is the correct
    /// answer, because the caller must reach for the output-enable term
    /// instead of quietly getting a driven pin.
    InputOnly,
}

/// Where a macrocell's feedback into the array comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeedbackSource {
    /// The pin itself, so an externally driven value is readable.
    Pin,
    /// The combinational sum, before any register.
    Combinational,
    /// The register's output.
    Registered,
    /// No feedback path.
    None,
}

/// A device option and the fuses that hold it.
///
/// Validated on construction, and the validation is what makes decoding
/// safe: the bit patterns are distinct, they all match the field's
/// width, and **every** pattern the fuses can hold is assigned a value.
/// A device model that left a pattern unassigned would be unable to
/// describe a chip somebody else programmed, which is the case
/// `jed inspect` exists for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigField<T> {
    id: ConfigFieldId,
    name: &'static str,
    bits: Vec<FuseId>,
    patterns_by_value: BTreeMap<T, Vec<bool>>,
}

/// Why a configuration field was refused, or could not be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigFieldError {
    #[error("configuration field {name} has no fuses")]
    NoBits { name: &'static str },
    #[error("configuration field {name}: {fuse} appears twice")]
    FuseUsedTwice { name: &'static str, fuse: FuseId },
    #[error("configuration field {name}: a {bits}-fuse field cannot hold a {pattern}-bit pattern")]
    PatternWidthMismatch { name: &'static str, bits: usize, pattern: usize },
    #[error("configuration field {name}: two values share the pattern {pattern:?}")]
    PatternUsedTwice { name: &'static str, pattern: Vec<bool> },
    #[error("configuration field {name}: one value is given two patterns")]
    ValueUsedTwice { name: &'static str },
    #[error("configuration field {name} spans {bits} fuses; a field may span at most {max}")]
    FieldTooWide { name: &'static str, bits: usize, max: usize },
    #[error("configuration field {name}: no value is assigned to the pattern {pattern:?}")]
    PatternUnassigned { name: &'static str, pattern: Vec<bool> },
    #[error("configuration field {name} ({id:?}) cannot encode that value on this device")]
    ValueNotEncodable { name: &'static str, id: ConfigFieldId },
    #[error("configuration field {name} ({id:?}): {fuse} is outside the fuse map")]
    FuseOutOfRange { name: &'static str, id: ConfigFieldId, fuse: FuseId },
    #[error("configuration field {name} ({id:?}): {source}")]
    Fuse { name: &'static str, id: ConfigFieldId, source: FuseWriteError },
}

impl<T: Copy + Ord> ConfigField<T> {
    /// Build a field, checking that it can encode and decode totally.
    pub fn new(
        id: ConfigFieldId,
        name: &'static str,
        bits: Vec<FuseId>,
        encoding: impl IntoIterator<Item = (T, Vec<bool>)>,
    ) -> Result<Self, ConfigFieldError> {
        if bits.is_empty() {
            return Err(ConfigFieldError::NoBits { name });
        }
        let mut seen = BTreeSet::new();
        for &fuse in &bits {
            if !seen.insert(fuse) {
                return Err(ConfigFieldError::FuseUsedTwice { name, fuse });
            }
        }

        // The width ceiling is load-bearing for CORRECTNESS, not for
        // termination. With overflow checks off, `1usize << usize::BITS`
        // masks to `1usize << 0 == 1`, so the totality loop below would
        // examine only the all-false pattern and silently accept a field
        // that is not total at all.
        const MAX_BITS: usize = usize::BITS as usize - 1;
        if bits.len() > MAX_BITS {
            return Err(ConfigFieldError::FieldTooWide { name, bits: bits.len(), max: MAX_BITS });
        }

        let mut patterns_by_value = BTreeMap::new();
        let mut assigned_patterns: BTreeSet<Vec<bool>> = BTreeSet::new();
        for (value, pattern) in encoding {
            if pattern.len() != bits.len() {
                return Err(ConfigFieldError::PatternWidthMismatch {
                    name,
                    bits: bits.len(),
                    pattern: pattern.len(),
                });
            }
            if !assigned_patterns.insert(pattern.clone()) {
                return Err(ConfigFieldError::PatternUsedTwice { name, pattern });
            }
            // One value, one pattern. Accepting a repeated value was
            // last-wins, which broke totality from the other side: the
            // field validated, and then `decode` -- which reads
            // `patterns_by_value` -- could not name a state the chip can
            // hold. It also made the declaration order of the encoding
            // list observable in the fuses (SPEC.md §0.2.5).
            if patterns_by_value.insert(value, pattern).is_some() {
                return Err(ConfigFieldError::ValueUsedTwice { name });
            }
        }

        // Totality, checked against the map `decode` actually reads
        // rather than a parallel set that could disagree with it.
        // Enumerated rather than counted, so the diagnostic names the
        // pattern nobody assigned instead of reporting an arithmetic
        // shortfall the reader has to work out.
        let assigned: BTreeSet<&Vec<bool>> = patterns_by_value.values().collect();
        for code in 0..(1usize << bits.len()) {
            let pattern: Vec<bool> = (0..bits.len()).map(|bit| code >> bit & 1 == 1).collect();
            if !assigned.contains(&pattern) {
                return Err(ConfigFieldError::PatternUnassigned { name, pattern });
            }
        }

        Ok(Self { id, name, bits, patterns_by_value })
    }

    #[must_use]
    pub fn id(&self) -> ConfigFieldId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The fuses this field occupies, in bit order.
    #[must_use]
    pub fn bits(&self) -> &[FuseId] {
        &self.bits
    }

    /// Every value this field can hold, ascending.
    pub fn values(&self) -> impl Iterator<Item = T> + '_ {
        self.patterns_by_value.keys().copied()
    }

    /// Write a value's pattern into the map.
    ///
    /// Atomic: a value the field cannot encode is refused before any
    /// fuse moves, so a macrocell cannot end up half-configured.
    pub fn encode(&self, map: &mut FuseMap, value: T) -> Result<(), ConfigFieldError> {
        let pattern = self
            .patterns_by_value
            .get(&value)
            .ok_or(ConfigFieldError::ValueNotEncodable { name: self.name, id: self.id })?;
        let writes: Vec<(FuseId, bool)> =
            self.bits.iter().copied().zip(pattern.iter().copied()).collect();
        map.set_all(writes).map_err(|source| ConfigFieldError::Fuse {
            name: self.name,
            id: self.id,
            source,
        })
    }

    /// Read the value the map's fuses describe.
    ///
    /// Total for any map that contains the field's fuses, because
    /// construction assigned every pattern a value.
    pub fn decode(&self, map: &FuseMap) -> Result<T, ConfigFieldError> {
        let mut pattern = Vec::with_capacity(self.bits.len());
        for &fuse in &self.bits {
            let state = map.get(fuse).ok_or(ConfigFieldError::FuseOutOfRange {
                name: self.name,
                id: self.id,
                fuse,
            })?;
            pattern.push(state);
        }
        self.patterns_by_value
            .iter()
            .find(|(_, candidate)| **candidate == pattern)
            .map(|(value, _)| *value)
            .ok_or(ConfigFieldError::PatternUnassigned { name: self.name, pattern })
    }
}

/// One output cell: its product terms, its options, and its pad.
///
/// SPEC.md §4.5. The fitter reads this to decide what a macrocell can
/// do; the encoder reads it to turn a decision into fuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacrocellSpec {
    pub id: MacrocellId,
    /// The I/O cell this macrocell drives, if it has one. `None` is a
    /// buried macrocell.
    pub pad: Option<PadId>,
    /// The rows summed to drive the output, in array order.
    pub data_terms: Vec<ProductTermId>,
    /// The row controlling the output driver, if the device has one per
    /// macrocell.
    pub oe_term: Option<ProductTermId>,
    pub supports_registered: bool,
    pub supports_combinational: bool,
    /// Whether the pin can be used as an input while this cell drives
    /// nothing.
    pub supports_input_only: bool,
    pub feedback_modes: Vec<FeedbackSource>,
    pub mode_field: Option<ConfigField<MacrocellMode>>,
    pub polarity_field: Option<ConfigField<OutputPolarity>>,
    /// The clock this cell's register uses, where the device does not
    /// let a design choose.
    pub fixed_clock: Option<ClockResourceId>,
}

impl MacrocellSpec {
    /// How many product terms this cell can sum.
    #[must_use]
    pub fn data_term_capacity(&self) -> usize {
        self.data_terms.len()
    }

    /// Whether this cell can be put in `mode`.
    ///
    /// Asks the capability flags, not the configuration field: a device
    /// may reach a mode without encoding it in fuses. On the ATF22V10 an
    /// input-only cell is a combinational one whose output-enable term
    /// is never true, so `supports_input_only` is set while
    /// `mode_field` has no `InputOnly` pattern to write.
    #[must_use]
    pub fn supports(&self, mode: MacrocellMode) -> bool {
        match mode {
            MacrocellMode::Registered => self.supports_registered,
            MacrocellMode::Combinational => self.supports_combinational,
            MacrocellMode::InputOnly => self.supports_input_only,
        }
    }
}

/// Identifies a logical output of the design a fitter placed.
///
/// Absent when a design came from fuses rather than from source: a
/// decoded part knows which macrocell drives which pin, and cannot know
/// what the designer called it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalOutputId(pub u32);

/// A cube placed in a specific product-term row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedCube {
    pub row: ProductTermId,
    pub cube: Cube,
}

/// One macrocell's configuration in a physical design. SPEC.md §4.5.
///
/// There is deliberately no `pad_enabled` field. Whether the pin is
/// driven is decided entirely by the output-enable term, so a separate
/// flag would be a second copy of one fact: a design could then carry
/// `pad_enabled: false` alongside an always-true enable, encode to
/// fuses that drive the pin, and fail §12.1's round-trip on a
/// difference no encoder ever wrote. See [`Self::pad_enabled`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacrocellConfig {
    pub id: MacrocellId,
    pub assigned_signal: Option<LogicalOutputId>,
    pub mode: MacrocellMode,
    pub polarity: OutputPolarity,
    pub feedback: FeedbackSource,
    pub data_terms: Vec<PlacedCube>,
    pub oe_term: Option<PlacedCube>,
}

impl MacrocellConfig {
    /// Whether this macrocell drives its pin.
    ///
    /// Derived, not stored: a pad is driven exactly when an
    /// output-enable term can be satisfied. An absent term is the
    /// disabled pad — a term that is present but can never be true is
    /// spelled the same way after a round-trip, because that is what
    /// the silicon holds.
    #[must_use]
    pub fn pad_enabled(&self) -> bool {
        self.oe_term.is_some()
    }
}

/// A design as the silicon holds it: every macrocell configured, every
/// product term placed.
///
/// The thing `encode` turns into fuses and `decode` recovers from them.
/// Comparing one of these with the design a compiler intended is
/// SPEC.md §12.1's round-trip, and it is why the type carries no
/// derived or presentational state — two designs are equal when they
/// describe the same chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalDesign {
    pub device: &'static str,
    pub package: PackageId,
    pub macrocells: Vec<MacrocellConfig>,
    /// Terms belonging to no macrocell — a device-wide reset or preset.
    pub global_terms: Vec<PlacedCube>,
}
