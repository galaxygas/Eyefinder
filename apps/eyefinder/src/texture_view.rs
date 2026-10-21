//! The texture browser (grid or list), the preview panel and texture export.

use std::sync::Arc;

use ef_core::{GameInstall, SupportLevel};
use ef_texture::catalog::{TexAsset, TexCatalog};
use egui::{Align, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use egui_phosphor::regular as icon;

use crate::app::{
    EyefinderApp, JobProgress, JobReport, LoadState, TexFilter, TexState, TextureExportDialog, View, accent_button,
    human_bytes, modal_frame, plural, support_badge,
};
use crate::audio_view::{destination, empty_state, field, game_overview, notices, search_box, select_click};
use crate::theme::{self, Palette, section_label, semibold};
use crate::thumbs::Thumb;

fn matches(cat: &TexCatalog, a: &TexAsset, filter: &TexFilter, needle: &str) -> bool {
    let src = &cat.sources[a.source];
    let in_filter = match filter {
        TexFilter::All => true,
        TexFilter::Group(g) => &src.group == g,
        TexFilter::Dict(d) => src.path.eq_ignore_ascii_case(d),
    };
    in_filter
        && (needle.is_empty() || a.name.to_lowercase().contains(needle) || src.path.to_lowercase().contains(needle))
}

/// Draws `tex` fitted into `rect`, keeping its aspect ratio, over a checkerboard.
fn draw_fitted(ui: &Ui, p: &Palette, rect: Rect, tex: &egui::TextureHandle, size: [u32; 2], cell: f32) {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let s = (rect.width() / w).min(rect.height() / h);
    let r = Rect::from_center_size(rect.center(), vec2(w * s, h * s));
    theme::checkerboard(&ui.painter_at(rect), r, p, cell);
    ui.painter().image(
        tex.id(),
        r,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

impl EyefinderApp {
    pub(crate) fn refresh_texture_view(&mut self) {
        let Some(sel) = self.selected_entry else {
            self.tex_view.clear();
            return;
        };
        let n = self.textures_of(sel).map_or(0, |c| c.assets.len());
        let key = (sel, self.view.clone(), self.tex_search.clone(), n);
        if self.tex_key.as_ref() == Some(&key) {
            return;
        }
        let filter = match &self.view {
            View::Textures(f) => f.clone(),
            _ => TexFilter::All,
        };
        let needle = self.tex_search.to_lowercase();
        self.tex_view = match self.textures_of(sel) {
            Some(cat) => (0..cat.assets.len()).filter(|&i| matches(cat, &cat.assets[i], &filter, &needle)).collect(),
            None => Vec::new(),
        };
        let view = &self.tex_view;
        self.tex_selection.retain(|i| view.contains(i));
        self.tex_key = Some(key);
    }

    pub(crate) fn texture_browser(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        self.refresh_texture_view();
        let entry = self.selected_entry.unwrap();
        let LoadState::Ready { install, .. } = &self.entries[entry].state else { return };
        let install = install.clone();
        let entry_id = self.entries[entry].id;
        let cat = match &self.entries[entry].textures {
            TexState::Ready(c) => c.clone(),
            TexState::Indexing { done, total } => {
                let (d, t) =
                    (done.load(std::sync::atomic::Ordering::Relaxed), total.load(std::sync::atomic::Ordering::Relaxed));
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    ui.spinner();
                    ui.label(RichText::new(format!("Reading texture dictionaries... {d} of {t}")).color(p.muted));
                });
                return;
            }
            TexState::Waiting => return,
        };
        let filter = match &self.view {
            View::Textures(f) => f.clone(),
            _ => TexFilter::All,
        };

        // header
        ui.horizontal(|ui| {
            let title = match &filter {
                TexFilter::All => "All textures".to_string(),
                TexFilter::Group(g) | TexFilter::Dict(g) => g.clone(),
            };
            ui.label(RichText::new(title).size(21.0).family(semibold()));
            ui.label(
                RichText::new(format!(
                    "{} · {} · {}",
                    install.game.short(),
                    install.platform.name(),
                    plural(self.tex_view.len(), "texture")
                ))
                .color(p.muted),
            );
            if !cat.notices.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = cat.notices.len();
                    let t =
                        format!("{} {n} {} about these textures", icon::WARNING, if n == 1 { "note" } else { "notes" });
                    if ui.add(egui::Button::new(RichText::new(t).color(p.spec)).frame(false)).clicked() {
                        self.show_notices = !self.show_notices;
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let avail = ui.available_width();
            // the slider needs room; on narrow windows the size stays as it is
            let show_slider = self.settings.texture_grid && avail > 860.0;
            let right = 330.0 + 76.0 + if show_slider { 130.0 } else { 0.0 };
            let w = (avail - right - 40.0).clamp(120.0, 320.0);
            if search_box(ui, p, &mut self.tex_search, "Search textures and dictionaries", w) {
                self.tex_key = None;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let busy = self.job.is_some();
                let n_sel = self.tex_selection.len();
                if ui
                    .add_enabled(
                        !busy && !self.tex_view.is_empty(),
                        accent_button(p, format!("{} Export {} shown", icon::EXPORT, self.tex_view.len())),
                    )
                    .clicked()
                {
                    let scope = plural(self.tex_view.len(), "shown texture");
                    self.open_texture_export(self.tex_view.clone(), scope);
                }
                if ui
                    .add_enabled(
                        !busy && n_sel > 0,
                        egui::Button::new(format!("{} Export {n_sel} selected", icon::EXPORT)),
                    )
                    .clicked()
                {
                    let scope = plural(n_sel, "selected texture");
                    self.open_texture_export(self.tex_selection.iter().copied().collect(), scope);
                }
                ui.add_space(6.0);
                for (grid, ic, tip) in [(false, icon::LIST, "List"), (true, icon::SQUARES_FOUR, "Grid")] {
                    let on = self.settings.texture_grid == grid;
                    let b =
                        egui::Button::new(RichText::new(ic).size(15.0).color(if on { p.on_accent } else { p.text }))
                            .fill(if on { p.accent } else { p.raised });
                    if ui.add(b).on_hover_text(tip).clicked() {
                        self.settings.texture_grid = grid;
                    }
                }
                if show_slider {
                    ui.spacing_mut().slider_width = 90.0;
                    ui.add(egui::Slider::new(&mut self.settings.tile_size, 84.0..=220.0).show_value(false))
                        .on_hover_text("Thumbnail size");
                    ui.label(RichText::new(icon::MAGNIFYING_GLASS_PLUS).color(p.muted));
                }
            });
        });
        if self.show_notices && !cat.notices.is_empty() {
            ui.add_space(6.0);
            notices(ui, p, &cat.notices);
        }
        ui.add_space(8.0);
        if self.tex_view.is_empty() {
            empty_state(
                ui,
                p,
                icon::IMAGE,
                if cat.assets.is_empty() {
                    "No textures were found in this copy"
                } else {
                    "Nothing matches your search"
                },
            );
            return;
        }
        let modifiers = ctx.input(|i| i.modifiers);
        let clicked = if self.settings.texture_grid {
            self.texture_grid(ui, p, &install, &cat, entry_id)
        } else {
            self.texture_list(ui, p, &install, &cat, entry_id)
        };
        if let Some(idx) = clicked {
            select_click(&mut self.tex_selection, &mut self.tex_anchor, &self.tex_view, idx, modifiers);
            if self.tex_focused != Some(idx) {
                self.tex_level = 0;
                self.tex_zoom = None;
            }
            self.tex_focused = Some(idx);
        }
    }

    fn texture_grid(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        install: &GameInstall,
        cat: &Arc<TexCatalog>,
        entry_id: u64,
    ) -> Option<usize> {
        let tile = self.settings.tile_size;
        let gap = 10.0;
        let label_h = 34.0;
        let cols = (((ui.available_width() + gap) / (tile + gap)).floor() as usize).max(1);
        let rows = self.tex_view.len().div_ceil(cols);
        let mut clicked = None;
        let view = self.tex_view.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(
            ui,
            tile + label_h + gap,
            rows,
            |ui, range| {
                for r in range {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for c in 0..cols {
                            let Some(&idx) = view.get(r * cols + c) else { break };
                            let a = &cat.assets[idx];
                            let (rect, resp) = ui.allocate_exact_size(vec2(tile, tile + label_h), Sense::click());
                            let sel = self.tex_selection.contains(&idx);
                            let hover =
                                ui.ctx().animate_bool_with_time(ui.id().with(("tile", idx)), resp.hovered(), 0.12);
                            let painter = ui.painter();
                            painter.rect_filled(rect, CornerRadius::same(6), if sel { p.accent_soft } else { p.panel });
                            let stroke = if sel {
                                Stroke::new(1.5, p.accent)
                            } else {
                                Stroke::new(1.0, p.border.lerp_to_gamma(p.accent, hover * 0.6))
                            };
                            painter.rect_stroke(rect, CornerRadius::same(6), stroke, egui::StrokeKind::Inside);
                            if sel {
                                theme::glow(painter, rect, 6, p.accent, 0.8);
                            }
                            let img_rect =
                                Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(tile - 12.0, tile - 12.0));
                            if a.support.can_decode() {
                                match self.thumbs.get((entry_id, idx, 0, false), install, cat) {
                                    Some(Thumb::Ready(t, size)) => draw_fitted(ui, p, img_rect, t, *size, 8.0),
                                    Some(Thumb::Failed(e)) => {
                                        ui.painter().text(
                                            img_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            icon::WARNING_CIRCLE,
                                            egui::FontId::proportional(24.0),
                                            p.danger,
                                        );
                                        resp.clone().on_hover_text(e.clone());
                                    }
                                    None => {
                                        ui.painter().text(
                                            img_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            icon::HOURGLASS,
                                            egui::FontId::proportional(20.0),
                                            p.muted,
                                        );
                                    }
                                }
                            } else {
                                ui.painter().text(
                                    img_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    icon::PROHIBIT,
                                    egui::FontId::proportional(26.0),
                                    p.unsupported,
                                );
                            }
                            let name_pos = egui::pos2(rect.left() + 8.0, rect.bottom() - label_h + 2.0);
                            let name = theme::elided(
                                ui.painter(),
                                a.name.clone(),
                                egui::FontId::new(12.0, semibold()),
                                p.text,
                                tile - 16.0,
                            );
                            ui.painter().galley(name_pos, name, p.text);
                            let info = theme::elided(
                                ui.painter(),
                                format!("{}×{} · {}", a.width, a.height, a.format),
                                egui::FontId::proportional(10.5),
                                p.muted,
                                tile - 16.0,
                            );
                            ui.painter().galley(egui::pos2(rect.left() + 8.0, rect.bottom() - 16.0), info, p.muted);
                            let resp = resp.on_hover_text(format!("{}\n{}", a.name, cat.sources[a.source].path));
                            if resp.clicked() {
                                clicked = Some(idx);
                            }
                        }
                    });
                    ui.add_space(gap - ui.spacing().item_spacing.y);
                }
            },
        );
        clicked
    }

    fn texture_list(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        install: &GameInstall,
        cat: &Arc<TexCatalog>,
        entry_id: u64,
    ) -> Option<usize> {
        let mut clicked = None;
        let view = self.tex_view.clone();
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(4, 4))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                TableBuilder::new(ui)
                    .sense(Sense::click())
                    .cell_layout(Layout::left_to_right(Align::Center))
                    .column(Column::exact(40.0))
                    .column(Column::remainder().at_least(140.0).clip(true))
                    .column(Column::initial(90.0).at_least(70.0))
                    .column(Column::initial(150.0).at_least(90.0).clip(true).resizable(true))
                    .column(Column::initial(46.0))
                    .column(Column::initial(200.0).at_least(100.0).clip(true).resizable(true))
                    .column(Column::exact(112.0))
                    .header(26.0, |mut h| {
                        for t in ["", "NAME", "SIZE", "FORMAT", "MIPS", "DICTIONARY", "SUPPORT"] {
                            h.col(|ui| {
                                ui.label(
                                    RichText::new(t)
                                        .size(10.5)
                                        .color(p.muted)
                                        .family(semibold())
                                        .extra_letter_spacing(0.8),
                                );
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(36.0, view.len(), |mut row| {
                            let idx = view[row.index()];
                            let a = &cat.assets[idx];
                            row.set_selected(self.tex_selection.contains(&idx));
                            row.col(|ui| {
                                let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                                if a.support.can_decode()
                                    && let Some(Thumb::Ready(t, size)) =
                                        self.thumbs.get((entry_id, idx, 0, false), install, cat)
                                {
                                    draw_fitted(ui, p, rect, t, *size, 4.0);
                                }
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&a.name).family(semibold()));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(format!("{}×{}", a.width, a.height)).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&a.format).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(a.levels.to_string()).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                ui.label(RichText::new(&cat.sources[a.source].path).color(p.muted).size(12.5));
                            });
                            row.col(|ui| {
                                support_badge(ui, p, a.support);
                            });
                            if row.response().clicked() {
                                clicked = Some(idx);
                            }
                        });
                    });
            });
        clicked
    }

    pub(crate) fn texture_details(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(sel) = self.selected_entry else { return };
        let entry_id = self.entries[sel].id;
        egui::Panel::right("tex-details")
            .resizable(true)
            .default_size(360.0)
            .size_range(260.0..=720.0)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(Margin::same(14)).stroke(Stroke::new(1.0, p.border)))
            .show(ui, |ui| {
                let Some((install, _)) = self.current() else { return };
                let install = install.clone();
                let Some(cat) = self.textures_of(sel).cloned() else {
                    game_overview(ui, p, &install);
                    return;
                };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(idx) = self.tex_focused.filter(|&i| i < cat.assets.len()) else {
                        game_overview(ui, p, &install);
                        ui.add_space(14.0);
                        ui.label(RichText::new("Select a texture to preview it.").color(p.muted).size(12.5));
                        return;
                    };
                    let a = cat.assets[idx].clone();
                    let src = cat.sources[a.source].clone();
                    section_label(ui, p, "Texture");
                    ui.add_space(4.0);
                    ui.label(RichText::new(&a.name).size(17.0).family(semibold()));
                    ui.add_space(4.0);
                    support_badge(ui, p, a.support);
                    if let Some(w) = a.why_unsupported {
                        ui.label(RichText::new(w).size(12.0).color(p.muted));
                    } else if a.support == SupportLevel::FromSpec {
                        ui.label(RichText::new(SupportLevel::FromSpec.explanation()).size(12.0).color(p.muted));
                    }
                    ui.add_space(8.0);
                    // preview
                    let w = ui.available_width();
                    let level = self.tex_level.min(a.levels.saturating_sub(1));
                    let (lw, lh) = ((a.width >> level).max(1), (a.height >> level).max(1));
                    let fit = (w / lw as f32).min(320.0 / lh as f32).min(8.0);
                    let zoom = self.tex_zoom.unwrap_or(fit);
                    let size = vec2(lw as f32 * zoom, lh as f32 * zoom);
                    egui::ScrollArea::both().id_salt("preview").max_height(340.0).show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(size.max(vec2(w.min(size.x.max(64.0)), 64.0)), Sense::hover());
                        let img = Rect::from_min_size(rect.min, size);
                        if a.support.can_decode() {
                            match self.thumbs.get((entry_id, idx, level, true), &install, &cat) {
                                Some(Thumb::Ready(t, _)) => {
                                    theme::checkerboard(&ui.painter_at(rect), img, p, 8.0);
                                    ui.painter().image(
                                        t.id(),
                                        img,
                                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                        egui::Color32::WHITE,
                                    );
                                }
                                Some(Thumb::Failed(e)) => {
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        format!("{} {e}", icon::WARNING),
                                        egui::FontId::proportional(12.0),
                                        p.danger,
                                    );
                                }
                                None => {
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        icon::HOURGLASS,
                                        egui::FontId::proportional(22.0),
                                        p.muted,
                                    );
                                }
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Fit").clicked() {
                            self.tex_zoom = None;
                        }
                        for z in [1.0f32, 2.0, 4.0] {
                            if ui.button(format!("{z}×")).clicked() {
                                self.tex_zoom = Some(z);
                            }
                        }
                        ui.label(RichText::new(format!("{:.0}%", zoom * 100.0)).color(p.muted).size(12.0));
                    });
                    if a.levels > 1 {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Mip level").size(12.0).color(p.muted));
                            let mut l = level;
                            if ui.add(egui::Slider::new(&mut l, 0..=a.levels - 1).integer()).changed() {
                                self.tex_level = l;
                            }
                        });
                    }
                    ui.add_space(8.0);
                    field(ui, p, "Size", format!("{} × {} (level {level}: {lw} × {lh})", a.width, a.height));
                    field(ui, p, "Format", a.format.clone());
                    field(ui, p, "Mip levels", a.levels.to_string());
                    field(ui, p, "Native platform", a.platform.label().to_string());
                    if !a.mask.is_empty() {
                        field(ui, p, "Alpha mask name", a.mask.clone());
                    }
                    field(ui, p, "Dictionary", src.path.clone());
                    field(ui, p, "RenderWare version", ef_texture::rw::version_label(src.rw_version));
                    field(ui, p, "Dictionary size", human_bytes(src.bytes));
                    if ui.button(format!("{} Show this dictionary only", icon::FUNNEL)).clicked() {
                        self.set_view(View::Textures(TexFilter::Dict(src.path.clone())));
                    }
                });
            });
    }

    fn open_texture_export(&mut self, assets: Vec<usize>, scope: String) {
        let Some(entry) = self.selected_entry else { return };
        let Some((install, _)) = self.current() else { return };
        let Some(cat) = self.textures_of(entry) else { return };
        let assets: Vec<usize> = assets.into_iter().filter(|&i| cat.assets[i].support.can_decode()).collect();
        if assets.is_empty() {
            self.toast("Nothing to export: the chosen textures cannot be decoded yet");
            return;
        }
        let out_dir = self.default_out_dir(install, "Textures");
        self.texture_export = Some(TextureExportDialog { entry, assets, scope, out_dir });
    }

    pub(crate) fn texture_export_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(d) = &mut self.texture_export else { return };
        let mut start = false;
        let mut close = false;
        let m = egui::Modal::new("tex-export".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(540.0);
            ui.label(RichText::new(format!("{} Export textures", icon::EXPORT)).size(19.0).family(semibold()));
            ui.label(
                RichText::new(format!("{} as PNG files with manifest.json and manifest.csv", d.scope)).color(p.muted),
            );
            ui.add_space(12.0);
            destination(ui, p, &mut d.out_dir);
            ui.add_space(10.0);
            ui.checkbox(&mut self.settings.texture_mipmaps, "Also write every mip level (name.mip1.png, ...)");
            ui.checkbox(&mut self.settings.texture_dds, "Keep DXT-compressed textures untouched as .dds")
                .on_hover_text("The compressed blocks are copied byte for byte, so nothing of the original is lost.");
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui
                    .add(accent_button(p, format!("{} Start export", icon::EXPORT)).min_size(vec2(130.0, 32.0)))
                    .clicked()
                {
                    start = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 32.0))).clicked() {
                    close = true;
                }
            });
        });
        if m.should_close() {
            close = true;
        }
        if start {
            let d = self.texture_export.take().unwrap();
            let Some((install, _)) = self.entry(d.entry) else { return };
            let install = install.clone();
            let Some(cat) = self.textures_of(d.entry).cloned() else { return };
            let opts = ef_texture::export::ExportOptions {
                mipmaps: self.settings.texture_mipmaps,
                originals: self.settings.texture_dds,
            };
            let (assets, out) = (d.assets, d.out_dir.clone());
            let c2 = ctx.clone();
            self.start_job("Exporting textures".into(), d.out_dir, assets.len(), ctx, move |progress, cancel| {
                let pr = |done: usize, total: usize| {
                    let mut g = progress.lock().unwrap();
                    *g = JobProgress { done, total, current: String::new() };
                    c2.request_repaint();
                };
                let r = ef_texture::export::export(&install, &cat, &assets, &out, opts, &pr, cancel)
                    .map_err(|e| e.to_string())?;
                let failed = r
                    .skipped
                    .into_iter()
                    .map(|s| match s.split_once(": ") {
                        Some((a, b)) => (a.to_string(), b.to_string()),
                        None => (s, String::new()),
                    })
                    .collect();
                Ok(JobReport { written: r.written, failed, cancelled: r.cancelled })
            });
        } else if close {
            self.texture_export = None;
        }
    }
}
