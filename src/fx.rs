//! Sound settings applied to every song as it plays (the same for all players):
//! - skip silence: a quiet start is skipped, and a long quiet end finishes the song;
//! - a 5-band equalizer (peaking filters at 60 Hz, 230 Hz, 910 Hz, 3.6 kHz and 14 kHz) with presets;
//! - loudness normalization: every song brought to about the same loudness, slowly, with a soft
//!   limiter so nothing spikes;
//! - pitch, apart from speed (speed itself is the player's: it moves pitch with it, like a record,
//!   and the pitch shifter makes up the difference);
//! - a light reverb (for "Slowed").
//!
//! Settings change while a song plays: each player's chain picks them up within a few
//! milliseconds.

use std::num::NonZero;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rodio::Source;
use rodio::source::SeekError;

/// The equalizer's band centres (Hz).
pub const BANDS: [f32; 5] = [60.0, 230.0, 910.0, 3600.0, 14000.0];

/// Built-in equalizer presets (dB per band); "Custom" is the user's own.
pub const PRESETS: [(&str, [f32; 5]); 6] = [
    ("Flat", [0.0; 5]),
    ("Bass Boost", [6.0, 4.0, 0.0, 0.0, 0.0]),
    ("Vocal", [-2.0, 0.0, 3.0, 4.0, 1.0]),
    ("Rock", [4.0, 2.0, -1.0, 2.0, 4.0]),
    ("Pop", [-1.0, 2.0, 4.0, 2.0, -1.0]),
    ("Custom", [0.0; 5]),
];

/// Speed and pitch quick picks: (name, speed, pitch, reverb mix).
pub const MODES: [(&str, f32, f32, f32); 3] = [("Normal", 1.0, 1.0, 0.0), ("Nightcore", 1.25, 1.25, 0.0), ("Slowed", 0.8, 0.85, 0.3)];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Gain per band, dB (-12 to 12).
    pub eq: [f32; 5],
    pub normalize: bool,
    pub skip_silence: bool,
    /// Pitch the shifter adds on top of the player's speed (1 = none).
    pub pitch: f32,
    /// Reverb mix (0 = off).
    pub reverb: f32,
    /// Karaoke: what's in the middle of the stereo (most often the voice) is taken out.
    pub vocals_off: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { eq: [0.0; 5], normalize: true, skip_silence: true, pitch: 1.0, reverb: 0.0, vocals_off: false }
    }
}

static SETTINGS: RwLock<Settings> = RwLock::new(Settings { eq: [0.0; 5], normalize: true, skip_silence: true, pitch: 1.0, reverb: 0.0, vocals_off: false });
static VERSION: AtomicU64 = AtomicU64::new(1);

pub fn set(s: Settings) {
    if let Ok(mut cur) = SETTINGS.write()
        && *cur != s {
            *cur = s;
            VERSION.fetch_add(1, Ordering::Release);
        }
}

pub fn get() -> Settings {
    SETTINGS.read().map(|s| *s).unwrap_or_default()
}

/// The effects' share of pitch for a wanted `speed` and `pitch` (the player's speed already moves
/// pitch by `speed`).
pub fn shift_for(speed: f32, pitch: f32) -> f32 {
    pitch / speed.max(0.1)
}

// ---------------------------------------------------------------- filters

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// A peaking filter (RBJ cookbook); `None` when it does nothing.
    fn peaking(freq: f32, gain_db: f32, rate: f32) -> Option<Biquad> {
        if gain_db.abs() < 0.05 || freq >= rate * 0.45 {
            return None;
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * 1.0);
        let a0 = 1.0 + alpha / a;
        Some(Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cos) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        })
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Pitch without changing speed: two read heads sweep a short delay line at the new rate and
/// cross-fade (a granular shifter, light enough for real time).
struct Shifter {
    buf: Vec<f32>,
    write: usize,
    phase: f32,
}

impl Shifter {
    fn new(rate: u32) -> Shifter {
        Shifter { buf: vec![0.0; (rate as usize / 20).max(64)], write: 0, phase: 0.0 }
    }

    fn tap(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let pos = self.write as f32 - delay;
        let pos = pos.rem_euclid(n as f32);
        let i = pos as usize % n;
        let frac = pos - pos.floor();
        self.buf[i] * (1.0 - frac) + self.buf[(i + 1) % n] * frac
    }

    fn run(&mut self, x: f32, ratio: f32) -> f32 {
        let n = self.buf.len();
        self.buf[self.write] = x;
        let window = (n - 2) as f32;
        self.phase = (self.phase + (1.0 - ratio) / window).rem_euclid(1.0);
        let p2 = (self.phase + 0.5) % 1.0;
        let gain = |p: f32| 1.0 - (2.0 * p - 1.0).abs();
        let y = self.tap(self.phase * window) * gain(self.phase) + self.tap(p2 * window) * gain(p2);
        self.write = (self.write + 1) % n;
        y
    }
}

/// A small room: four combs and two all-passes (Freeverb's shape), fed the mix of both channels.
struct Reverb {
    combs: Vec<(Vec<f32>, usize, f32)>,
    passes: Vec<(Vec<f32>, usize)>,
}

impl Reverb {
    fn new(rate: u32) -> Reverb {
        let k = rate as f32 / 44_100.0;
        let len = |n: f32| ((n * k) as usize).max(8);
        Reverb {
            combs: [1116.0, 1188.0, 1277.0, 1356.0].iter().map(|n| (vec![0.0; len(*n)], 0, 0.0)).collect(),
            passes: [556.0, 441.0].iter().map(|n| (vec![0.0; len(*n)], 0)).collect(),
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let mut out = 0.0;
        for (buf, at, low) in &mut self.combs {
            let y = buf[*at];
            *low = y * 0.8 + *low * 0.2;
            buf[*at] = x + *low * 0.84;
            *at = (*at + 1) % buf.len();
            out += y;
        }
        out *= 0.25;
        for (buf, at) in &mut self.passes {
            let b = buf[*at];
            let y = -out + b;
            buf[*at] = out + b * 0.5;
            *at = (*at + 1) % buf.len();
            out = y;
        }
        out
    }
}

// ---------------------------------------------------------------- the chain

/// A song with the sound settings applied.
pub struct Effects<S: Source<Item = f32>> {
    inner: S,
    channels: usize,
    rate: u32,
    version: u64,
    settings: Settings,
    eq: Vec<Vec<Biquad>>,
    shifters: Vec<Shifter>,
    reverb: Option<Reverb>,
    /// Normalization: the running mean square, and the gain now.
    ms: f32,
    gain: f32,
    frame: Vec<f32>,
    at: usize,
    /// Skip silence: the start is past, quiet frames in a row, frames played, the song's length.
    started: bool,
    quiet: u64,
    frames: u64,
    total: Option<u64>,
    /// Skip the quiet start even with "Skip silence" off (the automix's cue-in).
    skip_start: bool,
    /// "Even loudness" with the song's loudness from YouTube: one gain for the whole song, right
    /// from its first second (no listening and catching up).
    fixed_gain: Option<f32>,
}

/// About -50 dB: quieter than this is silence.
const SILENCE: f32 = 0.003;
/// Loudness the normalizer aims for (RMS).
const TARGET: f32 = 0.12;

/// The gain for a song YouTube measured at `db` above its reference (about -14 LUFS): brought
/// 4 dB under it (near the normalizer's own level), at most 15 dB down or 6 dB up.
pub fn gain_for_loudness(db: f32) -> f32 {
    10f32.powf((-db - 4.0).clamp(-15.0, 6.0) / 20.0)
}

impl<S: Source<Item = f32>> Effects<S> {
    pub fn new(inner: S) -> Self {
        let channels = inner.channels().get() as usize;
        let rate = inner.sample_rate().get();
        let total = inner.total_duration().map(|d| (d.as_secs_f64() * rate as f64) as u64);
        let mut fx = Effects {
            inner,
            channels,
            rate,
            version: 0,
            settings: Settings::default(),
            eq: Vec::new(),
            shifters: Vec::new(),
            reverb: None,
            ms: TARGET * TARGET,
            gain: 1.0,
            frame: vec![0.0; channels],
            at: channels,
            started: false,
            quiet: 0,
            frames: 0,
            total,
            skip_start: false,
            fixed_gain: None,
        };
        fx.update();
        fx
    }

    /// The song's loudness from YouTube (`loudnessDb`: how far above its reference it is).
    pub fn loudness(mut self, db: Option<f32>) -> Self {
        self.fixed_gain = db.map(gain_for_loudness);
        self
    }

    /// The quiet start is skipped whatever the settings say (the automix's cue-in).
    pub fn skip_start(mut self) -> Self {
        self.skip_start = true;
        self
    }

    /// Picks up changed settings.
    fn update(&mut self) {
        let v = VERSION.load(Ordering::Acquire);
        if v == self.version {
            return;
        }
        self.version = v;
        let s = get();
        let eq_changed = s.eq != self.settings.eq || self.eq.is_empty();
        self.settings = s;
        if eq_changed {
            let rate = self.rate as f32;
            let bands: Vec<Biquad> = BANDS.iter().zip(s.eq).filter_map(|(f, g)| Biquad::peaking(*f, g, rate)).collect();
            self.eq = vec![bands; self.channels];
        }
        if (s.pitch - 1.0).abs() > 0.005 && self.shifters.is_empty() {
            self.shifters = (0..self.channels).map(|_| Shifter::new(self.rate)).collect();
        }
        if s.reverb > 0.0 && self.reverb.is_none() {
            self.reverb = Some(Reverb::new(self.rate));
        }
    }

    /// The next frame, processed; `None` at the end.
    fn next_frame(&mut self) -> Option<()> {
        if self.frames.is_multiple_of(512) {
            self.update();
        }
        loop {
            for c in 0..self.channels {
                self.frame[c] = self.inner.next()?;
            }
            self.frames += 1;
            let peak = self.frame.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            // A quiet start (up to 10 s) is skipped.
            if (self.settings.skip_silence || self.skip_start) && !self.started {
                if peak < SILENCE && self.frames < self.rate as u64 * 10 {
                    continue;
                }
                self.started = true;
            }
            if self.settings.skip_silence {
                // Over 3 s of silence in the song's second half: it's over.
                self.quiet = if peak < SILENCE { self.quiet + 1 } else { 0 };
                if self.quiet > self.rate as u64 * 3 && self.total.is_some_and(|t| self.frames > t / 2) {
                    return None;
                }
            }
            break;
        }
        let s = self.settings;
        // Karaoke: left minus right cancels the middle (the voice), the sides stay.
        if s.vocals_off && self.channels >= 2 {
            let side = (self.frame[0] - self.frame[1]) * 0.7;
            self.frame[0] = side;
            self.frame[1] = side;
        }
        for c in 0..self.channels {
            let mut x = self.frame[c];
            for f in &mut self.eq[c] {
                x = f.run(x);
            }
            if (s.pitch - 1.0).abs() > 0.005
                && let Some(sh) = self.shifters.get_mut(c)
            {
                x = sh.run(x, s.pitch);
            }
            self.frame[c] = x;
        }
        if s.reverb > 0.0
            && let Some(r) = &mut self.reverb
        {
            let mono = self.frame.iter().sum::<f32>() / self.channels as f32;
            let wet = r.run(mono) * s.reverb;
            for x in &mut self.frame {
                *x = *x * (1.0 - s.reverb * 0.4) + wet;
            }
        }
        if s.normalize
            && let Some(g) = self.fixed_gain
        {
            for x in &mut self.frame {
                *x *= g;
            }
        } else if s.normalize {
            let power = self.frame.iter().map(|x| x * x).sum::<f32>() / self.channels as f32;
            // Loudness over about 3 s; quiet parts don't drag the gain up.
            let a = 1.0 / (self.rate as f32 * 3.0);
            if power > SILENCE * SILENCE {
                self.ms += (power - self.ms) * a;
            }
            let want = (TARGET / self.ms.sqrt().max(1e-4)).clamp(0.3, 3.0);
            // Down fast (no spikes), up slowly.
            let speed = if want < self.gain { 20.0 } else { 1.0 } / self.rate as f32;
            self.gain += (want - self.gain) * speed;
            for x in &mut self.frame {
                *x *= self.gain;
            }
        }
        // A soft limiter above 0.8.
        for x in &mut self.frame {
            let m = x.abs();
            if m > 0.8 {
                *x = x.signum() * (0.8 + 0.2 * ((m - 0.8) / 0.2).tanh());
            }
        }
        Some(())
    }
}

impl<S: Source<Item = f32>> Iterator for Effects<S> {
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

impl<S: Source<Item = f32>> Source for Effects<S> {
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
        self.frames = (pos.as_secs_f64() * self.rate as f64) as u64;
        self.started = true;
        self.quiet = 0;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test tone: `secs` of a 440 Hz sine at `level`, after `lead` seconds of silence.
    fn tone(level: f32, lead: f32, secs: f32) -> rodio::buffer::SamplesBuffer {
        let rate = 8000;
        let mut v = vec![0.0f32; (lead * rate as f32) as usize];
        v.extend((0..(secs * rate as f32) as usize).map(|i| level * (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / rate as f32).sin()));
        rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(rate).unwrap(), v)
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    #[test]
    fn silence_loudness_equalizer_pitch_reverb() {
        set(Settings { eq: [0.0; 5], normalize: true, skip_silence: true, pitch: 1.0, reverb: 0.0, vocals_off: false });
        // 2 s of silence first: skipped.
        let out: Vec<f32> = Effects::new(tone(0.02, 2.0, 8.0)).collect();
        assert!(out.len() <= 8 * 8000 + 10, "{}", out.len());
        // A quiet song comes out much louder at the end, a loud one is never above 1.
        let quiet: Vec<f32> = Effects::new(tone(0.02, 0.0, 8.0)).collect();
        assert!(rms(&quiet[quiet.len() - 8000..]) > 2.0 * rms(&quiet[..800]));
        let loud: Vec<f32> = Effects::new(tone(1.0, 0.0, 4.0)).collect();
        assert!(loud.iter().all(|x| x.abs() <= 1.0));
        // (One test: the settings are shared by every player.)
        // Karaoke: a voice in the middle (the same on both sides) goes; a side-only sound stays.
        set(Settings { eq: [0.0; 5], normalize: false, skip_silence: false, pitch: 1.0, reverb: 0.0, vocals_off: true });
        let middle: Vec<f32> = Effects::new(rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(2).unwrap(), std::num::NonZero::new(8000).unwrap(), vec![0.5f32; 8000])).collect();
        assert!(middle.iter().all(|x| x.abs() < 1e-4), "the middle is gone");
        let side: Vec<f32> = Effects::new(rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(2).unwrap(), std::num::NonZero::new(8000).unwrap(), [0.5f32, 0.0].repeat(4000))).collect();
        assert!(side.iter().any(|x| x.abs() > 0.2), "a side stays");
        set(Settings { eq: [6.0, 0.0, 0.0, 0.0, -6.0], normalize: false, skip_silence: false, pitch: 1.2, reverb: 0.3, vocals_off: false });
        let out: Vec<f32> = Effects::new(tone(0.5, 0.0, 1.0)).collect();
        assert_eq!(out.len(), 8000);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        set(Settings::default());
    }

    #[test]
    fn loudness_from_youtube_becomes_one_gain() {
        // 2 dB under YouTube's reference: brought to 4 dB under it, so 2 dB down.
        assert!((super::gain_for_loudness(-2.0) - 10f32.powf(-2.0 / 20.0)).abs() < 1e-4);
        // Very loud: at most 15 dB down; very quiet: at most 6 dB up.
        assert!((super::gain_for_loudness(20.0) - 10f32.powf(-15.0 / 20.0)).abs() < 1e-4);
        assert!((super::gain_for_loudness(-30.0) - 10f32.powf(6.0 / 20.0)).abs() < 1e-4);
    }
}
