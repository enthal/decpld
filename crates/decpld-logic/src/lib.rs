//! Boolean functions in sum-of-products form. SPEC.md §3.9.
//!
//! This layer speaks in Boolean inputs and literals. It does not know
//! what a pin is, what a macrocell is, or that fuses exist — a `Cube`
//! is an AND of literals whether it ends up in an ATF22V10 row or in a
//! truth table.
//!
//! Only the shapes SPEC.md §3.9 specifies. `Cube` and `Literal` are
//! what the device layer encodes today; `Sop` is here because a cube is
//! only meaningful as one term of a sum, and a macrocell's data terms
//! are an `Sop` the moment `MacrocellSpec` lands. The hash-consed
//! Boolean graph (§3.8) and the minimizer (§3.9) arrive with M2; empty
//! modules for them now would claim progress that does not exist.

/// An input to a Boolean function.
///
/// Opaque here. What it *is* — a signal bit, a pin, a macrocell's
/// feedback — is decided above by the elaborator and below by the
/// device's literal-source table, and this layer is deliberately in
/// between and ignorant of both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BoolInputId(pub u32);

/// Whether a literal uses its input directly or inverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Polarity {
    /// The input itself.
    True,
    /// The input inverted.
    Complement,
}

impl Polarity {
    /// The other one.
    #[must_use]
    pub fn inverted(self) -> Self {
        match self {
            Polarity::True => Polarity::Complement,
            Polarity::Complement => Polarity::True,
        }
    }

    /// This polarity applied to a value.
    #[must_use]
    pub fn applied_to(self, value: bool) -> bool {
        match self {
            Polarity::True => value,
            Polarity::Complement => !value,
        }
    }
}

/// One input at one polarity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Literal {
    pub input: BoolInputId,
    pub polarity: Polarity,
}

impl Literal {
    #[must_use]
    pub fn new(input: BoolInputId, polarity: Polarity) -> Self {
        Self { input, polarity }
    }

    /// The same input at the opposite polarity — the literal that
    /// contradicts this one.
    #[must_use]
    pub fn contradiction(self) -> Self {
        Self { input: self.input, polarity: self.polarity.inverted() }
    }
}

/// An AND of literals.
///
/// A cube with no literals is the empty AND, which is constantly
/// **true** — not constantly false, and not "unused". On a GAL that is
/// exactly what an all-blown product-term row means, so the convention
/// is load-bearing rather than a matter of taste.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cube {
    pub literals: Vec<Literal>,
}

impl Cube {
    /// The empty AND: constantly true.
    #[must_use]
    pub fn always() -> Self {
        Self { literals: Vec::new() }
    }

    #[must_use]
    pub fn new(literals: impl IntoIterator<Item = Literal>) -> Self {
        Self { literals: literals.into_iter().collect() }
    }

    /// The input this cube constrains at both polarities, if any.
    ///
    /// Such a cube is constantly false — `a & !a` — which is a
    /// different thing from a cube that happens to evaluate false for
    /// some assignment. Encoding one into a product term would ask the
    /// array to connect an input's true and complement columns in the
    /// same row, and on this hardware that is indistinguishable from a
    /// row nobody programmed.
    #[must_use]
    pub fn contradiction(&self) -> Option<BoolInputId> {
        for (index, literal) in self.literals.iter().enumerate() {
            if self.literals[index + 1..].contains(&literal.contradiction()) {
                return Some(literal.input);
            }
        }
        None
    }

    /// The same cube with its literals sorted and de-duplicated.
    ///
    /// Two cubes that AND the same literals are the same Boolean
    /// function however they were written, so comparing them by `Vec`
    /// order compares how they were *built* rather than what they mean.
    /// The device layer needs the distinction: encoding a cube and
    /// decoding the row back cannot preserve literal order, because the
    /// array has no order to preserve — a row is a set of connections.
    ///
    /// A repeated literal is dropped; a contradictory pair is *kept*,
    /// since `a & !a` is a real function (constantly false) and
    /// silently discarding half of it would turn a refusable design
    /// into a plausible one.
    #[must_use]
    pub fn canonical(&self) -> Self {
        let mut literals = self.literals.clone();
        literals.sort_unstable();
        literals.dedup();
        Self { literals }
    }

    /// Evaluate against an assignment.
    ///
    /// `value_of` returns `None` for an input the assignment does not
    /// cover, and the cube then has no value either — better than
    /// silently reading an unspecified input as false.
    #[must_use]
    pub fn evaluate(&self, mut value_of: impl FnMut(BoolInputId) -> Option<bool>) -> Option<bool> {
        for literal in &self.literals {
            if !literal.polarity.applied_to(value_of(literal.input)?) {
                return Some(false);
            }
        }
        Some(true)
    }
}

/// An OR of cubes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sop {
    pub cubes: Vec<Cube>,
}

impl Sop {
    /// The empty OR: constantly **false**, the dual of [`Cube::always`].
    #[must_use]
    pub fn never() -> Self {
        Self { cubes: Vec::new() }
    }

    #[must_use]
    pub fn new(cubes: impl IntoIterator<Item = Cube>) -> Self {
        Self { cubes: cubes.into_iter().collect() }
    }

    #[must_use]
    pub fn evaluate(&self, mut value_of: impl FnMut(BoolInputId) -> Option<bool>) -> Option<bool> {
        // Short-circuits on true, as `Cube::evaluate` short-circuits on
        // false. Without it an SOP whose first cube is already true
        // still answers `None` when a *later* cube mentions an
        // uncovered input — the dual of the cube rule, and the two must
        // agree or a partial assignment means different things at the
        // two levels.
        for cube in &self.cubes {
            if cube.evaluate(&mut value_of)? {
                return Some(true);
            }
        }
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: BoolInputId = BoolInputId(0);
    const B: BoolInputId = BoolInputId(1);

    fn t(input: BoolInputId) -> Literal {
        Literal::new(input, Polarity::True)
    }
    fn c(input: BoolInputId) -> Literal {
        Literal::new(input, Polarity::Complement)
    }

    #[test]
    fn the_empty_cube_is_true_and_the_empty_sop_is_false() {
        // The two empties are duals, and getting either backwards
        // inverts a whole design. An all-blown product-term row on a
        // GAL is the empty AND, so this is the convention the encoder
        // relies on.
        assert_eq!(Cube::always().evaluate(|_| Some(false)), Some(true));
        assert_eq!(Sop::never().evaluate(|_| Some(true)), Some(false));
    }

    #[test]
    fn a_cube_holding_one_input_at_both_polarities_is_a_contradiction() {
        assert_eq!(Cube::new([t(A), c(A)]).contradiction(), Some(A));
        assert_eq!(Cube::new([t(A), t(B), c(A)]).contradiction(), Some(A));
        assert_eq!(Cube::new([t(A), c(B)]).contradiction(), None);
        assert_eq!(Cube::new([t(A), t(A)]).contradiction(), None, "a repeat is not a conflict");
        assert_eq!(Cube::always().contradiction(), None);
    }

    #[test]
    fn a_contradictory_cube_evaluates_false_everywhere() {
        // The property behind the name, stated over every assignment of
        // two inputs rather than asserted once.
        let cube = Cube::new([t(A), c(A)]);
        for a in [false, true] {
            for b in [false, true] {
                let value = cube.evaluate(|i| Some(if i == A { a } else { b }));
                assert_eq!(value, Some(false), "a={a} b={b}");
            }
        }
    }

    #[test]
    fn an_input_the_assignment_does_not_cover_has_no_value() {
        // Reading an unspecified input as false would make a partial
        // assignment silently produce a definite answer, which is how a
        // simulator and an encoder come to disagree about the same
        // design.
        let cube = Cube::new([t(A), t(B)]);
        assert_eq!(cube.evaluate(|i| (i == A).then_some(true)), None);
        // Short-circuit: a literal already false answers without
        // needing the rest.
        assert_eq!(Cube::new([c(A), t(B)]).evaluate(|i| (i == A).then_some(true)), Some(false));
    }

    #[test]
    fn polarity_inverts_and_applies() {
        assert_eq!(Polarity::True.inverted(), Polarity::Complement);
        assert_eq!(Polarity::Complement.inverted(), Polarity::True);
        assert!(Polarity::True.applied_to(true));
        assert!(!Polarity::True.applied_to(false));
        assert!(Polarity::Complement.applied_to(false));
        assert!(!Polarity::Complement.applied_to(true));
        assert_eq!(t(A).contradiction(), c(A));
    }

    #[test]
    fn a_sop_that_is_already_true_does_not_need_the_rest_of_its_inputs() {
        // The dual of the cube rule. `Cube::evaluate` stops at the first
        // false literal; `Sop::evaluate` must stop at the first true
        // cube, or a partial assignment means one thing at the cube
        // level and another at the sum level.
        let sop = Sop::new([Cube::new([t(A)]), Cube::new([t(B)])]);
        assert_eq!(sop.evaluate(|i| (i == A).then_some(true)), Some(true));
        // But an SOP that is not yet true still needs the uncovered
        // input before it can answer.
        assert_eq!(sop.evaluate(|i| (i == A).then_some(false)), None);
    }

    #[test]
    fn canonical_ignores_order_and_repetition_but_keeps_contradictions() {
        // Order and repetition are how a cube was written; the literal
        // set is what it means. A round-trip through hardware can only
        // preserve the second.
        assert_eq!(Cube::new([t(B), t(A)]).canonical(), Cube::new([t(A), t(B)]).canonical());
        assert_eq!(Cube::new([t(A), t(A)]).canonical(), Cube::new([t(A)]).canonical());
        assert_eq!(Cube::always().canonical(), Cube::always());

        // A contradiction survives canonicalisation, so a cube that
        // must be refused still can be after it.
        let contradictory = Cube::new([c(A), t(A), t(A)]).canonical();
        assert_eq!(contradictory.literals.len(), 2);
        assert_eq!(contradictory.contradiction(), Some(A));

        // Canonicalising twice changes nothing.
        assert_eq!(contradictory.canonical(), contradictory);
    }

    #[test]
    fn canonical_preserves_the_function() {
        // The property that makes it safe to compare canonical forms:
        // the cube still evaluates the same everywhere.
        for code in 0..16u32 {
            let literals: Vec<Literal> = [t(A), c(A), t(B), c(B)]
                .into_iter()
                .enumerate()
                .filter(|(i, _)| code >> i & 1 == 1)
                .map(|(_, l)| l)
                .collect();
            let cube = Cube::new(literals);
            let canonical = cube.canonical();
            for a in [false, true] {
                for b in [false, true] {
                    let assign = |i: BoolInputId| Some(if i == A { a } else { b });
                    assert_eq!(cube.evaluate(assign), canonical.evaluate(assign), "{cube:?}");
                }
            }
        }
    }

    #[test]
    fn an_sop_is_the_or_of_its_cubes() {
        let sop = Sop::new([Cube::new([t(A)]), Cube::new([t(B)])]);
        for (a, b, expected) in
            [(false, false, false), (true, false, true), (false, true, true), (true, true, true)]
        {
            let value = sop.evaluate(|i| Some(if i == A { a } else { b }));
            assert_eq!(value, Some(expected), "a={a} b={b}");
        }
    }
}
