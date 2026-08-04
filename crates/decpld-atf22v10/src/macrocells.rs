//! The ATF22V10C's ten macrocells.
//!
//! Like [`crate::matrix`], this introduces no new device knowledge: the
//! row blocks, the architecture pairs, and the S0/S1 semantics were all
//! measured, and this assembles them into the shape a fitter and an
//! encoder consume.

use crate::geometry::{Atf22v10Geometry, MacrocellIndex};
use crate::matrix::{macrocell_id, product_term_of_row};
use crate::packages::GLOBAL_CLOCK;
use decpld_device::{
    ConfigField, ConfigFieldError, ConfigFieldId, FeedbackSource, MacrocellMode, MacrocellSpec,
    OutputPolarity, PadId,
};

/// Why the ATF22V10C's macrocells could not be assembled.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MacrocellError {
    #[error("macrocell {index} has no row block or no architecture pair in the geometry")]
    GeometryIncomplete { index: u8 },
    #[error("macrocell {index}: {source}")]
    Field { index: u8, source: ConfigFieldError },
}

/// Build every macrocell, in pin-ascending order — index 0 is pin 14.
///
/// # Errors
///
/// If the measured architecture pairs ever describe a configuration
/// field that cannot encode and decode totally, or if the geometry ever
/// lacks a row block or architecture pair for a macrocell this device
/// declares. Returned rather than panicked because the check is the
/// point.
///
/// The second case cannot happen today — both tables are indexed by
/// exactly `MACROCELLS` — but skipping the cell instead would return a
/// short `Vec`, and every caller treats position as identity. A missing
/// macrocell 3 would silently make macrocell 4 answer to its name.
pub fn macrocells() -> Result<Vec<MacrocellSpec>, MacrocellError> {
    let geometry = Atf22v10Geometry;
    let mut cells = Vec::with_capacity(usize::from(Atf22v10Geometry::MACROCELLS));

    for index in 0..Atf22v10Geometry::MACROCELLS {
        let macrocell = MacrocellIndex(index);
        let (Some(block), Some(pair)) =
            (geometry.row_block(macrocell), geometry.architecture_pair(macrocell))
        else {
            return Err(MacrocellError::GeometryIncomplete { index });
        };

        // S0 selects polarity: 1 is active high.
        //
        // Evidence: `arch-comb-high` and `arch-reg-high` set it;
        // `arch-comb-low` and `arch-reg-low` clear it, with mode held
        // constant across each pair.
        let polarity_field = ConfigField::new(
            ConfigFieldId(u16::from(index) * 2),
            "polarity",
            vec![pair.polarity],
            [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false])],
        )
        .map_err(|source| MacrocellError::Field { index, source })?;

        // S1 selects mode: 1 is combinational, 0 is registered.
        //
        // Evidence: `arch-comb-high` and `arch-comb-low` set it;
        // `arch-reg-high` and `arch-reg-low` clear it, with polarity
        // held constant across each pair.
        //
        // Two values, not three. `MacrocellMode::InputOnly` has no
        // pattern here because the architecture region is exactly two
        // fuses per macrocell and both are spoken for — see
        // `supports_input_only` below.
        let mode_field = ConfigField::new(
            ConfigFieldId(u16::from(index) * 2 + 1),
            "mode",
            vec![pair.mode],
            [(MacrocellMode::Combinational, vec![true]), (MacrocellMode::Registered, vec![false])],
        )
        .map_err(|source| MacrocellError::Field { index, source })?;

        cells.push(MacrocellSpec {
            id: macrocell_id(macrocell),
            // The pad index is the macrocell index, as the package map
            // also states — one number rather than two that must agree.
            pad: Some(PadId(index)),
            data_terms: block.data_rows.clone().map(product_term_of_row).collect(),
            oe_term: Some(product_term_of_row(block.output_enable_row)),
            supports_registered: true,
            supports_combinational: true,
            // Measured: `ioin14` … `ioin23` each use an undriven I/O pin
            // as an array input, and all ten compile. The cell reads
            // combinational and active low in every case, which is
            // indistinguishable from a combinational active-low output
            // — so this capability is NOT reachable through
            // `mode_field`, and asking it to encode `InputOnly` is an
            // error rather than a silent substitution.
            //
            // What differs in those designs is the output-enable row,
            // left entirely intact. That the row is the mechanism is an
            // inference from it being the only remaining control in the
            // fuse map, not a measurement; SPEC.md §7.4's output-enable
            // experiments would settle it.
            supports_input_only: true,
            // `Pin` is measured — an undriven I/O pin reaches the array
            // through this macrocell's feedback column (`ioin*`), and a
            // driven one feeds its own value back (`fb*`). Whether the
            // path is taken before or after the register is a separate
            // question these experiments do not distinguish, so no
            // stronger claim is made here.
            feedback_modes: vec![FeedbackSource::Pin],
            mode_field: Some(mode_field),
            polarity_field: Some(polarity_field),
            // One clock pin, and no per-macrocell clock select to
            // encode: the architecture region is two fuses per cell and
            // both are accounted for.
            fixed_clock: Some(GLOBAL_CLOCK),
        });
    }

    Ok(cells)
}
