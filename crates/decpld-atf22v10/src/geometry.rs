//! Where a signal enters the AND array, and which rows belong to which
//! macrocell.
//!
//! Everything here was measured. See
//! `targets/evidence/atf22v10-fuse-map.md`; each item names its
//! experiment.

use decpld_device::FuseId;

/// Rows in the AND array.
///
/// Evidence: Galette `af52987` `src/chips.rs:83` and GALasm `c376d56`
/// `src/galasm.h:71`, both giving 132; consistent with 132 × 44 = 5808,
/// which is every footprint's fuse count minus the tail.
pub const ROWS: u32 = 132;

/// Columns in the AND array: 22 signal sources × 2 senses.
///
/// Evidence: Galette `src/chips.rs:84`, and measured — the highest
/// column observed is 43 (`nc13`, the complement of pin 13).
pub const COLUMNS: u32 = 44;

/// A DIP-24 pin number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinNumber(pub u8);

/// A macrocell, indexed 0..10 by *pin ascending* — index 0 is pin 14.
///
/// The index is deliberately tied to the pin rather than to either fuse
/// ordering, because the two fuse orderings disagree with each other and
/// naming the index after one of them would guarantee the confusion this
/// device is most exposed to. See [`Atf22v10Geometry::row_block`] and
/// [`Atf22v10Geometry::architecture_pair`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacrocellIndex(pub u8);

/// What drives a column pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A dedicated input pin.
    Input(PinNumber),
    /// A macrocell's output fed back into the array.
    Feedback(MacrocellIndex),
}

/// One of the array's 22 signal sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalSource {
    pub index: u8,
    pub kind: SourceKind,
}

impl SignalSource {
    /// The column carrying this source in its true sense.
    ///
    /// Evidence: measured for every source. Inputs by sweeping the input
    /// pin (`in1`…`in11`, `in13`); feedbacks by routing each macrocell
    /// into pin 23 (`fb14`…`fb23`).
    #[must_use]
    pub fn true_column(self) -> u32 {
        u32::from(self.index) * 2
    }

    /// The column carrying this source complemented.
    ///
    /// Evidence: complement is always true + 1, measured on four input
    /// pins (`nc2`, `nc3`, `nc11`, `nc13`) and three feedback sources
    /// (`nfb22`, `nfb18`, `nfb14`). Both kinds were measured because
    /// establishing it on inputs alone and generalising is an assumption
    /// about uniform structure, and getting it wrong inverts a literal
    /// on hardware where nothing downstream would notice.
    #[must_use]
    pub fn complement_column(self) -> u32 {
        self.true_column() + 1
    }
}

/// The ATF22V10C's array geometry.
///
/// A unit struct rather than free functions so the mapping is reached
/// through one named thing that can be swapped for a variant part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Atf22v10Geometry;

/// Input pins in source order, index 0 first.
///
/// Evidence: the `in*` sweep. Pin 12 is GND and pin 24 is VCC
/// (datasheet Figure 2-2), so neither appears. Source 21 is pin 13,
/// which is why this is a table and not arithmetic — see
/// [`Atf22v10Geometry::source`].
const INPUT_PINS: [(u8, u8); 12] = [
    (0, 1),
    (2, 2),
    (4, 3),
    (6, 4),
    (8, 5),
    (10, 6),
    (12, 7),
    (14, 8),
    (16, 9),
    (18, 10),
    (20, 11),
    (21, 13),
];

impl Atf22v10Geometry {
    /// The number of macrocells. Datasheet Figure 1-1: "10 I/O PINS".
    pub const MACROCELLS: u8 = 10;

    /// The pin a macrocell drives.
    ///
    /// Evidence: measured at five points spanning the range — pins 23,
    /// 22, 20, 17, 14 (`arch-comb-high`, `fb22`, `mc20`, `mc17`,
    /// `mc14`).
    #[must_use]
    pub fn macrocell_pin(self, macrocell: MacrocellIndex) -> Option<PinNumber> {
        (macrocell.0 < Self::MACROCELLS).then(|| PinNumber(14 + macrocell.0))
    }

    /// The macrocell a pin belongs to, if it is an I/O pin.
    #[must_use]
    pub fn macrocell_of_pin(self, pin: PinNumber) -> Option<MacrocellIndex> {
        (14..14 + Self::MACROCELLS).contains(&pin.0).then(|| MacrocellIndex(pin.0 - 14))
    }

    /// What drives source `index`, or `None` if there is no such source.
    ///
    /// **A table, not a formula.** Even sources are input pins and odd
    /// sources are feedback — for sources 0..20. Source 21 is input pin
    /// 13, so a rule of "odd means feedback" holds for all ten feedbacks
    /// and still gets one of the eleven odd sources wrong. Each of the
    /// ten feedbacks was measured individually rather than extrapolated
    /// from the first, which is the only reason that boundary is known.
    #[must_use]
    pub fn source(self, index: u8) -> Option<SignalSource> {
        if let Some(&(_, pin)) = INPUT_PINS.iter().find(|(source, _)| *source == index) {
            return Some(SignalSource { index, kind: SourceKind::Input(PinNumber(pin)) });
        }
        // Odd sources 1..19 are macrocell feedback, descending from pin
        // 23 at source 1 — the same descending order as the architecture
        // pairs, and the opposite of the row blocks.
        if index < 20 && index % 2 == 1 {
            let pin = 23 - (index - 1) / 2;
            let macrocell = self.macrocell_of_pin(PinNumber(pin))?;
            return Some(SignalSource { index, kind: SourceKind::Feedback(macrocell) });
        }
        None
    }

    /// Every signal source, ascending by index.
    pub fn sources(self) -> impl Iterator<Item = SignalSource> {
        (0..22).filter_map(move |index| self.source(index))
    }

    /// The rows belonging to a macrocell: the OE row first, then its
    /// data rows.
    ///
    /// Evidence: measured at pins 23 (rows 1–9), 22 (rows 10–20), 20
    /// (from row 34), 17 (from row 83) and 14 (from row 122). The
    /// per-block extents come from Galette's `OLMC_ROWS_22V10` and
    /// `OLMC_SIZE_22V10`, cross-checked against those five measurements
    /// and against the datasheet's Figure 1-1 "8 TO 16 PRODUCT TERMS"
    /// (block size minus the OE row gives 8..16).
    ///
    /// Row blocks ascend with the pin: index 0 (pin 14) is at row 122,
    /// index 9 (pin 23) at row 1. This is the **opposite** direction
    /// from [`Self::architecture_pair`], and that reversal is measured,
    /// not assumed.
    #[must_use]
    pub fn row_block(self, macrocell: MacrocellIndex) -> Option<RowBlock> {
        // Indexed by macrocell index, i.e. by pin ascending from 14.
        const FIRST_ROW: [u32; 10] = [122, 111, 98, 83, 66, 49, 34, 21, 10, 1];
        const SIZE: [u32; 10] = [9, 11, 13, 15, 17, 17, 15, 13, 11, 9];

        let index = usize::from(macrocell.0);
        let first = *FIRST_ROW.get(index)?;
        let size = SIZE[index];
        Some(RowBlock { output_enable_row: first, data_rows: (first + 1)..(first + size) })
    }

    /// The two architecture fuses of a macrocell: S0 then S1.
    ///
    /// Evidence: pair index descends from pin 23, measured at pins 23,
    /// 22, 20, 17 and 14 — the same five points as the row blocks, which
    /// is how the reversal between the two orderings was established
    /// rather than inferred.
    #[must_use]
    pub fn architecture_pair(self, macrocell: MacrocellIndex) -> Option<ArchitecturePair> {
        let pin = self.macrocell_pin(macrocell)?;
        let pair = u32::from(23 - pin.0);
        let base = crate::regions::ARCHITECTURE_START + pair * 2;
        Some(ArchitecturePair { polarity: FuseId(base), mode: FuseId(base + 1) })
    }
}

/// A macrocell's rows in the AND array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowBlock {
    /// The output-enable product term. Evidence: in every measured
    /// design this row is entirely blown — a no-literal product term,
    /// i.e. permanently enabled.
    pub output_enable_row: u32,
    /// The data product terms, 8 to 16 of them.
    pub data_rows: std::ops::Range<u32>,
}

/// A macrocell's two architecture fuses.
///
/// Evidence: four experiments varying one thing at a time
/// (`arch-comb-high`, `arch-comb-low`, `arch-reg-high`, `arch-reg-low`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchitecturePair {
    /// `1` selects active high.
    pub polarity: FuseId,
    /// `1` selects combinational, `0` registered.
    pub mode: FuseId,
}
