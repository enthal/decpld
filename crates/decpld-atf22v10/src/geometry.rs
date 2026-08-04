//! Where a signal enters the AND array, and which rows belong to which
//! macrocell.
//!
//! Everything here was measured. See
//! `targets/evidence/atf22v10-fuse-map.md`; each item names its
//! experiment.

use decpld_device::{FuseId, PinNumber};

/// Rows in the AND array.
///
/// Evidence: Galette `af52987` `src/chips.rs:83` and GALasm `c376d56`
/// `src/galasm.h:71`, both giving 132; consistent with 132 × 44 = 5808,
/// which is every footprint's fuse count minus the tail.
pub const ROWS: u32 = 132;

/// Columns in the AND array: 22 signal sources × 2 senses.
///
/// Evidence: measured as the width of a single product term, from
/// absolute fuse addresses — see `targets/evidence/atf22v10-fuse-map.md`,
/// "Fuse addressing". Five two-term designs write the same 88-fuse
/// extent while their one literal moves between address 88 (`in1`) and
/// address 131 (`nc13`), so a term spans at least 44 addresses and two
/// terms in 88 fuses makes it exactly 44. Cross-checked against Galette
/// `src/chips.rs:84`.
///
/// An earlier citation here read "the highest column observed is 43",
/// which is a residue mod 44 and so presupposed the stride it was
/// offered as evidence for.
pub const COLUMNS: u32 = 44;

/// The array's two dimensions and its fuse count are cited separately —
/// rows and columns from Galette and GALasm, the count from the
/// footprint arithmetic — so nothing but this assertion stops them
/// drifting apart. A mismatch would surface as a `FuseRegions` gap at
/// runtime; here it is a compile error.
const _: () = assert!(ROWS * COLUMNS == crate::regions::ARRAY_FUSES);

/// One link in the AND array: where a signal column crosses a
/// product-term row.
///
/// A named struct rather than a `(u32, u32)` pair. Both coordinates are
/// small unsigned integers, so a transposed pair is not a type error and
/// not obviously wrong at a glance — it produces a perfectly plausible
/// fuse address in the wrong place, which on this device means a
/// programmed part that misbehaves in a circuit. Named fields make the
/// swap a compile error instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArrayCell {
    /// A product term, 0..[`ROWS`].
    pub row: u32,
    /// A signal sense, 0..[`COLUMNS`]. See [`SignalSource::true_column`].
    pub column: u32,
}

/// A macrocell, indexed 0..10 by *pin ascending* — index 0 is pin 14.
///
/// PROVISIONAL. SPEC.md §4.5 calls this `MacrocellId` and CLAUDE.md
/// lists it among the project-wide newtypes; it moves to
/// `decpld-device` with `MacrocellSpec`. Same for [`ArrayCell`]'s bare
/// `u32` fields: SPEC.md §4.4 types a column as `MatrixColumn`, and a
/// row is a product-term index, which CLAUDE.md lists as
/// `ProductTermId`. The named struct stops the two coordinates being
/// swapped at a call site; it does not stop either being confused with
/// some other integer, and only newtypes will.
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

    /// The fuse carrying an array cell, or `None` if the cell is outside
    /// the array.
    ///
    /// **The array is row-major with a stride of [`COLUMNS`]**: cell
    /// (*r*, *c*) is fuse 44*r* + *c*, and row *r* occupies the
    /// contiguous run 44*r*..44*r*+44.
    ///
    /// Evidence: measured, in absolute fuse addresses with nothing
    /// divided by 44 — see `targets/evidence/atf22v10-fuse-map.md`,
    /// "Fuse addressing", which records the blown extents of eight
    /// designs and the argument they support. The table is deliberately
    /// *not* repeated here: a fuse layout fact stated in two places is
    /// one that will drift, and the document is where the reasoning
    /// lives.
    ///
    /// In outline, from absolute addresses and a count of product terms
    /// read off the source text: five two-term designs write the same
    /// 88-fuse extent while their single literal moves between address
    /// 88 and address 131, so one product term is 44 fuses wide; every
    /// design's extent begins on a multiple of 44 and is a whole number
    /// of 44s; and one pin's literal placed in different terms lands at
    /// addresses differing by an exact multiple of 44, so a pin holds
    /// the same offset in every term.
    ///
    /// This relation was previously assumed rather than stated. Every
    /// earlier experiment was read by dividing a fuse address by 44,
    /// which makes the row/column view a *consequence* of the formula
    /// and useless as evidence for it — so it was measured again from
    /// addresses. Galette and GALasm agree on row·44 + column, which
    /// makes it `OpenSourceCrossChecked` too.
    #[must_use]
    pub fn fuse_of(self, cell: ArrayCell) -> Option<FuseId> {
        // The closure is load-bearing, though not for the reason it
        // looks like: `then_some` would evaluate the multiply before the
        // bounds check, so a caller passing a computed row near
        // `u32::MAX` gets a debug-build overflow panic. The *answer*
        // would still be `None` either way — the guard gates the result,
        // not the arithmetic — so this is a crash, never a wrong fuse.
        // Clippy does not suggest `then_some` here for that reason, and
        // it should not be "simplified" into one.
        (cell.row < ROWS && cell.column < COLUMNS).then(|| FuseId(cell.row * COLUMNS + cell.column))
    }

    /// The array cell a fuse carries, or `None` if the fuse is outside
    /// the array — the architecture, signature, and power-down fuses all
    /// return `None` rather than an out-of-range row.
    #[must_use]
    pub fn cell_of(self, fuse: FuseId) -> Option<ArrayCell> {
        (fuse.0 < ROWS * COLUMNS)
            .then_some(ArrayCell { row: fuse.0 / COLUMNS, column: fuse.0 % COLUMNS })
    }

    /// The pin a macrocell drives.
    ///
    /// Evidence: measured for every macrocell, one design each
    /// (`mc14` … `mc23`).
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

    /// The array source a pin drives, whichever kind of pin it is.
    ///
    /// A dedicated input pin answers with its own source; an I/O pin
    /// answers with its macrocell's feedback source, because that is
    /// physically the path an undriven I/O pin takes into the array.
    /// The supply rails answer `None`, as does any pin the package does
    /// not have.
    ///
    /// Evidence for the I/O case: experiments `ioin14` … `ioin23`, each
    /// driving one I/O pin from a *different*, undriven one. The literal
    /// lands on the undriven pin's macrocell feedback column in all ten
    /// — 14→38, 15→34, 16→30, 17→26, 18→22, 19→18, 20→14, 21→10, 22→6,
    /// 23→2 — matching the `fb*` sweep exactly. All ten were measured
    /// because the feedback column map runs opposite to the pin
    /// numbering, so confirming one end says nothing about the other.
    ///
    /// This is the question an encoder asks when it turns a literal into
    /// a fuse, and it is one function rather than two because the caller
    /// is precisely who does not yet know which kind of pin it holds.
    #[must_use]
    pub fn source_of_pin(self, pin: PinNumber) -> Option<SignalSource> {
        if let Some(macrocell) = self.macrocell_of_pin(pin) {
            return self.sources().find(|s| s.kind == SourceKind::Feedback(macrocell));
        }
        self.sources().find(|s| s.kind == SourceKind::Input(pin))
    }

    /// Every signal source, ascending by index.
    pub fn sources(self) -> impl Iterator<Item = SignalSource> {
        (0..22).filter_map(move |index| self.source(index))
    }

    /// The rows belonging to a macrocell: the OE row first, then its
    /// data rows.
    ///
    /// Evidence: **all ten measured**, one design per macrocell
    /// (`mc14` … `mc23`). The first row of each block is where that
    /// design's OE term appears. Pins 22 and 23 are additionally
    /// corroborated by `fb22` and `arch-comb-high`, which read the same
    /// values out of two-output designs.
    ///
    /// The measured table is identical to Galette's `OLMC_ROWS_22V10`,
    /// which is a cross-check rather than the source: an earlier version
    /// of this file adopted Galette's table with only five macrocells
    /// measured and the rest interpolated. Interpolating between
    /// measured points is still not measuring, and this device's two
    /// fuse orderings run in opposite directions — precisely the shape
    /// where an interpolation looks right and is wrong in the middle.
    ///
    /// Sizes follow from contiguity: block *i* ends where block *i−1*
    /// begins, and the topmost ends at row 131. They agree with
    /// Galette's `OLMC_SIZE_22V10` and with the datasheet's Figure 1-1
    /// "8 TO 16 PRODUCT TERMS" (size minus the OE row gives 8..16).
    ///
    /// Row blocks ascend with the pin: index 0 (pin 14) is at row 122,
    /// index 9 (pin 23) at row 1. This is the **opposite** direction
    /// from [`Self::architecture_pair`], and that reversal is measured,
    /// not assumed.
    #[must_use]
    pub fn row_block(self, macrocell: MacrocellIndex) -> Option<RowBlock> {
        // One table rather than two parallel ones: the sizes are not
        // independent data — block *i* ends where block *i−1* begins —
        // and two arrays could disagree about their own length while
        // only the second was indexed unchecked.
        //
        // Indexed by macrocell index, i.e. by pin ascending from 14.
        const BLOCKS: [(u32, u32); 10] = [
            (122, 9),
            (111, 11),
            (98, 13),
            (83, 15),
            (66, 17),
            (49, 17),
            (34, 15),
            (21, 13),
            (10, 11),
            (1, 9),
        ];

        let &(first, size) = BLOCKS.get(usize::from(macrocell.0))?;
        Some(RowBlock { output_enable_row: first, data_rows: (first + 1)..(first + size) })
    }

    /// The two architecture fuses of a macrocell: S0 then S1.
    ///
    /// Evidence: pair index descends from pin 23, measured for **all
    /// ten** macrocells (`mc14` … `mc23`) — the same designs that fix the
    /// row blocks, which is how the reversal between the two orderings
    /// is established at every point rather than at its ends.
    #[must_use]
    pub fn architecture_pair(self, macrocell: MacrocellIndex) -> Option<ArchitecturePair> {
        let pin = self.macrocell_pin(macrocell)?;
        let pair = u32::from(23 - pin.0);
        let base = crate::regions::ARCHITECTURE_START + pair * 2;
        Some(ArchitecturePair { polarity: FuseId(base), mode: FuseId(base + 1) })
    }
}

/// The asynchronous-reset product term, shared by every macrocell.
///
/// Evidence: measured. `o0.ar = rst` with `rst` on pin 3 leaves row 0
/// intact at column 8, which is pin 3's true column (experiment
/// `global-ar-sp`). Previously inferred from Galette printing AR before
/// its block listing — print order is not a fuse map.
pub const ASYNCHRONOUS_RESET_ROW: u32 = 0;

/// The synchronous-preset product term, shared by every macrocell.
///
/// Evidence: measured. `o0.sp = pre` with `pre` on pin 4 leaves row 131
/// intact at column 12, which is pin 4's true column (experiment
/// `global-ar-sp`).
pub const SYNCHRONOUS_PRESET_ROW: u32 = 131;

/// A macrocell's rows in the AND array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowBlock {
    /// The output-enable product term.
    ///
    /// Evidence: it is entirely blown — a no-literal product term, so
    /// permanently enabled — in every design that does not write an
    /// `.oe`, which is what identified the row. When a design does
    /// write one, the row carries that expression like any other:
    /// `oe-var` puts pin 3's true column in it, `oe-var-not` the
    /// complement, and `oe-never` leaves all 44 links intact, which is
    /// constantly false and holds the pin off.
    pub output_enable_row: u32,
    /// The data product terms, 8 to 16 of them.
    pub data_rows: std::ops::Range<u32>,
}

impl RowBlock {
    /// The last data row, or `None` if the block has none.
    ///
    /// Exists so callers do not write `data_rows.end - 1`. Every block
    /// this device defines has at least eight data terms, so the
    /// subtraction cannot underflow today — but `data_rows` is a public
    /// `Range`, and the first caller to reach for its last element
    /// teaches every later one how it is done. A device model with an
    /// empty block is not obviously impossible, and the difference
    /// between `None` and a wrapped `u32::MAX` row is the difference
    /// between a rejected build and a fuse address in another region.
    #[must_use]
    pub fn last_data_row(&self) -> Option<u32> {
        (self.data_rows.end > self.data_rows.start).then(|| self.data_rows.end - 1)
    }
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
