//! E-E real-engine replay (docs/contracts/engine-replay.md): rows typed key by key through one `core::engine::Engine`
//! built like the App's `Shell.build()`, with the user's re-picks simulated on request. Numbers only come out of here.
use core::engine::{Engine, Key, KeyKind, Layout, Output, PACK_ACG, PAGE_SIZE};
use core::eval::{pyround, sha256_hex, Lenient};
use core::lm::Profile;
use core::syllable::slot_of;
use core::tinylm::RerankLevel;
use core::Syls;
use std::path::Path;
use std::time::{Duration, Instant};

/// When the small model's work is applied (contract section 3).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Rerank {
    /// After every key, before the next one: the model always finishes in time.
    Each,
    /// Only after the last typing key, before any re-pick or Enter: the user typed faster than the model.
    Final,
}

#[derive(Clone, Copy)]
pub struct Cfg {
    pub correct: bool,
    pub rerank: Rerank,
}

/// What one row produced. `first` is the preedit once the last reading key (and, with `Rerank::Final`, the one apply) is in.
#[derive(Default)]
pub struct RowOut {
    pub first: String,
    pub committed: String,
    pub ideal: usize,
    pub actual: usize,
    pub picks: usize,
    pub first_ok: bool,
    pub final_ok: bool,
    /// `--correct` was asked for and the committed text is still not the gold text (no pick could fix it).
    pub unfixable: bool,
    /// The engine committed text before Enter (the composition limit); `first` and `committed` include it.
    pub autocommit: bool,
    /// `apply_rerank` calls that changed the preedit (contract section 3).
    pub rewrites: usize,
    /// `apply_rerank` calls that returned `Some` after a typing key other than the last one.
    pub typing_applies: usize,
    /// The engine's units just before Enter (fluent-input contract section 1, `--readings`): finished syllables, then the unfinished symbols.
    pub readings: Vec<String>,
    /// Syllables the engine had committed on its own when `readings` was taken (they are not in it).
    pub committed_syls: usize,
    /// The time of each typing key (the engine call only).
    pub key_times: Vec<Duration>,
}

/// Which keys a reading is typed with (fluent-input contract section 1, `--feed`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feed {
    /// Symbols in standard order, then the tone key or Space (today's replay).
    Full,
    /// Symbols of each syllable shuffled, tone key or Space still last.
    Perm,
    /// A first-tone syllable gets no Space when another syllable follows it; the last one still gets its Space.
    Omit1,
    /// The way `fluency` 2 is typed: a tone key for a toned syllable, nothing for a first-tone one, never Space.
    Tones,
    /// No tone key and no Space at all.
    Toneless,
}

impl Feed {
    pub fn parse(s: &str) -> Option<Feed> {
        Some(match s {
            "full" => Feed::Full,
            "perm" => Feed::Perm,
            "omit1" => Feed::Omit1,
            "tones" => Feed::Tones,
            "toneless" => Feed::Toneless,
            _ => return None,
        })
    }
}

/// splitmix64: a seed that gives the same keys on every run and machine.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// The keys of one syllable: its symbols, then its tone key (Space for the first tone). `None` for a symbol without a key.
pub fn keys_of(syl: &str) -> Option<Vec<Key>> {
    let layout = Layout::Standard;
    let (mut v, mut toned) = (Vec::new(), false);
    for c in syl.chars() {
        match layout.key_of_tone(c) {
            Some(tk) => {
                v.push(Key::ch(tk, 0));
                toned = true;
            }
            None => v.push(Key::ch(layout.key_of_symbol(c)?, 0)),
        }
    }
    if !toned {
        v.push(Key::new(KeyKind::Space));
    }
    Some(v)
}

/// The keys of syllable `i` of `n` under `feed`; `rng` is only used by `Feed::Perm`.
fn feed_keys(syl: &str, i: usize, n: usize, feed: Feed, rng: &mut Rng) -> Option<Vec<Key>> {
    if feed == Feed::Full {
        return keys_of(syl);
    }
    let layout = Layout::Standard;
    let (mut syms, mut tone) = (Vec::new(), None);
    for c in syl.chars() {
        match layout.key_of_tone(c) {
            Some(tk) => tone = Some(tk),
            None => syms.push(Key::ch(layout.key_of_symbol(c)?, 0)),
        }
    }
    if feed == Feed::Perm {
        for j in (1..syms.len()).rev() {
            syms.swap(j, (rng.next() % (j as u64 + 1)) as usize);
        }
    }
    match (feed, tone) {
        (_, Some(tk)) if feed != Feed::Toneless => syms.push(Key::ch(tk, 0)),
        (Feed::Perm, None) => syms.push(Key::new(KeyKind::Space)),
        (Feed::Omit1, None) if i + 1 == n => syms.push(Key::new(KeyKind::Space)),
        _ => {}
    }
    Some(syms)
}

/// Keys to bring the cursor from `from` to `to` among `n` syllables: Left/Right one syllable each, or Home / End first.
pub fn move_cost(from: usize, to: usize, n: usize) -> usize {
    from.abs_diff(to).min(1 + to).min(1 + (n - to))
}

/// The engine as `Shell.build()` makes it (acg pack, bigram, small model, demotion on, rerank on at the standard level, profile),
/// except the two recorded deviations: prediction row off and English mixing off (the word list is never loaded). `tinylm` None is
/// `--no-tinylm`. `learn` turns learning on once, in memory only (no learning file is opened).
pub fn build_engine(root: &Path, lm: &Path, tinylm: Option<&Path>, profile: Profile, learn: bool) -> Result<Engine, String> {
    fn fail<T>(what: &'static str) -> impl Fn(T) -> String {
        move |_| format!("{what} failed")
    }
    let packs = root.join("data/packs");
    let mut e = Engine::new_with_packs(&root.join("data/lexicon"), Layout::Standard, Some((&packs, PACK_ACG))).map_err(fail("engine build"))?;
    e.load_lm(lm).map_err(fail("load_lm"))?;
    if let Some(t) = tinylm {
        e.load_tinylm(t).map_err(fail("load_tinylm"))?;
    }
    e.set_demote(true).map_err(fail("set_demote"))?;
    e.set_prediction(false).map_err(fail("set_prediction"))?;
    e.set_abbreviation(false).map_err(fail("set_abbreviation"))?;
    e.set_english(false).map_err(fail("set_english"))?;
    e.set_candidate_vertical(false).map_err(fail("set_candidate_vertical"))?;
    e.set_rerank(true).map_err(fail("set_rerank"))?;
    e.set_rerank_level(RerankLevel::Standard).map_err(fail("set_rerank_level"))?;
    e.set_profile(profile).map_err(fail("set_profile"))?;
    if learn {
        e.set_learning(true);
    }
    Ok(e)
}

/// The engine as `ios/Keyboard/Engine.swift` makes it (fluent-input contract section 1, `--ios`): no word pack, no trigram table, no
/// small model, no rerank, no digit table; the same row, English and window settings as `build_engine`.
pub fn build_engine_ios(root: &Path, lm: &Path, profile: Profile, learn: bool) -> Result<Engine, String> {
    fn fail<T>(what: &'static str) -> impl Fn(T) -> String {
        move |_| format!("{what} failed")
    }
    let mut e = Engine::new(&root.join("data/lexicon"), Layout::Standard).map_err(fail("engine build"))?;
    e.clear_digit_table();
    e.load_lm_without_trigram(lm).map_err(fail("load_lm"))?;
    e.set_demote(true).map_err(fail("set_demote"))?;
    e.set_prediction(false).map_err(fail("set_prediction"))?;
    e.set_abbreviation(false).map_err(fail("set_abbreviation"))?;
    e.set_english(false).map_err(fail("set_english"))?;
    e.set_candidate_vertical(false).map_err(fail("set_candidate_vertical"))?;
    e.set_profile(profile).map_err(fail("set_profile"))?;
    if learn {
        e.set_learning(true);
    }
    Ok(e)
}

struct Session<'a> {
    e: &'a mut Engine,
    cur: Option<Output>,
    /// Text the engine committed before Enter (the composition limit, digit rows), in order.
    committed: String,
    /// The longest word a window can list: no window further right than `i` + this can cover `i`.
    max_len: usize,
    keys: usize,
    rewrites: usize,
    cursor: usize,
    n: usize,
    key_times: Vec<Duration>,
}

impl Session<'_> {
    fn cur(&self) -> &Output {
        self.cur.as_ref().expect("a key was pressed")
    }

    /// One key into the engine; only `counted` keys are part of `actual` (a scanning key is the tool looking, not the user typing).
    fn press(&mut self, k: Key, counted: bool) -> Result<(), String> {
        let o = self.e.key(k).map_err(|_| "engine error on a key".to_string())?;
        if k.kind != KeyKind::Enter {
            self.committed.push_str(&o.commit);
        }
        self.cur = Some(o);
        self.keys += counted as usize;
        Ok(())
    }

    /// take, run, apply; whether `apply_rerank` returned `Some`. A change of the preedit counts as a rewrite.
    fn rerank(&mut self) -> Result<bool, String> {
        let Some(job) = self.e.take_rerank_job() else { return Ok(false) };
        let before = self.cur().preedit.clone();
        match self.e.apply_rerank(job.run()).map_err(|_| "engine error on apply_rerank".to_string())? {
            Some(o) => {
                self.rewrites += (o.preedit != before) as usize;
                self.cur = Some(o);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn move_to(&mut self, j: usize) -> Result<(), String> {
        while self.cursor < j {
            self.press(Key::new(KeyKind::Right), false)?;
            self.cursor += 1;
        }
        while self.cursor > j {
            self.press(Key::new(KeyKind::Left), false)?;
            self.cursor -= 1;
        }
        Ok(())
    }

    /// Contract section 4: from j = i + 1 rightwards (to `i` + the longest word at most), the first cursor position whose window lists an
    /// item that covers syllable `i` and spells the gold text over its own span; `(j, index in the window's list)`. The scan opens
    /// windows page by page up to the first such item and closes them (uncounted); it leaves the cursor at the position it stopped on.
    fn find(&mut self, i: usize, gold: &[char], len: &Lenient) -> Result<Option<(usize, usize)>, String> {
        for j in i + 1..=self.n.min(i + self.max_len) {
            self.move_to(j)?;
            self.press(Key::new(KeyKind::Space), false)?;
            if self.cur().selected.is_none() {
                continue;
            }
            let (total, mut hit) = (self.cur().total as usize, None);
            loop {
                let (first, page) = (self.cur().first as usize, &self.cur().candidates);
                hit = hit.or_else(|| {
                    page.iter().position(|w| {
                        let l = w.chars().count();
                        l >= j - i && l <= j && len.apply(w) == len.apply(&gold[j - l..j].iter().collect::<String>())
                    })
                    .map(|k| first + k)
                });
                if hit.is_some() || first + page.len() >= total {
                    break;
                }
                self.press(Key::new(KeyKind::Space), false)?; // next page
            }
            self.press(Key::new(KeyKind::Esc), false)?;
            if let Some(idx) = hit {
                return Ok(Some((j, idx)));
            }
        }
        Ok(None)
    }

    /// Contract section 4: re-pick until the shown text is the gold text (lenient) or no pick can fix the first difference. `gold` is
    /// the part of the sentence the composition holds (what was committed before is not in it), the cursor is at its end.
    fn correct(&mut self, gold: &str, len: &Lenient, rerank: Rerank, picks: &mut usize) -> Result<(), String> {
        let (graw, gl): (Vec<char>, Vec<char>) = (gold.chars().collect(), len.apply(gold).chars().collect());
        for _ in 0..2 * graw.len() {
            let shown: Vec<char> = len.apply(&self.cur().preedit).chars().collect();
            if shown == gl {
                break;
            }
            // One character per syllable is how a pick's span is known; any other length is not fixable.
            if shown.len() != gl.len() || gl.len() != self.n {
                break;
            }
            let i = (0..gl.len()).find(|&i| shown[i] != gl[i]).expect("the texts differ");
            let (c0, before) = (self.cursor, self.cur().preedit.clone());
            let Some((j, idx)) = self.find(i, &graw, len)? else { break };
            self.keys += move_cost(c0, j, self.n);
            self.press(Key::new(KeyKind::Space), true)?; // opens on the first page
            for _ in 0..idx / PAGE_SIZE {
                self.press(Key::new(KeyKind::Space), true)?;
            }
            self.press(Key::ch(char::from(b'1' + (idx % PAGE_SIZE) as u8), 0), true)?; // a digit chooses on the spot
            *picks += 1;
            if rerank == Rerank::Each {
                self.rerank()?;
            }
            // A pick that leaves the text as it was would be chosen again: not fixable.
            if self.cur().preedit == before {
                break;
            }
        }
        Ok(())
    }
}

/// One row through `e` (contract section 1): left context, the reading key by key, the first display, the re-picks when asked for, Enter.
/// `gold` is the row's sentence, compared with the display through `len`. Errors carry no row text. Text the engine commits while the
/// row is typed (composition limit) is kept: it is part of the first display and of what is sent, and no pick can change it.
pub fn replay_row(e: &mut Engine, ctx: &str, syls: &[String], gold: &str, len: &Lenient, cfg: Cfg) -> Result<RowOut, String> {
    replay_row_feed(e, ctx, syls, gold, len, cfg, Feed::Full, 0)
}

/// `replay_row` with the keys of `feed`; `seed` is this row's (the random feeds only).
#[allow(clippy::too_many_arguments)]
pub fn replay_row_feed(e: &mut Engine, ctx: &str, syls: &[String], gold: &str, len: &Lenient, cfg: Cfg, feed: Feed, seed: u64) -> Result<RowOut, String> {
    let mut rng = Rng(seed);
    let per: Vec<Vec<Key>> = syls
        .iter()
        .enumerate()
        .map(|(i, s)| feed_keys(s, i, syls.len(), feed, &mut rng).ok_or("a reading symbol has no key"))
        .collect::<Result<_, _>>()?;
    // ends[s]: how many keys have been typed when syllable s is complete.
    let ends: Vec<usize> = per.iter().scan(0, |t, v| Some({ *t += v.len(); *t })).collect();
    let typed = per.concat();
    if typed.is_empty() {
        return Err("a row without a reading".into());
    }
    e.set_left_context(ctx);
    let max_len = e.max_window_len();
    let mut s = Session { e, cur: None, committed: String::new(), max_len, keys: 0, rewrites: 0, cursor: syls.len(), n: syls.len(), key_times: Vec::new() };
    let (mut out, last) = (RowOut { ideal: typed.len() + 1, ..RowOut::default() }, typed.len() - 1);
    let mut committed_syls = 0; // syllables complete when the engine last committed
    for (k, key) in typed.into_iter().enumerate() {
        let before = s.committed.len();
        let t = Instant::now();
        s.press(key, true)?;
        s.key_times.push(t.elapsed());
        if s.committed.len() > before {
            out.autocommit = true;
            committed_syls = ends.iter().filter(|&&end| end <= k + 1).count();
        }
        match cfg.rerank {
            Rerank::Each => out.typing_applies += (s.rerank()? && k != last) as usize,
            Rerank::Final if k == last => {
                s.rerank()?;
            }
            Rerank::Final => {}
        }
    }
    out.first = format!("{}{}", s.committed, s.cur().preedit);
    out.first_ok = len.apply(&out.first) == len.apply(gold);
    (out.readings, out.committed_syls) = (s.e.readings(), committed_syls);
    // The composition holds the syllables after the committed ones, one character each, or the row is not fixable.
    if cfg.correct && s.committed.chars().count() == committed_syls {
        (s.n, s.cursor) = (syls.len() - committed_syls, syls.len() - committed_syls);
        let rest: String = gold.chars().skip(committed_syls).collect();
        s.correct(&rest, len, cfg.rerank, &mut out.picks)?;
    }
    s.press(Key::new(KeyKind::Enter), true)?;
    out.committed = format!("{}{}", s.committed, s.cur().commit);
    out.final_ok = len.apply(&out.committed) == len.apply(gold);
    out.unfixable = cfg.correct && !out.final_ok;
    (out.actual, out.rewrites, out.key_times) = (s.keys, s.rewrites, std::mem::take(&mut s.key_times));
    Ok(out)
}

/// Fluent-input contract section 2, acceptance 3: the boundaries after a first-tone syllable that another syllable follows (gold
/// readings), and how many of them are a boundary in the engine's `readings` (compared by symbols typed so far, tone marks not
/// counted). A boundary inside text the engine committed on its own is left out. Returns (boundaries, found).
pub fn boundary_stats(rows: &[RowIn], outs: &[RowOut]) -> (usize, usize) {
    let symbols = |s: &str| s.chars().filter(|&c| slot_of(c).is_some()).count();
    let (mut total, mut found) = (0, 0);
    for (r, o) in rows.iter().zip(outs) {
        let mut ends = std::collections::HashSet::new();
        let mut at: usize = r.syls[..o.committed_syls].iter().map(|s| symbols(s)).sum();
        for u in &o.readings {
            at += symbols(u);
            ends.insert(at);
        }
        let mut at = 0;
        for (i, s) in r.syls.iter().enumerate() {
            at += symbols(s);
            let first_tone = !s.chars().any(|c| Layout::Standard.key_of_tone(c).is_some());
            if first_tone && i + 1 < r.syls.len() && i >= o.committed_syls {
                total += 1;
                found += ends.contains(&at) as usize;
            }
        }
    }
    (total, found)
}

/// `--readings` file: one line per row, the engine's units before Enter separated by spaces.
pub fn format_readings(outs: &[RowOut]) -> String {
    outs.iter().map(|o| format!("{}\n", o.readings.join(" "))).collect()
}

/// (p95, max) of the typing keys of all rows, in milliseconds; the p95 index is the replay test's (`len * 95 / 100`).
pub fn key_latency_ms(outs: &[RowOut]) -> Option<(f64, f64)> {
    let mut t: Vec<Duration> = outs.iter().flat_map(|o| o.key_times.iter().copied()).collect();
    t.sort();
    Some((t.get(t.len() * 95 / 100)?.as_secs_f64() * 1e3, t.last()?.as_secs_f64() * 1e3))
}

pub struct RowIn {
    pub ctx: String,
    pub sent: String,
    pub syls: Syls,
}

/// `--keys` file: `row\tideal\tactual\tpicks\tfinal_ok\trewrites` per row, rows counted from 1. Digits only.
pub fn format_keys(outs: &[RowOut]) -> String {
    outs.iter()
        .enumerate()
        .map(|(i, o)| format!("{}\t{}\t{}\t{}\t{}\t{}\n", i + 1, o.ideal, o.actual, o.picks, o.final_ok as u8, o.rewrites))
        .collect()
}

/// The summary line (contract section 5). `tag` is the setting marker, `gold_chars` the characters of all gold sentences.
/// `top1_sha256` is that of `shanjie-eval --lm`: the first displays joined with `\n`.
pub fn summary_line(name: &str, tag: &str, outs: &[RowOut], gold_chars: usize) -> String {
    let sum = |f: fn(&RowOut) -> usize| outs.iter().map(f).sum::<usize>();
    let (ideal, actual, picks) = (sum(|o| o.ideal), sum(|o| o.actual), sum(|o| o.picks));
    let ratio = |a: usize, b: usize, k: f64| if b == 0 { 0.0 } else { a as f64 * k / b as f64 };
    let firsts: Vec<&str> = outs.iter().map(|o| o.first.as_str()).collect();
    format!(
        "## {name}  {tag}  {{'n': {}, 'top1': {}, 'final_ok': {}, 'unfixable': {}, 'autocommits': {}, 'kyss': {}, 'picks_per100': {}, 'rewrites': {}, 'top1_sha256': '{}'}}",
        outs.len(),
        sum(|o| o.first_ok as usize),
        sum(|o| o.final_ok as usize),
        sum(|o| o.unfixable as usize),
        sum(|o| o.autocommit as usize),
        pyround(ratio(ideal, actual, 1.0), 4),
        pyround(ratio(picks, gold_chars, 100.0), 2),
        sum(|o| o.rewrites),
        sha256_hex(firsts.join("\n").as_bytes()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(v: &[Key]) -> String {
        v.iter().map(|k| if k.kind == KeyKind::Space { '_' } else { k.ch }).collect()
    }
    fn row(feed: Feed, seed: u64, syls: &[&str]) -> Vec<String> {
        let mut rng = Rng(seed);
        (0..syls.len()).map(|i| chars(&feed_keys(syls[i], i, syls.len(), feed, &mut rng).unwrap())).collect()
    }

    #[test]
    fn feeds_type_the_readings_as_documented() {
        let s = ["ㄊㄚ", "ㄇㄣ˙", "ㄏㄠˇ"]; // standard keys: ㄊ=w ㄚ=8 ㄇ=a ㄣ=p ˙=7 ㄏ=c ㄠ=l ˇ=3... checked through the key table
        let k = |sy: &str| chars(&keys_of(sy).unwrap());
        assert_eq!(row(Feed::Full, 1, &s), [k("ㄊㄚ"), k("ㄇㄣ˙"), k("ㄏㄠˇ")]);
        // Omit1: only the first-tone syllable that is not last loses its Space.
        assert_eq!(row(Feed::Omit1, 1, &s), [k("ㄊㄚ").trim_end_matches('_').to_string(), k("ㄇㄣ˙"), k("ㄏㄠˇ")]);
        assert_eq!(row(Feed::Omit1, 1, &["ㄊㄚ"]), [k("ㄊㄚ")], "a last first-tone syllable keeps its Space");
        // Tones: tone key kept, Space never. Toneless: neither.
        assert_eq!(row(Feed::Tones, 1, &s), [k("ㄊㄚ").trim_end_matches('_').to_string(), k("ㄇㄣ˙"), k("ㄏㄠˇ")]);
        assert_eq!(row(Feed::Toneless, 1, &s), [k("ㄊㄚ").trim_end_matches('_').to_string(), k("ㄇㄣ˙")[..2].to_string(), k("ㄏㄠˇ")[..2].to_string()]);
    }

    #[test]
    fn perm_is_seeded_keeps_the_tone_last_and_really_shuffles() {
        let s = ["ㄍㄨㄛˇ", "ㄏㄨㄞˋ", "ㄒㄧㄠ"];
        assert_eq!(row(Feed::Perm, 7, &s), row(Feed::Perm, 7, &s));
        let full = row(Feed::Full, 0, &s);
        assert!((0..20).any(|seed| row(Feed::Perm, seed, &s) != full), "some seed must reorder");
        for seed in 0..20 {
            for (p, f) in row(Feed::Perm, seed, &s).iter().zip(&full) {
                let (mut a, mut b): (Vec<char>, Vec<char>) = (p.chars().collect(), f.chars().collect());
                assert_eq!(a.pop(), b.pop(), "tone key or Space stays last");
                a.sort();
                b.sort();
                assert_eq!(a, b, "same keys");
            }
        }
    }

    #[test]
    fn boundaries_are_counted_by_symbols_after_first_tone_syllables() {
        let rows = vec![RowIn { ctx: String::new(), sent: String::new(), syls: ["ㄊㄚ", "ㄇㄣ˙", "ㄇㄚ", "ㄇㄚ˙"].map(String::from).to_vec() }];
        let out = |units: &[&str]| RowOut { readings: units.iter().map(|s| s.to_string()).collect(), ..RowOut::default() };
        // Gold boundaries after ㄊㄚ (2 symbols) and after ㄇㄚ (5 symbols): two; ㄇㄣ˙ has a tone, so not one.
        assert_eq!(boundary_stats(&rows, &[out(&["ㄊㄚ", "ㄇㄣ˙", "ㄇㄚ", "ㄇㄚ˙"])]), (2, 2));
        assert_eq!(boundary_stats(&rows, &[out(&["ㄇㄚ", "ㄇㄚ˙"])]), (2, 1), "a lost syllable shifts the later boundaries");
        assert_eq!(boundary_stats(&rows, &[out(&["ㄊㄚㄇㄣ˙", "ㄇㄚ", "ㄇㄚ˙"])]), (2, 1));
    }
}
