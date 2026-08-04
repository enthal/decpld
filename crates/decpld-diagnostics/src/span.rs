//! Source locations. SPEC.md §3.2.

use std::ops::Index;

/// Identifies a source file within a compilation.
///
/// Deliberately opaque: a file's *identity* and its *path* are different
/// concerns, and the resolver owns the mapping between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// A half-open byte range `[start, end)` within one file.
///
/// Half-open to match Rust slicing, so `&text[range]` needs no
/// adjustment and off-by-one errors have one fewer place to hide.
///
/// The invariant `start <= end` holds by construction: [`TextRange::new`]
/// orders its arguments. The fields stay public because SPEC.md §3.2
/// specifies them so, and because a range is a value type with nothing
/// to hide — but construct through `new` rather than a struct literal
/// so the ordering is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

impl TextRange {
    /// A range covering `[a, b)`, ordering the endpoints if reversed.
    #[must_use]
    pub const fn new(a: u32, b: u32) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }

    /// An empty range at `offset`, for pointing at a position rather
    /// than a region — "expected `*` here".
    #[must_use]
    pub const fn empty_at(offset: u32) -> Self {
        Self { start: offset, end: offset }
    }

    #[must_use]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Whether `offset` falls inside the range. The end offset does not,
    /// because it is the first byte *after* the range.
    #[must_use]
    pub const fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// The smallest range covering both operands, including any gap.
    #[must_use]
    pub fn cover(self, other: Self) -> Self {
        Self { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

impl Index<TextRange> for str {
    type Output = str;

    fn index(&self, range: TextRange) -> &str {
        &self[range.start as usize..range.end as usize]
    }
}

/// A range within a specific file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: FileId,
    pub range: TextRange,
}

impl Span {
    #[must_use]
    pub const fn new(file: FileId, range: TextRange) -> Self {
        Self { file, range }
    }

    /// The smallest span covering both operands, or `None` when they are
    /// in different files.
    ///
    /// Two files have unrelated coordinate systems, so a span covering
    /// both would name a region that does not exist. Returning `None`
    /// makes the caller decide rather than inventing a location — the
    /// kind of quietly wrong output that is worse than no output.
    #[must_use]
    pub fn cover(self, other: Self) -> Option<Self> {
        (self.file == other.file).then(|| Self::new(self.file, self.range.cover(other.range)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_range_reports_its_length() {
        let range = TextRange::new(4, 10);
        assert_eq!(range.len(), 6);
        assert!(!range.is_empty());
        assert!(TextRange::new(7, 7).is_empty());
    }

    #[test]
    fn text_range_orders_reversed_endpoints() {
        // A reversed range is a caller bug, but silently producing a
        // range whose `end < start` would make every downstream slice
        // panic far from the cause. Normalising keeps the invariant
        // `start <= end` true by construction.
        let range = TextRange::new(10, 4);
        assert_eq!((range.start, range.end), (4, 10));
    }

    #[test]
    fn text_range_contains_is_half_open() {
        let range = TextRange::new(4, 7);
        assert!(!range.contains(3));
        assert!(range.contains(4));
        assert!(range.contains(6));
        // Half-open: the end offset is the first byte *after* the range,
        // matching Rust slicing so `&text[range]` needs no adjustment.
        assert!(!range.contains(7));
    }

    #[test]
    fn text_range_covers_both_operands_when_merged() {
        let a = TextRange::new(4, 7);
        let b = TextRange::new(20, 25);
        let merged = a.cover(b);
        assert_eq!((merged.start, merged.end), (4, 25));
        assert_eq!(merged, b.cover(a), "cover is commutative");
    }

    #[test]
    fn text_range_indexes_a_string_slice() {
        let text = "QF500*C021A*";
        let range = TextRange::new(6, 12);
        assert_eq!(&text[range], "C021A*");
    }

    #[test]
    fn spans_in_the_same_file_cover_each_other() {
        let file = FileId(3);
        let a = Span::new(file, TextRange::new(4, 7));
        let b = Span::new(file, TextRange::new(20, 25));
        assert_eq!(a.cover(b), Some(Span::new(file, TextRange::new(4, 25))));
    }

    #[test]
    fn spans_in_different_files_do_not_cover() {
        // Two files have unrelated coordinate systems; a span covering
        // both would name a region that does not exist. Returning None
        // makes the caller decide rather than inventing a location.
        let a = Span::new(FileId(1), TextRange::new(4, 7));
        let b = Span::new(FileId(2), TextRange::new(4, 7));
        assert_eq!(a.cover(b), None);
    }
}
