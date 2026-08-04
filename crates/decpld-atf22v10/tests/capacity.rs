//! Product-term capacity and row ownership, measured.
//!
//! SPEC.md §7.4's capacity group: "one through N independent product
//! terms per macrocell; determine exact row ownership and fit
//! boundary."
//!
//! Until these ran, the capacities came from the datasheet's Figure 1-1
//! ("8 TO 16 PRODUCT TERMS") plus row blocks cross-checked against
//! Galette. That is two documents agreeing, which is worth having and
//! is not a boundary: neither says what happens at the ninth term.
//! `cap23-8` … `cap14-9` ask directly — a design WinCUPL accepts at
//! eight and refuses at nine is a measurement.
//!
//! Recorded in `targets/evidence/atf22v10-fuse-map.md`, "Capacity".

use decpld_atf22v10::*;
use decpld_device::{
    FuseId, MacrocellId, MacrocellMode, OutputPolarity, PhysicalDesign, PinNumber, PlacedCube,
    ProductTermId,
};
use decpld_logic::{Cube, Literal, Polarity};

const G: Atf22v10Geometry = Atf22v10Geometry;

fn literal(pin: u8) -> Literal {
    let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
    Literal::new(bool_input_of_source(source.index), Polarity::True)
}

/// A design driving `pin` from a sum of `pins`, one literal per term,
/// filling the block's data rows from the first.
fn sum_of_literals(pin: u8, pins: &[u8]) -> PhysicalDesign {
    let mut design = blank_design().expect("a blank design");
    let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present");
    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = pins
        .iter()
        .enumerate()
        .map(|(index, &source)| PlacedCube {
            row: ProductTermId(block.data_rows.start + u32::try_from(index).expect("under 16")),
            cube: Cube::new([literal(source)]),
        })
        .collect();
    design
}

#[test]
fn the_measured_blocks_are_the_ones_the_oracle_fills() {
    // Row ownership, at absolute rows, from designs that use every data
    // row a block has. `cap23-8` writes rows 1-9 and stops; `cap19-16`
    // writes 49-65; `cap14-8` writes 122-130. Each block's first row is
    // its output enable and the rest are data.
    //
    // This measures directly what was previously cross-checked against
    // Galette's `OLMC_ROWS_22V10` and `OLMC_SIZE_22V10`. Two independent
    // sources agreeing is what SPEC.md §13.1 calls
    // `OpenSourceCrossChecked`; this makes it a measurement as well.
    for (pin, first, last, data_terms) in
        [(23u8, 1u32, 9u32, 8usize), (19, 49, 65, 16), (14, 122, 130, 8)]
    {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a measured block");
        assert_eq!(block.output_enable_row, first, "pin {pin}'s enable row");
        assert_eq!(block.data_rows.start, first + 1, "pin {pin}'s first data row");
        assert_eq!(block.last_data_row(), Some(last), "pin {pin}'s last data row");
        assert_eq!(block.data_rows.len(), data_terms, "pin {pin}'s data-term count");
    }
}

#[test]
fn eight_terms_on_pin_23_reproduce_the_fuses_the_oracle_wrote() {
    // `cap23-8`: `o0 = i0 # i1 # ... # i7` over pins 1-8. A sum of
    // eight distinct single literals is already minimal SOP, so CUPL
    // cannot merge them — eight literals need eight product terms.
    //
    // The oracle's intact links, one per data row:
    //
    //     row 2: 88   row 3: 136  row 4: 184  row 5: 232
    //     row 6: 280  row 7: 328  row 8: 376  row 9: 424
    //
    // which are columns 0, 4, 8, 12, 16, 20, 24, 28 — pins 1 through 8.
    let design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let fuses = encode_design(&design, Footprint::Gal).expect("eight terms fit");

    for (index, fuse) in [88, 136, 184, 232, 280, 328, 376, 424].into_iter().enumerate() {
        assert_eq!(fuses.get(FuseId(fuse)), Some(false), "term {index}'s literal at {fuse}");
    }
    // The block is written and nothing past it: row 9 ends at 439.
    for fuse in 440..5808 {
        assert_eq!(fuses.get(FuseId(fuse)), Some(false), "fuse {fuse} is outside pin 23's block");
    }
}

#[test]
fn sixteen_terms_fit_the_widest_block_and_fill_it_exactly() {
    // `cap19-16`. Pin 19's macrocell is the widest — sixteen data rows,
    // twice pin 23's — and the asymmetry is why a fitter needs a
    // per-macrocell capacity rather than one number for the part.
    let pins = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17];
    let design = sum_of_literals(19, &pins);
    let fuses = encode_design(&design, Footprint::Gal).expect("sixteen terms fit");

    // The oracle's intact links for the first twelve, ascending by row:
    // pins 1-11 at columns 0..40 in steps of 4, then pin 13 at 42.
    for (index, fuse) in [2200, 2248, 2296, 2344, 2392, 2440, 2488, 2536, 2584, 2632, 2680, 2726]
        .into_iter()
        .enumerate()
    {
        assert_eq!(fuses.get(FuseId(fuse)), Some(false), "term {index} at {fuse}");
    }
    // Then four I/O pins used as inputs, at their feedback columns
    // 38, 34, 30, 26 — measured by the `fb*` and `ioin*` sweeps.
    for (index, fuse) in [2766, 2806, 2846, 2886].into_iter().enumerate() {
        assert_eq!(fuses.get(FuseId(fuse)), Some(false), "feedback term {index} at {fuse}");
    }
}

#[test]
fn a_ninth_term_on_an_eight_term_macrocell_is_refused() {
    // The boundary, and the point of the whole group. `cap23-9` is
    // `cap23-8` plus one literal, and WinCUPL turns it away — so
    // deCPLD must too, at the same place, or one of the two compilers
    // is wrong about the part.
    //
    // deCPLD refuses it as an ownership violation rather than as a
    // resource error, because at this layer a term names the row it
    // goes in: there is no ninth row for it to name.
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let mut design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let overflow = ProductTermId(block.data_rows.end);
    design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present")
        .data_terms
        .push(PlacedCube { row: overflow, cube: Cube::new([literal(9)]) });

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("nine terms do not fit"),
        DesignError::TermNotOwned { macrocell: MacrocellId(9), row: overflow }
    );
}

#[test]
fn a_seventeenth_term_on_the_widest_macrocell_is_refused() {
    // `cap19-17`. Measured separately from pin 23 because the
    // capacities are not uniform: a boundary confirmed at eight says
    // nothing about one at sixteen.
    let macrocell = G.macrocell_of_pin(PinNumber(19)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let pins = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17];
    let mut design = sum_of_literals(19, &pins);
    let overflow = ProductTermId(block.data_rows.end);
    design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present")
        .data_terms
        .push(PlacedCube { row: overflow, cube: Cube::new([literal(18)]) });

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("seventeen terms do not fit"),
        DesignError::TermNotOwned { macrocell: MacrocellId(5), row: overflow }
    );
}

#[test]
fn the_row_after_a_block_belongs_to_the_next_macrocell_not_to_no_one() {
    // What makes the refusals above a *capacity* boundary rather than
    // an off-by-one: row 10 is not spare, it is pin 22's output-enable
    // row. A ninth term on pin 23 would have to take another output's
    // enable, which is why the fitter cannot quietly borrow it.
    let next = G.macrocell_of_pin(PinNumber(22)).expect("an I/O pin");
    let block = G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("block");
    assert_eq!(G.row_block(next).expect("a block").output_enable_row, block.data_rows.end);
}

#[test]
fn the_measured_capacity_is_capacity_in_the_true_cover() {
    // The qualification that keeps `data_term_capacity` from being
    // misread. Every driven macrocell in these designs reads S0 = 1,
    // active high — `cap23-8` sets 5808/5809, `cap14-8` sets 5826/5827
    // — so WinCUPL implemented each sum directly.
    //
    // It could have inverted. `!(i0 # ... # i8)` is
    // `!i0 & !i1 & ... & !i8`, ONE product term, so a nine-input OR
    // fits an eight-term macrocell as an active-low output. CUPL does
    // not search for that: `o0 = ...` fixes the polarity.
    //
    // So eight is a bound on product TERMS, not on logic, and a fitter
    // selecting between true and complement covers (SPEC.md §3.9) will
    // fit designs the oracle refuses. Asserting the polarity here is
    // what stops the capacity tests above from being read as the
    // stronger claim.
    let design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let fuses = encode_design(&design, Footprint::Gal).expect("eight terms fit");
    assert_eq!(fuses.get(FuseId(5808)), Some(true), "pin 23 active high, as the oracle wrote it");
    assert_eq!(fuses.get(FuseId(5809)), Some(true), "and combinational");

    let design = sum_of_literals(14, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let fuses = encode_design(&design, Footprint::Gal).expect("eight terms fit");
    assert_eq!(fuses.get(FuseId(5826)), Some(true), "pin 14's pair is the LAST, not the first");
    assert_eq!(fuses.get(FuseId(5827)), Some(true));
}
