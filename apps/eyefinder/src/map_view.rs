//! The game header strip with its "Export Map files" button, and the map export dialog. The catalog
//! is built and the export runs on worker threads (`ef_map`).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use ef_map::catalog::{self, Availability, MapCatalog};
use ef_map::export::ExportOptions;
use egui::{CornerRadius, Margin, RichText, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

use crate::app::{EyefinderApp, JobProgress, JobReport, accent_button, modal_frame, pill};
use crate::audio_view::destination;
use crate::theme::{Palette, semibold};

pub(crate) struct MapExportDialog {
    pub entry: usize,
    pub out_dir: PathBuf,
    pub opts: ExportOptions,
    pub availability: Availability,
    /// Built on a worker thread; `None` while it runs.
    pub catalog: Arc<Mutex<Option<Result<MapCatalog, String>>>>,
}

fn reason(r: &Result<(), String>) -> Option<&str> {
    r.as_ref().err().map(String::as_str)
}

impl EyefinderApp {
    /// The strip above the browsers: which game, version and store, and the map export.
    pub(crate) fn game_header(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context) {
        let Some(entry) = self.selected_entry else { return };
        let Some((install, _)) = self.entry(entry) else { return };
        let avail = catalog::availability(install.game, install.platform);
        let (game, platform, edition, store) =
            (install.game.name(), install.platform.name(), install.edition.label.clone(), install.store.clone());
        let mut open = false;
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(game).size(15.0).family(semibold()));
                    ui.label(RichText::new(format!("{platform} · {edition}")).color(p.muted));
                    if !store.is_unknown() {
                        pill(ui, &store.label(), p.text, p.raised);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let busy = self.job.is_some();
                        let b = ui.add_enabled(
                            avail.any() && !busy,
                            accent_button(p, format!("{} Export Map files", icon::MAP_TRIFOLD)).min_size(vec2(0.0, 30.0)),
                        );
                        let tip = if avail.any() {
                            "Raw map files, individual files, and full maps: top-down renders, radar and pause-menu maps".to_string()
                        } else {
                            reason(&avail.raw).unwrap_or("Not available").to_string()
                        };
                        if b.on_hover_text(tip).on_disabled_hover_text(reason(&avail.raw).unwrap_or("")).clicked() {
                            open = true;
                        }
                    });
                });
            });
        ui.add_space(8.0);
        if open {
            self.open_map_export(entry, ctx);
        }
    }

    pub(crate) fn open_map_export(&mut self, entry: usize, ctx: &egui::Context) {
        let Some((install, _)) = self.entry(entry) else { return };
        let install = install.clone();
        let availability = catalog::availability(install.game, install.platform);
        let out_dir = self.default_out_dir(&install, "Maps");
        let slot = Arc::new(Mutex::new(None));
        let (s, c) = (slot.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("map-catalog".into()).spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| catalog::build(&install)))
                .map_err(|_| "Reading the map files failed.".to_string());
            *s.lock().unwrap() = Some(r);
            c.request_repaint();
        });
        let opts = ExportOptions {
            raw: false,
            individual: availability.cities.is_ok() || availability.premade.is_ok(),
            topdown: availability.cities.is_ok(),
            tiles: false,
            premade: availability.premade.is_ok(),
            styles: Default::default(),
        };
        self.map_export = Some(MapExportDialog { entry, out_dir, opts, availability, catalog: slot });
    }

    pub(crate) fn map_export_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(d) = &mut self.map_export else { return };
        let mut start = false;
        let mut close = false;
        let cat = d.catalog.lock().unwrap().clone();
        let m = egui::Modal::new("map-export".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.label(RichText::new(format!("{} Export Map files", icon::MAP_TRIFOLD)).size(19.0).family(semibold()));
            ui.label(RichText::new("Everything is written with manifest.json and manifest.csv.").color(p.muted));
            ui.add_space(10.0);
            let cat = match &cat {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Looking for the map files…");
                    });
                    None
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(p.danger));
                    None
                }
                Some(Ok(c)) => Some(c),
            };
            let a = &d.availability;
            let option = |ui: &mut Ui, value: &mut bool, ok: &Result<(), String>, text: &str, detail: &str| {
                ui.add_enabled_ui(ok.is_ok(), |ui| {
                    ui.checkbox(value, RichText::new(text).family(semibold()));
                    ui.label(RichText::new(detail).size(12.0).color(p.muted));
                })
                .response
                .on_disabled_hover_text(reason(ok).unwrap_or(""));
                if ok.is_err() {
                    *value = false;
                }
            };
            option(ui, &mut d.opts.raw, &a.raw, "Raw map files", "Every map file byte for byte, IMG entries extracted, by their path in the copy.");
            let individual_ok = if a.cities.is_ok() || a.premade.is_ok() { Ok(()) } else { a.cities.clone() };
            option(
                ui,
                &mut d.opts.individual,
                &individual_ok,
                "Individual files",
                "2D cities: every block, column, zone, object, route and light as map.json, and an overlay.svg. 3D games: each radar tile as PNG.",
            );
            option(ui, &mut d.opts.topdown, &a.cities, "Full map: top-down", "The whole city at the game's native 64 pixels per block, one PNG per style (24-bit and 8-bit when both exist).");
            if d.opts.topdown {
                ui.indent("tiles", |ui| {
                    ui.checkbox(&mut d.opts.tiles, "Also cut it into 16 x 16-block PNG tiles");
                });
            }
            option(ui, &mut d.opts.premade, &a.premade, "Full map: radar and pause-menu maps", "The map the game draws its radar from, assembled from every radar tile, and the pause-menu map (Vice City: its own 3 x 3 textures; San Andreas: the radar tiles over the menu colour; GTA III has none).");
            let mut no_scene = false;
            option(ui, &mut no_scene, &a.scene, "Full map: 3D scene and top-down render", "Every placed model of the world as a glTF scene, and a top-down render of it.");
            if let Some(c) = cat {
                if !c.levels.is_empty() && (d.opts.topdown || d.opts.individual) {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Maps").size(12.0).color(p.muted));
                    for l in &c.levels {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&l.name).family(semibold()));
                            ui.label(RichText::new(&l.map).size(11.5).color(p.muted));
                            if l.styles.is_empty() {
                                let chosen = d.opts.styles.get(&l.map).and_then(|v| v.first()).cloned();
                                egui::ComboBox::from_id_salt(("style", l.map.as_str()))
                                    .selected_text(chosen.clone().unwrap_or_else(|| "Choose the style".into()))
                                    .show_ui(ui, |ui| {
                                        for s in &c.style_files {
                                            if ui.selectable_label(chosen.as_deref() == Some(s), s).clicked() {
                                                d.opts.styles.insert(l.map.clone(), vec![s.clone()]);
                                            }
                                        }
                                    })
                                    .response
                                    .on_hover_text("No source pairs this map with a style; pick the one it uses.");
                            } else {
                                for s in &l.styles {
                                    pill(ui, s.path.rsplit('/').next().unwrap_or(&s.path), p.text, p.raised);
                                }
                            }
                        });
                    }
                }
                if !c.notices.is_empty() {
                    egui::CollapsingHeader::new(RichText::new(format!("{} notes", c.notices.len())).size(12.0)).id_salt("map-notes").show(ui, |ui| {
                        for n in &c.notices {
                            ui.label(RichText::new(n).size(11.5).color(p.muted));
                        }
                    });
                }
                ui.label(RichText::new(format!("{} map files found.", c.files.len())).size(12.0).color(p.muted));
            }
            ui.add_space(10.0);
            destination(ui, p, &mut d.out_dir);
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let any = d.opts.raw || d.opts.individual || d.opts.topdown || d.opts.premade;
                if ui
                    .add_enabled(cat.is_some() && any, accent_button(p, format!("{} Start export", icon::EXPORT)).min_size(vec2(130.0, 32.0)))
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
            let d = self.map_export.take().unwrap();
            let Some((install, _)) = self.entry(d.entry) else { return };
            let install = install.clone();
            let Some(Ok(cat)) = d.catalog.lock().unwrap().clone() else { return };
            let (opts, out) = (d.opts, d.out_dir.clone());
            let c2 = ctx.clone();
            self.start_job("Exporting map files".into(), d.out_dir, 1, ctx, move |progress, cancel| {
                let pr = |done: usize, total: usize, current: &str| {
                    *progress.lock().unwrap() = JobProgress { done, total, current: current.to_string() };
                    c2.request_repaint();
                };
                let r = ef_map::export::export(&install, &cat, &opts, &out, &pr, cancel).map_err(|e| e.to_string())?;
                Ok(JobReport {
                    written: r.written,
                    failed: r.failed,
                    cancelled: r.cancelled || cancel.load(Ordering::Relaxed),
                })
            });
        } else if close {
            self.map_export = None;
        }
    }
}
