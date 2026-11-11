//! Smart automix: one song turns into the next like a DJ mix, with light signal processing
//! (no models):
//! - cue-out: the change starts as soon as the song has faded out (below -28 dB for 400 ms in its
//!   last 15 seconds), or at the mix window before its end;
//! - cue-in: the next song's silent start is skipped, so it comes in on its first sound (when
//!   "Skip silence" is on);
//! - equal-power gains (cos / sin), so the loudness holds through the change;
//! - bass swap: the outgoing song loses its lows (a high-pass rising to 200 Hz) while the
//!   incoming one brings its own, so the two basses don't clash;
//! - beats: the incoming song starts on one of the outgoing song's beats, and when their tempos
//!   are within 3% it's nudged to the same tempo while they overlap.
//!
//! The same rules as the phone app's `playback/Automix.kt`.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

/// Loudness blocks per second (one every 10 ms).
pub const BLOCKS_PER_SEC: usize = 100;
/// Seconds of loudness kept.
const KEEP_SECS: usize = 16;
/// Quieter than this (dB) for [`CUE_OUT_HOLD`] blocks: the song has faded out.
pub const CUE_OUT_DB: f32 = -28.0;
pub const CUE_OUT_HOLD: usize = 40;
/// The end of a song where a fade-out starts the change (seconds).
pub const WINDOW_SECS: f32 = 15.0;
/// Where the outgoing song's high-pass ends up (Hz).
pub const BASS_HZ: f32 = 200.0;
/// The most the incoming song's tempo is nudged (3%).
pub const MAX_NUDGE: f32 = 0.03;

/// The next song takes a moment to load: the mix is asked for this much earlier (seconds), so the
/// two overlap for the whole mix.
pub const LOAD_LEAD: f32 = 1.5;

/// The mix's length: the crossfade setting, or 6 seconds when that's off.
pub fn mix_secs(crossfade: u8) -> f32 {
    if crossfade == 0 { 6.0 } else { crossfade as f32 }
}

pub fn db(rms: f32) -> f32 {
    20.0 * rms.max(1e-6).log10()
}

/// The song has faded out: its last 400 ms are below -28 dB, and well below its own usual level
/// (a song that's quiet all along has to go quieter still).
pub fn faded_out(env: &[f32]) -> bool {
    if env.len() < CUE_OUT_HOLD * 3 {
        return false;
    }
    let mut levels: Vec<f32> = env.iter().map(|x| db(*x)).collect();
    let tail = levels.split_off(levels.len() - CUE_OUT_HOLD);
    levels.sort_by(|a, b| a.total_cmp(b));
    let usual = levels[levels.len() / 2];
    let threshold = CUE_OUT_DB.min(usual - 8.0);
    tail.iter().all(|l| *l < threshold)
}

/// The outgoing song's high-pass at `t` (0 to 1) through the mix: off at first, rising to
/// [`BASS_HZ`] by two thirds of the way (eased, so the bass leaves smoothly).
pub fn bass_cut(t: f32) -> f32 {
    let x = (t / 0.66).clamp(0.0, 1.0);
    let eased = x * x * (3.0 - 2.0 * x);
    if eased <= 0.0 { 0.0 } else { 20.0 + (BASS_HZ - 20.0) * eased }
}

/// Beats found in a loudness envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Beat {
    /// Blocks between beats.
    pub period: f32,
    /// Blocks from the envelope's last block back to its last beat.
    pub last: f32,
    /// How clearly it beats (0 to 1).
    pub confidence: f32,
}

impl Beat {
    pub fn bpm(&self) -> f32 {
        60.0 * BLOCKS_PER_SEC as f32 / self.period
    }
}

/// Tempo and beat phase: onsets (rises in loudness), their autocorrelation over 70 to 180 BPM,
/// then where the beats fall. `None` when there's no clear beat (or under 3 seconds of sound).
pub fn beat(env: &[f32]) -> Option<Beat> {
    if env.len() < 3 * BLOCKS_PER_SEC {
        return None;
    }
    let log: Vec<f32> = env.iter().map(|x| (x + 1e-4).ln()).collect();
    let mut onset: Vec<f32> = log.windows(2).map(|w| (w[1] - w[0]).max(0.0)).collect();
    let mean = onset.iter().sum::<f32>() / onset.len() as f32;
    for o in &mut onset {
        *o -= mean;
    }
    let n = onset.len();
    let energy: f32 = onset.iter().map(|o| o * o).sum();
    if energy <= 1e-9 {
        return None;
    }
    let corr = |lag: usize| -> f32 { (lag..n).map(|i| onset[i] * onset[i - lag]).sum::<f32>() / (n - lag) as f32 * n as f32 };
    let (lo, hi) = (BLOCKS_PER_SEC * 60 / 180, BLOCKS_PER_SEC * 60 / 70);
    let scores: Vec<f32> = (lo - 1..=hi + 1).map(corr).collect();
    let mut best = 1;
    for i in 1..scores.len() - 1 {
        if scores[i] > scores[best] {
            best = i;
        }
    }
    // Every second (or, in a triplet feel, every 1.5th) beat lines up as well as every beat: the
    // shorter period wins when it scores nearly as high (150 BPM, not 75; 122, not 81).
    let top = scores[best];
    for (num, den) in [(1, 2), (2, 3)] {
        let lag = (lo - 1 + best) * num / den;
        if lag > lo && lag < hi {
            let i = (lag - 1 - (lo - 1)..=lag + 1 - (lo - 1)).max_by(|a, b| scores[*a].total_cmp(&scores[*b])).unwrap_or(best);
            if scores[i] >= 0.8 * top && i < best {
                best = i;
                break;
            }
        }
    }
    let confidence = scores[best] / energy;
    if confidence < 0.2 {
        return None;
    }
    // Between blocks: a parabola through the peak and its neighbours.
    let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
    let shift = if a - 2.0 * b + c != 0.0 { 0.5 * (a - c) / (a - 2.0 * b + c) } else { 0.0 };
    let period = (lo - 1 + best) as f32 + shift.clamp(-0.5, 0.5);
    // The phase: where a comb of beats `period` apart catches the most onsets.
    let mut last = 0.0;
    let mut most = f32::MIN;
    for p in 0..period.ceil() as usize {
        let mut sum = 0.0;
        let mut at = (n - 1) as f32 - p as f32;
        while at >= 0.0 {
            sum += onset[at.round() as usize];
            at -= period;
        }
        if sum > most {
            most = sum;
            last = p as f32;
        }
    }
    Some(Beat { period, last, confidence: confidence.min(1.0) })
}

/// The incoming song's speed factor to meet the outgoing song's tempo (counting double and half
/// time), when it's within 3%; `None` when they're too far apart to nudge.
pub fn nudge(out_bpm: f32, in_bpm: f32) -> Option<f32> {
    [0.5, 1.0, 2.0]
        .iter()
        .map(|k| out_bpm / (in_bpm * k))
        .min_by(|a, b| (a - 1.0).abs().total_cmp(&(b - 1.0).abs()))
        .filter(|r| (r - 1.0).abs() <= MAX_NUDGE)
}

// ---------------------------------------------------------------- live measurements

/// A ring of loudness blocks written by the audio thread and read by the app.
struct Ring {
    blocks: Vec<AtomicU32>,
    written: AtomicU64,
}

impl Ring {
    fn new() -> Ring {
        Ring { blocks: (0..KEEP_SECS * BLOCKS_PER_SEC).map(|_| AtomicU32::new(0)).collect(), written: AtomicU64::new(0) }
    }

    fn push(&self, rms: f32) {
        let n = self.written.load(Ordering::Relaxed);
        self.blocks[(n % self.blocks.len() as u64) as usize].store(rms.to_bits(), Ordering::Relaxed);
        self.written.store(n + 1, Ordering::Release);
    }

    /// The last `secs`, oldest first.
    fn last(&self, secs: f32) -> Vec<f32> {
        let n = self.written.load(Ordering::Acquire);
        let want = ((secs * BLOCKS_PER_SEC as f32) as u64).min(n).min(self.blocks.len() as u64);
        (n - want..n).map(|i| f32::from_bits(self.blocks[(i % self.blocks.len() as u64) as usize].load(Ordering::Relaxed))).collect()
    }
}

/// One song's measurements and controls, shared between its player (audio thread) and the app.
pub struct Meter {
    /// Its loudness as decoded (before the effects): for cue-out.
    level: Ring,
    /// Its loudness as played (after the effects), in step with [`frames`](Self::frames): for beats.
    played: Ring,
    /// Frames it has played (its own sample rate).
    frames: AtomicU64,
    /// The high-pass on it (Hz; 0 = off).
    highpass: AtomicU32,
    /// It has started sounding (its start gate opened).
    open: AtomicBool,
    rate: AtomicU32,
    /// The latest block's bass (below about 150 Hz), left and right levels (RMS): the moving
    /// backdrop and the needle meters.
    bass: AtomicU32,
    left: AtomicU32,
    right: AtomicU32,
    /// Its song has run out (gapless: the next one starts on that very frame).
    ended: AtomicBool,
    /// Silent frames it sent while its start gate was shut (the player counts them as played).
    waited: AtomicU64,
}

/// A gate that opens when the other song runs out, not at a frame (gapless playback).
pub const UNTIL_END: u64 = u64::MAX;

impl Meter {
    pub fn new() -> Arc<Meter> {
        Arc::new(Meter {
            level: Ring::new(),
            played: Ring::new(),
            frames: AtomicU64::new(0),
            highpass: AtomicU32::new(0),
            open: AtomicBool::new(false),
            rate: AtomicU32::new(48_000),
            bass: AtomicU32::new(0),
            left: AtomicU32::new(0),
            right: AtomicU32::new(0),
            ended: AtomicBool::new(false),
            waited: AtomicU64::new(0),
        })
    }

    /// Seconds of silence it sent before its start gate opened.
    pub fn waited_secs(&self) -> f32 {
        self.waited.load(Ordering::Acquire) as f32 / self.rate().max(1) as f32
    }

    /// The last `secs` of its loudness as decoded.
    pub fn level(&self, secs: f32) -> Vec<f32> {
        self.level.last(secs)
    }

    /// The last `secs` of its loudness as played.
    pub fn played(&self, secs: f32) -> Vec<f32> {
        self.played.last(secs)
    }

    pub fn set_highpass(&self, hz: f32) {
        self.highpass.store(hz.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    /// Frames played, and its sample rate.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }

    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    /// The latest bass, left and right levels (RMS, 0 to about 1).
    pub fn bands(&self) -> (f32, f32, f32) {
        let get = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        (get(&self.bass), get(&self.left), get(&self.right))
    }

    /// The frame where its next beat falls, at least `ahead` seconds from now; `None` without a
    /// clear beat.
    pub fn next_beat_frame(&self, ahead: f32) -> Option<(u64, Beat)> {
        let beat = beat(&self.played(12.0))?;
        let rate = self.rate.load(Ordering::Relaxed) as f32;
        let block = rate / BLOCKS_PER_SEC as f32;
        let written = self.played.written.load(Ordering::Acquire) as f32;
        let now = self.frames.load(Ordering::Acquire) as f32;
        let step = beat.period * block;
        let mut at = (written - 1.0 - beat.last) * block;
        // The first beat after now, either way.
        while at < now + ahead * rate {
            at += step;
        }
        while at - step >= now + ahead * rate {
            at -= step;
        }
        Some((at as u64, beat))
    }
}

/// Sums squares into 10 ms blocks.
struct Blocks {
    sum: f32,
    frames: usize,
    size: usize,
}

impl Blocks {
    fn new(rate: u32) -> Blocks {
        Blocks { sum: 0.0, frames: 0, size: (rate as usize / BLOCKS_PER_SEC).max(1) }
    }

    /// One frame's mean square in; a block's RMS out when one is full.
    fn add(&mut self, power: f32) -> Option<f32> {
        self.sum += power;
        self.frames += 1;
        if self.frames < self.size {
            return None;
        }
        let rms = (self.sum / self.frames as f32).sqrt();
        self.sum = 0.0;
        self.frames = 0;
        Some(rms)
    }
}

/// Measures a song's loudness as it's decoded (before the effects), passing it through unchanged.
pub struct Probe<S: Source<Item = f32>> {
    inner: S,
    meter: Arc<Meter>,
    channels: usize,
    blocks: Blocks,
    power: f32,
    at: usize,
    /// The frame's samples summed (its mono), the bass filter's state and its step.
    mono: f32,
    low: f32,
    low_step: f32,
    /// This block's bass, left and right power, and its frames.
    bands: (f32, f32, f32),
    band_frames: f32,
}

impl<S: Source<Item = f32>> Probe<S> {
    pub fn new(inner: S, meter: Arc<Meter>) -> Self {
        let channels = inner.channels().get() as usize;
        let rate = inner.sample_rate().get();
        // A one-pole low-pass at 150 Hz.
        let low_step = 1.0 - (-2.0 * std::f32::consts::PI * 150.0 / rate.max(1) as f32).exp();
        Probe { inner, meter, channels, blocks: Blocks::new(rate), power: 0.0, at: 0, mono: 0.0, low: 0.0, low_step, bands: (0.0, 0.0, 0.0), band_frames: 0.0 }
    }
}

impl<S: Source<Item = f32>> Iterator for Probe<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let x = self.inner.next()?;
        self.power += x * x;
        self.mono += x;
        if self.at == 0 {
            self.bands.1 += x * x;
        }
        if self.at == 1 || self.channels == 1 {
            self.bands.2 += x * x;
        }
        self.at += 1;
        if self.at >= self.channels {
            self.low += self.low_step * (self.mono / self.channels as f32 - self.low);
            self.bands.0 += self.low * self.low;
            self.band_frames += 1.0;
            self.mono = 0.0;
            if let Some(rms) = self.blocks.add(self.power / self.channels as f32) {
                self.meter.level.push(rms);
                let n = self.band_frames.max(1.0);
                self.meter.bass.store((self.bands.0 / n).sqrt().to_bits(), Ordering::Relaxed);
                self.meter.left.store((self.bands.1 / n).sqrt().to_bits(), Ordering::Relaxed);
                self.meter.right.store((self.bands.2 / n).sqrt().to_bits(), Ordering::Relaxed);
                self.bands = (0.0, 0.0, 0.0);
                self.band_frames = 0.0;
            }
            self.power = 0.0;
            self.at = 0;
        }
        Some(x)
    }
}

impl<S: Source<Item = f32>> Source for Probe<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.at = 0;
        self.power = 0.0;
        self.inner.try_seek(pos)
    }
}

/// A high-pass filter (RBJ cookbook, Butterworth Q).
#[derive(Clone, Copy, Default)]
struct HighPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl HighPass {
    fn set(&mut self, hz: f32, rate: f32) {
        let w0 = 2.0 * std::f32::consts::PI * hz.min(rate * 0.45) / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        self.b0 = (1.0 + cos) / 2.0 / a0;
        self.b1 = -(1.0 + cos) / a0;
        self.b2 = (1.0 + cos) / 2.0 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// The last step before the player: the song's loudness as played and its frame count, the
/// bass-swap high-pass, and the start gate (silence until the outgoing song reaches a beat).
pub struct Tap<S: Source<Item = f32>> {
    inner: S,
    meter: Arc<Meter>,
    channels: usize,
    rate: u32,
    /// Silent until this meter has played that many frames.
    gate: Option<(Arc<Meter>, u64)>,
    filters: Vec<HighPass>,
    cutoff: f32,
    blocks: Blocks,
    power: f32,
    frame: Vec<f32>,
    at: usize,
    /// The first frame, read ahead (the cue-in's silence skip happens off the audio thread).
    first: Option<Vec<f32>>,
    count: u64,
}

impl<S: Source<Item = f32>> Tap<S> {
    pub fn new(inner: S, meter: Arc<Meter>, gate: Option<(Arc<Meter>, u64)>) -> Self {
        let channels = inner.channels().get() as usize;
        let rate = inner.sample_rate().get();
        meter.rate.store(rate, Ordering::Relaxed);
        if gate.is_none() {
            meter.open.store(true, Ordering::Release);
        }
        Tap {
            inner,
            meter,
            channels,
            rate,
            gate,
            filters: vec![HighPass::default(); channels],
            cutoff: 0.0,
            blocks: Blocks::new(rate),
            power: 0.0,
            frame: vec![0.0; channels],
            at: channels,
            first: None,
            count: 0,
        }
    }

    /// Reads the first frame now (skipping a silent start happens here, on the caller's thread,
    /// not in the middle of playback).
    pub fn preroll(&mut self) {
        let mut f = Vec::with_capacity(self.channels);
        for _ in 0..self.channels {
            match self.inner.next() {
                Some(x) => f.push(x),
                None => return,
            }
        }
        self.first = Some(f);
    }

    fn next_frame(&mut self) -> Option<()> {
        if let Some((other, at)) = &self.gate {
            let open = if *at == UNTIL_END { other.ended.load(Ordering::Acquire) } else { other.frames.load(Ordering::Acquire) >= *at };
            if !open {
                self.frame.fill(0.0);
                self.meter.waited.fetch_add(1, Ordering::AcqRel);
                return Some(());
            }
            self.gate = None;
            self.meter.open.store(true, Ordering::Release);
        }
        match self.first.take() {
            Some(f) => self.frame.copy_from_slice(&f),
            None => {
                for c in 0..self.channels {
                    match self.inner.next() {
                        Some(x) => self.frame[c] = x,
                        None => {
                            self.meter.ended.store(true, Ordering::Release);
                            return None;
                        }
                    }
                }
            }
        }
        if self.count.is_multiple_of(256) {
            let want = f32::from_bits(self.meter.highpass.load(Ordering::Relaxed));
            if (want - self.cutoff).abs() > 0.5 {
                if self.cutoff < 1.0 {
                    // Starting fresh: no old state to ring out.
                    self.filters = vec![HighPass::default(); self.channels];
                }
                self.cutoff = want;
                for f in &mut self.filters {
                    f.set(want.max(1.0), self.rate as f32);
                }
            }
        }
        if self.cutoff >= 1.0 {
            for (x, f) in self.frame.iter_mut().zip(&mut self.filters) {
                *x = f.run(*x);
            }
        }
        self.count += 1;
        self.meter.frames.store(self.count, Ordering::Release);
        self.power = self.frame.iter().map(|x| x * x).sum::<f32>() / self.channels as f32;
        if let Some(rms) = self.blocks.add(self.power) {
            self.meter.played.push(rms);
        }
        Some(())
    }
}

impl<S: Source<Item = f32>> Iterator for Tap<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.at >= self.channels {
            self.next_frame()?;
            self.at = 0;
        }
        let x = self.frame[self.at];
        self.at += 1;
        Some(x)
    }
}

impl<S: Source<Item = f32>> Source for Tap<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels.max(1) as u16).unwrap()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate.max(1)).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.at = self.channels;
        self.first = None;
        self.gate = None;
        self.meter.open.store(true, Ordering::Release);
        self.count = (pos.as_secs_f64() * self.rate as f64) as u64;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An envelope of clicks at `bpm` over a low hum, `secs` long, the first click `offset` blocks in.
    fn clicks(bpm: f32, secs: usize, offset: usize) -> Vec<f32> {
        let period = 60.0 * BLOCKS_PER_SEC as f32 / bpm;
        let mut env = vec![0.02; secs * BLOCKS_PER_SEC];
        let mut at = offset as f32;
        while (at as usize) < env.len() {
            let i = at.round() as usize;
            for (k, v) in [0.5, 0.3, 0.15].iter().enumerate() {
                if i + k < env.len() {
                    env[i + k] = *v;
                }
            }
            at += period;
        }
        env
    }

    #[test]
    fn tempo_phase_and_nudge() {
        for bpm in [90.0, 120.0, 128.0, 150.0] {
            let b = beat(&clicks(bpm, 8, 13)).expect("a beat");
            assert!((b.bpm() - bpm).abs() < bpm * 0.02, "{bpm}: found {}", b.bpm());
            // The last beat: a whole number of periods after the first one at block 13.
            let n = 8 * BLOCKS_PER_SEC - 2;
            let last_at = n as f32 - b.last;
            let beats = (last_at - 13.0) / b.period;
            assert!((beats - beats.round()).abs() < 0.25, "{bpm}: phase off by {:.2} beats", beats - beats.round());
        }
        // No beat in a steady hum.
        assert!(beat(&vec![0.1; 800]).is_none());
        assert_eq!(nudge(124.0, 124.0), Some(1.0));
        assert!(nudge(124.0, 121.0).is_some_and(|r| (r - 124.0 / 121.0).abs() < 1e-4));
        assert!(nudge(124.0, 62.5).is_some(), "half time counts");
        assert!(nudge(124.0, 110.0).is_none());
    }

    #[test]
    fn cue_out_and_bass_cut() {
        // Loud, then a fade under -28 dB held for 400 ms.
        let mut env = vec![0.2; 1200];
        assert!(!faded_out(&env));
        env.extend(std::iter::repeat_n(0.02, 30));
        assert!(!faded_out(&env), "300 ms isn't enough");
        env.extend(std::iter::repeat_n(0.02, 15));
        assert!(faded_out(&env));
        // A song quiet all along (-30 dB) isn't "faded out" by staying there.
        assert!(!faded_out(&vec![0.03; 1500]));
        assert_eq!(bass_cut(0.0), 0.0);
        assert!((bass_cut(0.66) - BASS_HZ).abs() < 0.01 && (bass_cut(1.0) - BASS_HZ).abs() < 0.01);
        assert!(bass_cut(0.3) > 20.0 && bass_cut(0.3) < BASS_HZ);
    }

    fn sine(hz: f32, secs: f32, rate: u32) -> rodio::buffer::SamplesBuffer {
        let n = (secs * rate as f32) as usize;
        let v: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5).collect();
        rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(rate).unwrap(), v)
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn high_pass_and_gate() {
        // 60 Hz bass mostly gone under a 200 Hz high-pass; 2 kHz untouched.
        let m = Meter::new();
        m.set_highpass(BASS_HZ);
        let bass: Vec<f32> = Tap::new(sine(60.0, 1.0, 48_000), m.clone(), None).skip(4800).collect();
        assert!(rms(&bass) < 0.35 * 0.1, "60 Hz left: {}", rms(&bass));
        let m = Meter::new();
        m.set_highpass(BASS_HZ);
        let treble: Vec<f32> = Tap::new(sine(2000.0, 1.0, 48_000), m, None).skip(4800).collect();
        assert!((rms(&treble) - 0.35).abs() < 0.03);
        // The gate: silent until the other song has played 1000 frames.
        let out = Meter::new();
        let incoming = Meter::new();
        let mut tap = Tap::new(sine(440.0, 1.0, 48_000), incoming.clone(), Some((out.clone(), 1000)));
        assert!((0..500).all(|_| tap.next() == Some(0.0)) && !incoming.is_open());
        out.frames.store(1000, Ordering::Release);
        let heard: Vec<f32> = tap.by_ref().take(200).collect();
        assert!(incoming.is_open() && rms(&heard) > 0.1);
    }

    #[test]
    fn gapless_starts_on_the_frame_the_other_ends() {
        let first = Meter::new();
        let second = Meter::new();
        let mut a = Tap::new(sine(440.0, 0.01, 48_000), first.clone(), None);
        let mut b = Tap::new(sine(440.0, 1.0, 48_000), second.clone(), Some((first.clone(), UNTIL_END)));
        // Mixed side by side as the player does: the second is silent while the first plays.
        let mut waited = 0;
        while a.next().is_some() {
            assert_eq!(b.next(), Some(0.0));
            waited += 1;
        }
        assert!(!second.is_open());
        assert_eq!(second.waited.load(Ordering::Acquire), waited);
        // The first ran out: the very next frame of the second is its song.
        let heard: Vec<f32> = b.by_ref().take(200).collect();
        assert!(second.is_open() && rms(&heard) > 0.1);
        assert!((second.waited_secs() - waited as f32 / 48_000.0).abs() < 1e-6);
    }
}
