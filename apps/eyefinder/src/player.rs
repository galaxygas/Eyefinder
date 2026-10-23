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
        // `Some(0)` = exhausted (rodio `Source` contract); otherwise one format until the end.
        if self.finished.load(Ordering::Relaxed) { Some(0) } else { None }
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

/// The output device and the player of the current track.
///
/// Every track gets its own [`rodio::Player`], with the track appended *before* the player is
/// added to the mixer. rodio 0.23.0 converts a player's whole queue through one
/// `UniformSourceIterator` that `Mixer::add` builds from the queue's format (`mixer.rs` 57-65,
/// `queue.rs` 172-210), and that converter only re-reads the format at span boundaries
/// (`source/uniform.rs` 49-89). A queue reused across tracks therefore kept converting every later
/// track at the first track's sample rate and channel count, which played it too fast or too slow.
pub struct Audio {
    device: rodio::MixerDeviceSink,
    /// Set by the device's error callback; the device is reopened before the next track.
    device_failed: Arc<AtomicBool>,
    player: Option<rodio::Player>,
    paused: bool,
    pub volume: f32,
}

fn open_device(failed: &Arc<AtomicBool>) -> Result<rodio::MixerDeviceSink, String> {
    let f = failed.clone();
    let mut device = rodio::DeviceSinkBuilder::from_default_device()
        .and_then(|b| b.with_error_callback(move |_| f.store(true, Ordering::Relaxed)).open_stream())
        .or_else(|_| rodio::DeviceSinkBuilder::open_default_sink())
        .map_err(|e| format!("no audio output device: {e}"))?;
    device.log_on_drop(false);
    failed.store(false, Ordering::Relaxed);
    Ok(device)
}

impl Audio {
    pub fn open() -> Result<Self, String> {
        let device_failed = Arc::new(AtomicBool::new(false));
        let device = open_device(&device_failed)?;
        Ok(Self { device, device_failed, player: None, paused: false, volume: 0.8 })
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v;
        if let Some(p) = &self.player {
            p.set_volume(v);
        }
    }
    pub fn pause(&mut self) {
        self.paused = true;
        if let Some(p) = &self.player {
            p.pause();
        }
    }
    pub fn resume(&mut self) {
        self.paused = false;
        if let Some(p) = &self.player {
            p.play();
        }
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
    }

    /// Starts playing `asset` from `start_frame`. Blocks only until the stream format is known.
    pub fn play(&mut self, vfs: Arc<dyn Vfs>, asset: AudioAsset, asset_index: usize, start_frame: u64) -> Playback {
        self.stop();
        if self.device_failed.load(Ordering::Relaxed)
            && let Ok(d) = open_device(&self.device_failed)
        {
            self.device = d;
        }
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
                let (player, queue) = rodio::Player::new();
                player.append(ChunkSource {
                    rx,
                    cur: Vec::new(),
                    idx: 0,
                    channels: facts.channels.max(1),
                    rate: facts.rate.max(1),
                    consumed: consumed.clone(),
                    finished: finished.clone(),
                    silence_left: 0,
                });
                player.set_volume(self.volume);
                self.device.mixer().add(queue);
                self.paused = false;
                self.player = Some(player);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn track(rate: u32, channels: u16, frames: usize) -> ChunkSource {
        let (tx, rx) = bounded(1);
        tx.send(vec![0.5f32; frames * channels as usize]).unwrap();
        ChunkSource {
            rx,
            cur: Vec::new(),
            idx: 0,
            channels,
            rate,
            consumed: Arc::new(AtomicU64::new(0)),
            finished: Arc::new(AtomicBool::new(false)),
            silence_left: 0,
        }
    }

    /// Output samples (48 kHz stereo) carrying the track's signal, as a device mixer sees them.
    fn converted(src: ChunkSource) -> usize {
        let (player, queue) = rodio::Player::new();
        player.append(src);
        let out =
            rodio::source::UniformSourceIterator::new(queue, NonZero::new(2).unwrap(), NonZero::new(48000).unwrap());
        // the queue keeps the stream alive with silence after the track: count up to it
        out.take(1_000_000).skip_while(|v| *v == 0.0).take_while(|v| *v != 0.0).count()
    }

    #[test]
    fn every_track_is_converted_at_its_own_rate() {
        // one second each, whatever the order: 48000 stereo frames = 96000 samples (± resampler edge)
        for (rate, ch) in [(22050, 1), (44100, 2), (5500, 1), (48000, 2), (22050, 2)] {
            let n = converted(track(rate, ch, rate as usize));
            assert!((95_900..=96_100).contains(&n), "{rate} Hz {ch} ch: {n} samples");
        }
    }
}
