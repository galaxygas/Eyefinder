//! The audio browser, its details panel, the player bar and audio export.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use ef_audio::export::{ExportOptions, Progress};
use ef_audio::{AssetKind, AudioAsset, DecodeOptions};
use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::TableBuilder;
use egui_phosphor::regular as icon;

use crate::app::{
    AudioExportDialog, EyefinderApp, JobProgress, JobReport, LoadState, ROW_H, View, accent_button, fmt_dur,
    human_bytes, modal_frame, plural, support_badge,
};
use crate::player::Waveform;
use crate::theme::{Palette, section_label, semibold};

/// Player clock: tenths of a second for clips under a minute, whole seconds otherwise.
pub(crate) fn fmt_player_time(frames: u64, rate: u32, total: Option<u64>) -> String {
    if rate > 0 && total.is_some_and(|t| t < 60 * rate as u64) {
        let tenths = frames * 10 / rate as u64;
        return format!("0:{:02}.{}", tenths / 10, tenths % 10);
    }
    fmt_dur(frames, rate)
}

fn format_label(a: &AudioAsset) -> String {
    match &a.kind {
        AssetKind::Stream { path } => path.rsplit('.').next().unwrap_or("").to_ascii_uppercase(),
        AssetKind::Adf { .. } => "ADF (MP3)".into(),
        AssetKind::Vb { .. } => "VB (PS-ADPCM)".into(),
        AssetKind::Movie { .. } => "MPEG movie".into(),
        AssetKind::LegacyPcm { bits, channels, .. } => format!("PCM {bits}-bit / {channels} ch"),
        AssetKind::Gta2Sfx { bits, .. } => format!("PCM {bits}-bit (GTA2 SDT)"),
        AssetKind::Sfx { .. } => "PCM (SFX.RAW)".into(),
        AssetKind::SaTrack { .. } => "Ogg Vorbis".into(),
        AssetKind::SaSfx { .. } => "PCM (bank)".into(),
        AssetKind::CdAudio { .. } => "CD audio (PCM 16-bit / 2 ch)".into(),
        AssetKind::DcStr { .. } => "Dreamcast STR (AICA ADPCM)".into(),
    }
}

impl EyefinderApp {
    pub(crate) fn refresh_audio_view(&mut self) {
        let Some(sel) = self.selected_entry else {
            self.audio_view.clear();
            return;
        };
        let key = (sel, self.view.clone(), self.search.clone());
        if self.audio_key.as_ref() == Some(&key) {
            return;
        }
        let category = match self.view {
            View::Audio(c) => c,
            _ => None,
        };
        let needle = self.search.to_lowercase();
        self.audio_view = match self.current() {
            Some((_, cat)) => cat
                .assets
                .iter()
                .enumerate()
                .filter(|(_, a)| category.is_none_or(|c| a.category == c))
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
        let view = &self.audio_view;
        self.selection.retain(|i| view.contains(i));
        self.audio_key = Some(key);
    }

    // ---- playback ----------------------------------------------------------------------------------

    /// Whether `idx` of the selected entry is the asset currently loaded in the player.
    pub(crate) fn is_playing(&self, idx: usize) -> bool {
        self.playing_entry.is_some()
            && self.playing_entry == self.selected_entry
            && self.playback.as_ref().is_some_and(|p| p.asset_index == idx)
    }

    pub(crate) fn play(&mut self, entry: usize, idx: usize, start_frame: u64, ctx: &egui::Context) {
        let Some((install, cat)) = self.entry(entry) else { return };
        let asset = cat.assets[idx].clone();
        let vfs = install.vfs.clone();
        if !asset.support.can_decode() {
            self.toast(format!("{} cannot be played: {}", asset.title, asset.support.label()));
            return;
        }
        let Ok(audio) = &mut self.audio else {
            self.toast("No audio output device is available");
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
            self.toast(format!("Playback failed: {e}"));
        }
    }

    pub(crate) fn stop_playback(&mut self) {
        if let Ok(a) = &mut self.audio {
            a.stop();
        }
        self.playback = None;
        self.playing_entry = None;
        self.waveform = None;
    }

    pub(crate) fn toggle_pause(&mut self, ctx: &egui::Context) {
        match (&mut self.audio, &self.playback) {
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

    // ---- browser -----------------------------------------------------------------------------------

    pub(crate) fn audio_browser(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        // the sidebar may have switched game or category earlier in this frame
        self.refresh_audio_view();
        let entry = self.selected_entry.unwrap();
        let LoadState::Ready { install, catalog } = &self.entries[entry].state else { return };
        let (install, catalog) = (install.clone(), catalog.clone());
        let category = match self.view {
            View::Audio(c) => c,
            _ => None,
        };

        ui.horizontal(|ui| {
            let title = category.map_or("All audio", |c| c.label());
            ui.label(RichText::new(title).size(21.0).family(semibold()));
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    install.game.short(),
                    install.platform.name(),
                    plural(self.audio_view.len(), "item")
                ))
                .color(p.muted),
            );
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
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 360.0).clamp(120.0, 320.0);
            if search_box(ui, p, &mut self.search, "Search titles, ids and files", w) {
                self.audio_key = None;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.job.is_some();
                let n_sel = self.selection.len();
                let all = accent_button(p, format!("{} Export {} shown", icon::EXPORT, self.audio_view.len()));
                if ui.add_enabled(!busy && !self.audio_view.is_empty(), all).clicked() {
                    let scope = plural(self.audio_view.len(), "shown item");
                    self.open_audio_export(self.audio_view.clone(), scope);
                }
                if ui
                    .add_enabled(
                        !busy && n_sel > 0,
                        egui::Button::new(format!("{} Export {n_sel} selected", icon::EXPORT)),
                    )
                    .clicked()
                {
                    let scope = plural(n_sel, "selected item");
                    self.open_audio_export(self.selection.iter().copied().collect(), scope);
                }
            });
        });
        if self.show_notices && !catalog.notices.is_empty() {
            ui.add_space(6.0);
            notices(ui, p, &catalog.notices);
        }
        ui.add_space(8.0);

        if self.audio_view.is_empty() {
            empty_state(
                ui,
                p,
                icon::SPEAKER_SLASH,
                if catalog.assets.is_empty() {
                    "No decodable audio in this copy"
                } else {
                    "Nothing matches your search"
                },
            );
            return;
        }

        let playing = self.playback.as_ref().map(|p| p.asset_index).filter(|_| self.playing_entry == Some(entry));
        let modifiers = ctx.input(|i| i.modifiers);
        let mut clicked: Option<usize> = None;
        let mut play_req: Option<usize> = None;
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(4, 4))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let view = &self.audio_view;
                crate::table_columns::audio(TableBuilder::new(ui))
                    .striped(false)
                    .sense(Sense::click())
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .header(26.0, |mut h| {
                        for t in ["", "TITLE", "CATEGORY", "FORMAT", "SIZE", "SUPPORT"] {
                            h.col(|ui| {
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                                    ui.label(
                                        RichText::new(t)
                                            .size(10.5)
                                            .color(p.muted)
                                            .family(semibold())
                                            .extra_letter_spacing(0.8),
                                    );
                                });
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
                                    p.accent_text
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
                                    t = t.color(p.accent_text).family(semibold());
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
            select_click(&mut self.selection, &mut self.anchor, &self.audio_view, idx, modifiers);
            self.focused = Some(idx);
        }
        if let Some(idx) = play_req {
            self.focused = Some(idx);
            self.play(entry, idx, 0, ctx);
        }
    }

    pub(crate) fn audio_details(&mut self, ui: &mut Ui, p: &Palette) {
        egui::Panel::right("details")
            .resizable(true)
            .default_size(290.0)
            .size_range(230.0..=480.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::same(14)).stroke(Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                let Some((install, cat)) = self.current() else { return };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(idx) = self.focused else {
                        game_overview(ui, p, install);
                        ui.add_space(14.0);
                        ui.label(RichText::new("Select an item to see its details. Double-click or press Enter to play, Space to pause.").color(p.muted).size(12.5));
                        return;
                    };
                    let a = &cat.assets[idx];
                    section_label(ui, p, "Details");
                    ui.add_space(6.0);
                    ui.label(RichText::new(&a.title).size(17.0).family(semibold()));
                    ui.add_space(4.0);
                    support_badge(ui, p, a.support);
                    ui.label(RichText::new(a.support.explanation()).size(12.0).color(p.muted));
                    ui.add_space(10.0);
                    field(ui, p, "Category", a.group.as_ref().map_or(a.category.label().to_string(), |g| format!("{} / {g}", a.category.label())));
                    field(ui, p, "Game identifier", a.game_id.clone());
                    field(ui, p, "Source file", a.source.clone());
                    field(ui, p, "Stored size", human_bytes(a.bytes));
                    field(ui, p, "Container", format_label(a));
                    if let Some(pb) = self.playback.as_ref().filter(|pb| self.is_playing(idx) && pb.error.is_none()) {
                        ui.separator();
                        section_label(ui, p, "Decoded");
                        ui.add_space(4.0);
                        field(ui, p, "Codec", pb.facts.info.codec.clone());
                        field(ui, p, "Sample rate", format!("{} Hz", pb.facts.rate));
                        field(ui, p, "Channels", pb.facts.channels.to_string());
                        if let Some(t) = pb.facts.total_frames {
                            field(ui, p, "Duration", format!("{} ({t} frames)", fmt_player_time(t, pb.facts.rate, Some(t))));
                        }
                        field(ui, p, "Exported as", pb.facts.output.clone());
                        for n in &pb.facts.info.notes {
                            ui.label(RichText::new(format!("• {n}")).size(12.0).color(p.muted));
                        }
                    }
                });
            });
    }

    pub(crate) fn player_bar(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
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
                    let play_btn = egui::Button::new(RichText::new(ic).size(20.0).color(p.on_accent))
                        .fill(p.accent)
                        .corner_radius(CornerRadius::same(20))
                        .min_size(vec2(40.0, 40.0));
                    let r = ui.add_enabled(self.playback.is_some() || self.focused.is_some(), play_btn);
                    if active && !paused {
                        crate::theme::glow(ui.painter(), r.rect, 20, p.accent, 1.0);
                    }
                    if r.clicked() {
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
                    painter.rect_filled(rect, CornerRadius::same(4), p.bg);
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
                            let t = x as f32 / cols as f32;
                            // played part in accent, fading slightly towards the playhead
                            let col = if t <= frac {
                                p.accent.gamma_multiply(0.55 + 0.45 * (t / frac.max(1e-3)))
                            } else {
                                p.muted.gamma_multiply(0.45)
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
                    if self.playback.is_some() && total.is_none() {
                        resp.on_hover_text("Seeking becomes available once the length is known");
                    }
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

    // ---- export ------------------------------------------------------------------------------------

    fn open_audio_export(&mut self, assets: Vec<usize>, scope: String) {
        let Some(entry) = self.selected_entry else { return };
        let Some((install, cat)) = self.current() else { return };
        let assets: Vec<usize> = assets.into_iter().filter(|&i| cat.assets[i].support.can_decode()).collect();
        if assets.is_empty() {
            self.toast("Nothing to export: the chosen items cannot be decoded");
            return;
        }
        let out_dir = self.default_out_dir(install, "Audio");
        self.audio_export = Some(AudioExportDialog { entry, assets, scope, out_dir });
    }

    pub(crate) fn audio_export_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(d) = &mut self.audio_export else { return };
        let mut start = false;
        let mut close = false;
        let modal = egui::Modal::new("export".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(540.0);
            ui.label(RichText::new(format!("{} Export audio", icon::EXPORT)).size(19.0).family(semibold()));
            ui.label(RichText::new(format!("{} as WAV files with manifest.json, manifest.csv and tracklist.txt", d.scope)).color(p.muted));
            ui.add_space(12.0);
            destination(ui, p, &mut d.out_dir);
            ui.add_space(10.0);
            ui.checkbox(&mut self.settings.float_output, "Decode MP3 / Vorbis to 32-bit float (full decoder precision)")
                .on_hover_text("Off: 16-bit PCM. PCM sources are always copied bit-exact and ADPCM always decodes to its exact 16-bit output.");
            ui.checkbox(&mut self.settings.split_markers, "Also write one file per cue marker when a stream has markers");
            ui.checkbox(&mut self.settings.keep_bitstream, "Keep the untouched audio bitstream of movies (.mp2)");
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.add(accent_button(p, format!("{} Start export", icon::EXPORT)).min_size(vec2(130.0, 32.0))).clicked() {
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
            self.start_audio_export(ctx);
        } else if close {
            self.audio_export = None;
        }
    }

    fn start_audio_export(&mut self, ctx: &egui::Context) {
        let Some(d) = self.audio_export.take() else { return };
        let LoadState::Ready { install, catalog } = &self.entries[d.entry].state else { return };
        let install = install.clone();
        let assets: Vec<AudioAsset> = d.assets.iter().map(|&i| catalog.assets[i].clone()).collect();
        let opts = ExportOptions {
            out_dir: d.out_dir.clone(),
            decode: DecodeOptions { float_output: self.settings.float_output },
            split_markers: self.settings.split_markers,
            keep_movie_bitstream: self.settings.keep_bitstream,
        };
        let total = assets.len();
        let c2 = ctx.clone();
        self.start_job("Exporting audio".into(), d.out_dir, total, ctx, move |progress, cancel| {
            let refs: Vec<&AudioAsset> = assets.iter().collect();
            let r = ef_audio::export::export(
                &install,
                &refs,
                &opts,
                |pr: Progress| {
                    *progress.lock().unwrap() = JobProgress { done: pr.done, total: pr.total, current: pr.current };
                    c2.request_repaint();
                },
                cancel,
            )
            .map_err(|e| e.to_string())?;
            Ok(JobReport { written: r.written, failed: r.failed, cancelled: r.cancelled })
        });
    }
}

// ---- shared widgets ----------------------------------------------------------------------------

pub(crate) fn field(ui: &mut Ui, p: &Palette, k: &str, v: String) {
    ui.label(RichText::new(k).size(11.5).color(p.muted));
    ui.label(RichText::new(v).size(13.0));
    ui.add_space(4.0);
}

pub(crate) fn game_overview(ui: &mut Ui, p: &Palette, install: &ef_core::GameInstall) {
    section_label(ui, p, "Game");
    ui.add_space(6.0);
    ui.label(RichText::new(install.game.name()).family(semibold()).size(15.0));
    ui.label(RichText::new(format!("{} · {}", install.platform.name(), install.edition.label)).color(p.muted));
    if install.user_choice {
        ui.label(RichText::new(format!("{} chosen by you", icon::USER)).size(11.5).color(p.accent_text));
    }
    ui.add_space(8.0);
    ui.label(RichText::new("How it was identified").size(12.0).color(p.muted));
    for e in &install.evidence {
        ui.label(RichText::new(format!("• {e}")).size(12.5));
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Source").size(12.0).color(p.muted));
    ui.label(RichText::new(install.vfs.describe()).size(12.5));
}

/// Search field; returns true when the text changed.
pub(crate) fn search_box(ui: &mut Ui, p: &Palette, text: &mut String, hint: &str, width: f32) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(icon::MAGNIFYING_GLASS).color(p.muted));
            let r =
                ui.add(egui::TextEdit::singleline(text).hint_text(hint).frame(egui::Frame::NONE).desired_width(width));
            changed |= r.changed();
            if !text.is_empty() && ui.small_button(icon::X).clicked() {
                text.clear();
                changed = true;
            }
        });
    changed
}

pub(crate) fn notices(ui: &mut Ui, p: &Palette, notes: &[String]) {
    egui::Frame::new()
        .fill(p.spec.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, p.spec.gamma_multiply(0.5)))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                for n in notes {
                    ui.label(RichText::new(format!("• {n}")).size(12.5));
                }
            });
        });
}

pub(crate) fn empty_state(ui: &mut Ui, p: &Palette, ic: &str, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(60.0);
        ui.label(RichText::new(ic).size(36.0).color(p.muted));
        ui.label(RichText::new(text).color(p.muted));
    });
}

pub(crate) fn destination(ui: &mut Ui, p: &Palette, out_dir: &mut PathBuf) {
    ui.label(RichText::new("Destination").size(12.0).color(p.muted));
    ui.horizontal(|ui| {
        let mut s = out_dir.display().to_string();
        if ui.add(egui::TextEdit::singleline(&mut s).desired_width(420.0)).changed() {
            *out_dir = PathBuf::from(s);
        }
        if ui.button(format!("{} Browse", icon::FOLDER_OPEN)).clicked()
            && let Some(f) =
                rfd::FileDialog::new().set_directory(out_dir.parent().unwrap_or(Path::new("."))).pick_folder()
        {
            *out_dir = f;
        }
    });
}

/// Click handling shared by the lists: Ctrl toggles, Shift selects a range, a plain click selects one.
pub(crate) fn select_click(
    selection: &mut std::collections::BTreeSet<usize>,
    anchor: &mut Option<usize>,
    view: &[usize],
    idx: usize,
    modifiers: egui::Modifiers,
) {
    if modifiers.command {
        if !selection.remove(&idx) {
            selection.insert(idx);
        }
        *anchor = Some(idx);
    } else if modifiers.shift {
        if let Some(a) = anchor.and_then(|a| view.iter().position(|&v| v == a)) {
            let b = view.iter().position(|&v| v == idx).unwrap_or(a);
            let (lo, hi) = (a.min(b), a.max(b));
            *selection = view[lo..=hi].iter().copied().collect();
        }
    } else {
        selection.clear();
        selection.insert(idx);
        *anchor = Some(idx);
    }
}
