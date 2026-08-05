//! How well a device fact is established. SPEC.md §13.1.
//!
//! The ATF22V10 and ATF16V8 fuse maps are this project's factual
//! bedrock, and a wrong bit is invisible until hardware misbehaves. A
//! constant on its own cannot say whether it was measured, read off a
//! datasheet, copied from another implementation, or guessed — so the
//! type says it, and a threshold makes "no hypotheses in a production
//! target" a check rather than a habit.

use std::fmt;

/// How many independent witnesses stand behind a fact.
///
/// **The ordering counts witnesses; it does not rank authority.**
/// [`Self::DatasheetSpecified`] sits below
/// [`Self::DifferentiallyVerified`] because one document is one
/// witness, not because a vendor is less trustworthy than an oracle
/// run. For a fact no fuse experiment can observe — which pin is bonded
/// to ground, what a supply rail is — the datasheet is the *better*
/// witness and the only one available. A field at that level is a field
/// nothing has corroborated yet, which is what the level is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceLevel {
    /// Nothing established this. Belongs in oracle-analysis code or a
    /// disabled experimental target, never in a production target.
    Hypothesis,
    /// One document says so, and nothing else has corroborated it.
    DatasheetSpecified,
    /// A controlled experiment changed one thing and observed the
    /// result.
    DifferentiallyVerified,
    /// An independent implementation agrees.
    OpenSourceCrossChecked,
    /// A part was programmed and its behaviour measured. The only level
    /// that settles a convention both the encoder and the decoder
    /// share.
    HardwareVerified,
}

impl EvidenceLevel {
    /// Every level, weakest first.
    pub const ALL: [EvidenceLevel; 5] = [
        EvidenceLevel::Hypothesis,
        EvidenceLevel::DatasheetSpecified,
        EvidenceLevel::DifferentiallyVerified,
        EvidenceLevel::OpenSourceCrossChecked,
        EvidenceLevel::HardwareVerified,
    ];

    /// The weakest level a production target field may hold.
    ///
    /// One cited witness. Deliberately not higher: package pinouts and
    /// supply rails are real, citable knowledge that no fuse experiment
    /// can reach, and a threshold above this would exclude facts that
    /// have all the evidence they can ever have.
    pub const PRODUCTION_THRESHOLD: EvidenceLevel = EvidenceLevel::DatasheetSpecified;

    /// Whether a field at this level may appear in a production target.
    #[must_use]
    pub fn is_production_ready(self) -> bool {
        self >= Self::PRODUCTION_THRESHOLD
    }
}

impl fmt::Display for EvidenceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            EvidenceLevel::Hypothesis => "hypothesis",
            EvidenceLevel::DatasheetSpecified => "datasheet-specified",
            EvidenceLevel::DifferentiallyVerified => "differentially verified",
            EvidenceLevel::OpenSourceCrossChecked => "open-source cross-checked",
            EvidenceLevel::HardwareVerified => "hardware verified",
        };
        f.write_str(name)
    }
}

/// A level and what established it.
///
/// The sources are the point. A level with nothing behind it is a claim
/// about a claim — naming the experiments is what lets a reader
/// re-derive the fact, and what lets every constant that trusted an
/// oracle run be found if the run is later shown wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub level: EvidenceLevel,
    /// Experiment names, document sections, or implementation
    /// references — whatever a reader would go and look at.
    pub sources: &'static [&'static str],
}

impl Evidence {
    /// Record evidence.
    #[must_use]
    pub fn new(level: EvidenceLevel, sources: &'static [&'static str]) -> Self {
        Self { level, sources }
    }

    /// Record evidence, refusing a claim with nothing behind it.
    ///
    /// # Errors
    ///
    /// If `level` is above [`EvidenceLevel::Hypothesis`] and no source
    /// is named. `Hypothesis` may stand alone — "nothing established
    /// this yet" is exactly what it means — but every other level
    /// asserts that something did, and has to say what.
    pub fn checked(
        level: EvidenceLevel,
        sources: &'static [&'static str],
    ) -> Result<Self, EvidenceLevel> {
        if level > EvidenceLevel::Hypothesis && sources.is_empty() {
            return Err(level);
        }
        Ok(Self { level, sources })
    }

    #[must_use]
    pub fn is_production_ready(&self) -> bool {
        self.level.is_production_ready()
    }

    /// What a conclusion drawn from several facts actually rests on.
    ///
    /// The **weakest** level, not an average: a mapping assembled from
    /// a measured column table and an assumed link convention is only
    /// as established as the convention. Every source is carried
    /// through, so the weak link stays findable.
    #[must_use]
    pub fn weakest(parts: impl IntoIterator<Item = Evidence>) -> CombinedEvidence {
        let mut level = EvidenceLevel::HardwareVerified;
        let mut sources = Vec::new();
        for part in parts {
            level = level.min(part.level);
            sources.extend_from_slice(part.sources);
        }
        CombinedEvidence { level, sources }
    }
}

/// Evidence drawn from several facts, owning its collected sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombinedEvidence {
    pub level: EvidenceLevel,
    pub sources: Vec<&'static str>,
}

impl CombinedEvidence {
    #[must_use]
    pub fn is_production_ready(&self) -> bool {
        self.level.is_production_ready()
    }
}
