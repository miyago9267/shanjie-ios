//! The offline lattice path of `shanjie-eval --lm` (fluent-input contract section 3, R2): a row's reading typed as `--feed` says, one
//! item at a time, through the same pure `finalize_tail` the engine will call, then decoded by `decode_lattice`. No engine in it.
use crate::replay::{feed_units, Feed, Rng, Unit};
use core::lm::{decode_lattice, finalize_tail, End, LatticeCfg, Lm};
use core::lm::CappedLexicon;
use core::syllable::{lattice_edges, slot_of, Raw};
use std::time::{Duration, Instant};

/// How a row is fed to the lattice: each syllable as a decided syllable (`Syl`, R2 rule 1: the tone is given, the first tone too), or as
/// typed keys under a replay feed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OfflineFeed {
    Syl,
    Keys(Feed),
}

impl OfflineFeed {
    /// `syl`, `tones`, `toneless`, `tonepart:P`; the replay feeds that press Space for a first tone have no lattice form.
    pub fn parse(s: &str) -> Option<OfflineFeed> {
        if s == "syl" {
            return Some(OfflineFeed::Syl);
        }
        match Feed::parse(s)? {
            f @ (Feed::Tones | Feed::Toneless | Feed::TonePart(_)) => Some(OfflineFeed::Keys(f)),
            _ => None,
        }
    }
}

impl std::fmt::Display for OfflineFeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OfflineFeed::Syl => write!(f, "syl"),
            OfflineFeed::Keys(Feed::Tones) => write!(f, "tones"),
            OfflineFeed::Keys(Feed::Toneless) => write!(f, "toneless"),
            OfflineFeed::Keys(Feed::TonePart(p)) => write!(f, "tonepart:{}", *p as f64 / 1e6),
            OfflineFeed::Keys(_) => write!(f, "other"),
        }
    }
}

/// The seed of row `i` (the replay's: one stream per row, independent of the rows before it).
pub fn row_seed(seed: u64, i: usize) -> u64 {
    seed ^ (i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// What one row gave.
#[derive(Default)]
pub struct LatRow {
    /// The surfaces of the paths that cover everything, best first (the best prefix path alone when nothing covers).
    pub paths: Vec<(f64, Vec<String>)>,
    pub covered: bool,
    /// Symbols (not tone keys) typed up to the end of each syllable of the best path, finalized ones included.
    pub ends: Vec<usize>,
    /// The lexicon names of the best path's syllables (an untyped tone as the path read it).
    pub reading: Vec<String>,
    /// Typed items, and the lattice edges summed over the keys (the tail's lattice after each key).
    pub keys: usize,
    pub edges: usize,
    /// The time of each key (decode and window, only with `timing`).
    pub times: Vec<Duration>,
}

fn symbols(name: &str) -> usize {
    name.chars().filter(|&c| slot_of(c).is_some()).count()
}

/// The items typed for `syls` under `feed`.
pub fn raw_of(syls: &[String], feed: OfflineFeed, seed: u64) -> Result<Vec<Raw>, String> {
    let OfflineFeed::Keys(f) = feed else { return Ok(syls.iter().cloned().map(Raw::Syl).collect()) };
    let mut rng = Rng(seed);
    let mut out = Vec::new();
    for (i, s) in syls.iter().enumerate() {
        for u in feed_units(s, i, syls.len(), f, &mut rng).ok_or("a reading symbol has no key")? {
            out.push(match u {
                Unit::Sym(c) => Raw::Sym(c),
                Unit::Tone(c) => Raw::Tone(c),
                Unit::Space => return Err("this feed presses Space".into()),
            });
        }
    }
    Ok(out)
}

/// One row: every item goes into the tail, then `finalize_tail` (each key), and after the last key the whole composition (the finalized
/// syllables and the tail) is decoded once; its best path is the row's top-1. With `timing` each key also costs the decode the engine
/// would run after it, and that time is recorded.
pub fn run_row(capped: &CappedLexicon, lm: &Lm, cfg: &LatticeCfg, syls: &[String], feed: OfflineFeed, seed: u64, timing: bool) -> Result<LatRow, String> {
    let raw = raw_of(syls, feed, seed)?;
    let (mut finalized, mut tail, mut row) = (Vec::<String>::new(), Vec::<Raw>::new(), LatRow::default());
    let joined = |finalized: &[String], tail: &[Raw]| -> Vec<Raw> { finalized.iter().cloned().map(Raw::Syl).chain(tail.iter().cloned()).collect() };
    if feed == OfflineFeed::Syl {
        tail = raw.clone();
        row.keys = raw.len();
    } else {
        for item in raw {
            let t = Instant::now();
            tail.push(item);
            row.keys += 1;
            row.edges += lattice_edges(&tail, capped.base()).iter().map(Vec::len).sum::<usize>();
            if timing {
                std::hint::black_box(decode_lattice(capped, &joined(&finalized, &tail), lm, cfg, End::Eos, None));
            }
            let (new, rest) = finalize_tail(&finalized, &tail, lm, capped, cfg);
            finalized.extend(new);
            tail = rest;
            if timing {
                row.times.push(t.elapsed());
            }
        }
    }
    let out = decode_lattice(capped, &joined(&finalized, &tail), lm, cfg, End::Eos, None);
    row.covered = !out.full.is_empty();
    let best = out.full.first().or(out.prefix.as_ref());
    if let Some(p) = best {
        let mut at = 0;
        row.ends = p.syllables().map(|(s, _)| { at += symbols(s); at }).collect();
        row.reading = p.syllables().map(|(s, _)| s.to_string()).collect();
    }
    row.paths = if row.covered {
        out.full.iter().map(|p| (p.score, p.words.iter().map(|w| w.word.to_string()).collect())).collect()
    } else {
        out.prefix.iter().map(|p| (p.score, p.words.iter().map(|w| w.word.to_string()).collect())).collect()
    };
    Ok(row)
}

/// The boundaries after a first-tone syllable that another syllable follows (gold readings, as the replay's `--readings` counts them
/// in fluent-input section 2) and how many of them the best path has: (boundaries, found), by symbols typed so far.
pub fn boundaries(syls: &[String], ends: &[usize]) -> (usize, usize) {
    let (mut total, mut found, mut at) = (0, 0, 0);
    for (i, s) in syls.iter().enumerate() {
        at += symbols(s);
        let first_tone = !s.chars().any(|c| "ˊˇˋ˙".contains(c));
        if first_tone && i + 1 < syls.len() {
            total += 1;
            found += ends.contains(&at) as usize;
        }
    }
    (total, found)
}

/// Why a row the syllable decoder gets right is lost on the lattice: the best path splits the symbols differently from the gold reading
/// (`Segmentation`), splits them the same way but reads a syllable with another tone or sound (`Tone`), or reads exactly the gold
/// reading and still picks other characters (`Ranking`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    Segmentation,
    Tone,
    Ranking,
}

pub fn cause(gold: &[String], row: &LatRow) -> Cause {
    let mut at = 0;
    let gold_ends: Vec<usize> = gold.iter().map(|s| { at += symbols(s); at }).collect();
    if gold_ends != row.ends {
        Cause::Segmentation
    } else if gold != row.reading.as_slice() {
        Cause::Tone
    } else {
        Cause::Ranking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syls(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn show(raw: &[Raw]) -> String {
        raw.iter()
            .map(|r| match r {
                Raw::Sym(c) | Raw::Tone(c) => c.to_string(),
                Raw::Syl(s) => format!("[{s}]"),
            })
            .collect()
    }

    #[test]
    fn feeds_make_the_documented_items() {
        let s = syls(&["ㄊㄚ", "ㄇㄣ˙", "ㄏㄠˇ"]);
        let f = |name: &str, seed| show(&raw_of(&s, OfflineFeed::parse(name).unwrap(), seed).unwrap());
        assert_eq!(f("syl", 0), "[ㄊㄚ][ㄇㄣ˙][ㄏㄠˇ]");
        assert_eq!(f("tones", 0), "ㄊㄚㄇㄣ˙ㄏㄠˇ");
        assert_eq!(f("toneless", 0), "ㄊㄚㄇㄣㄏㄠ");
        assert_eq!(f("tonepart:0", 3), "ㄊㄚㄇㄣㄏㄠ");
        assert_eq!(f("tonepart:1", 3), "ㄊㄚㄇㄣ˙ㄏㄠˇ");
        assert!(OfflineFeed::parse("full").is_none() && OfflineFeed::parse("omit1").is_none(), "Space has no lattice form");
    }

    #[test]
    fn tonepart_is_seeded_and_keeps_some_tones() {
        let s = syls(&["ㄏㄠˇ", "ㄇㄚˇ", "ㄊㄚˋ", "ㄇㄣˊ", "ㄅㄚ˙", "ㄌㄧˇ", "ㄍㄜˋ", "ㄧㄡˇ"]);
        let f = OfflineFeed::parse("tonepart:0.5").unwrap();
        let a = raw_of(&s, f, 11).unwrap();
        assert_eq!(a, raw_of(&s, f, 11).unwrap(), "the same seed, the same items");
        let tones = a.iter().filter(|r| matches!(r, Raw::Tone(_))).count();
        assert!(tones > 0 && tones < s.len(), "{tones} of {}", s.len());
        assert!((0..20).any(|k| raw_of(&s, f, k).unwrap() != a), "another seed gives other items");
    }

    #[test]
    fn boundaries_count_symbols_after_first_tone_syllables_only() {
        let s = syls(&["ㄊㄚ", "ㄇㄣ˙", "ㄇㄚ", "ㄇㄚ˙"]);
        assert_eq!(boundaries(&s, &[2, 4, 6, 8]), (2, 2));
        assert_eq!(boundaries(&s, &[4, 6, 8]), (2, 1));
    }

    #[test]
    fn a_lost_row_is_blamed_on_the_split_then_the_tone_then_the_ranking() {
        let gold = syls(&["ㄒㄧㄢ", "ㄕㄥ"]);
        let row = |ends: &[usize], reading: &[&str]| LatRow { ends: ends.to_vec(), reading: syls(reading), ..LatRow::default() };
        assert_eq!(cause(&gold, &row(&[2, 3, 5], &["ㄒㄧ", "ㄢ", "ㄕㄥ"])), Cause::Segmentation);
        assert_eq!(cause(&gold, &row(&[3, 5], &["ㄒㄧㄢˋ", "ㄕㄥ"])), Cause::Tone);
        assert_eq!(cause(&gold, &row(&[3, 5], &["ㄒㄧㄢ", "ㄕㄥ"])), Cause::Ranking);
    }
}
