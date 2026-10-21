//! Fluent-input contract section 3: the toneless view of the real lexicon, as the lattice decoder uses it.
use core::engine::load_lexicon;
use core::syllable::Slots;
use std::path::Path;

fn lexicon() -> std::sync::Arc<core::Lexicon> {
    load_lexicon(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/lexicon")).expect("the lexicon loads")
}

#[test]
fn legal_toneless_syllables_exclude_the_fake_lone_initials() {
    let lex = lexicon();
    let tl = lex.tl();
    let lone = |c: char| tl.row_id(&[Some(c), None, None]);
    for c in "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒ".chars() {
        assert!(lone(c).is_none(), "{c} alone is a symbol entry, not a syllable");
    }
    for c in "ㄓㄔㄕㄖㄗㄘㄙ".chars() {
        assert!(lone(c).is_some(), "{c} alone is a syllable");
    }
    let (a, xian): (Slots, Slots) = ([None, None, Some('ㄢ')], [Some('ㄒ'), Some('ㄧ'), Some('ㄢ')]);
    assert!(tl.row_id(&a).is_some() && tl.row_id(&xian).is_some());
    assert!(tl.row_id(&[Some('ㄒ'), Some('ㄧ'), None]).is_some(), "ㄒㄧ");
    // The contract counts 429 toneless syllables; without the 14 lone initials that are symbol entries, 415 are left.
    assert_eq!(tl.legal_len(), 415);
}

#[test]
fn every_syllable_has_a_toneless_id_and_a_tone() {
    let lex = lexicon();
    assert_eq!(lex.tl_syllable("ㄏㄠˇ").map(|(_, t)| t), Some(3));
    assert_eq!(lex.tl_syllable("ㄇㄚ˙").map(|(_, t)| t), Some(5));
    assert_eq!(lex.tl_syllable("ㄇㄚ").map(|(_, t)| t), Some(1));
    assert_eq!(lex.tl_syllable("ㄇㄚˊ").map(|(id, _)| id), lex.tl_syllable("ㄇㄚ").map(|(id, _)| id), "the same toneless syllable");
    assert!(lex.tl_syllable("ㄅㄛㄛ").is_none());
}
