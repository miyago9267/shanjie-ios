//! Fluent-input contract (docs/contracts/fluent-input.md) section 2, `fluency` 1: symbol order is free and a one-tone syllable
//! ends when the next symbol cannot join it. Real lexicon, no model; fails loudly without it, never skips.
use core::engine::*;
use core::syllable::{slot_of, SylTable};
use core::Lexicon;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const L: Layout = Layout::Standard;
const TONES: &str = "ˊˇˋ˙";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn lex() -> Arc<Lexicon> {
    static S: OnceLock<Arc<Lexicon>> = OnceLock::new();
    S.get_or_init(|| load_lexicon_packs(&root().join("data/lexicon"), None).unwrap().0).clone()
}

fn engine(level: u8) -> Engine {
    // SHANJIE_LIVENESS_LM=1: the iOS setup with its language model (slow; the rules under test do not depend on it).
    let mut e = if std::env::var_os("SHANJIE_LIVENESS_LM").is_some() {
        let mut e = Engine::new(&root().join("data/lexicon"), L).unwrap();
        e.clear_digit_table();
        e.load_lm_without_trigram(&root().join("data/lm/bigram.sjlm")).expect("data/lm/bigram.sjlm");
        e
    } else {
        Engine::with_lexicon(lex(), L)
    };
    e.set_prediction(false).unwrap();
    assert!(e.set_fluency(level).is_some());
    e
}

/// One key per char: a symbol, a tone mark, or `' '` for Space (the first tone).
fn key_of(c: char) -> Key {
    if c == ' ' {
        return Key::new(KeyKind::Space);
    }
    let ch = L.key_of_symbol(c).or_else(|| L.key_of_tone(c)).expect("a Zhuyin symbol or tone mark");
    Key::ch(ch, 0)
}

fn type_str(e: &mut Engine, s: &str) {
    s.chars().for_each(|c| assert!(e.key(key_of(c)).unwrap().handled, "key {c:?} was not handled"));
}

fn bs(e: &mut Engine) {
    assert!(e.key(Key::new(KeyKind::Backspace)).unwrap().handled);
}

fn perms(v: &[char]) -> Vec<Vec<char>> {
    if v.len() <= 1 {
        return vec![v.to_vec()];
    }
    (0..v.len())
        .flat_map(|i| {
            let mut rest = v.to_vec();
            let x = rest.remove(i);
            perms(&rest).into_iter().map(move |mut p| {
                p.insert(0, x);
                p
            })
        })
        .collect()
}

/// The lexicon's real syllables with a tone: the names `SylTable` accepts, minus the fake single-symbol ones the lexicon
/// holds for punctuation-like entries (only ㄓㄔㄕㄖㄗㄘㄙ are syllables alone).
fn real_syllables() -> Vec<String> {
    let l = lex();
    let mut v: Vec<String> = l
        .syllable_names()
        .filter(|n| SylTable::new(std::iter::once(*n)).extends(&[None; 3]))
        .filter(|n| n.chars().filter(|c| slot_of(*c).is_some()).count() > 1 || n.chars().any(|c| "ㄓㄔㄕㄖㄗㄘㄙ".contains(c)) || n.chars().any(|c| slot_of(c).is_some_and(|s| s > 0)))
        .filter(|n| !l.entries(&[n.to_string()]).is_empty())
        .map(str::to_string)
        .collect();
    v.sort();
    v
}

#[test]
fn every_permutation_of_a_syllable_gives_it_at_fluency_0_and_1() {
    let sylls = real_syllables();
    assert_eq!(sylls.len(), 1413 - 14, "1,413 toned syllables less the 14 lone initials that are no syllable");
    for level in [0, 1] {
        let mut e = engine(level);
        for syl in &sylls {
            let symbols: Vec<char> = syl.chars().filter(|c| !TONES.contains(*c)).collect();
            let tone = syl.chars().find(|c| TONES.contains(*c));
            for p in perms(&symbols) {
                let keys: String = p.iter().collect::<String>() + &tone.map_or(" ".to_string(), |t| t.to_string());
                type_str(&mut e, &keys);
                assert_eq!(e.readings(), [syl.clone()], "fluency {level}, keys {keys:?}");
                bs(&mut e);
                assert!(e.readings().is_empty());
            }
        }
    }
}

#[test]
fn a_same_column_symbol_ends_a_one_tone_syllable() {
    // ㄊㄚ ㄇㄣ˙ without the space in between: ㄇ meets ㄊ's column and ㄊㄚ is a syllable (他).
    let mut e = engine(1);
    type_str(&mut e, "ㄊㄚㄇㄣ˙");
    assert_eq!(e.readings(), ["ㄊㄚ", "ㄇㄣ˙"]);
    let mut e = engine(1);
    type_str(&mut e, "ㄇㄚㄇㄚ˙");
    assert_eq!(e.readings(), ["ㄇㄚ", "ㄇㄚ˙"]);
    // Strict: the second ㄇ replaces nothing new (ㄇ is in ㄇ's column), the first syllable is lost.
    let mut e = engine(0);
    type_str(&mut e, "ㄊㄚㄇㄣ˙");
    assert_eq!(e.readings(), ["ㄇㄣ˙"]);
}

#[test]
fn a_symbol_that_cannot_extend_ends_a_one_tone_syllable() {
    // ㄧ is a syllable (一); ㄧ then ㄨ meets the occupied medial column.
    let mut e = engine(1);
    type_str(&mut e, "ㄧㄨ");
    assert_eq!(e.readings(), ["ㄧ", "ㄨ"]);
    // ㄏㄠ then ㄚ: ㄚ's column (final) is occupied, ㄏㄠ is a syllable.
    let mut e = engine(1);
    type_str(&mut e, "ㄏㄠㄚ");
    assert_eq!(e.readings(), ["ㄏㄠ", "ㄚ"]);
}

#[test]
fn replacement_stays_when_the_pending_symbols_are_not_a_one_tone_syllable() {
    let l = lex();
    assert!(l.entries(&["ㄍㄧ".to_string()]).is_empty(), "the test needs ㄍㄧ to be no syllable");
    for level in [0, 1] {
        let mut e = engine(level);
        type_str(&mut e, "ㄍㄧㄎ");
        assert_eq!(e.readings(), ["ㄎㄧ"], "fluency {level}");
        // A lone initial that is no real syllable is replaced, not finished.
        let mut e = engine(level);
        type_str(&mut e, "ㄅㄉ");
        assert_eq!(e.readings(), ["ㄉ"], "fluency {level}");
    }
}

#[test]
fn a_free_column_that_can_extend_takes_the_symbol() {
    // ㄧ is a syllable, but ㄅ's column is free and ㄅㄧ exists: the symbol goes in.
    let mut e = engine(1);
    type_str(&mut e, "ㄧㄅ");
    assert_eq!(e.readings(), ["ㄅㄧ"]);
    // A free column where nothing extends and the pending symbols are no syllable: today's behaviour, the symbol goes in.
    let mut e = engine(1);
    type_str(&mut e, "ㄍㄧㄚ");
    assert_eq!(e.readings(), ["ㄍㄧㄚ"], "ㄍㄧ is no syllable; ㄚ's column is free and nothing extends: it goes in");
}

#[test]
fn backspace_takes_the_last_symbol_and_then_the_finished_syllable() {
    let mut e = engine(1);
    type_str(&mut e, "ㄊㄚㄇ");
    assert_eq!(e.readings(), ["ㄊㄚ", "ㄇ"]);
    bs(&mut e);
    assert_eq!(e.readings(), ["ㄊㄚ"]);
    bs(&mut e);
    assert!(e.readings().is_empty());
}

#[test]
fn fluency_0_is_the_strict_engine() {
    let mut e = engine(0);
    type_str(&mut e, "ㄊㄚㄇ");
    assert_eq!(e.readings(), ["ㄇㄚ"]);
}

#[test]
fn set_fluency_takes_0_and_1_only_for_now() {
    let mut e = engine(0);
    for bad in [2, 3, 4, u8::MAX] {
        assert!(e.set_fluency(bad).is_none());
        assert_eq!(e.fluency(), 0, "state unchanged after {bad}");
    }
    assert!(e.set_fluency(1).is_some() && e.fluency() == 1);
    assert!(e.set_fluency(2).is_none() && e.fluency() == 1);
}

// ---- key liveness (contract section 1): no key of a non-empty composition is "handled" and then does nothing, except the listed ones.

/// The engine's current snapshot (what the shell would show) and units, without a key.
fn state(e: &mut Engine) -> (Output, Vec<String>, String) {
    (e.set_fluency(e.fluency()).unwrap(), e.readings(), e.unfinished())
}

/// Presses `k`; `Some(output)` when it was handled, did not commit, and left the state exactly as it was.
fn does_nothing(e: &mut Engine, k: Key) -> bool {
    let before = state(e);
    let o = e.key(k).unwrap();
    o.handled && o.commit.is_empty() && state(e) == before
}

/// Exception 1: a tone key or Space on an unfinished syllable that is no syllable of the lexicon with that tone.
#[test]
fn liveness_exception_1_a_tone_that_makes_no_syllable_does_nothing() {
    let l = lex();
    assert!(l.entries(&["ㄍㄧˇ".to_string()]).is_empty() && l.entries(&["ㄍㄧ".to_string()]).is_empty(), "the test needs ㄍㄧ to be no syllable");
    for level in [0, 1] {
        for k in [key_of('ˇ'), key_of(' ')] {
            let mut e = engine(level);
            type_str(&mut e, "ㄍㄧ");
            assert!(does_nothing(&mut e, k), "fluency {level}");
            assert_eq!(e.unfinished(), "ㄍㄧ");
        }
    }
}

/// Exception 2: with an unfinished syllable, the cursor and window keys fall through and do nothing (`fluency` 0 and 1).
#[test]
fn liveness_exception_2_navigation_keys_do_nothing_on_an_unfinished_syllable() {
    use KeyKind::*;
    for level in [0, 1] {
        for kind in [Left, Right, Up, Down, Home, End, Delete, Tab, PageUp, PageDown] {
            let mut e = engine(level);
            type_str(&mut e, "ㄊㄚ ㄍㄧ");
            let before = state(&mut e);
            let o = e.key(Key::new(kind)).unwrap();
            // PageUp and PageDown are only for the vertical window: they pass through (not handled); the others are handled.
            let passes = matches!(kind, PageUp | PageDown);
            assert!(o.handled != passes && o.commit.is_empty() && state(&mut e) == before, "fluency {level}, key {}", kind as u32);
            assert_eq!((e.unfinished(), e.readings().len()), ("ㄍㄧ".to_string(), 2));
        }
    }
}

/// Exception 4: a tone key with nothing unfinished to close does nothing; Backspace on an empty composition is not handled.
#[test]
fn liveness_exception_4_nothing_to_close_and_nothing_to_delete() {
    for level in [0, 1] {
        let mut e = engine(level);
        type_str(&mut e, "ㄊㄚ ");
        assert!(does_nothing(&mut e, key_of('ˇ')), "fluency {level}");
        let mut e = engine(level);
        let before = state(&mut e);
        let o = e.key(Key::new(KeyKind::Backspace)).unwrap();
        assert!(!o.handled && o.commit.is_empty() && state(&mut e) == before, "fluency {level}: Backspace passes through");
    }
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n as u64) as usize
    }
}

/// The keys of the liveness run: the 37 symbol keys, the four tone keys and the keys of `KeyKind` (the digits are symbol keys
/// in this layout, so they are also the candidate-window digits).
fn liveness_keys() -> Vec<Key> {
    use KeyKind::*;
    let symbols = "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ";
    symbols.chars().map(key_of).chain(TONES.chars().map(key_of)).chain([Space, Enter, Backspace, Esc, Left, Right, Up, Down, Home, End, Delete, Tab, PageUp, PageDown].map(Key::new)).collect()
}

fn tone_mark_of(k: Key) -> Option<&'static str> {
    match k.kind {
        KeyKind::Space => Some(""),
        KeyKind::Char => ["ˊ", "ˇ", "ˋ", "˙"].into_iter().find(|m| L.key_of_tone(m.chars().next().unwrap()) == Some(k.ch)),
        _ => None,
    }
}

fn key_name(k: Key) -> String {
    match (k.kind, tone_mark_of(k)) {
        (KeyKind::Char, Some(m)) => format!("tone{m}"),
        (KeyKind::Char, None) => "symbol".to_string(),
        (kind, _) => format!("kind{}", kind as u32),
    }
}

/// Why a key that did nothing is allowed to, or `None` when it is a stuck key. Labels 1, 2, 4 are the contract's exceptions
/// (section 1, keys with the unfinished syllable and the tone key with nothing to close); `edge` (nothing to move or delete at an
/// end of the composition, or in the candidate window) and `same` (a symbol typed over itself) are not in the contract's list.
fn no_op_reason(unfinished: &str, k: Key, before: &Output) -> Option<&'static str> {
    use KeyKind::*;
    let window = before.selected.is_some();
    let len16 = before.preedit.encode_utf16().count() as u32;
    let (at_start, at_end) = (before.cursor_utf16 == 0, before.cursor_utf16 == len16);
    if !unfinished.is_empty() {
        // A symbol typed again over itself replaces it with the same symbol (strict typing; at `fluency` 1 only when the pending
        // symbols are no one-tone syllable).
        if let (Char, Some((col, sym))) = (k.kind, L.symbol_of(k.ch)) {
            return unfinished.chars().any(|c| slot_of(c) == Some(col) && c == sym).then_some("same");
        }
        return match tone_mark_of(k) {
            Some(m) => lex().entries(&[format!("{unfinished}{m}")]).is_empty().then_some("1"),
            None => matches!(k.kind, Left | Right | Up | Down | Home | End | Delete | Tab | PageUp | PageDown).then_some("2"),
        };
    }
    if window {
        return Some("edge");
    }
    match k.kind {
        Char if tone_mark_of(k).is_some() => Some("4"),
        Left | Home | Backspace if at_start => Some("edge"),
        Right | End | Delete if at_end => Some("edge"),
        Up | Tab | PageUp | PageDown => Some("edge"),
        _ => None,
    }
}

/// 10^5 random key sequences per level on one engine (reset between): no panic, a snapshot that agrees with itself, no stuck key,
/// and Backspace presses enough always empty the composition. Returns how often each allowed no-op happened, by reason and key.
fn liveness(level: u8, sequences: usize, seed: u64) -> std::collections::BTreeMap<String, usize> {
    let keys = liveness_keys();
    let mut rng = Rng(seed);
    let mut e = engine(level);
    let mut seen = std::collections::BTreeMap::new();
    for n in 0..sequences {
        let mut o = e.reset(ResetMode::Discard);
        for step in 0..1 + rng.below(24) {
            let k = keys[rng.below(keys.len())];
            let (before, unfinished, units) = (o.clone(), e.unfinished(), e.readings());
            o = e.key(k).unwrap_or_else(|_| panic!("engine error: sequence {n} step {step}"));
            let after_units = e.readings();
            // The snapshot agrees with itself.
            let len16 = o.preedit.encode_utf16().count() as u32;
            assert!(o.cursor_utf16 <= len16, "sequence {n} step {step}: cursor past the preedit");
            assert!(o.selected.is_none_or(|s| s < o.candidates.len()) && o.first as usize + o.candidates.len() <= o.total.max(o.candidates.len() as u32) as usize, "sequence {n} step {step}: window fields");
            if !before.preedit.is_empty() && o.handled && o.commit.is_empty() && o.clone() == before && after_units == units && e.unfinished() == unfinished {
                match no_op_reason(&unfinished, k, &before) {
                    Some(why) => *seen.entry(format!("{why} {}", key_name(k))).or_insert(0) += 1,
                    None => panic!("stuck key {} (sequence {n} step {step}, unfinished {unfinished:?}, units {})", key_name(k), units.len()),
                }
            }
        }
        if n % 2 == 0 {
            // Esc closes an open window first, then clears everything.
            for _ in 0..2 {
                o = e.key(Key::new(KeyKind::Esc)).unwrap();
            }
        } else {
            // Backspace from the end: the unfinished symbols go first, then End, then one syllable per press.
            while !e.unfinished().is_empty() {
                o = e.key(Key::new(KeyKind::Backspace)).unwrap();
            }
            o = e.key(Key::new(KeyKind::End)).unwrap();
            for _ in 0..200 {
                if o.preedit.is_empty() {
                    break;
                }
                o = e.key(Key::new(KeyKind::Backspace)).unwrap();
            }
        }
        assert!(o.preedit.is_empty() && e.readings().is_empty(), "sequence {n}: Esc or Backspace did not empty the composition: {} chars, units {:?}, window {:?}", o.preedit.chars().count(), e.readings(), o.selected);
    }
    seen
}

#[test]
fn liveness_100k_random_sequences_per_level() {
    let n = std::env::var("SHANJIE_LIVENESS_N").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    for level in [0, 1] {
        let seen = liveness(level, n, 0x5EED_0000 + u64::from(level));
        println!("fluency {level}, {n} sequences, allowed no-ops by reason: {seen:?}");
    }
}
