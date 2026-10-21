//! MPEG audio (layers I-III) and Ogg Vorbis through Symphonia.
//!
//! For MPEG audio, gapless trimming is disabled on purpose: encoder delay/padding comes from the
//! non-normative LAME/Xing tag, and the games' own player (Miles Sound System on PC) outputs every
//! decoded frame, so Eyefinder does too. For Ogg Vorbis, trimming is kept: the granule positions
//! are normative (Vorbis I specification) and define the exact stream length.
//!
//! Timeline: Symphonia's packet timestamps count playable frames, so with trimming disabled the
//! first MPEG packet sits at `-delay`. Eyefinder's output frame of a packet is therefore
//! `pts + delay` for MPEG and `pts + trim_start` for Vorbis (where the decoder applies the trim).
//! Seeking lands `SEEK_PREROLL` frames early and discards up to the target, so decoder state (MP3
//! bit reservoir, MDCT overlap) is fully rebuilt and the output equals a decode from the start.

use symphonia::core::codecs::audio::well_known::{CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3, CODEC_ID_VORBIS};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration as SymDuration, Timestamp};

use crate::pcm::{PcmFormat, PcmSource, SampleKind, SourceInfo, f32_to_i16};
use crate::stream::ReopenMedia;
use crate::{Error, Result};

/// Frames decoded and discarded before a seek target (several MP3 / Vorbis packets).
const SEEK_PREROLL: u64 = 16384;

/// Which container to expect, as a probe hint.
#[derive(Debug, Clone, Copy)]
pub enum Container {
    MpegAudio,
    Ogg,
}

struct Opened {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: u32,
    channels: u16,
    codec: &'static str,
    num_frames: Option<u64>,
    delay: u64,
    padding: u64,
}

fn probe(source: Box<dyn ReopenMedia>, container: Container) -> Result<Opened> {
    let mss = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    hint.with_extension(match container {
        Container::MpegAudio => "mp3",
        Container::Ogg => "ogg",
    });
    let what = match container {
        Container::MpegAudio => "MPEG audio",
        Container::Ogg => "Ogg",
    };
    let reader = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| {
            Error::Format(format!(
                "no valid {what} frames found ({e}); the stream may be damaged or violate its specification"
            ))
        })?;
    let track = reader.default_track(TrackType::Audio).ok_or_else(|| Error::Format("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| Error::Format("track has no audio parameters".into()))?
        .clone();
    let (track_id, num_frames, delay, padding) =
        (track.id, track.num_frames, track.delay.unwrap_or(0) as u64, track.padding.unwrap_or(0) as u64);
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Format(format!("no decoder: {e}")))?;
    let rate = params.sample_rate.ok_or_else(|| Error::Format("unknown sample rate".into()))?;
    let channels =
        params.channels.as_ref().map(|c| c.count()).ok_or_else(|| Error::Format("unknown channel count".into()))?
            as u16;
    let codec = match params.codec {
        CODEC_ID_MP1 => "MPEG audio Layer I",
        CODEC_ID_MP2 => "MPEG audio Layer II",
        CODEC_ID_MP3 => "MPEG audio Layer III",
        CODEC_ID_VORBIS => "Ogg Vorbis",
        _ => "compressed audio",
    };
    Ok(Opened { reader, decoder, track_id, rate, channels, codec, num_frames, delay, padding })
}

pub struct SymphSource {
    origin: Box<dyn ReopenMedia>,
    container: Container,
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    format: PcmFormat,
    info: SourceInfo,
    total: Option<u64>,
    /// MPEG: untrimmed output (delay frames ahead of Symphonia's timeline). Vorbis: 0.
    ts_offset: i64,
    keep_all_frames: bool,
    /// After a seek: output starts exactly at this frame.
    target: Option<u64>,
    scratch: Vec<f32>,
    byte_len: Option<u64>,
}

impl SymphSource {
    pub fn new(source: Box<dyn ReopenMedia>, container: Container, float_output: bool) -> Result<Self> {
        let byte_len = source.byte_len();
        let origin = source.reopen_media()?;
        let o = probe(source, container)?;
        let keep_all_frames = matches!(container, Container::MpegAudio);
        let (ts_offset, total) = if keep_all_frames {
            (o.delay as i64, o.num_frames.map(|n| n + o.delay + o.padding))
        } else {
            (0, o.num_frames)
        };
        let kind = if float_output { SampleKind::F32 } else { SampleKind::I16 };
        let mut info = SourceInfo { codec: o.codec.into(), rate: o.rate, channels: o.channels, ..Default::default() };
        if keep_all_frames && (o.delay > 0 || o.padding > 0) {
            info.notes.push(format!(
                "encoder tag lists {} delay and {} padding frames; they are kept, as the game's player does not trim them",
                o.delay, o.padding
            ));
        }
        Ok(Self {
            origin,
            container,
            reader: o.reader,
            decoder: o.decoder,
            track_id: o.track_id,
            format: PcmFormat::new(kind, o.channels, o.rate),
            info,
            total,
            ts_offset,
            keep_all_frames,
            target: None,
            scratch: Vec::new(),
            byte_len,
        })
    }

    pub fn info_mut(&mut self) -> &mut SourceInfo {
        &mut self.info
    }

    /// Average bitrate from the byte length and duration, if both are known.
    pub fn estimate_bitrate(&self, frames: u64) -> Option<u32> {
        let bytes = self.byte_len?;
        if frames == 0 {
            return None;
        }
        Some(((bytes as f64 * 8.0) / (frames as f64 / self.format.rate as f64) / 1000.0).round() as u32)
    }

    fn restart(&mut self) -> Result<()> {
        let o = probe(self.origin.reopen_media()?, self.container)?;
        self.reader = o.reader;
        self.decoder = o.decoder;
        self.track_id = o.track_id;
        Ok(())
    }
}

impl PcmSource for SymphSource {
    fn format(&self) -> &PcmFormat {
        &self.format
    }
    fn info(&self) -> &SourceInfo {
        &self.info
    }
    fn total_frames(&self) -> Option<u64> {
        self.total
    }

    fn read(&mut self, out: &mut Vec<u8>) -> Result<usize> {
        loop {
            let mut packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(0),
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(e) => return Err(Error::Format(format!("demux error: {e}"))),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let trim_start = packet.trim_start.get() as i64;
            if self.keep_all_frames {
                // keep every decoded MPEG frame (see module docs)
                packet.trim_start = SymDuration::ZERO;
                packet.trim_end = SymDuration::ZERO;
            }
            let start = packet.pts.get() + if self.keep_all_frames { self.ts_offset } else { trim_start };
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(SymError::DecodeError(_)) => continue, // corrupt frame: skipped, like the game's decoder
                Err(e) => return Err(Error::Format(format!("decode error: {e}"))),
            };
            if buf.spec().channels().count() != self.format.channels as usize || buf.spec().rate() != self.format.rate {
                self.info
                    .notes
                    .push("skipped a frame whose sample rate or channel count differs from the stream".into());
                continue;
            }
            let frames = buf.frames();
            if frames == 0 {
                continue;
            }
            self.scratch.resize(buf.samples_interleaved(), 0.0);
            buf.copy_to_slice_interleaved(&mut self.scratch[..]);
            let drop = match self.target {
                Some(t) => {
                    let d = (t as i64 - start).clamp(0, frames as i64) as usize;
                    if d < frames {
                        self.target = None;
                    }
                    d
                }
                None => 0,
            };
            let ch = self.format.channels as usize;
            let s = &self.scratch[drop * ch..];
            match self.format.kind {
                SampleKind::F32 => s.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes())),
                _ => s.iter().for_each(|&v| out.extend_from_slice(&f32_to_i16(v).to_le_bytes())),
            }
            let n = frames - drop;
            if n > 0 {
                return Ok(n);
            }
        }
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let frame = self.total.map_or(frame, |t| frame.min(t));
        if frame <= SEEK_PREROLL {
            // close to the start: decode from the beginning
            self.restart()?;
        } else {
            let ts = (frame - SEEK_PREROLL) as i64 - self.ts_offset;
            let to = SeekTo::Timestamp { ts: Timestamp::from(ts), track_id: self.track_id };
            if self.reader.seek(SeekMode::Accurate, to).is_err() {
                self.restart()?;
            }
            self.decoder.reset();
        }
        self.target = Some(frame);
        Ok(())
    }
}
