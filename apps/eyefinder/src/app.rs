use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ef_audio::export::{ExportOptions, ExportReport, Progress};
use ef_audio::{AssetKind, AudioAsset, Catalog, Category, DecodeOptions};
use ef_core::{GameInstall, SupportLevel};
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

use crate::player::{Audio, Playback, Waveform};
use crate::theme::{self, Palette, semibold};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library: Vec<PathBuf>,
    pub out_root: Option<PathBuf>,
    pub theme: ThemeChoice,
    pub float_output: bool,
    pub split_markers: bool,
    pub keep_bitstream: bool,
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library: Vec::new(),
            out_root: None,
            theme: ThemeChoice::Dark,
            float_output: true,
            split_markers: true,
            keep_bitstream: true,
            volume: 0.8,
        }
    }
}

enum LoadState {
    Loading,
    Ready { install: GameInstall, catalog: Arc<Catalog> },
    Failed(String),
}

struct Entry {
    path: PathBuf,
    state: LoadState,
}

type LoadResult = (PathBuf, Result<(GameInstall, Catalog), String>);

struct ExportDialog {
    entry: usize,
    assets: Vec<usize>,
    scope: String,
    out_dir: PathBuf,
}

struct Job {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<ExportReport, String>>>>,
    started: Instant,
    out_dir: PathBuf,
}

pub struct EyefinderApp {
    settings: Settings,
    entries: Vec<Entry>,
    selected_entry: Option<usize>,
    load_tx: Sender<LoadResult>,
    load_rx: Receiver<LoadResult>,
    category: Option<Category>,
    search: String,
    view: Vec<usize>,
    view_key: Option<(usize, Option<Category>, String)>,
    selection: BTreeSet<usize>,
    anchor: Option<usize>,
    focused: Option<usize>,
    audio: Result<Audio, String>,
    playback: Option<Playback>,
    /// library entry the current playback belongs to
    playing_entry: Option<usize>,
    waveform: Option<Waveform>,
    export: Option<ExportDialog>,
    job: Option<Job>,
    toast: Option<(String, Instant)>,
    show_about: bool,
    show_notices: bool,
}

const ROW_H: f32 = 30.0;

impl EyefinderApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        apply_theme(&cc.egui_ctx, settings.theme);
        let (load_tx, load_rx) = unbounded();
        let mut audio = Audio::open();
        if let Ok(a) = &mut audio {
            a.set_volume(settings.volume);
        }
        let mut app = Self {
            entries: Vec::new(),
            selected_entry: None,
            load_tx,
            load_rx,
            category: None,
            search: String::new(),
            view: Vec::new(),
            view_key: None,
            selection: BTreeSet::new(),
            anchor: None,
            focused: None,
            audio,
            playback: None,
            playing_entry: None,
            waveform: None,
            export: None,
            job: None,
            toast: None,
            show_about: false,
            show_notices: false,
            settings,
        };
        for p in app.settings.library.clone() {
            app.add_game(p, &cc.egui_ctx, false);
        }
        // paths given on the command line ("Open with", drag onto the executable) are added and selected
        for a in std::env::args_os().skip(1) {
            let p = PathBuf::from(a);
            if p.exists() {
                app.add_game(std::path::absolute(&p).unwrap_or(p), &cc.egui_ctx, true);
            }
        }
        app
    }

    fn add_game(&mut self, path: PathBuf, ctx: &egui::Context, select: bool) {
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            if select {
                self.select_entry(i);
            }
            return;
        }
        self.entries.push(Entry { path: path.clone(), state: LoadState::Loading });
        if !self.settings.library.contains(&path) {
            self.settings.library.push(path.clone());
        }
        if select || self.selected_entry.is_none() {
            self.select_entry(self.entries.len() - 1);
        }
        let tx = self.load_tx.clone();
        let ctx = ctx.clone();
        thread::Builder::new()
            .name("load-game".into())
            .spawn(move || {
                let r = ef_core::open_and_detect(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|i| ef_audio::catalog::build(&i).map(|c| (i, c)).map_err(|e| e.to_string()));
                let _ = tx.send((path, r));
                ctx.request_repaint();
            })
            .expect("spawn loader");
    }

    fn select_entry(&mut self, i: usize) {
        if self.selected_entry != Some(i) {
            self.selected_entry = Some(i);
            self.category = None;
            self.selection.clear();
            self.anchor = None;
            self.focused = None;
            self.view_key = None;
        }
    }

    fn remove_entry(&mut self, i: usize) {
        let e = self.entries.remove(i);
        self.settings.library.retain(|p| *p != e.path);
        // indexes after the removed entry shift down by one
        let shift = |v: &mut Option<usize>| match *v {
            Some(x) if x > i => *v = Some(x - 1),
            _ => {}
        };
        if self.playing_entry == Some(i) {
            self.stop_playback();
        }
        shift(&mut self.playing_entry);
        if self.selected_entry == Some(i) {
            self.selected_entry = if self.entries.is_empty() { None } else { Some(i.min(self.entries.len() - 1)) };
            self.selection.clear();
            self.focused = None;
            self.category = None;
        } else {
            shift(&mut self.selected_entry);
        }
        self.view_key = None;
    }

    fn current(&self) -> Option<(&GameInstall, &Arc<Catalog>)> {
        self.entry(self.selected_entry?)
    }

    fn entry(&self, i: usize) -> Option<(&GameInstall, &Arc<Catalog>)> {
        match &self.entries.get(i)?.state {
            LoadState::Ready { install, catalog } => Some((install, catalog)),
            _ => None,
        }
    }

    fn refresh_view(&mut self) {
        let Some(sel) = self.selected_entry else {
            self.view.clear();
            return;
        };
        let key = (sel, self.category, self.search.clone());
        if self.view_key.as_ref() == Some(&key) {
            return;
        }
        let needle = self.search.to_lowercase();
        self.view = match self.current() {
            Some((_, cat)) => cat
                .assets
                .iter()
                .enumerate()
                .filter(|(_, a)| self.category.is_none_or(|c| a.category == c))
                .filter(|(_, a)| {
                    needle.is_empty()
                        || a.title.to_lowercase().contains(&needle)
                        || a.game_id.to_lowercase().contains(&needle)
                        || a.source.to_lowercase().contains(&needle)
                        || a.group.as_deref().is_some_and(|g| g.to_lowercase().contains(&needle))
                })
                .map(|(i, _)| i)
                .collect(),
            None => Vec::new(),
        };
        self.selection.retain(|i| self.view.contains(i));
        self.view_key = Some(key);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((path, r)) = self.load_rx.try_recv() {
            if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
                e.state = match r {
                    Ok((install, catalog)) => LoadState::Ready { install, catalog: Arc::new(catalog) },
                    Err(err) => LoadState::Failed(err),
                };
                self.view_key = None;
            }
        }
        if let Some(pb) = &self.playback {
            if pb.finished() && self.audio.as_ref().is_ok_and(|_| true) && pb.error.is_none() {
                // keep the bar showing the finished track; repaint stops being needed
            } else {
                ctx.request_repaint_after(Duration::from_millis(33));
            }
        }
        if let Some(job) = &self.job
            && job.result.lock().unwrap().is_none()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.toast.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4)) {
            self.toast = None;
        }
    }

    // ---------------------------------------------------------------------------------------------
    // playback

    /// Whether `idx` of the selected entry is the asset currently loaded in the player.
    fn is_playing(&self, idx: usize) -> bool {
        self.playing_entry.is_some()
            && self.playing_entry == self.selected_entry
            && self.playback.as_ref().is_some_and(|p| p.asset_index == idx)
    }

    fn play(&mut self, entry: usize, idx: usize, start_frame: u64, ctx: &egui::Context) {
        let Some((install, cat)) = self.entry(entry) else { return };
        let asset = cat.assets[idx].clone();
        let vfs = install.vfs.clone();
        if !asset.support.can_decode() {
            self.toast = Some((format!("{} cannot be played: {}", asset.title, asset.support.label()), Instant::now()));
            return;
        }
        let Ok(audio) = &mut self.audio else {
            self.toast = Some(("No audio output device is available".into(), Instant::now()));
            return;
        };
        let new_asset =
            self.playing_entry != Some(entry) || self.playback.as_ref().is_none_or(|p| p.asset_index != idx);
        self.playback = Some(audio.play(vfs.clone(), asset.clone(), idx, start_frame));
        self.playing_entry = Some(entry);
        if new_asset || self.waveform.as_ref().is_none_or(|w| w.asset_index != idx) {
            self.waveform = Some(Waveform::compute(vfs, asset, idx, ctx.clone()));
        }
        if let Some(e) = self.playback.as_ref().and_then(|p| p.error.clone()) {
            self.toast = Some((format!("Playback failed: {e}"), Instant::now()));
        }
    }

    fn stop_playback(&mut self) {
        if let Ok(a) = &self.audio {
            a.stop();
        }
        self.playback = None;
        self.playing_entry = None;
        self.waveform = None;
    }

    fn toggle_pause(&mut self, ctx: &egui::Context) {
        match (&self.audio, &self.playback) {
            (Ok(a), Some(p)) if !p.finished() => {
                if a.is_paused() {
                    a.resume()
                } else {
                    a.pause()
                }
            }
            (Ok(_), Some(p)) => {
                let i = p.asset_index;
                if let Some(e) = self.playing_entry {
                    self.play(e, i, 0, ctx);
                }
            }
            _ => {
                if let (Some(e), Some(i)) = (self.selected_entry, self.focused) {
                    self.play(e, i, 0, ctx);
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // export

    fn default_out_dir(&self, install: &GameInstall) -> PathBuf {
        let root = self
            .settings
            .out_root
            .clone()
            .or_else(|| dirs::document_dir().or_else(dirs::home_dir).map(|d| d.join("Eyefinder Exports")))
            .or_else(|| std::env::current_dir().ok().map(|d| d.join("Eyefinder Exports")))
            .unwrap_or_else(|| PathBuf::from("Eyefinder Exports"));
        root.join(ef_audio::catalog::sanitize(&format!("{} ({})", install.game.name(), install.platform.short())))
    }

    fn open_export(&mut self, assets: Vec<usize>, scope: String) {
        let Some(entry) = self.selected_entry else { return };
        let Some((install, cat)) = self.current() else { return };
        let assets: Vec<usize> = assets.into_iter().filter(|&i| cat.assets[i].support.can_decode()).collect();
        if assets.is_empty() {
            self.toast = Some(("Nothing to export: the chosen items cannot be decoded".into(), Instant::now()));
            return;
        }
        let out_dir = self.default_out_dir(install);
        self.export = Some(ExportDialog { entry, assets, scope, out_dir });
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        let Some(d) = self.export.take() else { return };
        let LoadState::Ready { install, catalog } = &self.entries[d.entry].state else { return };
        let install = install.clone();
        let assets: Vec<AudioAsset> = d.assets.iter().map(|&i| catalog.assets[i].clone()).collect();
        let opts = ExportOptions {
            out_dir: d.out_dir.clone(),
            decode: DecodeOptions { float_output: self.settings.float_output },
            split_markers: self.settings.split_markers,
            keep_movie_bitstream: self.settings.keep_bitstream,
        };
        if let Some(parent) = d.out_dir.parent() {
            self.settings.out_root = Some(parent.to_path_buf());
        }
        let progress = Arc::new(Mutex::new(Progress { done: 0, total: assets.len(), current: String::new() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let (p, c, r, ctx) = (progress.clone(), cancel.clone(), result.clone(), ctx.clone());
        thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                let refs: Vec<&AudioAsset> = assets.iter().collect();
                let res = ef_audio::export::export(
                    &install,
                    &refs,
                    &opts,
                    |pr| {
                        *p.lock().unwrap() = pr;
                        ctx.request_repaint();
                    },
                    &c,
                )
                .map_err(|e| e.to_string());
                *r.lock().unwrap() = Some(res);
                ctx.request_repaint();
            })
            .expect("spawn export");
        self.job = Some(Job { progress, cancel, result, started: Instant::now(), out_dir: d.out_dir });
    }
}

fn apply_theme(ctx: &egui::Context, t: ThemeChoice) {
    ctx.set_theme(match t {
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::System => egui::ThemePreference::System,
    });
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Player clock: tenths of a second for clips under a minute, whole seconds otherwise.
fn fmt_player_time(frames: u64, rate: u32, total: Option<u64>) -> String {
    if rate > 0 && total.is_some_and(|t| t < 60 * rate as u64) {
        let tenths = frames * 10 / rate as u64;
        return format!("0:{:02}.{}", tenths / 10, tenths % 10);
    }
    fmt_dur(frames, rate)
}

fn fmt_dur(frames: u64, rate: u32) -> String {
    if rate == 0 {
        return "--:--".into();
    }
    let s = frames / rate as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn format_label(a: &AudioAsset) -> String {
    match &a.kind {
        AssetKind::Stream { path } => path.rsplit('.').next().unwrap_or("").to_ascii_uppercase(),
        AssetKind::Adf { .. } => "ADF (MP3)".into(),
        AssetKind::Vb { .. } => "VB (PS-ADPCM)".into(),
        AssetKind::Movie { .. } => "MPEG movie".into(),
        AssetKind::Sfx { .. } => "PCM (SFX.RAW)".into(),
        AssetKind::SaTrack { .. } => "Ogg Vorbis".into(),
        AssetKind::SaSfx { .. } => "PCM (bank)".into(),
    }
}

fn category_icon(c: Category) -> &'static str {
    match c {
        Category::Radio => icon::RADIO,
        Category::Streams => icon::MUSIC_NOTES,
        Category::PoliceAmbience => icon::SIREN,
        Category::Announcements => icon::MEGAPHONE,
        Category::Cutscenes => icon::FILM_SLATE,
        Category::MusicCues => icon::MUSIC_NOTE,
        Category::MissionAudio => icon::CHAT_CIRCLE_TEXT,
        Category::Movies => icon::FILM_STRIP,
        Category::SfxBank => icon::WAVEFORM,
        Category::Other => icon::FILE_AUDIO,
    }
}

fn open_in_file_manager(p: &Path) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    let _ = r;
}

fn support_badge(ui: &mut Ui, p: &Palette, s: SupportLevel) -> egui::Response {
    let c = p.support(s);
    let (txt, ic) = match s {
        SupportLevel::Verified => ("Verified", icon::SEAL_CHECK),
        SupportLevel::FromSpec => ("From spec", icon::FLASK),
        SupportLevel::DetectedOnly => ("Unsupported", icon::PROHIBIT),
    };
    egui::Frame::new()
        .fill(c.gamma_multiply(0.14))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| ui.label(RichText::new(format!("{ic} {txt}")).size(11.5).color(c)))
        .response
        .on_hover_text(s.explanation())
}

fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 1))
        .show(ui, |ui| ui.label(RichText::new(text).size(11.0).color(fg)));
}

impl eframe::App for EyefinderApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(a) = &self.audio {
            self.settings.volume = a.volume;
        }
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.refresh_view();
        let p = theme::palette(&ctx);

        // drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        for d in dropped {
            self.add_game(d, &ctx, true);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // keyboard
        if self.export.is_none() && self.job.is_none() && !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle_pause(&ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter))
                && let (Some(e), Some(f)) = (self.selected_entry, self.focused)
            {
                self.play(e, f, 0, &ctx);
            }
        }

        self.top_bar(ui, &p, &ctx);
        self.player_bar(ui, &p, &ctx);
        self.library_panel(ui, &p, &ctx);
        if self.current().is_some() {
            self.details_panel(ui, &p);
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Frame::new().fill(p.bg).inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.selected_entry.is_none() {
                    self.welcome(ui, &p, &ctx);
                } else {
                    self.browser(ui, &p, &ctx);
                }
            });
        });

        if hovering {
            let rect = ctx.content_rect();
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, "drop".into())).rect(
                rect.shrink(12.0),
                CornerRadius::same(14),
                p.accent.gamma_multiply(0.12),
                Stroke::new(2.0, p.accent),
                egui::StrokeKind::Inside,
            );
            egui::Area::new("drop-label".into())
                .order(egui::Order::Foreground)
                .fixed_pos(rect.center() - vec2(170.0, 20.0))
                .show(&ctx, |ui| {
                    ui.label(
                        RichText::new(format!("{} Drop to add to your library", icon::DOWNLOAD_SIMPLE))
                            .size(20.0)
                            .family(semibold())
                            .color(p.accent),
                    );
                });
        }

        self.export_modal(&p, &ctx);
        self.job_modal(&p, &ctx);
        self.about_modal(&p, &ctx);
        if let Some((msg, _)) = &self.toast {
            egui::Area::new("toast".into())
                .order(egui::Order::Tooltip)
                .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -96.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(p.raised)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("{} {msg}", icon::INFO)).color(p.text));
                        });
                });
        }
    }
}

impl EyefinderApp {
    fn top_bar(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        egui::Panel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(Margin::symmetric(14, 9))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::EYE).size(22.0).color(p.accent));
                    ui.label(RichText::new("Eyefinder").size(18.0).family(semibold()));
                    ui.label(RichText::new("GTA asset viewer & extractor").size(12.0).color(p.muted));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(RichText::new(icon::INFO).size(16.0)).on_hover_text("About Eyefinder").clicked() {
                            self.show_about = true;
                        }
                        let (ic, tip, next) = match self.settings.theme {
                            ThemeChoice::Dark => (icon::MOON, "Theme: dark", ThemeChoice::Light),
                            ThemeChoice::Light => (icon::SUN, "Theme: light", ThemeChoice::System),
                            ThemeChoice::System => (icon::DESKTOP, "Theme: follow system", ThemeChoice::Dark),
                        };
                        if ui.button(RichText::new(ic).size(16.0)).on_hover_text(tip).clicked() {
                            self.settings.theme = next;
                            apply_theme(ctx, next);
                        }
                        ui.add_space(6.0);
                        if ui
                            .button(format!("{} Open disc image", icon::DISC))
                            .on_hover_text("Add a PS2 or PC disc image (.iso)")
                            .clicked()
                            && let Some(f) =
                                rfd::FileDialog::new().add_filter("Disc image", &["iso", "ISO"]).pick_file()
                        {
                            self.add_game(f, ctx, true);
                        }
                        let add = egui::Button::new(
                            RichText::new(format!("{} Add game folder", icon::FOLDER_OPEN)).color(Color32::WHITE),
                        )
                        .fill(p.accent);
                        if ui.add(add).on_hover_text("Add an installed or extracted game folder").clicked()
                            && let Some(f) = rfd::FileDialog::new().pick_folder()
                        {
                            self.add_game(f, ctx, true);
                        }
                    });
                });
            });
    }

    fn library_panel(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        egui::Panel::left("library")
            .resizable(true)
            .default_size(270.0)
            .size_range(220.0..=420.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::same(12)).stroke(Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                ui.label(RichText::new("LIBRARY").size(11.0).color(p.muted).family(semibold()));
                ui.add_space(4.0);
                let mut select = None;
                let mut remove = None;
                for (i, e) in self.entries.iter().enumerate() {
                    let selected = self.selected_entry == Some(i);
                    let fill = if selected { p.accent_soft } else { p.raised };
                    let r = egui::Frame::new()
                        .fill(fill)
                        .stroke(Stroke::new(1.0, if selected { p.accent } else { p.border }))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            match &e.state {
                                LoadState::Ready { install, catalog } => {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(install.game.short()).family(semibold()).size(14.5));
                                        pill(ui, install.platform.short(), p.accent, p.accent.gamma_multiply(0.15));
                                    });
                                    ui.label(
                                        RichText::new(format!("{} audio items", catalog.assets.len()))
                                            .size(11.5)
                                            .color(p.muted),
                                    );
                                }
                                LoadState::Loading => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label("Reading game files...");
                                    });
                                }
                                LoadState::Failed(err) => {
                                    ui.label(
                                        RichText::new(format!("{} Not recognised", icon::WARNING)).color(p.danger),
                                    );
                                    ui.label(RichText::new(err).size(11.0).color(p.muted));
                                }
                            }
                            let name = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            ui.label(RichText::new(name).size(11.0).color(p.muted))
                                .on_hover_text(e.path.display().to_string());
                        })
                        .response
                        .interact(Sense::click());
                    if r.clicked() {
                        select = Some(i);
                    }
                    r.context_menu(|ui| {
                        if ui.button(format!("{} Show in file manager", icon::FOLDER_OPEN)).clicked() {
                            open_in_file_manager(if e.path.is_dir() {
                                &e.path
                            } else {
                                e.path.parent().unwrap_or(&e.path)
                            });
                            ui.close();
                        }
                        if ui.button(format!("{} Remove from library", icon::TRASH)).clicked() {
                            remove = Some(i);
                            ui.close();
                        }
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = select {
                    self.select_entry(i);
                }
                if let Some(i) = remove {
                    self.remove_entry(i);
                }
                if self.entries.is_empty() {
                    ui.label(RichText::new("No games yet.").color(p.muted));
                }

                // categories of the selected game
                let counts: Option<Vec<(Category, usize)>> = self.current().map(|(_, cat)| {
                    Category::ALL
                        .iter()
                        .map(|&c| (c, cat.assets.iter().filter(|a| a.category == c).count()))
                        .filter(|(_, n)| *n > 0)
                        .collect()
                });
                if let Some(counts) = counts {
                    ui.add_space(14.0);
                    ui.label(RichText::new("BROWSE").size(11.0).color(p.muted).family(semibold()));
                    ui.add_space(2.0);
                    let total: usize = counts.iter().map(|(_, n)| n).sum();
                    let mut rows: Vec<(Option<Category>, &str, String, usize)> =
                        vec![(None, icon::STACK, "All audio".into(), total)];
                    rows.extend(counts.iter().map(|&(c, n)| (Some(c), category_icon(c), c.label().to_string(), n)));
                    for (c, ic, label, n) in rows {
                        let sel = self.category == c;
                        let resp = ui
                            .allocate_ui(vec2(ui.available_width(), 28.0), |ui| {
                                let rect = ui.max_rect();
                                let resp = ui.interact(rect, ui.id().with(&label), Sense::click());
                                if sel || resp.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        CornerRadius::same(6),
                                        if sel { p.accent_soft } else { p.raised },
                                    );
                                }
                                ui.horizontal_centered(|ui| {
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(ic).color(if sel { p.accent } else { p.muted }));
                                    ui.label(RichText::new(&label).color(if sel {
                                        p.text
                                    } else {
                                        p.text.gamma_multiply(0.85)
                                    }));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.add_space(6.0);
                                        ui.label(RichText::new(n.to_string()).size(11.5).color(p.muted));
                                    });
                                });
                                resp
                            })
                            .inner;
                        if resp.clicked() {
                            self.category = c;
                            self.view_key = None;
                        }
                    }
                }
                let _ = ctx;
            });
    }

    fn welcome(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.22).max(20.0));
            ui.label(RichText::new(icon::EYE).size(64.0).color(p.accent));
            ui.add_space(8.0);
            ui.label(RichText::new("Welcome to Eyefinder").size(26.0).family(semibold()));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Browse, play and extract the audio of GTA III, Vice City and San Andreas.")
                    .size(14.5)
                    .color(p.muted),
            );
            ui.add_space(22.0);
            ui.horizontal(|ui| {
                let w = 420.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                let add = egui::Button::new(
                    RichText::new(format!("{}  Add game folder", icon::FOLDER_OPEN)).size(15.0).color(Color32::WHITE),
                )
                .fill(p.accent)
                .min_size(vec2(200.0, 40.0));
                if ui.add(add).clicked()
                    && let Some(f) = rfd::FileDialog::new().pick_folder()
                {
                    self.add_game(f, ctx, true);
                }
                let iso = egui::Button::new(RichText::new(format!("{}  Open disc image", icon::DISC)).size(15.0))
                    .min_size(vec2(200.0, 40.0));
                if ui.add(iso).clicked()
                    && let Some(f) = rfd::FileDialog::new().add_filter("Disc image", &["iso", "ISO"]).pick_file()
                {
                    self.add_game(f, ctx, true);
                }
            });
            ui.add_space(14.0);
            ui.label(RichText::new("...or drop a game folder or .iso anywhere in this window.").color(p.muted));
            ui.add_space(30.0);
            egui::Frame::new()
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_max_width(560.0);
                    ui.label(RichText::new("Support levels").family(semibold()));
                    ui.add_space(4.0);
                    for s in [SupportLevel::Verified, SupportLevel::FromSpec, SupportLevel::DetectedOnly] {
                        ui.horizontal_wrapped(|ui| {
                            support_badge(ui, p, s);
                            ui.label(RichText::new(s.explanation()).size(12.5).color(p.muted));
                        });
                    }
                });
        });
    }

    fn browser(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        // the library panel may have switched game or category earlier in this frame
        self.refresh_view();
        let entry = self.selected_entry.unwrap();
        let (install, catalog) = match &self.entries[entry].state {
            LoadState::Ready { install, catalog } => (install.clone(), catalog.clone()),
            LoadState::Loading => {
                ui.centered_and_justified(|ui| ui.spinner());
                return;
            }
            LoadState::Failed(e) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(RichText::new(icon::WARNING).size(42.0).color(p.danger));
                    ui.label(RichText::new("This folder or image was not recognised").size(18.0).family(semibold()));
                    ui.label(RichText::new(e).color(p.muted));
                });
                return;
            }
        };

        // header
        ui.horizontal(|ui| {
            let title = self.category.map_or("All audio", |c| c.label());
            ui.label(RichText::new(title).size(21.0).family(semibold()));
            ui.label(RichText::new(format!("{} · {}", install.game.short(), install.platform.name())).color(p.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.job.is_some();
                let n_sel = self.selection.len();
                let all = egui::Button::new(
                    RichText::new(format!("{} Export {} shown", icon::EXPORT, self.view.len())).color(Color32::WHITE),
                )
                .fill(p.accent);
                if ui.add_enabled(!busy && !self.view.is_empty(), all).clicked() {
                    let scope = plural(self.view.len(), "shown item");
                    self.open_export(self.view.clone(), scope);
                }
                if ui
                    .add_enabled(
                        !busy && n_sel > 0,
                        egui::Button::new(format!("{} Export {n_sel} selected", icon::EXPORT)),
                    )
                    .clicked()
                {
                    let scope = plural(n_sel, "selected item");
                    self.open_export(self.selection.iter().copied().collect(), scope);
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            egui::Frame::new()
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(8, 3))
                .show(ui, |ui| {
                    ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(p.muted));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search titles, ids and files")
                            .frame(egui::Frame::NONE)
                            .desired_width(300.0),
                    );
                    if r.changed() {
                        self.view_key = None;
                    }
                    if !self.search.is_empty() && ui.small_button(icon::X).clicked() {
                        self.search.clear();
                        self.view_key = None;
                    }
                });
            ui.label(RichText::new(format!("{} items", self.view.len())).color(p.muted));
            if !catalog.notices.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = catalog.notices.len();
                    let t = format!("{} {n} {} about this copy", icon::WARNING, if n == 1 { "note" } else { "notes" });
                    if ui.add(egui::Button::new(RichText::new(t).color(p.spec)).frame(false)).clicked() {
                        self.show_notices = !self.show_notices;
                    }
                });
            }
        });
        if self.show_notices && !catalog.notices.is_empty() {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(p.spec.gamma_multiply(0.08))
                .stroke(Stroke::new(1.0, p.spec.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                        for n in &catalog.notices {
                            ui.label(RichText::new(format!("• {n}")).size(12.5));
                        }
                    });
                });
        }
        ui.add_space(8.0);

        if self.view.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.label(RichText::new(icon::MAGNIFYING_GLASS).size(36.0).color(p.muted));
                ui.label(
                    RichText::new(if catalog.assets.is_empty() {
                        "No decodable audio in this copy"
                    } else {
                        "Nothing matches your search"
                    })
                    .color(p.muted),
                );
            });
            return;
        }

        // table
        let playing = self.playback.as_ref().map(|p| p.asset_index).filter(|_| self.playing_entry == Some(entry));
        let modifiers = ctx.input(|i| i.modifiers);
        let mut clicked: Option<usize> = None;
        let mut play_req: Option<usize> = None;
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(4, 4))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let view = &self.view;
                TableBuilder::new(ui)
                    .striped(false)
                    .sense(Sense::click())
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(34.0))
                    .column(Column::remainder().at_least(160.0).clip(true))
                    .column(Column::initial(160.0).at_least(100.0).clip(true).resizable(true))
                    .column(Column::initial(110.0).at_least(80.0).clip(true).resizable(true))
                    .column(Column::initial(70.0).at_least(60.0))
                    .column(Column::exact(112.0))
                    .header(26.0, |mut h| {
                        for t in ["", "Title", "Category", "Format", "Size", "Support"] {
                            h.col(|ui| {
                                ui.label(RichText::new(t).size(11.5).color(p.muted).family(semibold()));
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(ROW_H, view.len(), |mut row| {
                            let idx = view[row.index()];
                            let a = &catalog.assets[idx];
                            row.set_selected(self.selection.contains(&idx));
                            let is_playing = playing == Some(idx);
                            row.col(|ui| {
                                let ic = if is_playing { icon::SPEAKER_HIGH } else { icon::PLAY };
                                let col = if !a.support.can_decode() {
                                    p.unsupported
                                } else if is_playing {
                                    p.accent
                                } else {
                                    p.muted
                                };
                                if ui
                                    .add(egui::Button::new(RichText::new(ic).color(col)).frame(false))
                                    .on_hover_text("Play")
                                    .clicked()
                                {
                                    play_req = Some(idx);
                                }
                            });
                            row.col(|ui| {
                                let mut t = RichText::new(&a.title);
                                if is_playing {
                                    t = t.color(p.accent).family(semibold());
                                }
                                ui.label(t).on_hover_text(&a.game_id);
                            });
                            row.col(|ui| {
                                let c = match &a.group {
                                    Some(g) => format!("{} / {g}", a.category.label()),
                                    None => a.category.label().to_string(),
                                };
                                ui.label(RichText::new(c).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(format_label(a)).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(human_bytes(a.bytes)).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                support_badge(ui, p, a.support);
                            });
                            let r = row.response();
                            if r.double_clicked() {
                                play_req = Some(idx);
                            } else if r.clicked() {
                                clicked = Some(idx);
                            }
                        });
                    });
            });
        if let Some(idx) = clicked {
            if modifiers.command {
                if !self.selection.remove(&idx) {
                    self.selection.insert(idx);
                }
                self.anchor = Some(idx);
            } else if modifiers.shift {
                if let Some(a) = self.anchor.and_then(|a| self.view.iter().position(|&v| v == a)) {
                    let b = self.view.iter().position(|&v| v == idx).unwrap_or(a);
                    let (lo, hi) = (a.min(b), a.max(b));
                    self.selection = self.view[lo..=hi].iter().copied().collect();
                }
            } else {
                self.selection.clear();
                self.selection.insert(idx);
                self.anchor = Some(idx);
            }
            self.focused = Some(idx);
        }
        if let Some(idx) = play_req {
            self.focused = Some(idx);
            self.play(entry, idx, 0, ctx);
        }
    }

    fn details_panel(&mut self, ui: &mut Ui, p: &Palette) {
        egui::Panel::right("details")
            .resizable(true)
            .default_size(280.0)
            .size_range(230.0..=480.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::same(14)).stroke(Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                let Some((install, cat)) = self.current() else { return };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(idx) = self.focused else {
                        ui.label(RichText::new("DETAILS").size(11.0).color(p.muted).family(semibold()));
                        ui.add_space(6.0);
                        ui.label(RichText::new(install.game.name()).family(semibold()).size(15.0));
                        ui.label(RichText::new(install.platform.name()).color(p.muted));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Detected because").size(12.0).color(p.muted));
                        for e in &install.evidence {
                            ui.label(RichText::new(format!("• {e}")).size(12.5));
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Source").size(12.0).color(p.muted));
                        ui.label(RichText::new(install.vfs.describe()).size(12.5));
                        ui.add_space(14.0);
                        ui.label(RichText::new("Select an item to see its details. Double-click or press Enter to play, Space to pause.").color(p.muted).size(12.5));
                        return;
                    };
                    let a = &cat.assets[idx];
                    ui.label(RichText::new("DETAILS").size(11.0).color(p.muted).family(semibold()));
                    ui.add_space(6.0);
                    ui.label(RichText::new(&a.title).size(17.0).family(semibold()));
                    ui.add_space(4.0);
                    support_badge(ui, p, a.support);
                    ui.label(RichText::new(a.support.explanation()).size(12.0).color(p.muted));
                    ui.add_space(10.0);
                    let field = |ui: &mut Ui, k: &str, v: String| {
                        ui.label(RichText::new(k).size(11.5).color(p.muted));
                        ui.label(RichText::new(v).size(13.0));
                        ui.add_space(4.0);
                    };
                    field(ui, "Category", a.group.as_ref().map_or(a.category.label().to_string(), |g| format!("{} / {g}", a.category.label())));
                    field(ui, "Game identifier", a.game_id.clone());
                    field(ui, "Source file", a.source.clone());
                    field(ui, "Stored size", human_bytes(a.bytes));
                    field(ui, "Container", format_label(a));
                    if let Some(pb) = self.playback.as_ref().filter(|pb| self.is_playing(idx) && pb.error.is_none()) {
                        ui.separator();
                        ui.label(RichText::new("DECODED").size(11.0).color(p.muted).family(semibold()));
                        ui.add_space(4.0);
                        field(ui, "Codec", pb.facts.info.codec.clone());
                        field(ui, "Sample rate", format!("{} Hz", pb.facts.rate));
                        field(ui, "Channels", pb.facts.channels.to_string());
                        if let Some(t) = pb.facts.total_frames {
                            field(ui, "Duration", format!("{} ({t} frames)", fmt_player_time(t, pb.facts.rate, Some(t))));
                        }
                        field(ui, "Exported as", pb.facts.output.clone());
                        for n in &pb.facts.info.notes {
                            ui.label(RichText::new(format!("• {n}")).size(12.0).color(p.muted));
                        }
                    }
                });
            });
    }

    fn player_bar(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        egui::Panel::bottom("player")
            .exact_size(76.0)
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .inner_margin(Margin::symmetric(16, 10))
                    .stroke(Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let paused = self.audio.as_ref().is_ok_and(|a| a.is_paused());
                    let active = self.playback.as_ref().is_some_and(|pb| !pb.finished() && pb.error.is_none());
                    let ic = if active && !paused { icon::PAUSE } else { icon::PLAY };
                    let play_btn = egui::Button::new(RichText::new(ic).size(20.0).color(Color32::WHITE))
                        .fill(p.accent)
                        .corner_radius(CornerRadius::same(20))
                        .min_size(vec2(40.0, 40.0));
                    if ui.add_enabled(self.playback.is_some() || self.focused.is_some(), play_btn).clicked() {
                        self.toggle_pause(ctx);
                    }
                    if ui
                        .add_enabled(
                            self.playback.is_some(),
                            egui::Button::new(RichText::new(icon::STOP).size(16.0)).min_size(vec2(32.0, 32.0)),
                        )
                        .on_hover_text("Stop")
                        .clicked()
                    {
                        self.stop_playback();
                    }
                    ui.add_space(8.0);

                    let title = self.playback.as_ref().and_then(|pb| {
                        self.entry(self.playing_entry?).map(|(_, c)| c.assets[pb.asset_index].title.clone())
                    });
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        match (&self.audio, &title) {
                            (Err(e), _) => {
                                ui.label(
                                    RichText::new(format!("{} Audio output unavailable", icon::SPEAKER_SLASH))
                                        .color(p.danger),
                                );
                                ui.label(RichText::new(e).size(11.0).color(p.muted));
                            }
                            (Ok(_), Some(t)) => {
                                ui.label(RichText::new(t).family(semibold()));
                                let pb = self.playback.as_ref().unwrap();
                                let s = match &pb.error {
                                    Some(e) => e.clone(),
                                    None => format!(
                                        "{} · {} Hz · {} ch",
                                        pb.facts.info.codec, pb.facts.rate, pb.facts.channels
                                    ),
                                };
                                ui.label(RichText::new(s).size(11.5).color(p.muted));
                            }
                            (Ok(_), None) => {
                                ui.label(RichText::new("Nothing playing").color(p.muted));
                                ui.label(RichText::new("Double-click an item to play it").size(11.5).color(p.muted));
                            }
                        }
                    });

                    // waveform + seek
                    let vol_w = 150.0;
                    let w = (ui.available_width() - vol_w - 130.0).max(120.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click_and_drag());
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, CornerRadius::same(6), p.bg);
                    let (pos, total, rate) = match &self.playback {
                        Some(pb) => {
                            let total = pb.facts.total_frames.or_else(|| {
                                self.waveform
                                    .as_ref()
                                    .filter(|w| w.done.load(Ordering::Relaxed) && w.asset_index == pb.asset_index)
                                    .map(|w| {
                                        w.peaks.lock().unwrap().len() as u64 * w.frames_per_peak.load(Ordering::Relaxed)
                                    })
                            });
                            (pb.position_frames(), total, pb.facts.rate)
                        }
                        None => (0, None, 0),
                    };
                    let frac = total.filter(|t| *t > 0).map_or(0.0, |t| (pos as f32 / t as f32).clamp(0.0, 1.0));
                    if let Some(wf) = &self.waveform {
                        let peaks = wf.peaks.lock().unwrap();
                        let fpp = wf.frames_per_peak.load(Ordering::Relaxed).max(1);
                        let n_total = total.map_or(peaks.len() as u64, |t| t.div_ceil(fpp)).max(1) as f32;
                        let cols = rect.width() as usize;
                        let mid = rect.center().y;
                        let h = rect.height() * 0.46;
                        for x in 0..cols {
                            let a = (x as f32 / cols as f32 * n_total) as usize;
                            let b = (((x + 1) as f32 / cols as f32 * n_total) as usize).max(a + 1);
                            if a >= peaks.len() {
                                break;
                            }
                            let (mut lo, mut hi) = (0f32, 0f32);
                            for pk in &peaks[a..b.min(peaks.len())] {
                                lo = lo.min(pk.0);
                                hi = hi.max(pk.1);
                            }
                            let xf = rect.left() + x as f32 + 0.5;
                            let col = if (x as f32) / (cols as f32) <= frac {
                                p.accent
                            } else {
                                p.muted.gamma_multiply(0.55)
                            };
                            painter.line_segment(
                                [egui::pos2(xf, mid - hi * h), egui::pos2(xf, mid - lo * h)],
                                Stroke::new(1.0, col),
                            );
                        }
                    }
                    if total.is_some() {
                        let x = rect.left() + rect.width() * frac;
                        painter.line_segment(
                            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                            Stroke::new(1.5, p.text),
                        );
                    }
                    if let (Some(t), true) = (total, resp.clicked() || resp.drag_stopped())
                        && let (Some(pt), Some(pb)) = (resp.interact_pointer_pos(), &self.playback)
                    {
                        let f = ((pt.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        let idx = pb.asset_index;
                        if let Some(e) = self.playing_entry {
                            self.play(e, idx, (f as f64 * t as f64) as u64, ctx);
                        }
                    }
                    let resp = if self.playback.is_some() && total.is_none() {
                        resp.on_hover_text("Seeking becomes available once the length is known")
                    } else {
                        resp
                    };
                    let _ = resp;
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            fmt_player_time(pos, rate, total),
                            total.map_or("--:--".into(), |t| fmt_player_time(t, rate, total))
                        ))
                        .monospace()
                        .color(p.muted),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Ok(a) = &mut self.audio {
                            let mut v = a.volume;
                            if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                                a.set_volume(v);
                            }
                            ui.label(RichText::new(icon::SPEAKER_HIGH).color(p.muted));
                        }
                    });
                });
            });
    }

    fn export_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(d) = &mut self.export else { return };
        let mut start = false;
        let mut close = false;
        let modal = egui::Modal::new("export".into())
            .frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(20)))
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(RichText::new(format!("{} Export audio", icon::EXPORT)).size(19.0).family(semibold()));
                ui.label(RichText::new(format!("{} as WAV files with manifest.json, manifest.csv and tracklist.txt", d.scope)).color(p.muted));
                ui.add_space(12.0);
                ui.label(RichText::new("Destination").size(12.0).color(p.muted));
                ui.horizontal(|ui| {
                    let mut s = d.out_dir.display().to_string();
                    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(400.0)).changed() {
                        d.out_dir = PathBuf::from(s);
                    }
                    if ui.button(format!("{} Browse", icon::FOLDER_OPEN)).clicked()
                        && let Some(f) = rfd::FileDialog::new().set_directory(d.out_dir.parent().unwrap_or(Path::new("."))).pick_folder() {
                            d.out_dir = f;
                        }
                });
                ui.add_space(10.0);
                ui.checkbox(&mut self.settings.float_output, "Decode MP3 / Vorbis to 32-bit float (full decoder precision)")
                    .on_hover_text("Off: 16-bit PCM. PCM sources are always copied bit-exact and ADPCM always decodes to its exact 16-bit output.");
                ui.checkbox(&mut self.settings.split_markers, "Also write one file per cue marker when a stream has markers");
                ui.checkbox(&mut self.settings.keep_bitstream, "Keep the untouched audio bitstream of movies (.mp2)");
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let go = egui::Button::new(RichText::new(format!("{} Start export", icon::EXPORT)).color(Color32::WHITE)).fill(p.accent).min_size(vec2(130.0, 32.0));
                    if ui.add(go).clicked() {
                        start = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 32.0))).clicked() {
                        close = true;
                    }
                });
            });
        if modal.should_close() {
            close = true;
        }
        if start {
            self.start_export(ctx);
        } else if close {
            self.export = None;
        }
    }

    fn job_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let result = job.result.lock().unwrap().take();
        let mut close = false;
        if let Some(r) = result {
            // keep the finished result visible
            *job.result.lock().unwrap() = Some(r);
        }
        let finished = job.result.lock().unwrap().is_some();
        egui::Modal::new("job".into())
            .frame(
                egui::Frame::new()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(20)),
            )
            .show(ctx, |ui| {
                ui.set_width(520.0);
                let pr = job.progress.lock().unwrap().clone();
                if !finished {
                    ui.label(RichText::new("Exporting...").size(19.0).family(semibold()));
                    ui.add_space(8.0);
                    let frac = if pr.total > 0 { pr.done as f32 / pr.total as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(frac).fill(p.accent).text(format!("{} of {}", pr.done, pr.total)));
                    ui.label(RichText::new(&pr.current).color(p.muted));
                    ui.label(
                        RichText::new(format!("Elapsed {}", fmt_dur(job.started.elapsed().as_secs(), 1)))
                            .size(11.5)
                            .color(p.muted),
                    );
                    ui.add_space(10.0);
                    if ui.button(format!("{} Cancel", icon::X)).clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                } else {
                    let guard = job.result.lock().unwrap();
                    match guard.as_ref().unwrap() {
                        Ok(rep) => {
                            let (ic, col, head) = if rep.cancelled {
                                (icon::WARNING, p.spec, "Export cancelled")
                            } else if rep.failed.is_empty() {
                                (icon::CHECK_CIRCLE, p.verified, "Export complete")
                            } else {
                                (icon::WARNING, p.spec, "Export finished with problems")
                            };
                            ui.label(RichText::new(format!("{ic} {head}")).size(19.0).family(semibold()).color(col));
                            ui.label(format!(
                                "{} written in {}",
                                plural(rep.written, "file"),
                                fmt_dur(job.started.elapsed().as_secs(), 1)
                            ));
                            if !rep.failed.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format!("{} failed:", plural(rep.failed.len(), "item")))
                                        .color(p.danger),
                                );
                                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                                    for (t, e) in &rep.failed {
                                        ui.label(RichText::new(format!("• {t}: {e}")).size(12.0));
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            ui.label(
                                RichText::new(format!("{} Export failed", icon::WARNING))
                                    .size(19.0)
                                    .family(semibold())
                                    .color(p.danger),
                            );
                            ui.label(e);
                        }
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{} Open folder", icon::FOLDER_OPEN)).color(Color32::WHITE),
                                )
                                .fill(p.accent),
                            )
                            .clicked()
                        {
                            open_in_file_manager(&job.out_dir);
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                }
            });
        if close {
            self.job = None;
        }
    }

    fn about_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let m = egui::Modal::new("about".into())
            .frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.border)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(20)))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::EYE).size(30.0).color(p.accent));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Eyefinder").size(20.0).family(semibold()));
                        ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(p.muted));
                    });
                });
                ui.add_space(8.0);
                ui.label("A viewer and extractor for the assets of Grand Theft Auto III, Vice City and San Andreas.");
                ui.add_space(6.0);
                ui.label(RichText::new("Licensed under the GNU General Public License v3.0 or later.").size(12.5).color(p.muted));
                ui.label(RichText::new("Uses Symphonia (MPL-2.0), egui (MIT/Apache-2.0), Inter (OFL-1.1) and Phosphor Icons (MIT). Format knowledge comes from the re3/reVC, gta-reversed and vgmstream projects. Eyefinder contains no game data and is not affiliated with Rockstar Games or Take-Two Interactive.").size(12.5).color(p.muted));
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    self.show_about = false;
                }
            });
        if m.should_close() {
            self.show_about = false;
        }
    }
}
