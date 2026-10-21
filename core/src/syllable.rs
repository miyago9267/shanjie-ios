//! Fluent-input contract (docs/contracts/fluent-input.md) section 2: Zhuyin symbols by column and the syllables the
//! lexicon has, as three-column rows, to ask whether the symbols typed so far can still become a syllable.
use crate::engine::SYMBOLS;
use crate::Lexicon;
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

/// One typed item of a lattice input (contract fluent-input section 3): a Zhuyin symbol, a tone key (ˊ ˇ ˋ ˙; there is no key for the
/// first tone), or a syllable already decided (`Syl`: its lexicon name with the tone mark, none for the first tone). `Syl` is the
/// "tone given" marker of R2's test inputs and what the finalized part of a composition is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raw {
    Sym(char),
    Tone(char),
    Syl(String),
}

/// One lattice edge: a run of items that spells one syllable, from the node it starts at to the node `to`. `tl` is the toneless id (`Tl`)
/// of the syllable; `tone` is `Some` when the syllable is bound to that tone (a tone key closed it, or it is a `Syl`) and `None` when any
/// tone will do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub to: u32,
    pub tl: u32,
    pub tone: Option<u8>,
}

/// Tone code (1 none, 2 ˊ, 3 ˇ, 4 ˋ, 5 ˙) of a tone key's mark.
pub fn tone_code(mark: char) -> Option<u8> {
    Some(match mark {
        'ˊ' => 2,
        'ˇ' => 3,
        'ˋ' => 4,
        '˙' => 5,
        _ => return None,
    })
}

/// The edges out of every node of `raw` (nodes are positions, `raw.len() + 1` of them). Symbols of a run are put in their columns in
/// any order; a run is a syllable when its columns hold at most one symbol each and the row is a legal toneless syllable (the lexicon's,
/// without the 14 lone initials that are not syllables). A second symbol for a taken column ends the run. A tone key is a hard boundary:
/// it closes the run before it, and only an edge that ends right before it and takes it along (bound to that tone) exists, so no untyped
/// edge ends there. A syllable typed without a tone key may take any tone, the first and the fifth (˙) included. A tone key with no symbol
/// before it, or after a decided syllable, has no edge. Nothing here depends on the lexicon's readings, only on its syllables.
pub fn lattice_edges(raw: &[Raw], lex: &Lexicon) -> Vec<Vec<Edge>> {
    let tl = lex.tl();
    let mut out: Vec<Vec<Edge>> = vec![Vec::new(); raw.len() + 1];
    for (j, item) in raw.iter().enumerate() {
        match item {
            Raw::Tone(_) => {}
            Raw::Syl(name) => {
                if let Some((id, tone)) = lex.tl_syllable(name) {
                    out[j].push(Edge { to: j as u32 + 1, tl: id, tone: Some(tone) });
                }
            }
            Raw::Sym(_) => {
                let mut slots: Slots = [None; 3];
                for (k, it) in raw.iter().enumerate().skip(j) {
                    let Raw::Sym(c) = it else { break };
                    let Some(col) = slot_of(*c) else { break };
                    if slots[col].replace(*c).is_some() {
                        break;
                    }
                    let Some(id) = tl.row_id(&slots) else { continue };
                    match raw.get(k + 1) {
                        Some(Raw::Tone(t)) => {
                            if let Some(tone) = tone_code(*t) {
                                out[j].push(Edge { to: k as u32 + 2, tl: id, tone: Some(tone) });
                            }
                        }
                        _ => out[j].push(Edge { to: k as u32 + 1, tl: id, tone: None }),
                    }
                }
            }
        }
    }
    out
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

    fn edges(text: &str, typed: &str) -> Vec<Vec<(u32, &'static str, Option<u8>)>> {
        let lex = Lexicon::parse(text).unwrap();
        let raw: Vec<Raw> = typed.chars().map(|c| if tone_code(c).is_some() { Raw::Tone(c) } else { Raw::Sym(c) }).collect();
        let tl = lex.tl();
        lattice_edges(&raw, &lex)
            .into_iter()
            .map(|v| v.into_iter().map(|e| (e.to, Box::leak(tl.name(e.tl).to_string().into_boxed_str()) as &'static str, e.tone)).collect())
            .collect()
    }

    #[test]
    fn edges_leave_out_the_fake_lone_initials_and_keep_the_real_ones() {
        // ㄅ is a symbol entry of the lexicon, ㄓ is a syllable (zhi).
        let text = "ㄅ ㄅ -1.0\nㄓ 之 -1.0\nㄓˋ 製 -1.0\nㄅㄚ 八 -1.0\n";
        assert_eq!(edges(text, "ㄅ"), [vec![], vec![]]);
        assert_eq!(edges(text, "ㄓ"), [vec![(1, "ㄓ", None)], vec![]]);
        assert_eq!(edges(text, "ㄅㄚ"), [vec![(2, "ㄅㄚ", None)], vec![], vec![]]);
    }

    #[test]
    fn a_tone_key_is_a_hard_boundary() {
        let text = "ㄅㄚ 八 -1.0\nㄅㄚˋ 爸 -1.0\nㄚ 啊 -1.0\n";
        // ㄅ ㄚ ˋ ㄅ ㄚ: the first syllable is closed by ˋ at node 3 (no untyped edge ends before it); the second is untyped.
        let e = edges(text, "ㄅㄚˋㄅㄚ");
        assert_eq!(e[0], [(3, "ㄅㄚ", Some(4))]);
        assert_eq!(e[1], [(3, "ㄚ", Some(4))], "ㄚ alone, closed by the tone, is a syllable of its own");
        assert!(e[2].is_empty(), "the tone key starts nothing");
        assert_eq!(e[3], [(5, "ㄅㄚ", None)]);
        // A second symbol for a taken column ends the run: ㄅ ㄅ ㄚ is never one syllable.
        let e = edges("ㄅㄚ 八 -1.0\n", "ㄅㄅㄚ");
        assert!(e[0].is_empty());
        assert_eq!(e[1], [(3, "ㄅㄚ", None)]);
    }

    #[test]
    fn decided_syllables_are_single_edges_bound_to_their_tone() {
        let lex = Lexicon::parse("ㄅㄚˋ 爸 -1.0\nㄅㄚ 八 -1.0\n").unwrap();
        let e = lattice_edges(&[Raw::Syl("ㄅㄚˋ".into()), Raw::Syl("ㄅㄚ".into()), Raw::Syl("ㄅㄛ".into())], &lex);
        let tl = lex.tl();
        assert_eq!((e[0][0].to, tl.name(e[0][0].tl), e[0][0].tone), (1, "ㄅㄚ", Some(4)));
        assert_eq!((e[1][0].to, e[1][0].tone), (2, Some(1)), "the first tone is a tone too");
        assert!(e[2].is_empty(), "a syllable the lexicon does not have has no edge");
    }
}
