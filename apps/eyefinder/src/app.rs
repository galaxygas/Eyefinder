//! Application state, the window layout (top bar, sidebar, centre, details, player) and dialogs.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ef_audio::{Catalog, Category};
use ef_core::img::{ImgVfs, is_img_name};
use ef_core::vfs::{DirEntry, Vfs};
use ef_core::{Edition, Game, GameInstall, ImportChoice, Platform, SupportLevel};
use ef_texture::catalog::TexCatalog;
use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

use crate::player::{Audio, Playback, Waveform};
use crate::settings::{LibraryItem, Settings, ThemeChoice};
use crate::theme::{self, Palette, section_label, semibold};
use crate::thumbs::Thumbs;

pub(crate) enum LoadState {
    Loading,
    Ready { install: GameInstall, catalog: Arc<Catalog> },
    Failed(String),
}

pub(crate) enum TexState {
    Waiting,
    Indexing { done: Arc<AtomicUsize>, total: Arc<AtomicUsize> },
    Ready(Arc<TexCatalog>),
}

pub(crate) struct Entry {
    /// Stable identity (indexes shift when entries are removed).
    pub id: u64,
    pub path: PathBuf,
    pub state: LoadState,
    pub textures: TexState,
}

enum LoadMsg {
    Game(u64, Result<(GameInstall, Catalog), String>),
    Textures(u64, TexCatalog),
}

/// What the centre of the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum View {
    Audio(Option<Category>),
    /// All textures, one group (folder or archive), or one dictionary (`Dict`).
    Textures(TexFilter),
    File(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TexFilter {
    All,
    Group(String),
    Dict(String),
}

pub(crate) struct ImportDialog {
    pub path: PathBuf,
    /// Changing a game already in the library: it is replaced only when the user confirms.
    pub replace: bool,
    pub detected: Result<GameInstall, String>,
    pub choices: Vec<(Game, Platform)>,
    pub game: Game,
    pub platform: Platform,
    pub edition: String,
}

pub(crate) enum ScanState {
    Running(Arc<Mutex<Option<Vec<ef_core::locate::Found>>>>),
    Done { found: Vec<ef_core::locate::Found>, picked: Vec<bool> },
}

#[derive(Clone, Default)]
pub(crate) struct JobProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

pub(crate) struct JobReport {
    pub written: usize,
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
}

pub(crate) struct Job {
    pub title: String,
    pub progress: Arc<Mutex<JobProgress>>,
    pub cancel: Arc<AtomicBool>,
    pub result: Arc<Mutex<Option<Result<JobReport, String>>>>,
    pub started: Instant,
    pub out_dir: PathBuf,
}

pub(crate) struct AudioExportDialog {
    pub entry: usize,
    pub assets: Vec<usize>,
    pub scope: String,
    pub out_dir: PathBuf,
}

pub(crate) struct TextureExportDialog {
    pub entry: usize,
    pub assets: Vec<usize>,
    pub scope: String,
    pub out_dir: PathBuf,
}

/// Per-entry file tree state: opened archives and the selected path.
#[derive(Default)]
pub(crate) struct FileTree {
    pub archives: HashMap<String, Option<Arc<ImgVfs>>>,
}

pub struct EyefinderApp {
    pub(crate) settings: Settings,
    pub(crate) entries: Vec<Entry>,
    next_id: u64,
    pub(crate) selected_entry: Option<usize>,
    load_tx: Sender<LoadMsg>,
    load_rx: Receiver<LoadMsg>,
    pub(crate) view: View,
    // audio browser
    pub(crate) search: String,
    pub(crate) audio_view: Vec<usize>,
    pub(crate) audio_key: Option<(usize, View, String)>,
    pub(crate) selection: BTreeSet<usize>,
    pub(crate) anchor: Option<usize>,
    pub(crate) focused: Option<usize>,
    // texture browser
    pub(crate) tex_search: String,
    pub(crate) tex_view: Vec<usize>,
    pub(crate) tex_key: Option<(usize, View, String, usize)>,
    pub(crate) tex_selection: BTreeSet<usize>,
    pub(crate) tex_anchor: Option<usize>,
    pub(crate) tex_focused: Option<usize>,
    pub(crate) tex_level: usize,
    pub(crate) tex_zoom: Option<f32>,
    pub(crate) thumbs: Thumbs,
    // files
    pub(crate) trees: HashMap<u64, FileTree>,
    // playback
    pub(crate) audio: Result<Audio, String>,
    pub(crate) playback: Option<Playback>,
    /// library entry the current playback belongs to
    pub(crate) playing_entry: Option<usize>,
    pub(crate) waveform: Option<Waveform>,
    // dialogs
    pub(crate) import: Option<ImportDialog>,
    pending_imports: Vec<PathBuf>,
    pub(crate) scan: Option<ScanState>,
    pub(crate) audio_export: Option<AudioExportDialog>,
    pub(crate) texture_export: Option<TextureExportDialog>,
    pub(crate) job: Option<Job>,
    pub(crate) toast: Option<(String, Instant)>,
    pub(crate) show_about: bool,
    pub(crate) show_notices: bool,
}

pub(crate) const ROW_H: f32 = 30.0;

impl EyefinderApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        let mut app = Self::empty(&cc.egui_ctx, settings, Audio::open());
        for item in app.settings.library.clone() {
            app.add_game(item.path, item.choice, &cc.egui_ctx, false);
        }
        // paths given on the command line ("Open with", drag onto the executable) are added and selected
        for a in std::env::args_os().skip(1) {
            let p = PathBuf::from(a);
            if p.exists() {
                app.begin_import(std::path::absolute(&p).unwrap_or(p));
            }
        }
        app
    }

    fn empty(ctx: &egui::Context, settings: Settings, mut audio: Result<Audio, String>) -> Self {
        apply_theme(ctx, settings.theme);
        let (load_tx, load_rx) = unbounded();
        if let Ok(a) = &mut audio {
            a.set_volume(settings.volume);
        }
        Self {
            entries: Vec::new(),
            next_id: 1,
            selected_entry: None,
            load_tx,
            load_rx,
            view: View::Audio(None),
            search: String::new(),
            audio_view: Vec::new(),
            audio_key: None,
            selection: BTreeSet::new(),
            anchor: None,
            focused: None,
            tex_search: String::new(),
            tex_view: Vec::new(),
            tex_key: None,
            tex_selection: BTreeSet::new(),
            tex_anchor: None,
            tex_focused: None,
            tex_level: 0,
            tex_zoom: None,
            thumbs: Thumbs::new(ctx),
            trees: HashMap::new(),
            audio,
            playback: None,
            playing_entry: None,
            waveform: None,
            import: None,
            pending_imports: Vec::new(),
            scan: None,
            audio_export: None,
            texture_export: None,
            job: None,
            toast: None,
            show_about: false,
            show_notices: false,
            settings,
        }
    }

    pub(crate) fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Opens the import dialog for `path` (detection is quick: file lookups and one header read).
    pub(crate) fn begin_import(&mut self, path: PathBuf) {
        let path = ef_core::import::resolve_path(&path).unwrap_or(path);
        if self.import.as_ref().is_some_and(|d| d.path == path) || self.pending_imports.contains(&path) {
            return;
        }
        self.open_import(path, false);
    }

    fn open_import(&mut self, path: PathBuf, replace: bool) {
        if self.import.is_some() {
            self.pending_imports.push(path);
            return;
        }
        if !replace && let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.select_entry(i);
            self.toast("This game is already in your library");
            return;
        }
        let detected = ef_core::open_and_detect(&path).map_err(|e| e.to_string());
        let choices = match &detected {
            Ok(i) => ef_core::possible_choices(i.vfs.as_ref()),
            Err(_) => {
                ef_core::import::open_path(&path).map(|v| ef_core::possible_choices(v.as_ref())).unwrap_or_default()
            }
        };
        let (game, platform, edition) = match &detected {
            Ok(i) => (i.game, i.platform, i.edition.id.clone()),
            Err(_) => (Game::Gta3, Platform::Pc, "unknown".into()),
        };
        let (mut game, mut platform, mut edition) = (game, platform, edition);
        if replace && let Some(c) = self.settings.library.iter().find(|l| l.path == path).and_then(|l| l.choice.clone())
        {
            // start from the current choice
            (game, platform, edition) = (c.game, c.platform, c.edition);
        }
        self.import = Some(ImportDialog { path, replace, detected, choices, game, platform, edition });
    }

    fn next_import(&mut self) {
        self.import = None;
        if !self.pending_imports.is_empty() {
            let p = self.pending_imports.remove(0);
            self.begin_import(p);
        }
    }

    pub(crate) fn add_game(&mut self, path: PathBuf, choice: Option<ImportChoice>, ctx: &egui::Context, select: bool) {
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            if select {
                self.select_entry(i);
            }
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(Entry { id, path: path.clone(), state: LoadState::Loading, textures: TexState::Waiting });
        if let Some(item) = self.settings.library.iter_mut().find(|l| l.path == path) {
            item.choice = choice.clone();
        } else {
            self.settings.library.push(LibraryItem { path: path.clone(), choice: choice.clone() });
        }
        if select || self.selected_entry.is_none() {
            self.select_entry(self.entries.len() - 1);
        }
        let tx = self.load_tx.clone();
        let ctx = ctx.clone();
        let (done, total) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        if let Some(e) = self.entries.last_mut() {
            e.textures = TexState::Indexing { done: done.clone(), total: total.clone() };
        }
        thread::Builder::new()
            .name("load-game".into())
            .spawn(move || {
                let r = ef_core::open_with(&path, choice.as_ref())
                    .map_err(|e| e.to_string())
                    .and_then(|i| ef_audio::catalog::build(&i).map(|c| (i, c)).map_err(|e| e.to_string()));
                let install = r.as_ref().ok().map(|(i, _)| i.clone());
                let _ = tx.send(LoadMsg::Game(id, r));
                ctx.request_repaint();
                if let Some(install) = install {
                    let progress = |d: usize, t: usize| {
                        done.store(d, Ordering::Relaxed);
                        total.store(t, Ordering::Relaxed);
                        if d.is_multiple_of(64) {
                            ctx.request_repaint();
                        }
                    };
                    let cat = ef_texture::catalog::build(&install, &progress, &AtomicBool::new(false));
                    let _ = tx.send(LoadMsg::Textures(id, cat));
                    ctx.request_repaint();
                }
            })
            .expect("spawn loader");
    }

    pub(crate) fn select_entry(&mut self, i: usize) {
        if self.selected_entry != Some(i) {
            self.selected_entry = Some(i);
            self.view = View::Audio(None);
            self.selection.clear();
            self.anchor = None;
            self.focused = None;
            self.tex_selection.clear();
            self.tex_anchor = None;
            self.tex_focused = None;
            self.audio_key = None;
            self.tex_key = None;
        }
    }

    fn remove_entry(&mut self, i: usize) {
        let e = self.entries.remove(i);
        self.settings.library.retain(|l| l.path != e.path);
        self.thumbs.forget(e.id);
        self.trees.remove(&e.id);
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
            self.tex_selection.clear();
            self.tex_focused = None;
            self.view = View::Audio(None);
        } else {
            shift(&mut self.selected_entry);
        }
        self.audio_key = None;
        self.tex_key = None;
    }

    pub(crate) fn current(&self) -> Option<(&GameInstall, &Arc<Catalog>)> {
        self.entry(self.selected_entry?)
    }

    pub(crate) fn entry(&self, i: usize) -> Option<(&GameInstall, &Arc<Catalog>)> {
        match &self.entries.get(i)?.state {
            LoadState::Ready { install, catalog } => Some((install, catalog)),
            _ => None,
        }
    }

    pub(crate) fn textures_of(&self, i: usize) -> Option<&Arc<TexCatalog>> {
        match &self.entries.get(i)?.textures {
            TexState::Ready(c) => Some(c),
            _ => None,
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.load_rx.try_recv() {
            match msg {
                LoadMsg::Game(id, r) => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                        e.state = match r {
                            Ok((install, catalog)) => LoadState::Ready { install, catalog: Arc::new(catalog) },
                            Err(err) => {
                                e.textures = TexState::Waiting;
                                LoadState::Failed(err)
                            }
                        };
                    }
                }
                LoadMsg::Textures(id, cat) => {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                        e.textures = TexState::Ready(Arc::new(cat));
                    }
                }
            }
            self.audio_key = None;
            self.tex_key = None;
        }
        self.thumbs.poll(ctx);
        if self.entries.iter().any(|e| matches!(e.textures, TexState::Indexing { .. })) {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if let Some(pb) = &self.playback
            && !(pb.finished() && pb.error.is_none())
        {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        if let Some(job) = &self.job
            && job.result.lock().unwrap().is_none()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let Some(ScanState::Running(r)) = &self.scan {
            let done = r.lock().unwrap().take();
            match done {
                Some(found) => {
                    let picked = found.iter().map(|f| f.result.is_ok() && !self.has_path(&f.path)).collect();
                    self.scan = Some(ScanState::Done { found, picked });
                }
                None => ctx.request_repaint_after(Duration::from_millis(100)),
            }
        }
        if self.toast.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4)) {
            self.toast = None;
        }
    }

    fn has_path(&self, p: &Path) -> bool {
        let key = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        self.entries.iter().any(|e| std::fs::canonicalize(&e.path).unwrap_or_else(|_| e.path.clone()) == key)
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        let slot = Arc::new(Mutex::new(None));
        let (s, c) = (slot.clone(), ctx.clone());
        thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                *s.lock().unwrap() = Some(ef_core::locate::scan());
                c.request_repaint();
            })
            .expect("spawn scan");
        self.scan = Some(ScanState::Running(slot));
    }

    pub(crate) fn default_out_dir(&self, install: &GameInstall, kind: &str) -> PathBuf {
        let root = self
            .settings
            .out_root
            .clone()
            .or_else(|| dirs::document_dir().or_else(dirs::home_dir).map(|d| d.join("Eyefinder Exports")))
            .or_else(|| std::env::current_dir().ok().map(|d| d.join("Eyefinder Exports")))
            .unwrap_or_else(|| PathBuf::from("Eyefinder Exports"));
        root.join(ef_audio::catalog::sanitize(&format!("{} ({})", install.game.name(), install.platform.short())))
            .join(kind)
    }
}

pub(crate) fn apply_theme(ctx: &egui::Context, t: ThemeChoice) {
    ctx.set_theme(match t {
        ThemeChoice::Dark => egui::ThemePreference::Dark,
        ThemeChoice::Light => egui::ThemePreference::Light,
        ThemeChoice::System => egui::ThemePreference::System,
    });
}

pub(crate) fn plural(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

pub(crate) fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

pub(crate) fn fmt_dur(frames: u64, rate: u32) -> String {
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

pub(crate) fn open_in_file_manager(p: &Path) {
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    let _ = r;
}

pub(crate) fn support_badge(ui: &mut Ui, p: &Palette, s: SupportLevel) -> egui::Response {
    let c = p.support(s);
    let (txt, ic) = match s {
        SupportLevel::Verified => ("Verified", icon::SEAL_CHECK),
        SupportLevel::FromSpec => ("From spec", icon::FLASK),
        SupportLevel::DetectedOnly => ("Unsupported", icon::PROHIBIT),
    };
    egui::Frame::new()
        .fill(c.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0, c.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(format!("{ic} {txt}")).size(11.0).color(c)))
        .response
        .on_hover_text(s.explanation())
}

pub(crate) fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(text).size(10.5).family(semibold()).color(fg)));
}

/// A filled accent button.
pub(crate) fn accent_button(p: &Palette, text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(p.on_accent).family(semibold())).fill(p.accent)
}

/// A navigation row in the sidebar: icon, label, count; selected rows get an accent bar.
pub(crate) fn nav_row(
    ui: &mut Ui,
    p: &Palette,
    id: impl std::hash::Hash + std::fmt::Debug,
    ic: &str,
    label: &str,
    count: Option<String>,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 27.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(ui.id().with(&id).with("hover"), resp.hovered(), 0.12);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(4), p.accent_soft);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top() + vec2(0.0, 5.0), vec2(3.0, rect.height() - 10.0)),
            CornerRadius::same(2),
            p.accent,
        );
    } else if hover > 0.0 {
        painter.rect_filled(rect, CornerRadius::same(4), p.raised.gamma_multiply(hover));
    }
    let text_col = if selected { p.text } else { p.text.gamma_multiply(0.86) };
    let ic_col = if selected { p.accent_text } else { p.muted };
    let font = egui::FontId::proportional(13.0);
    painter.text(rect.left_center() + vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, ic, font.clone(), ic_col);
    let max_w = rect.width() - 34.0 - if count.is_some() { 44.0 } else { 0.0 };
    let galley = theme::elided(painter, label, font, text_col, max_w);
    let ty = rect.center().y - galley.size().y / 2.0;
    painter.galley(egui::pos2(rect.left() + 32.0, ty), galley, text_col);
    if let Some(c) = count {
        painter.text(
            rect.right_center() - vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            c,
            egui::FontId::proportional(11.5),
            p.muted,
        );
    }
    resp
}

pub(crate) fn category_icon(c: Category) -> &'static str {
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

pub(crate) fn modal_frame(p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(20))
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
        self.refresh_audio_view();
        self.refresh_texture_view();
        let p = theme::palette(&ctx);

        // drag & drop
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        for d in dropped {
            self.begin_import(d);
        }
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // keyboard
        let modal_open = self.audio_export.is_some()
            || self.texture_export.is_some()
            || self.job.is_some()
            || self.import.is_some()
            || self.scan.is_some();
        if !modal_open && !ctx.egui_wants_keyboard_input() && matches!(self.view, View::Audio(_)) {
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
        if matches!(self.view, View::Audio(_)) || self.playback.is_some() {
            self.player_bar(ui, &p, &ctx);
        }
        self.sidebar(ui, &p, &ctx);
        if self.current().is_some() {
            match self.view {
                View::Audio(_) => self.audio_details(ui, &p),
                View::Textures(_) => self.texture_details(ui, &p),
                View::File(_) => {}
            }
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            egui::Frame::new().fill(p.bg).inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.selected_entry.is_none() {
                    self.welcome(ui, &p, &ctx);
                } else if self.loading_or_failed(ui, &p) {
                } else {
                    match self.view.clone() {
                        View::Audio(_) => self.audio_browser(ui, &p, &ctx),
                        View::Textures(_) => self.texture_browser(ui, &p, &ctx),
                        View::File(path) => self.file_view(ui, &p, &ctx, &path),
                    }
                }
            });
        });

        if hovering {
            let rect = ctx.content_rect();
            ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, "drop".into())).rect(
                rect.shrink(12.0),
                CornerRadius::same(10),
                p.accent.gamma_multiply(0.10),
                Stroke::new(2.0, p.accent),
                egui::StrokeKind::Inside,
            );
            egui::Area::new("drop-label".into())
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.label(
                        RichText::new(format!("{} Drop to add to your library", icon::DOWNLOAD_SIMPLE))
                            .size(20.0)
                            .family(semibold())
                            .color(p.accent_text),
                    );
                });
        }

        self.import_modal(&p, &ctx);
        self.scan_modal(&p, &ctx);
        self.audio_export_modal(&p, &ctx);
        self.texture_export_modal(&p, &ctx);
        self.job_modal(&p, &ctx);
        self.about_modal(&p, &ctx);
        if let Some((msg, _)) = &self.toast {
            egui::Area::new("toast".into())
                .order(egui::Order::Tooltip)
                .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -96.0))
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(p.raised)
                        .stroke(Stroke::new(1.0, p.accent.gamma_multiply(0.6)))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(14, 9))
                        .show(ui, |ui| {
                            ui.label(RichText::new(format!("{} {msg}", icon::INFO)).color(p.text));
                        });
                });
        }
    }
}

impl EyefinderApp {
    /// Shows the loading spinner or the error of the selected entry; true if it did.
    fn loading_or_failed(&self, ui: &mut Ui, p: &Palette) -> bool {
        let Some(e) = self.selected_entry.and_then(|i| self.entries.get(i)) else { return false };
        match &e.state {
            LoadState::Ready { .. } => false,
            LoadState::Loading => {
                ui.centered_and_justified(|ui| ui.spinner());
                true
            }
            LoadState::Failed(err) => {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.label(RichText::new(icon::WARNING).size(42.0).color(p.danger));
                    ui.label(RichText::new("This folder or image could not be opened").size(18.0).family(semibold()));
                    ui.label(RichText::new(err).color(p.muted));
                });
                true
            }
        }
    }

    fn top_bar(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        let r = egui::Panel::top("top")
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::symmetric(14, 9)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::EYE).size(22.0).color(p.accent_text));
                    ui.label(RichText::new("EYEFINDER").size(16.0).family(semibold()).extra_letter_spacing(2.0));
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
                        self.add_buttons(ui, p, ctx, false);
                    });
                });
            });
        let rect = r.response.rect;
        let line = egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom());
        theme::accent_line(
            &ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, "topline".into())),
            line,
            p.accent,
        );
    }

    /// "Add game folder", "Open files" and "Find installed games".
    fn add_buttons(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context, large: bool) {
        let size = if large { vec2(196.0, 40.0) } else { vec2(0.0, 0.0) };
        let t = |s: String| if large { RichText::new(s).size(14.5) } else { RichText::new(s) };
        let add = egui::Button::new(
            t(format!("{}  Add game folder", icon::FOLDER_OPEN)).color(p.on_accent).family(semibold()),
        )
        .fill(p.accent)
        .min_size(size);
        let iso = egui::Button::new(t(format!("{}  Open files", icon::DISC))).min_size(size);
        let find = egui::Button::new(t(format!("{}  Find installed games", icon::MAGNIFYING_GLASS))).min_size(size);
        let mut go = |ui: &mut Ui, b: egui::Button, tip: &str, which: u8| {
            if ui.add(b).on_hover_text(tip).clicked() {
                match which {
                    0 => {
                        if let Some(f) = rfd::FileDialog::new().pick_folder() {
                            self.begin_import(f);
                        }
                    }
                    1 => {
                        if let Some(files) = rfd::FileDialog::new()
                            .add_filter("Game assets and disc images", ef_core::import::FILE_EXTENSIONS)
                            .add_filter("All files", &["*"])
                            .pick_files()
                        {
                            for file in files {
                                self.begin_import(file);
                            }
                        }
                    }
                    _ => self.start_scan(ctx),
                }
            }
        };
        // in the top bar's right-to-left layout this order puts "Add game folder" at the far right
        go(ui, add, "Add an installed or extracted game folder", 0);
        go(ui, iso, "Select multiple assets or disc images. Companion files in the same game folder are included.", 1);
        go(ui, find, "Look in Steam libraries and the Rockstar Games folders", 2);
    }

    fn sidebar(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        egui::Panel::left("library")
            .resizable(true)
            .default_size(286.0)
            .size_range(230.0..=460.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::same(12)).stroke(Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                section_label(ui, p, "Library");
                ui.add_space(6.0);
                self.library_cards(ui, p);
                if self.entries.is_empty() {
                    ui.label(
                        RichText::new(
                            "No games yet. Add a folder or disc image, or let Eyefinder look for installed copies.",
                        )
                        .color(p.muted)
                        .size(12.5),
                    );
                }
                if self.selected_entry.is_none() || self.current().is_none() {
                    return;
                }
                ui.add_space(12.0);
                // Assets | Files switch
                ui.horizontal(|ui| {
                    // two buttons and the spacing between them fill the width exactly
                    let w = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).floor();
                    for (files, label) in [
                        (false, format!("{} Assets", icon::SQUARES_FOUR)),
                        (true, format!("{} Files", icon::TREE_STRUCTURE)),
                    ] {
                        let on = self.settings.file_tree == files;
                        let b = egui::Button::new(
                            RichText::new(label).color(if on { p.on_accent } else { p.text }).family(semibold()),
                        )
                        .fill(if on { p.accent } else { p.raised })
                        .min_size(vec2(w, 28.0));
                        if ui.add(b).clicked() {
                            self.settings.file_tree = files;
                        }
                    }
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if self.settings.file_tree {
                        self.file_tree(ui, p, ctx);
                    } else {
                        self.asset_sections(ui, p);
                    }
                });
            });
    }

    fn library_cards(&mut self, ui: &mut Ui, p: &Palette) {
        let mut select = None;
        let mut remove = None;
        let mut change = None;
        for (i, e) in self.entries.iter().enumerate() {
            let selected = self.selected_entry == Some(i);
            let r = egui::Frame::new()
                .fill(if selected { p.accent_soft } else { p.raised })
                .stroke(Stroke::new(1.0, if selected { p.accent } else { p.border }))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match &e.state {
                        LoadState::Ready { install, catalog } => {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(install.game.short()).family(semibold()).size(14.5));
                                pill(ui, install.platform.short(), p.on_accent, p.accent);
                                if !install.edition.is_unknown() {
                                    pill(ui, &install.edition.label, p.text, p.border);
                                }
                            });
                            let tex = match &e.textures {
                                TexState::Ready(t) => plural(t.assets.len(), "texture"),
                                TexState::Indexing { .. } => "indexing textures...".into(),
                                TexState::Waiting => String::new(),
                            };
                            ui.label(
                                RichText::new(format!("{} · {tex}", plural(catalog.assets.len(), "audio item")))
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
                            ui.label(RichText::new(format!("{} Could not open", icon::WARNING)).color(p.danger));
                            ui.label(RichText::new(err).size(11.0).color(p.muted));
                        }
                    }
                    let name = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(name).size(11.0).color(p.muted)).on_hover_text(e.path.display().to_string());
                });
            if selected {
                theme::glow(ui.painter(), r.response.rect, 6, p.accent, 1.0);
            }
            let r = r.response.interact(Sense::click());
            if r.clicked() {
                select = Some(i);
            }
            r.context_menu(|ui| {
                if ui.button(format!("{} Change game or version...", icon::SLIDERS_HORIZONTAL)).clicked() {
                    change = Some(i);
                    ui.close();
                }
                if ui.button(format!("{} Show in file manager", icon::FOLDER_OPEN)).clicked() {
                    open_in_file_manager(if e.path.is_dir() { &e.path } else { e.path.parent().unwrap_or(&e.path) });
                    ui.close();
                }
                if ui.button(format!("{} Remove from library", icon::TRASH)).clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
            ui.add_space(6.0);
        }
        if let Some(i) = select {
            self.select_entry(i);
        }
        if let Some(i) = change {
            let path = self.entries[i].path.clone();
            self.open_import(path, true);
        }
        if let Some(i) = remove {
            self.remove_entry(i);
        }
    }

    fn asset_sections(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(sel) = self.selected_entry else { return };
        let Some((_, cat)) = self.current() else { return };
        let counts: Vec<(Category, usize)> = Category::ALL
            .iter()
            .map(|&c| (c, cat.assets.iter().filter(|a| a.category == c).count()))
            .filter(|(_, n)| *n > 0)
            .collect();
        let total: usize = counts.iter().map(|(_, n)| n).sum();
        let tex_groups: Option<(usize, Vec<(String, usize)>)> =
            self.textures_of(sel).map(|t| (t.assets.len(), t.groups()));
        let indexing = match &self.entries[sel].textures {
            TexState::Indexing { done, total } => Some((done.load(Ordering::Relaxed), total.load(Ordering::Relaxed))),
            _ => None,
        };
        let mut new_view = None;

        let section = |ui: &mut Ui, id: &str, ic: &str, title: &str, count: String, body: &mut dyn FnMut(&mut Ui)| {
            let sid = ui.make_persistent_id(("section", id));
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), sid, true);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            if resp.clicked() {
                state.toggle(ui);
            }
            let open = state.openness(ui.ctx());
            let painter = ui.painter();
            if resp.hovered() {
                painter.rect_filled(rect, CornerRadius::same(4), p.raised);
            }
            // chevron rotates as the section opens
            let c = rect.left_center() + vec2(9.0, 0.0);
            let rot = egui::emath::Rot2::from_angle(std::f32::consts::FRAC_PI_2 * open);
            let pts = [vec2(-3.0, -4.5), vec2(3.0, 0.0), vec2(-3.0, 4.5)].map(|v| c + rot * v);
            painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.6, p.muted)));
            painter.text(
                rect.left_center() + vec2(22.0, 0.0),
                egui::Align2::LEFT_CENTER,
                ic,
                egui::FontId::proportional(14.0),
                p.accent_text,
            );
            painter.text(
                rect.left_center() + vec2(42.0, 0.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::new(12.0, semibold()),
                p.text,
            );
            painter.text(
                rect.right_center() - vec2(8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                count,
                egui::FontId::proportional(11.5),
                p.muted,
            );
            state.store(ui.ctx());
            state.show_body_unindented(ui, |ui| {
                body(ui);
                ui.add_space(6.0);
            });
        };

        // AUDIO
        let view = self.view.clone();
        section(ui, "audio", icon::SPEAKER_HIGH, "AUDIO", total.to_string(), &mut |ui| {
            if nav_row(ui, p, "a-all", icon::STACK, "All audio", Some(total.to_string()), view == View::Audio(None))
                .clicked()
            {
                new_view = Some(View::Audio(None));
            }
            for &(c, n) in &counts {
                if nav_row(
                    ui,
                    p,
                    ("a", c.label()),
                    category_icon(c),
                    c.label(),
                    Some(n.to_string()),
                    view == View::Audio(Some(c)),
                )
                .clicked()
                {
                    new_view = Some(View::Audio(Some(c)));
                }
            }
        });
        ui.add_space(4.0);
        // TEXTURES
        let count = match (&tex_groups, indexing) {
            (Some((n, _)), _) => n.to_string(),
            (None, Some((d, t))) => format!("{d}/{t}"),
            _ => "-".into(),
        };
        section(ui, "textures", icon::IMAGE, "TEXTURES", count, &mut |ui| match &tex_groups {
            Some((n, groups)) => {
                if *n == 0 {
                    ui.label(
                        RichText::new("No texture dictionaries were found in this copy.").size(12.0).color(p.muted),
                    );
                    return;
                }
                if nav_row(
                    ui,
                    p,
                    "t-all",
                    icon::SQUARES_FOUR,
                    "All textures",
                    Some(n.to_string()),
                    view == View::Textures(TexFilter::All),
                )
                .clicked()
                {
                    new_view = Some(View::Textures(TexFilter::All));
                }
                for (g, n) in groups {
                    let ic = if is_img_name(g) { icon::ARCHIVE } else { icon::FOLDER_SIMPLE };
                    let sel = view == View::Textures(TexFilter::Group(g.clone()));
                    if nav_row(ui, p, ("t", g.as_str()), ic, g, Some(n.to_string()), sel).on_hover_text(g).clicked() {
                        new_view = Some(View::Textures(TexFilter::Group(g.clone())));
                    }
                }
            }
            None => {
                let (d, t) = indexing.unwrap_or((0, 0));
                ui.add_space(4.0);
                ui.add(
                    egui::ProgressBar::new(if t > 0 { d as f32 / t as f32 } else { 0.0 })
                        .fill(p.accent)
                        .desired_height(6.0),
                );
                ui.label(RichText::new(format!("Reading texture dictionaries ({d} of {t})")).size(12.0).color(p.muted));
            }
        });
        if let Some(v) = new_view {
            self.set_view(v);
        }
    }

    pub(crate) fn set_view(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.audio_key = None;
            self.tex_key = None;
            self.tex_level = 0;
        }
    }

    fn welcome(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.18).max(20.0));
            ui.label(RichText::new(icon::EYE).size(68.0).color(p.accent_text));
            ui.add_space(6.0);
            ui.label(RichText::new("WELCOME TO EYEFINDER").size(24.0).family(semibold()).extra_letter_spacing(3.0));
            ui.label(
                RichText::new(format!(
                    "Build {}",
                    &env!("EYEFINDER_REVISION")[..7.min(env!("EYEFINDER_REVISION").len())]
                ))
                .size(11.0)
                .color(p.muted),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Audio and textures: GTA1, London 1969/1961, GTA2, GTA III, Vice City and San Andreas.")
                    .size(14.5)
                    .color(p.muted),
            );
            ui.add_space(22.0);
            ui.horizontal(|ui| {
                let w = 3.0 * 196.0 + 16.0;
                ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                self.add_buttons(ui, p, ctx, true);
            });
            ui.add_space(14.0);
            ui.label(
                RichText::new("Drop folders or multiple files here. Older PC games: GRY, G24, STY, FON and SDT/RAW.")
                    .color(p.muted),
            );
            ui.add_space(30.0);
            egui::Frame::new()
                .fill(p.panel)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_max_width(600.0);
                    section_label(ui, p, "Support levels");
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

    // ---- dialogs -----------------------------------------------------------------------------------

    fn import_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(d) = &mut self.import else { return };
        let mut action: Option<Option<ImportChoice>> = None;
        let mut close = false;
        let m = egui::Modal::new("import".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(560.0);
            let title = if d.replace { format!("{} Change game or version", icon::SLIDERS_HORIZONTAL) } else { format!("{} Add to library", icon::PLUS_CIRCLE) };
            ui.label(RichText::new(title).size(19.0).family(semibold()));
            ui.label(RichText::new(d.path.display().to_string()).color(p.muted).size(12.5));
            ui.add_space(10.0);
            match &d.detected {
                Err(e) => {
                    let label = if d.choices.is_empty() { "Cannot import this selection" } else { "Choose the game" };
                    ui.label(RichText::new(format!("{} {label}", icon::WARNING)).color(p.danger).family(semibold()));
                    ui.label(RichText::new(e).color(p.muted));
                    if d.choices.is_empty() {
                        ui.label("Choose the installation folder, GTADATA/DATA folder, or supported asset files. A compressed installer/archive must be extracted first. PS1, GBC, Dreamcast and GBA asset decoders are not implemented.");
                    }
                    for &(game, platform) in &d.choices {
                        if ui.button(format!("Import as {} ({})", game.name(), platform.name())).clicked() {
                            action = Some(Some(ImportChoice { game, platform, edition: "unknown".into() }));
                        }
                    }
                    ui.add_space(12.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
                Ok(det) => {
                    egui::Frame::new().fill(p.raised).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Detected").size(11.5).color(p.muted));
                            ui.label(RichText::new(format!("{} · {} · {}", det.game.name(), det.platform.name(), det.edition.label)).family(semibold()));
                        });
                        for e in &det.evidence {
                            ui.label(RichText::new(format!("• {e}")).size(12.0).color(p.muted));
                        }
                    });
                    ui.add_space(12.0);
                    let games: Vec<Game> = d.choices.iter().fold(Vec::new(), |mut v, (g, _)| {
                        if !v.contains(g) {
                            v.push(*g);
                        }
                        v
                    });
                    egui::Grid::new("import-grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                        ui.label("Game");
                        egui::ComboBox::from_id_salt("game").width(360.0).selected_text(d.game.name()).show_ui(ui, |ui| {
                            for g in &games {
                                if ui.selectable_value(&mut d.game, *g, g.name()).changed() {
                                    d.platform = d.choices.iter().find(|c| c.0 == *g).map_or(d.platform, |c| c.1);
                                    d.edition = "unknown".into();
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Platform");
                        let plats: Vec<Platform> = d.choices.iter().filter(|c| c.0 == d.game).map(|c| c.1).collect();
                        egui::ComboBox::from_id_salt("platform").width(360.0).selected_text(d.platform.name()).show_ui(ui, |ui| {
                            for pl in plats {
                                if ui.selectable_value(&mut d.platform, pl, pl.name()).changed() {
                                    d.edition = if (d.game, pl) == (det.game, det.platform) { det.edition.id.clone() } else { "unknown".into() };
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("Version");
                        let editions: Vec<Edition> = ef_core::edition::known(d.game, d.platform);
                        let cur = editions.iter().find(|e| e.id == d.edition).map_or("Unknown version".to_string(), |e| e.label.clone());
                        egui::ComboBox::from_id_salt("edition").width(360.0).selected_text(cur).show_ui(ui, |ui| {
                            for e in &editions {
                                let mut label = e.label.clone();
                                if (d.game, d.platform) == (det.game, det.platform) && e.id == det.edition.id && !e.is_unknown() {
                                    label.push_str("  (detected)");
                                }
                                ui.selectable_value(&mut d.edition, e.id.clone(), label);
                            }
                        });
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Only the choices these files support are offered. The version is recorded in every export manifest; it does not change how data is decoded.")
                            .size(11.5)
                            .color(p.muted),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let label = if d.replace { format!("{} Apply", icon::CHECK) } else { format!("{} Add to library", icon::PLUS) };
                        if ui.add(accent_button(p, label).min_size(vec2(150.0, 32.0))).clicked() {
                            let same = (d.game, d.platform, d.edition.as_str()) == (det.game, det.platform, det.edition.id.as_str());
                            action = Some(if same {
                                None
                            } else {
                                Some(ImportChoice { game: d.game, platform: d.platform, edition: d.edition.clone() })
                            });
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 32.0))).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if m.should_close() {
            close = true;
        }
        if let Some(choice) = action {
            self.confirm_import(choice, ctx);
        } else if close {
            self.next_import();
        }
    }

    fn confirm_import(&mut self, choice: Option<ImportChoice>, ctx: &egui::Context) {
        let (path, replace) = self.import.as_ref().map(|d| (d.path.clone(), d.replace)).unwrap();
        if replace && let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.remove_entry(i);
        }
        self.add_game(path, choice, ctx, true);
        self.next_import();
    }

    fn scan_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(state) = &mut self.scan else { return };
        let mut close = false;
        let mut import: Vec<PathBuf> = Vec::new();
        let m = egui::Modal::new("scan".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(680.0);
            ui.label(
                RichText::new(format!("{} Find installed games", icon::MAGNIFYING_GLASS)).size(19.0).family(semibold()),
            );
            ui.label(
                RichText::new(
                    "Steam libraries, and on Windows the setup registry entries and the Rockstar Games folders.",
                )
                .color(p.muted)
                .size(12.5),
            );
            ui.add_space(10.0);
            match state {
                ScanState::Running(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for installed GTA games...");
                    });
                }
                ScanState::Done { found, picked } => {
                    if found.is_empty() {
                        ui.label(
                            RichText::new(
                                "No copies were found in the usual places. Use Add game folder to pick one yourself.",
                            )
                            .color(p.muted),
                        );
                    } else {
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (f, pick) in found.iter().zip(picked.iter_mut()) {
                                egui::Frame::new()
                                    .fill(p.raised)
                                    .corner_radius(CornerRadius::same(6))
                                    .inner_margin(Margin::symmetric(10, 7))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            let ok = f.result.is_ok();
                                            ui.add_enabled(ok, egui::Checkbox::without_text(pick));
                                            ui.vertical(|ui| {
                                                match &f.result {
                                                    Ok(i) => {
                                                        ui.horizontal(|ui| {
                                                            ui.label(RichText::new(i.game.name()).family(semibold()));
                                                            pill(ui, i.platform.short(), p.on_accent, p.accent);
                                                            if !i.edition.is_unknown() {
                                                                pill(ui, &i.edition.label, p.text, p.border);
                                                            }
                                                        });
                                                    }
                                                    Err(e) => {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{} Not recognised: {e}",
                                                                icon::WARNING
                                                            ))
                                                            .color(p.danger)
                                                            .size(12.5),
                                                        );
                                                    }
                                                }
                                                ui.label(RichText::new(f.path.display().to_string()).size(12.0));
                                                ui.label(RichText::new(&f.source).size(11.0).color(p.muted));
                                            });
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Select all").clicked() {
                                for (f, pick) in found.iter().zip(picked.iter_mut()) {
                                    *pick = f.result.is_ok();
                                }
                            }
                            if ui.button("Select none").clicked() {
                                picked.iter_mut().for_each(|x| *x = false);
                            }
                        });
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let n = picked.iter().filter(|x| **x).count();
                        if ui
                            .add_enabled(
                                n > 0,
                                accent_button(p, format!("{} Add {}", icon::PLUS, plural(n, "game")))
                                    .min_size(vec2(150.0, 32.0)),
                            )
                            .clicked()
                        {
                            import = found
                                .iter()
                                .zip(picked.iter())
                                .filter(|(_, x)| **x)
                                .map(|(f, _)| f.path.clone())
                                .collect();
                            close = true;
                        }
                        if ui.add(egui::Button::new("Close").min_size(vec2(90.0, 32.0))).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if m.should_close() {
            close = true;
        }
        for path in import {
            self.add_game(path, None, ctx, false);
        }
        if close {
            self.scan = None;
        }
    }

    pub(crate) fn start_job<F>(&mut self, title: String, out_dir: PathBuf, total: usize, ctx: &egui::Context, work: F)
    where
        F: FnOnce(&Arc<Mutex<JobProgress>>, &Arc<AtomicBool>) -> Result<JobReport, String> + Send + 'static,
    {
        if let Some(parent) = out_dir.parent().and_then(|p| p.parent()) {
            self.settings.out_root = Some(parent.to_path_buf());
        }
        let progress = Arc::new(Mutex::new(JobProgress { done: 0, total, current: String::new() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let (p, c, r, ctx) = (progress.clone(), cancel.clone(), result.clone(), ctx.clone());
        thread::Builder::new()
            .name("export".into())
            .spawn(move || {
                let res = work(&p, &c);
                *r.lock().unwrap() = Some(res);
                ctx.request_repaint();
            })
            .expect("spawn export");
        self.job = Some(Job { title, progress, cancel, result, started: Instant::now(), out_dir });
    }

    fn job_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let finished = job.result.lock().unwrap().is_some();
        let mut close = false;
        egui::Modal::new("job".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(540.0);
            let pr = job.progress.lock().unwrap().clone();
            if !finished {
                ui.label(RichText::new(format!("{}...", job.title)).size(19.0).family(semibold()));
                ui.add_space(8.0);
                let frac = if pr.total > 0 { pr.done as f32 / pr.total as f32 } else { 0.0 };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .fill(p.accent)
                        .text(RichText::new(format!("{} of {}", pr.done, pr.total)).color(p.text)),
                );
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
                                RichText::new(format!("{} skipped or failed:", plural(rep.failed.len(), "item")))
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
                    if ui.add(accent_button(p, format!("{} Open folder", icon::FOLDER_OPEN))).clicked() {
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
        let m = egui::Modal::new("about".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::EYE).size(30.0).color(p.accent_text));
                ui.vertical(|ui| {
                    ui.label(RichText::new("EYEFINDER").size(19.0).family(semibold()).extra_letter_spacing(2.0));
                    ui.label(RichText::new(format!("Version {} · build {}", env!("CARGO_PKG_VERSION"), env!("EYEFINDER_REVISION"))).color(p.muted));
                });
            });
            ui.add_space(8.0);
            ui.label("A viewer and extractor for the assets of Grand Theft Auto III, Vice City and San Andreas.");
            ui.add_space(6.0);
            ui.label(RichText::new("Licensed under the GNU General Public License v3.0 or later.").size(12.5).color(p.muted));
            ui.label(RichText::new("Uses Symphonia (MPL-2.0), egui (MIT/Apache-2.0), png (MIT/Apache-2.0), Inter (OFL-1.1) and Phosphor Icons (MIT). Format knowledge comes from the re3/reVC, gta-reversed, librw (MIT) and vgmstream projects. Eyefinder contains no game data and is not affiliated with Rockstar Games or Take-Two Interactive.").size(12.5).color(p.muted));
            ui.add_space(10.0);
            if ui.button("Close").clicked() {
                self.show_about = false;
            }
        });
        if m.should_close() {
            self.show_about = false;
        }
    }

    // ---- file tree (sidebar) -------------------------------------------------------------------------

    fn file_tree(&mut self, ui: &mut Ui, p: &Palette, _ctx: &egui::Context) {
        let Some(sel) = self.selected_entry else { return };
        let Some((install, _)) = self.current() else { return };
        let vfs = install.vfs.clone();
        let id = self.entries[sel].id;
        let mut clicked = None;
        let selected = match &self.view {
            View::File(f) => Some(f.clone()),
            _ => None,
        };
        let tree = self.trees.entry(id).or_default();
        dir_node(ui, p, &vfs, "", tree, &selected, &mut clicked, 0);
        if let Some(c) = clicked {
            self.set_view(View::File(c));
        }
    }
}

/// One directory level of the file tree; IMG archives open like folders.
#[allow(clippy::too_many_arguments)]
fn dir_node(
    ui: &mut Ui,
    p: &Palette,
    vfs: &Arc<dyn Vfs>,
    dir: &str,
    tree: &mut FileTree,
    selected: &Option<String>,
    clicked: &mut Option<String>,
    depth: usize,
) {
    let Some(entries) = vfs.list(dir) else { return };
    let (dirs, files): (Vec<DirEntry>, Vec<DirEntry>) = entries.into_iter().partition(|e| e.is_dir);
    for e in dirs.into_iter().chain(files) {
        let path = if dir.is_empty() { e.name.clone() } else { format!("{dir}/{}", e.name) };
        let is_img = !e.is_dir && is_img_name(&e.name);
        if e.is_dir || is_img {
            let sid = ui.make_persistent_id(("tree", &path));
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), sid, false);
            let ic = if is_img {
                icon::ARCHIVE
            } else if state.is_open() {
                icon::FOLDER_OPEN
            } else {
                icon::FOLDER
            };
            let count = if is_img { Some(human_bytes(e.size)) } else { None };
            let r = tree_row(
                ui,
                p,
                &path,
                ic,
                &e.name,
                count,
                selected.as_deref() == Some(path.as_str()),
                depth,
                Some(state.openness(ui.ctx())),
            );
            if r.clicked() {
                state.toggle(ui);
                if is_img {
                    *clicked = Some(path.clone());
                }
            }
            state.store(ui.ctx());
            if state.is_open() {
                if is_img {
                    let archive = tree
                        .archives
                        .entry(path.clone())
                        .or_insert_with(|| ImgVfs::open(vfs, &path).ok().map(Arc::new))
                        .clone();
                    match archive {
                        Some(a) => {
                            for en in a.list("").unwrap_or_default() {
                                let ep = format!("{path}/{}", en.name);
                                let r = tree_row(
                                    ui,
                                    p,
                                    &ep,
                                    file_icon(&en.name),
                                    &en.name,
                                    Some(human_bytes(en.size)),
                                    selected.as_deref() == Some(ep.as_str()),
                                    depth + 1,
                                    None,
                                );
                                if r.clicked() {
                                    *clicked = Some(ep);
                                }
                            }
                        }
                        None => {
                            ui.label(RichText::new("    not an IMG archive").size(11.5).color(p.muted));
                        }
                    }
                } else {
                    dir_node(ui, p, vfs, &path, tree, selected, clicked, depth + 1);
                }
            }
        } else {
            let r = tree_row(
                ui,
                p,
                &path,
                file_icon(&e.name),
                &e.name,
                Some(human_bytes(e.size)),
                selected.as_deref() == Some(path.as_str()),
                depth,
                None,
            );
            if r.clicked() {
                *clicked = Some(path);
            }
        }
    }
}

pub(crate) fn file_icon(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    let ext = l.rsplit('.').next().unwrap_or("");
    match ext {
        "txd" => icon::IMAGE,
        "wav" | "mp3" | "adf" | "vb" | "raw" | "sdt" => icon::FILE_AUDIO,
        "mpg" | "mpeg" | "pss" => icon::FILM_STRIP,
        "dff" => icon::CUBE,
        "col" => icon::SHAPES,
        "ifp" => icon::PERSON_SIMPLE_RUN,
        "gxt" => icon::TEXT_AA,
        "dat" | "ide" | "ipl" | "cfg" | "txt" | "cnf" => icon::FILE_TEXT,
        "exe" | "dll" | "asi" | "elf" | "xbe" => icon::GEAR,
        "img" | "dir" => icon::ARCHIVE,
        _ => icon::FILE,
    }
}

#[allow(clippy::too_many_arguments)]
fn tree_row(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    ic: &str,
    label: &str,
    right: Option<String>,
    selected: bool,
    depth: usize,
    openness: Option<f32>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 23.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(3), p.accent_soft);
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top() + vec2(0.0, 4.0), vec2(3.0, rect.height() - 8.0)),
            CornerRadius::same(2),
            p.accent,
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), p.raised);
    }
    let x0 = rect.left() + 6.0 + depth as f32 * 14.0;
    if let Some(open) = openness {
        let c = egui::pos2(x0 + 4.0, rect.center().y);
        let rot = egui::emath::Rot2::from_angle(std::f32::consts::FRAC_PI_2 * open);
        let pts = [vec2(-2.5, -3.5), vec2(2.5, 0.0), vec2(-2.5, 3.5)].map(|v| c + rot * v);
        painter.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.4, p.muted)));
    }
    painter.text(
        egui::pos2(x0 + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        ic,
        egui::FontId::proportional(13.0),
        if selected { p.accent_text } else { p.muted },
    );
    let right_w = if right.is_some() { 60.0 } else { 0.0 };
    let max_w = (rect.right() - right_w - (x0 + 32.0)).max(20.0);
    let galley = theme::elided(painter, label, egui::FontId::proportional(12.5), p.text, max_w);
    painter.galley(egui::pos2(x0 + 32.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    if let Some(r) = right {
        painter.text(
            rect.right_center() - vec2(6.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            r,
            egui::FontId::proportional(10.5),
            p.muted,
        );
    }
    let _ = id;
    resp
}

#[cfg(test)]
mod import_tests {
    use super::*;
    fn fixture_game(game: Game, suffix: &str) -> PathBuf {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = std::env::temp_dir()
            .join(format!("ef-import-{game:?}-{suffix}-{}", std::process::id()))
            .join("Installed game");
        let _ = std::fs::remove_dir_all(&root);
        let (fixture, data, banks, style) = if game == Game::Gta2 {
            ("gta2_pc", "DATA", vec!["FSTYLE"], "style.sty")
        } else {
            ("gta1_pc", "GTADATA", vec!["LEVEL000", "LEVEL001"], "style.g24")
        };
        std::fs::create_dir_all(root.join(data).join("AUDIO")).unwrap();
        for bank in banks {
            for ext in ["SDT", "RAW"] {
                let relative = format!("{data}/AUDIO/{bank}.{ext}");
                std::fs::copy(
                    repo.join("crates/ef-audio/tests/fixtures").join(fixture).join(&relative),
                    root.join(relative),
                )
                .unwrap();
            }
        }
        std::fs::copy(
            repo.join("crates/ef-texture/tests/fixtures").join(style),
            root.join(data).join(if game == Game::Gta2 { "FSTYLE.STY" } else { "STYLE.G24" }),
        )
        .unwrap();
        root
    }
    fn loaded(app: &mut EyefinderApp, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            app.poll(ctx);
            if let LoadState::Failed(e) = &app.entries[0].state {
                panic!("import failed: {e}");
            }
            if app.current().is_some() && app.textures_of(0).is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "catalog worker timed out");
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn folder_subfolder_and_multiple_asset_imports_populate_both_views() {
        let ctx = egui::Context::default();
        for game in [Game::Gta1, Game::Gta2] {
            let root = fixture_game(game, "views");
            let (data, style, bank) = if game == Game::Gta2 {
                ("DATA", "FSTYLE.STY", "FSTYLE.SDT")
            } else {
                ("GTADATA", "STYLE.G24", "LEVEL000.SDT")
            };
            for selected in [
                root.parent().unwrap().to_path_buf(),
                root.clone(),
                root.join(data),
                root.join(data).join("AUDIO"),
                root.join(data).join(style),
                root.join(data).join("AUDIO").join(bank),
            ] {
                let mut app = EyefinderApp::empty(&ctx, Settings::default(), Err("test without audio device".into()));
                app.begin_import(selected);
                let d = app.import.as_ref().expect("import dialog");
                assert_eq!(d.detected.as_ref().expect("detected installation").game, game);
                // Multi-selecting companion assets queues only one installation.
                app.begin_import(root.join(data).join(style));
                app.begin_import(root.join(data).join("AUDIO").join(bank));
                assert!(app.pending_imports.is_empty());
                app.confirm_import(None, &ctx);
                loaded(&mut app, &ctx);
                app.refresh_audio_view();
                assert!(!app.audio_view.is_empty(), "audio view must have rows");
                app.view = View::Textures(TexFilter::All);
                app.refresh_texture_view();
                assert!(!app.tex_view.is_empty(), "texture view must have rows");
                let (install, audio) = app.current().unwrap();
                assert!(!audio.assets.is_empty());
                for asset in &audio.assets {
                    let opened =
                        ef_audio::open::open(asset, install.vfs.as_ref(), ef_audio::DecodeOptions::default()).unwrap();
                    assert!(opened.source.total_frames().unwrap() > 0);
                }
                let textures = app.textures_of(0).unwrap();
                let collection = textures.load(install, 0).unwrap();
                assert!(!collection.textures[0].raster.decode(0).unwrap().pixels.is_empty());
                // The saved path and choice must reopen after restarting the app.
                let saved = &app.settings.library[0];
                assert_eq!(ef_core::open_with(&saved.path, saved.choice.as_ref()).unwrap().game, game);
            }
            std::fs::remove_dir_all(root).unwrap();
        }
    }
    #[test]
    fn loose_texture_import_offers_identity_choice_and_loads_london() {
        let ctx = egui::Context::default();
        let root = std::env::temp_dir().join(format!("ef-loose-fon-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("IMAGE.FON");
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/ef-texture/tests/fixtures/images.fon"),
            &path,
        )
        .unwrap();
        let mut app = EyefinderApp::empty(&ctx, Settings::default(), Err("test".into()));
        app.begin_import(path);
        let d = app.import.as_ref().unwrap();
        assert!(d.detected.is_err());
        assert!(d.choices.contains(&(Game::London1961, Platform::Pc)));
        app.confirm_import(
            Some(ImportChoice { game: Game::London1961, platform: Platform::Pc, edition: "unknown".into() }),
            &ctx,
        );
        loaded(&mut app, &ctx);
        assert_eq!(app.textures_of(0).unwrap().assets.len(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
}
