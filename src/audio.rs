//! Audio playback: symphonia (+libopus) decodes, rodio outputs.

use std::fs::File;
use std::num::NonZero;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rodio::source::SeekError;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;
use symphonia_adapter_libopus::OpusDecoder;

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
    seekable: bool,
    buf: Vec<f32>,
    pos: usize,
    channels: u16,
    rate: u32,
    duration: Option<Duration>,
}

impl FileSource {
    fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        Self::from_source(Box::new(file), hint)
    }

    fn from_source(source: Box<dyn MediaSource>, hint: Hint) -> Result<Self> {
        let seekable = source.is_seekable();
        let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
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
            seekable,
            buf: Vec::new(),
            pos: 0,
            channels,
            rate,
            duration,
        };
        // Decode the first packet so the reported spec matches the real output. Reject a
        // stream with no usable first frame instead of appending a silent, already-finished source.
        if !src.refill() {
            return Err(anyhow!(
                "the selected audio stream contains no decodable audio frames"
            ));
        }
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

/// A decoder prepared off the UI thread and ready to append to rodio.
pub struct PreparedAudio(std::sync::Arc<std::sync::Mutex<Option<FileSource>>>);

impl Clone for PreparedAudio {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for PreparedAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedAudio")
    }
}

/// Probe a local audio file on a worker before handing it to the playback device.
pub fn prepare_file(path: &Path) -> Result<PreparedAudio> {
    FileSource::open(path)
        .map(|src| PreparedAudio(std::sync::Arc::new(std::sync::Mutex::new(Some(src)))))
}

fn prepare_stream(source: crate::stream::StreamingAudio) -> Result<PreparedAudio> {
    let mut hint = Hint::new();
    if let Some(ext) = source.ext() {
        hint.with_extension(ext);
    }
    let seconds = source.seconds();
    FileSource::from_source(Box::new(source), hint)
        .map(|mut src| {
            src.duration = src.duration.or(seconds.map(Duration::from_secs_f32));
            src
        })
        .map(|src| PreparedAudio(std::sync::Arc::new(std::sync::Mutex::new(Some(src)))))
}

/// Resolve a song and prepare its decoder off the UI thread. Audio streams straight into the
/// decoder, with no temporary song file or disk cache.
pub async fn prepare_playback(
    video_id: String,
    cookies: Option<std::path::PathBuf>,
) -> Result<PreparedAudio> {
    let source = crate::stream::open_playback(video_id, cookies).await?;
    tokio::task::spawn_blocking(move || prepare_stream(source)).await?
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
        if !self.seekable {
            return Err(SeekError::NotSupported {
                underlying_source: "direct audio stream",
            });
        }
        let time = Time::try_from_secs_f64(pos.as_secs_f64()).unwrap_or(Time::ZERO);
        let to = SeekTo::Time {
            time,
            track_id: Some(self.track_id),
        };
        if self.reader.seek(SeekMode::Accurate, to).is_ok() {
            self.decoder.reset();
            self.refill();
        }
        Ok(())
    }
}

/// Reads the duration for a local audio file when its container provides one.
pub fn file_duration(path: &Path) -> Option<f32> {
    let file = File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let reader = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;
    let track = reader.first_track(TrackType::Audio)?;
    let (time_base, duration) = (track.time_base?, track.duration?);
    Some(time_base.calc_duration(duration)?.as_secs_f64() as f32)
}

pub struct Audio {
    _sink: MixerDeviceSink,
    player: Player,
    loaded: bool,
    duration: Option<Duration>,
}

impl Audio {
    pub fn new() -> Result<Self> {
        let mut sink = DeviceSinkBuilder::open_default_sink()?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Audio {
            _sink: sink,
            player,
            loaded: false,
            duration: None,
        })
    }

    /// `known_secs`: length from YouTube Music, used when the file doesn't say (WebM).
    pub fn play_file(&mut self, path: &Path, known_secs: Option<f32>) -> Result<()> {
        self.play_prepared(prepare_file(path)?, known_secs);
        Ok(())
    }

    pub fn play_prepared(&mut self, prepared: PreparedAudio, known_secs: Option<f32>) {
        let Ok(mut prepared) = prepared.0.lock() else {
            return;
        };
        let Some(src) = prepared.take() else { return };
        self.duration = src
            .total_duration()
            .or(known_secs.map(Duration::from_secs_f32));
        self.loaded = true;
        self.player.clear();
        self.player.append(src);
        self.player.play();
    }

    pub fn toggle(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
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
        self.loaded = false;
        self.duration = None;
    }

    pub fn seek(&self, secs: f32) {
        let _ = self.player.try_seek(Duration::from_secs_f32(secs.max(0.0)));
    }

    pub fn set_volume(&self, v: f32) {
        // Perceptual curve: slider is linear, loudness is not.
        self.player.set_volume(v * v);
    }

    pub fn position(&self) -> f32 {
        self.player.get_pos().as_secs_f32()
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
