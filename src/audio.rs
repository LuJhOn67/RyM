//! Audio playback: symphonia (+libopus) decodes, rodio outputs.

use std::fs::File;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rodio::source::SeekError;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use symphonia_adapter_libopus::OpusDecoder;

use crate::automix::{self, Meter, Probe, Tap};

fn codecs() -> &'static CodecRegistry {
    static REG: std::sync::OnceLock<CodecRegistry> = std::sync::OnceLock::new();
    REG.get_or_init(|| {
        let mut reg = CodecRegistry::new();
        reg.register_audio_decoder::<OpusDecoder>();
        symphonia::default::register_enabled_codecs(&mut reg);
        reg
    })
}

/// A rodio source that decodes a local file with symphonia.
struct FileSource {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    buf: Vec<f32>,
    pos: usize,
    channels: u16,
    rate: u32,
    duration: Option<Duration>,
}

impl FileSource {
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        Self::from_source(Box::new(file), path.extension().and_then(|e| e.to_str()))
    }

    /// Decodes any byte source (a file, or a song still arriving); `ext` helps guess the format.
    fn from_source(source: Box<dyn symphonia::core::io::MediaSource>, ext: Option<&str>) -> Result<Self> {
        let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        if let Some(ext) = ext {
            hint.with_extension(ext);
        }
        let reader = symphonia::default::get_probe().probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )?;
        let track = reader
            .first_track(TrackType::Audio)
            .ok_or_else(|| anyhow!("no audio track"))?;
        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            return Err(anyhow!("track has no audio codec parameters"));
        };
        let duration = match (track.time_base, track.duration) {
            (Some(tb), Some(dur)) => tb
                .calc_duration(dur)
                .map(|t| Duration::from_secs_f64(t.as_secs_f64())),
            _ => None,
        };
        let decoder = codecs().make_audio_decoder(params, &AudioDecoderOptions::default())?;
        let track_id = track.id;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2) as u16;
        let rate = params.sample_rate.unwrap_or(48_000);
        let mut src = FileSource {
            reader,
            decoder,
            track_id,
            buf: Vec::new(),
            pos: 0,
            channels,
            rate,
            duration,
        };
        // Decode the first packet so the reported spec matches the real output.
        src.refill();
        Ok(src)
    }

    /// Decodes packets until there are samples in the buffer. Returns false at the end.
    fn refill(&mut self) -> bool {
        self.buf.clear();
        self.pos = 0;
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return false,
                Err(SymError::ResetRequired) => return false,
                Err(_) => return false,
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    self.channels = decoded.spec().channels().count() as u16;
                    self.rate = decoded.spec().rate();
                    decoded.copy_to_vec_interleaved(&mut self.buf);
                    if !self.buf.is_empty() {
                        return true;
                    }
                }
                Err(SymError::DecodeError(_)) => continue,
                Err(_) => return false,
            }
        }
    }
}

impl Iterator for FileSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.buf.len() && !self.refill() {
            return None;
        }
        let s = self.buf[self.pos];
        self.pos += 1;
        Some(s)
    }
}

impl Source for FileSource {
    fn current_span_len(&self) -> Option<usize> {
        // The sample spec may change between packets, so each buffer is its own span.
        Some(self.buf.len() - self.pos)
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels.max(1)).unwrap()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate.max(1)).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let time = Time::try_from_secs_f64(pos.as_secs_f64()).unwrap_or(Time::ZERO);
        let to = SeekTo::Time { time, track_id: Some(self.track_id) };
        if self.reader.seek(SeekMode::Accurate, to).is_ok() {
            self.decoder.reset();
            self.refill();
        }
        Ok(())
    }
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

/// What a player plays: the song, its effects, then the automix's measurements and filter.
type Chain = Tap<crate::fx::Effects<Probe<FileSource>>>;

/// A song going out while the next comes in.
struct Fade {
    old: Player,
    old_meter: Arc<Meter>,
    secs: f32,
    /// Smart automix (the bass swap, the beat-timed start, the tempo nudge), or a plain crossfade.
    smart: bool,
    /// Gapless: nothing moves until the incoming song's gate opens (the outgoing one ran out).
    wait_gate: bool,
    /// When the incoming song started sounding (a smart one waits for a beat).
    began: Option<Instant>,
    /// The outgoing song's tempo, and the nudge given to the incoming one (1 = none).
    out_bpm: Option<f32>,
    nudge: Option<f32>,
}

pub struct Audio {
    sink: MixerDeviceSink,
    player: Player,
    meter: Arc<Meter>,
    loaded: bool,
    duration: Option<Duration>,
    /// The volume slider (0 to 1), the speed, and a fade on top (the sleep timer's, 1 = none),
    /// as f32 bits (set from `&self`).
    volume: AtomicU32,
    speed: AtomicU32,
    fade: AtomicU32,
    fading: Option<Fade>,
    /// After a tempo nudge: back to the set speed, from that factor, since then.
    easing: Option<(Instant, f32)>,
    /// Seeked since the song started: the player's position is the song's own again (before
    /// that, the silence the song waited through at its gate is taken off).
    seeked: std::sync::atomic::AtomicBool,
}

impl Audio {
    pub fn new() -> Result<Self> {
        let mut sink = DeviceSinkBuilder::open_default_sink()?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        let f = |x: f32| AtomicU32::new(x.to_bits());
        Ok(Audio {
            sink,
            player,
            meter: Meter::new(),
            loaded: false,
            duration: None,
            volume: f(0.8),
            speed: f(1.0),
            fade: f(1.0),
            fading: None,
            easing: None,
            seeked: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// The song's source with the sound settings (equalizer, normalization...) and the automix's
    /// measurements on it. `gate`: silent until that meter has played so many frames; `cue_in`:
    /// its quiet start is skipped (read ahead here, off the audio thread).
    fn source(song: &crate::stream::Playable, gate: Option<(Arc<Meter>, u64)>, cue_in: bool) -> Result<(Chain, Option<Duration>, Arc<Meter>)> {
        let (src, seconds, id) = match song {
            crate::stream::Playable::File(path) => (FileSource::open(path)?, None, path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()),
            crate::stream::Playable::Arriving(a) => (FileSource::from_source(Box::new(a.reader()), Some(&a.ext))?, a.seconds, a.id.clone()),
        };
        let d = src.total_duration().or(seconds.map(Duration::from_secs_f32));
        let meter = Meter::new();
        let fx = crate::fx::Effects::new(Probe::new(src, meter.clone())).loudness(crate::stream::loudness(&id));
        let mut tap = Tap::new(if cue_in { fx.skip_start() } else { fx }, meter.clone(), gate);
        if cue_in {
            tap.preroll();
        }
        Ok((tap, d, meter))
    }

    /// A cached file or a song still arriving. `known_secs`: length from YouTube Music, used
    /// when the file doesn't say (WebM).
    pub fn play(&mut self, song: &crate::stream::Playable, known_secs: Option<f32>) -> Result<()> {
        let (src, d, meter) = Self::source(song, None, false)?;
        self.meter = meter;
        self.duration = d.or(known_secs.map(Duration::from_secs_f32));
        self.loaded = true;
        self.fading = None;
        self.end_nudge();
        self.player.clear();
        self.player.set_speed(load(&self.speed));
        self.player.append(src);
        self.player.play();
        self.apply_volume();
        self.seeked.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Gapless: `song` waits silently and starts on the very frame the playing one runs out (no
    /// gap, no overlap). Nothing playing: it just plays.
    pub fn gapless_to(&mut self, song: &crate::stream::Playable, known_secs: Option<f32>) -> Result<()> {
        if !self.is_playing() {
            return self.play(song, known_secs);
        }
        let (src, d, meter) = Self::source(song, Some((self.meter.clone(), automix::UNTIL_END)), false)?;
        let next = Player::connect_new(self.sink.mixer());
        next.set_speed(load(&self.speed));
        // Full volume at once: it's silent until its gate opens.
        next.set_volume(self.level());
        next.append(src);
        next.play();
        self.end_nudge();
        let old = std::mem::replace(&mut self.player, next);
        let old_meter = std::mem::replace(&mut self.meter, meter);
        self.fading = Some(Fade { old, old_meter, secs: 0.01, smart: false, wait_gate: true, began: None, out_bpm: None, nudge: None });
        self.duration = d.or(known_secs.map(Duration::from_secs_f32));
        self.loaded = true;
        self.seeked.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub fn play_file(&mut self, path: &Path, known_secs: Option<f32>) -> Result<()> {
        self.play(&crate::stream::Playable::File(path.to_path_buf()), known_secs)
    }

    pub fn play_arriving(&mut self, song: &crate::arriving::Arriving, known_secs: Option<f32>) -> Result<()> {
        self.play(&crate::stream::Playable::Arriving(song.clone()), known_secs)
    }

    /// Starts `song` over `secs` while the playing one fades out (both play at once; see [`tick`](Self::tick)).
    pub fn crossfade_to(&mut self, song: &crate::stream::Playable, known_secs: Option<f32>, secs: f32) -> Result<()> {
        self.mix_to(song, known_secs, secs, false)
    }

    /// Smart automix to `song` over `secs`: it comes in on its first sound, on one of the playing
    /// song's beats (when it has a clear one), while the playing song's bass is taken out; tempos
    /// within 3% are matched while they overlap. See [`crate::automix`].
    pub fn automix_to(&mut self, song: &crate::stream::Playable, known_secs: Option<f32>, secs: f32) -> Result<()> {
        self.mix_to(song, known_secs, secs, true)
    }

    fn mix_to(&mut self, song: &crate::stream::Playable, known_secs: Option<f32>, secs: f32, smart: bool) -> Result<()> {
        if secs <= 0.05 || !self.is_playing() {
            return self.play(song, known_secs);
        }
        // On the next beat at least 0.15 s away (time to start), when it's within 1.2 s.
        let beat = if smart { self.meter.next_beat_frame(0.15) } else { None };
        let rate = self.meter.rate() as f32;
        let gate = beat.filter(|(at, _)| (*at as f32 - self.meter.frames() as f32) < rate * 1.2).map(|(at, _)| (self.meter.clone(), at));
        // Cue-in: its silent start skipped, unless "Skip silence" is off.
        let (src, d, meter) = Self::source(song, gate, smart && crate::fx::get().skip_silence)?;
        let next = Player::connect_new(self.sink.mixer());
        next.set_speed(load(&self.speed));
        next.set_volume(0.0);
        next.append(src);
        next.play();
        self.end_nudge();
        let old = std::mem::replace(&mut self.player, next);
        let old_meter = std::mem::replace(&mut self.meter, meter);
        self.fading = Some(Fade { old, old_meter, secs, smart, wait_gate: false, began: None, out_bpm: beat.map(|(_, b)| b.bpm()), nudge: None });
        self.seeked.store(false, Ordering::Relaxed);
        self.duration = d.or(known_secs.map(Duration::from_secs_f32));
        self.loaded = true;
        Ok(())
    }

    /// Moves a crossfade along (call it often, from the app's timer). Equal-power curves, so the
    /// overall loudness holds; a smart one also takes the outgoing song's bass out and nudges the
    /// incoming song's tempo.
    pub fn tick(&mut self) {
        let v = self.level();
        let speed = load(&self.speed);
        if let Some((since, from)) = self.easing {
            // Back to the set speed over 4 s after a nudge.
            let t = (since.elapsed().as_secs_f32() / 4.0).min(1.0);
            self.player.set_speed(speed * (from + (1.0 - from) * t));
            if t >= 1.0 {
                self.easing = None;
            }
        }
        let Some(f) = &mut self.fading else { return };
        // A smart mix starts counting once the incoming song sounds (it may wait for a beat).
        let began = match f.began {
            Some(b) => b,
            None if (!f.smart && !f.wait_gate) || self.meter.is_open() => *f.began.insert(Instant::now()),
            None => {
                f.old.set_volume(v);
                return;
            }
        };
        let elapsed = began.elapsed().as_secs_f32();
        let t = (elapsed / f.secs).min(1.0);
        let a = t * std::f32::consts::FRAC_PI_2;
        f.old.set_volume(v * a.cos());
        self.player.set_volume(v * a.sin());
        if f.smart {
            f.old_meter.set_highpass(automix::bass_cut(t));
            // Tempo: once 3 s of the incoming song are in, nudged to the outgoing one's when close.
            if f.nudge.is_none() && elapsed > 3.0 {
                let ratio = f.out_bpm.zip(automix::beat(&self.meter.played(3.0))).and_then(|(out, inc)| automix::nudge(out, inc.bpm())).unwrap_or(1.0);
                f.nudge = Some(ratio);
                if ratio != 1.0 {
                    self.player.set_speed(speed * ratio);
                }
            }
        }
        if t >= 1.0 || f.old.empty() {
            let nudge = f.nudge.unwrap_or(1.0);
            self.fading = None;
            self.player.set_volume(v);
            if nudge != 1.0 {
                self.easing = Some((Instant::now(), nudge));
            }
        }
    }

    /// A tempo nudge is let go at once (a new song, or the speed set by hand).
    fn end_nudge(&mut self) {
        if self.easing.take().is_some() {
            self.player.set_speed(load(&self.speed));
        }
    }

    /// The playing song has faded out (see [`automix::faded_out`]).
    pub fn faded_out(&self) -> bool {
        automix::faded_out(&self.meter.level(automix::WINDOW_SECS))
    }

    /// A crossfade is under way.
    pub fn crossfading(&self) -> bool {
        self.fading.is_some()
    }

    /// The volume slider (0 to 1).
    pub fn volume(&self) -> f32 {
        load(&self.volume)
    }

    fn level(&self) -> f32 {
        // Perceptual curve: the slider is linear, loudness is not.
        let v = load(&self.volume);
        v * v * load(&self.fade)
    }

    fn apply_volume(&self) {
        if self.fading.is_none() {
            self.player.set_volume(self.level());
        }
    }

    pub fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
            if let Some(f) = &self.fading {
                f.old.pause();
            }
        }
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    /// A track is loaded, not paused, and not finished.
    pub fn is_playing(&self) -> bool {
        self.loaded && !self.player.is_paused() && !self.player.empty()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// True once a track was loaded and has played to the end.
    pub fn finished(&self) -> bool {
        self.loaded && self.player.empty()
    }

    pub fn stop(&mut self) {
        self.player.clear();
        self.fading = None;
        self.loaded = false;
        self.duration = None;
    }

    pub fn seek(&self, secs: f32) {
        let _ = self.player.try_seek(Duration::from_secs_f32(secs.max(0.0)));
        self.seeked.store(true, Ordering::Relaxed);
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.to_bits(), Ordering::Relaxed);
        self.apply_volume();
    }

    /// A fade on top of the volume (1 = none; the sleep timer goes to 0).
    pub fn set_fade(&self, f: f32) {
        self.fade.store(f.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.apply_volume();
    }

    /// Playback speed (0.5 to 2; it moves pitch with it, the effects' pitch shifter makes up the rest).
    pub fn set_speed(&self, speed: f32) {
        let speed = speed.clamp(0.5, 2.0);
        self.speed.store(speed.to_bits(), Ordering::Relaxed);
        self.player.set_speed(speed);
    }

    /// The last `secs` of the playing song's loudness as played (100 values a second): the
    /// visualizer.
    pub fn loudness(&self, secs: f32) -> Vec<f32> {
        self.meter.played(secs)
    }

    /// The playing song's bass, left and right levels now (RMS): the moving backdrop and the
    /// needle meters.
    pub fn bands(&self) -> (f32, f32, f32) {
        self.meter.bands()
    }

    pub fn position(&self) -> f32 {
        let pos = self.player.get_pos().as_secs_f32();
        if self.seeked.load(Ordering::Relaxed) { pos } else { (pos - self.meter.waited_secs()).max(0.0) }
    }

    pub fn duration(&self) -> Option<f32> {
        self.duration.map(|d| d.as_secs_f32())
    }
}

/// Decodes a whole file and returns its length in seconds (used by tests).
pub fn decode_seconds(path: &Path) -> Result<f64> {
    let mut src = FileSource::open(path)?;
    let (ch, rate) = (src.channels as f64, src.rate as f64);
    let samples = src.by_ref().count() as f64;
    Ok(samples / ch / rate)
}

#[cfg(test)]
mod seek_tests {
    use super::*;

    /// Uses a song the live test downloaded.
    #[test]
    #[ignore]
    fn webm_duration_and_seek() {
        let path = crate::stream::cached("lYBUbBu4W08").expect("run the live test first");
        let mut src = FileSource::open(&path).unwrap();
        println!("duration: {:?}", src.duration);
        let r = Source::try_seek(&mut src, Duration::from_secs(60));
        println!("seek: {r:?}");
        let rest = src.by_ref().count() as f64 / src.channels as f64 / src.rate as f64;
        println!("seconds left after seeking to 60s: {rest:.1}");
    }
}

#[cfg(test)]
mod arriving_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    /// A real cached song served by a local server (no internet), decoded while it arrives, gives
    /// the same audio as decoding the file: `cargo test --lib arriving_decodes -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn arriving_decodes_like_the_file() {
        let song = std::fs::read_dir(crate::stream::cache_dir())
            .expect("a song cache")
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "webm"))
            .expect("a cached webm song");
        let data = Arc::new(std::fs::read(&song).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let served = data.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let data = served.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(conn.try_clone().unwrap());
                    let mut range = (0, data.len() - 1);
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            range = (a.parse().unwrap(), b.parse::<usize>().unwrap().min(data.len() - 1));
                        }
                    }
                    let (a, b) = range;
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {a}-{b}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len(),
                        b - a + 1
                    );
                    let _ = conn.write_all(head.as_bytes());
                    for piece in data[a..=b].chunks(32 << 10) {
                        let _ = conn.write_all(piece);
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                });
            }
        });
        let dir = std::env::temp_dir().join(format!("rym-arriving-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let started = std::time::Instant::now();
        let a = crate::arriving::Arriving::start(url, Vec::new(), "webm".into(), None, dir.join("s.webm")).await.unwrap();
        let ready = started.elapsed();
        let (streamed, from_file) = tokio::task::spawn_blocking(move || {
            let streamed = FileSource::from_source(Box::new(a.reader()), Some("webm")).unwrap().count();
            let from_file = FileSource::open(&song).unwrap().count();
            (streamed, from_file)
        })
        .await
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        println!("ready to play after {ready:?}; {streamed} samples streamed, {from_file} from the file");
        assert_eq!(streamed, from_file);
    }
}

#[cfg(test)]
mod automix_tests {
    use super::*;

    /// Real cached songs: the tempo found a minute in, and where in the last 15 s the fade-out
    /// would start the mix: `cargo test --lib automix_on_cached -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn automix_on_cached_songs() {
        let songs: Vec<_> = std::fs::read_dir(crate::stream::cache_dir())
            .expect("a song cache")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "webm" || e == "m4a"))
            .take(8)
            .collect();
        for path in songs {
            let Ok(src) = FileSource::open(&path) else { continue };
            let meter = Meter::new();
            let rate = src.rate as f32;
            let ch = src.channels as usize;
            let total = src.total_duration().map(|d| d.as_secs_f32());
            let mut tap = Tap::new(crate::fx::Effects::new(Probe::new(src, meter.clone())), meter.clone(), None);
            let step = (rate * 0.1) as usize * ch;
            let mut secs = 0.0f32;
            let _ = total;
            let mut tempos = Vec::new();
            let mut faded = Vec::new();
            loop {
                if tap.by_ref().take(step).count() < step {
                    break;
                }
                secs += 0.1;
                if (secs - 60.0).abs() < 0.05 || (secs - 90.0).abs() < 0.05 {
                    tempos.push(automix::beat(&meter.played(12.0)).map(|b| (b.bpm().round(), (b.confidence * 100.0).round() / 100.0)));
                }
                faded.push((secs, automix::faded_out(&meter.level(automix::WINDOW_SECS))));
            }
            // The first fade-out in the last 15 s (the app knows the length from YouTube Music).
            let cue = faded.iter().find(|(t, f)| *f && *t >= secs - automix::WINDOW_SECS).map(|(t, _)| *t);
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            println!("{name}: {secs:.0} s; tempo at 60/90 s {tempos:?}; fade-out cue {}", cue.map_or("none (mix window)".into(), |c| format!("{:.1} s before the end", secs - c)));
        }
    }
}
