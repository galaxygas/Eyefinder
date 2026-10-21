//! Streaming playback and waveform analysis.
//!
//! A decode thread pushes interleaved f32 chunks into a bounded queue; a rodio `Source` drains it on
//! the audio thread without ever blocking. Seeking restarts the decode thread at the new frame, so
//! hour-long radio streams never have to sit in memory.

use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, TryRecvError, bounded};
use ef_audio::pcm::to_f32;
use ef_audio::{AudioAsset, DecodeOptions, SourceInfo};
use ef_core::vfs::Vfs;

/// What the decode thread learned about the stream.
#[derive(Clone, Default)]
pub struct StreamFacts {
    pub info: SourceInfo,
    pub rate: u32,
    pub channels: u16,
    pub total_frames: Option<u64>,
    pub output: String,
}

struct ChunkSource {
    rx: Receiver<Vec<f32>>,
    cur: Vec<f32>,
    idx: usize,
    channels: u16,
    rate: u32,
    /// samples handed to the device (for the position display)
    consumed: Arc<AtomicU64>,
    finished: Arc<AtomicBool>,
    silence_left: u16,
}

impl Iterator for ChunkSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.silence_left > 0 {
            self.silence_left -= 1;
            return Some(0.0);
        }
        if self.idx >= self.cur.len() {
            match self.rx.try_recv() {
                Ok(c) => {
                    self.cur = c;
                    self.idx = 0;
                }
                Err(TryRecvError::Empty) => {
                    // decoder is behind: output one silent frame (chunks always hold whole frames,
                    // so we are on a frame boundary here and channels stay aligned)
                    self.silence_left = self.channels - 1;
                    return Some(0.0);
                }
                Err(TryRecvError::Disconnected) => {
                    self.finished.store(true, Ordering::Relaxed);
                    return None;
                }
            }
        }
        let v = self.cur[self.idx];
        self.idx += 1;
        self.consumed.fetch_add(1, Ordering::Relaxed);
        Some(v)
    }
}

impl rodio::Source for ChunkSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct Playback {
    pub asset_index: usize,
    pub facts: StreamFacts,
    start_frame: u64,
    consumed: Arc<AtomicU64>,
    finished: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    pub error: Option<String>,
}

impl Playback {
    pub fn position_frames(&self) -> u64 {
        self.start_frame + self.consumed.load(Ordering::Relaxed) / self.facts.channels.max(1) as u64
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub struct Audio {
    _device: rodio::MixerDeviceSink,
    player: rodio::Player,
    pub volume: f32,
}

impl Audio {
    pub fn open() -> Result<Self, String> {
        let mut device =
            rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| format!("no audio output device: {e}"))?;
        device.log_on_drop(false);
        let player = rodio::Player::connect_new(device.mixer());
        Ok(Self { _device: device, player, volume: 0.8 })
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v;
        self.player.set_volume(v);
    }
    pub fn pause(&self) {
        self.player.pause();
    }
    pub fn resume(&self) {
        self.player.play();
    }
    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }
    pub fn stop(&self) {
        self.player.stop();
    }

    /// Starts playing `asset` from `start_frame`. Blocks only until the stream format is known.
    pub fn play(&mut self, vfs: Arc<dyn Vfs>, asset: AudioAsset, asset_index: usize, start_frame: u64) -> Playback {
        self.player.stop();
        let (tx, rx) = bounded::<Vec<f32>>(16);
        let (facts_tx, facts_rx) = bounded::<Result<StreamFacts, String>>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        thread::Builder::new()
            .name("decode".into())
            .spawn(move || {
                let opened = ef_audio::open::open(&asset, vfs.as_ref(), DecodeOptions { float_output: true });
                let mut o = match opened {
                    Ok(o) => o,
                    Err(e) => {
                        let _ = facts_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                if start_frame > 0
                    && let Err(e) = o.source.seek(start_frame)
                {
                    let _ = facts_tx.send(Err(format!("seek failed: {e}")));
                    return;
                }
                let f = o.source.format().clone();
                let _ = facts_tx.send(Ok(StreamFacts {
                    info: o.source.info().clone(),
                    rate: f.rate,
                    channels: f.channels,
                    total_frames: o.source.total_frames(),
                    output: f.kind.describe().to_string(),
                }));
                let mut bytes = Vec::with_capacity(1 << 18);
                let mut floats = Vec::new();
                while !stop_t.load(Ordering::Relaxed) {
                    bytes.clear();
                    match o.source.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            floats.clear();
                            to_f32(f.kind, &bytes, &mut floats);
                            if tx.send(std::mem::take(&mut floats)).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .expect("spawn decode thread");
        let consumed = Arc::new(AtomicU64::new(0));
        let finished = Arc::new(AtomicBool::new(false));
        let facts =
            facts_rx.recv_timeout(Duration::from_secs(20)).unwrap_or_else(|_| Err("decoder did not respond".into()));
        match facts {
            Ok(facts) => {
                self.player.append(ChunkSource {
                    rx,
                    cur: Vec::new(),
                    idx: 0,
                    channels: facts.channels.max(1),
                    rate: facts.rate.max(1),
                    consumed: consumed.clone(),
                    finished: finished.clone(),
                    silence_left: 0,
                });
                self.player.set_volume(self.volume);
                self.player.play();
                Playback { asset_index, facts, start_frame, consumed, finished, stop, error: None }
            }
            Err(e) => {
                finished.store(true, Ordering::Relaxed);
                Playback {
                    asset_index,
                    facts: StreamFacts::default(),
                    start_frame,
                    consumed,
                    finished,
                    stop,
                    error: Some(e),
                }
            }
        }
    }
}

/// Min/max envelope of a whole asset, computed in the background.
pub struct Waveform {
    pub asset_index: usize,
    pub peaks: Arc<Mutex<Vec<(f32, f32)>>>,
    /// frames represented by one peak
    pub frames_per_peak: Arc<AtomicU64>,
    pub done: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Waveform {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Waveform {
    pub fn compute(vfs: Arc<dyn Vfs>, asset: AudioAsset, asset_index: usize, ctx: egui::Context) -> Self {
        let peaks = Arc::new(Mutex::new(Vec::new()));
        let fpp = Arc::new(AtomicU64::new(0));
        let done = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, f, d, c) = (peaks.clone(), fpp.clone(), done.clone(), cancel.clone());
        thread::Builder::new()
            .name("waveform".into())
            .spawn(move || {
                let Ok(mut o) = ef_audio::open::open(&asset, vfs.as_ref(), DecodeOptions { float_output: true }) else {
                    d.store(true, Ordering::Relaxed);
                    return;
                };
                let fmt = o.source.format().clone();
                let ch = fmt.channels.max(1) as usize;
                // ~3000 peaks across the asset; 20 ms per peak when the length is unknown
                let per = o.source.total_frames().map_or(fmt.rate as u64 / 50, |t| (t / 3000).max(1)).max(1);
                f.store(per, Ordering::Relaxed);
                let (mut lo, mut hi, mut n) = (0f32, 0f32, 0u64);
                let mut bytes = Vec::new();
                let mut floats = Vec::new();
                while !c.load(Ordering::Relaxed) {
                    bytes.clear();
                    match o.source.read(&mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    floats.clear();
                    to_f32(fmt.kind, &bytes, &mut floats);
                    let mut local = Vec::new();
                    for frame in floats.chunks_exact(ch) {
                        let v = frame.iter().sum::<f32>() / ch as f32;
                        lo = lo.min(v);
                        hi = hi.max(v);
                        n += 1;
                        if n == per {
                            local.push((lo, hi));
                            (lo, hi, n) = (0.0, 0.0, 0);
                        }
                    }
                    if !local.is_empty() {
                        p.lock().unwrap().extend(local);
                        ctx.request_repaint();
                    }
                }
                if n > 0 {
                    p.lock().unwrap().push((lo, hi));
                }
                d.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            })
            .expect("spawn waveform thread");
        Self { asset_index, peaks, frames_per_peak: fpp, done, cancel }
    }
}
