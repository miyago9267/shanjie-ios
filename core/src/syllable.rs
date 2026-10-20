//! Fluent-input contract (docs/contracts/fluent-input.md) section 2: Zhuyin symbols by column and the syllables the
//! lexicon has, as three-column rows, to ask whether the symbols typed so far can still become a syllable.
use crate::engine::SYMBOLS;
use std::collections::HashSet;

/// The symbols of one syllable by column: initial, medial, final.
pub type Slots = [Option<char>; 3];

/// Column of a Zhuyin symbol: 0 initial, 1 medial, 2 final.
pub fn slot_of(sym: char) -> Option<usize> {
    let i = SYMBOLS.chars().position(|c| c == sym)?;
    Some(if i < 21 { 0 } else if i < 24 { 1 } else { 2 })
}

/// The toneless syllables of the lexicon as column rows (about 430 of them).
pub struct SylTable {
    rows: HashSet<Slots>,
}

impl SylTable {
    /// `names` are syllable names with their tone mark (or none); a name that is not plain Zhuyin symbols (punctuation tokens,
    /// a symbol twice in one column) is skipped. Order inside a name does not matter, the column decides.
    pub fn new<'a>(names: impl Iterator<Item = &'a str>) -> SylTable {
        let rows = names
            .filter_map(|n| {
                let mut slots: Slots = [None; 3];
                for c in n.chars().filter(|c| !"ˊˇˋ˙".contains(*c)) {
                    let col = slot_of(c)?;
                    if slots[col].replace(c).is_some() {
                        return None;
                    }
                }
                slots.iter().any(Option::is_some).then_some(slots)
            })
            .collect();
        SylTable { rows }
    }

    /// Whether some syllable (any tone) holds every symbol of `p` in its own column.
    pub fn extends(&self, p: &Slots) -> bool {
        if p.iter().all(Option::is_some) {
            return self.rows.contains(p);
        }
        self.rows.iter().any(|r| (0..3).all(|c| p[c].is_none() || p[c] == r[c]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_columns() {
        assert_eq!([slot_of('ㄅ'), slot_of('ㄧ'), slot_of('ㄚ'), slot_of('ˇ'), slot_of('a')], [Some(0), Some(1), Some(2), None, None]);
    }

    #[test]
    fn extension_is_a_subset_test() {
        let t = SylTable::new(["ㄏㄠˇ", "ㄇㄚ˙", "ㄊㄚ", "_punctuation_x", "ㄅㄅ"].into_iter());
        assert!(t.extends(&[Some('ㄏ'), None, None]));
        assert!(t.extends(&[None, None, Some('ㄠ')]));
        assert!(t.extends(&[Some('ㄇ'), None, Some('ㄚ')]));
        assert!(!t.extends(&[Some('ㄏ'), None, Some('ㄚ')]), "no syllable is ㄏㄚ here");
        assert!(!t.extends(&[Some('ㄅ'), None, None]), "a name with a symbol twice in a column is skipped");
    }
}
