//! Opening an asset for decoding.

use ef_core::vfs::Vfs;
use serde_json::{Map, Value, json};

use crate::catalog::{AssetKind, AudioAsset};
use crate::movie::{self, AudioElementaryStream};
use crate::pcm::PcmSource;
use crate::sources::{self, RawPcmSource, VbSource};
use crate::stream::{ByteStream, MemStream, Xform};
use crate::symph::{Container, SymphSource};
use crate::wav::Markers;
use crate::{Error, Result, sa};

#[derive(Debug, Clone, Copy)]
pub struct DecodeOptions {
    /// Decode MPEG audio / Vorbis to 32-bit float (full decoder precision) instead of 16-bit PCM.
    pub float_output: bool,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self { float_output: true }
    }
}

pub struct Opened {
    pub source: Box<dyn PcmSource>,
    pub markers: Markers,
    /// Extra facts for the manifest (beat data, loop points, movie timing, ...).
    pub extra: Map<String, Value>,
    /// Movie: the untouched audio bitstream of the decoded stream and its file extension.
    pub bitstream: Option<(&'static str, Vec<u8>)>,
    /// Movie: further audio streams beyond the first.
    pub more_streams: Vec<AudioElementaryStream>,
}

impl Opened {
    fn plain(source: Box<dyn PcmSource>) -> Self {
        Self { source, markers: Markers::default(), extra: Map::new(), bitstream: None, more_streams: Vec::new() }
    }
}

/// MPEG audio layer of the first frame header, used to name the original bitstream file.
pub fn mpeg_layer_ext(data: &[u8]) -> &'static str {
    for w in data.windows(2).take(1 << 16) {
        if w[0] == 0xFF && w[1] & 0xE0 == 0xE0 {
            match (w[1] >> 1) & 3 {
                3 => return "mp1",
                2 => return "mp2",
                1 => return "mp3",
                _ => {}
            }
        }
    }
    "mpa"
}

pub fn open_es(es: &AudioElementaryStream, opts: DecodeOptions) -> Result<Box<dyn PcmSource>> {
    Ok(Box::new(SymphSource::new(Box::new(MemStream::new(es.data.clone())), Container::MpegAudio, opts.float_output)?))
}

pub fn open(asset: &AudioAsset, vfs: &dyn Vfs, opts: DecodeOptions) -> Result<Opened> {
    if !asset.support.can_decode() {
        return Err(Error::Unsupported(asset.support.explanation().into()));
    }
    match &asset.kind {
        AssetKind::LegacyPcm { raw, offset, size, rate, channels, bits } => {
            use crate::pcm::{PcmFormat, SampleKind};
            let kind = match bits {
                8 => SampleKind::U8,
                16 => SampleKind::I16,
                _ => return Err(Error::Format("invalid legacy PCM depth".into())),
            };
            if *rate == 0
                || !matches!(channels, 1 | 2)
                || !(*size as usize).is_multiple_of(kind.bytes() * *channels as usize)
            {
                return Err(Error::Format("invalid legacy PCM sample metadata".into()));
            }
            let s = ByteStream::open_range(vfs, raw, *offset as u64, *size as u64, Xform::None)?;
            let mut o = Opened::plain(Box::new(RawPcmSource::with_format(
                s,
                PcmFormat::new(kind, *channels, *rate),
                "GTA1 SDT PCM",
            )));
            o.extra.insert("sdt_raw_offset".into(), json!(offset));
            Ok(o)
        }
        AssetKind::Stream { path } => {
            let (source, markers) = sources::open_stream(ByteStream::open(vfs, path)?, opts.float_output)?;
            Ok(Opened { markers, ..Opened::plain(source) })
        }
        AssetKind::Adf { path } => {
            let s = ByteStream::open(vfs, path)?.with_xform(Xform::Xor(0x22));
            let mut src = SymphSource::new(Box::new(s), Container::MpegAudio, opts.float_output)?;
            src.info_mut().notes.push("ADF: MPEG audio with every byte XOR 0x22 (decoded transparently)".into());
            Ok(Opened::plain(Box::new(src)))
        }
        AssetKind::Vb { path, rate, channels } => {
            let mut o = Opened::plain(Box::new(VbSource::new(ByteStream::open(vfs, path)?, *rate, *channels)?));
            o.extra.insert("vb_sample_rate_source".into(), json!("game table (IsThisTrackAt16KHz)"));
            Ok(o)
        }
        AssetKind::Movie { path } => {
            let mut f = ByteStream::open(vfs, path)?;
            let mut streams = movie::demux_audio(&mut f)?;
            if streams.is_empty() {
                return Err(Error::Format("no MPEG audio stream in this movie".into()));
            }
            let first = streams.remove(0);
            let src = open_es(&first, opts)?;
            let mut o = Opened::plain(src);
            o.extra.insert("stream_id".into(), json!(format!("0x{:02X}", first.stream_id)));
            o.extra.insert("audio_bitstream_bytes".into(), json!(first.data.len()));
            if let Some(pts) = first.first_pts {
                o.extra.insert("first_audio_pts_90khz".into(), json!(pts));
                o.extra.insert("audio_start_in_movie_seconds".into(), json!(pts as f64 / 90000.0));
            }
            o.bitstream = Some((mpeg_layer_ext(&first.data), first.data));
            o.more_streams = streams;
            Ok(o)
        }
        AssetKind::Sfx { raw, offset, size, rate, loop_start, loop_end }
        | AssetKind::Gta2Sfx { raw, offset, size, rate, loop_start, loop_end, .. } => {
            let s = ByteStream::open_range(vfs, raw, *offset as u64, *size as u64, Xform::None)?;
            let mut o = Opened::plain(Box::new(RawPcmSource::new(s, *rate, "PCM 16-bit (SFX.RAW)")));
            // SDT loop points are byte offsets into the 16-bit sample (re3 divides them by 2); -1 = end
            if let AssetKind::Gta2Sfx { random_displacement, .. } = &asset.kind {
                o.extra.insert("random_displacement".into(), json!(random_displacement));
            }
            o.extra.insert("sfx_raw_offset".into(), json!(offset));
            o.extra.insert("loop_start_bytes".into(), json!(loop_start));
            o.extra.insert("loop_end_bytes".into(), json!(loop_end));
            o.extra.insert("loop_start_frame".into(), json!(loop_start / 2));
            o.extra.insert("loop_end_frame".into(), if *loop_end == -1 { json!("end") } else { json!(loop_end / 2) });
            Ok(o)
        }
        AssetKind::SaTrack { pack, offset, size } => {
            let (ogg, header) = sa::open_track(vfs, pack, *offset, *size)?;
            let mut o = Opened::plain(Box::new(SymphSource::new(Box::new(ogg), Container::Ogg, opts.float_output)?));
            o.extra.insert("stream_pack_offset".into(), json!(offset));
            o.extra.insert("track_header".into(), header);
            Ok(o)
        }
        AssetKind::SaSfx { pak, bank, sound, offset, size, rate, loop_start, headroom } => {
            let s = ByteStream::open_range(vfs, &format!("AUDIO/SFX/{pak}"), *offset, *size, Xform::None)?;
            let mut o = Opened::plain(Box::new(RawPcmSource::new(s, *rate as u32, "PCM 16-bit (SFX bank)")));
            o.extra.insert("bank".into(), json!(bank));
            o.extra.insert("sound".into(), json!(sound));
            o.extra.insert("pak_offset".into(), json!(offset));
            o.extra.insert("loop_start_offset".into(), json!(loop_start));
            o.extra.insert("headroom".into(), json!(headroom));
            Ok(o)
        }
    }
}
