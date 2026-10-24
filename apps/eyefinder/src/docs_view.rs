//! The Documentation page: a tracker of what this build of Eyefinder knows about each game's
//! files (`ef_core::docs`): verified, implemented but unverified, documented but not implemented,
//! not supported, and undocumented. Each game has a full card: dates, releases, how copies are
//! identified, executables, every file type with its status and sources, and what is missing.

use ef_core::docs::{self, Entry, Registry, Status};
use ef_core::index::{self, IndexGame};
use egui::{Color32, CornerRadius, Margin, RichText, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

use crate::app::{EyefinderApp, Page, pill};
use crate::theme::{self, Palette, section_label, semibold};

pub(crate) fn status_colour(p: &Palette, s: Status) -> Color32 {
    match s {
        Status::Verified => p.verified,
        Status::Spec => p.spec,
        Status::Documented => p.accent_text,
        Status::Unsupported => p.unsupported,
        Status::Missing => p.danger,
    }
}

fn status_icon(s: Status) -> &'static str {
    match s {
        Status::Verified => icon::SEAL_CHECK,
        Status::Spec => icon::FLASK,
        Status::Documented => icon::BOOK_OPEN,
        Status::Unsupported => icon::PROHIBIT,
        Status::Missing => icon::QUESTION,
    }
}

pub(crate) fn status_badge(ui: &mut Ui, p: &Palette, s: Status) -> egui::Response {
    let c = status_colour(p, s);
    egui::Frame::new()
        .fill(c.gamma_multiply(0.13))
        .stroke(Stroke::new(1.0, c.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(format!("{} {}", status_icon(s), s.label())).size(11.0).color(c)))
        .response
        .on_hover_text(s.explanation())
}

/// A bar split by status counts.
fn progress(ui: &mut Ui, p: &Palette, counts: [usize; 5], height: f32) {
    let total: usize = counts.iter().sum();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), p.raised);
    if total == 0 {
        return;
    }
    let mut x = rect.left();
    for (i, s) in Status::ALL.iter().enumerate() {
        let w = rect.width() * counts[i] as f32 / total as f32;
        if w > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(w, rect.height())),
                CornerRadius::ZERO,
                status_colour(p, *s),
            );
        }
        x += w;
    }
    resp.on_hover_text(
        Status::ALL.iter().zip(counts).map(|(s, n)| format!("{}: {n}", s.label())).collect::<Vec<_>>().join("\n"),
    );
}

const KINDS: [(&str, &str); 10] = [
    ("identification", "Identification"),
    ("container", "Disc images and containers"),
    ("exe", "Executables"),
    ("audio", "Audio"),
    ("texture", "Graphics"),
    ("text", "Text"),
    ("map", "Maps"),
    ("video", "Video"),
    ("script", "Scripts"),
    ("other", "Other"),
];

impl EyefinderApp {
    pub(crate) fn docs_page(&mut self, ui: &mut Ui, p: &Palette) {
        let reg = docs::registry();
        let idx = index::index();
        let revision = env!("EYEFINDER_REVISION");
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{} Documentation", icon::BOOK_OPEN_TEXT)).size(20.0).family(semibold()));
            ui.label(
                RichText::new(format!(
                    "Eyefinder {} · build {}",
                    env!("CARGO_PKG_VERSION"),
                    &revision[..7.min(revision.len())]
                ))
                .color(p.muted),
            );
        });
        let all = Registry::counts(reg.entries.iter());
        ui.horizontal_wrapped(|ui| {
            for (i, s) in Status::ALL.iter().enumerate() {
                status_badge(ui, p, *s);
                ui.label(RichText::new(all[i].to_string()).family(semibold()));
                ui.add_space(6.0);
            }
        });
        progress(ui, p, all, 10.0);
        ui.label(
            RichText::new(format!(
                "{} file types and identification methods tracked. Statuses are earned: \"Verified\" needs a real-file report. Undocumented items are listed in MISSING.md.",
                reg.entries.len()
            ))
            .size(12.0)
            .color(p.muted),
        );
        ui.add_space(8.0);
        let mut games: Vec<&IndexGame> = idx.games.iter().filter(|g| g.id != "collections").collect();
        games.sort_by_key(|g| g.first_release.clone().unwrap_or_else(|| "9999".into()));
        let avail = ui.available_height();
        ui.horizontal_top(|ui| {
            // game list
            ui.allocate_ui_with_layout(vec2(230.0, avail), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                egui::ScrollArea::vertical().id_salt("docs-games").auto_shrink([false, false]).show(ui, |ui| {
                    section_label(ui, p, "Games");
                    for g in &games {
                        let counts = Registry::counts(reg.for_game(&g.id));
                        let selected = self.docs_game == g.id;
                        let r = egui::Frame::new()
                            .fill(if selected { p.accent_soft } else { p.raised })
                            .stroke(Stroke::new(1.0, if selected { p.accent } else { p.border }))
                            .corner_radius(CornerRadius::same(5))
                            .inner_margin(Margin::symmetric(8, 5))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let title = format!(
                                    "{} · {}",
                                    g.first_release.as_deref().and_then(|d| d.get(..4)).unwrap_or("—"),
                                    g.title
                                );
                                let galley = theme::elided(
                                    ui.painter(),
                                    title,
                                    egui::FontId::new(12.5, semibold()),
                                    p.text,
                                    ui.available_width(),
                                );
                                ui.add(egui::Label::new(galley).selectable(false)).on_hover_text(&g.title);
                                progress(ui, p, counts, 5.0);
                            });
                        if selected && self.docs_scroll {
                            r.response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if r.response.interact(Sense::click()).clicked() {
                            self.docs_game = g.id.clone();
                        }
                        ui.add_space(3.0);
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("docs-card", self.docs_game.as_str()))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(g) = idx.game(&self.docs_game)
                            && self.game_doc(ui, p, g)
                        {
                            self.page = Page::Index;
                            self.index_search = g.title.clone();
                        }
                    });
            });
        });
        self.docs_scroll = false;
    }

    /// The full card of one game. Returns whether "Show in Index" was clicked.
    fn game_doc(&self, ui: &mut Ui, p: &Palette, g: &IndexGame) -> bool {
        let reg = docs::registry();
        let idx = index::index();
        let mut to_index = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&g.title).size(22.0).family(semibold()));
            if ui.button(format!("{} Show in Index", icon::LIST_MAGNIFYING_GLASS)).clicked() {
                to_index = true;
            }
        });
        let mut facts = Vec::new();
        if let Some(d) = &g.first_release {
            facts.push(format!("First released {d}"));
        }
        if !g.developer.is_empty() {
            facts.push(format!("Developer: {}", g.developer));
        }
        if !g.publisher.is_empty() {
            facts.push(format!("Publisher: {}", g.publisher));
        }
        ui.label(RichText::new(facts.join(" · ")).color(p.muted));
        if let Some(w) = g.wikipedia.as_ref().and_then(|w| idx.sources.get(w)) {
            ui.hyperlink_to(RichText::new(format!("{} {}", icon::LINK, w.name)).size(11.5), &w.url);
        }
        let entries: Vec<&Entry> = reg.for_game(&g.id).collect();
        let counts = Registry::counts(entries.iter().copied());
        ui.add_space(4.0);
        progress(ui, p, counts, 8.0);
        ui.add_space(8.0);

        // Platforms and releases
        card(ui, p, icon::GLOBE_HEMISPHERE_WEST, "Platforms and releases", |ui| {
            egui::Grid::new(("plat", g.id.as_str())).striped(true).spacing(vec2(14.0, 4.0)).show(ui, |ui| {
                for h in ["Platform", "Released", "Releases in the index", "Eyefinder"] {
                    ui.label(RichText::new(h).size(11.0).family(semibold()).color(p.muted));
                }
                ui.end_row();
                for pl in g.platforms() {
                    ui.label(idx.platform_name(&pl));
                    let dates: Vec<String> = g
                        .platform_dates
                        .iter()
                        .filter(|d| d.platform == pl)
                        .map(|d| if d.region.is_empty() { d.date.clone() } else { format!("{} {}", d.region, d.date) })
                        .collect();
                    ui.label(RichText::new(if dates.is_empty() { "—".into() } else { dates.join(", ") }).size(12.0));
                    ui.label(g.releases.iter().filter(|r| r.platform == pl).count().to_string());
                    match (g.game(), index::platform_of(&pl)) {
                        (Some(game), Some(platform)) => {
                            let (level, text) = ef_core::support_summary(game, platform);
                            crate::app::support_badge(ui, p, level).on_hover_text(text);
                        }
                        _ => {
                            ui.label(RichText::new("Not read").size(11.5).color(p.muted));
                        }
                    }
                    ui.end_row();
                }
            });
            let collections: Vec<String> = idx
                .game("collections")
                .map(|c| c.releases.iter().filter(|r| r.games.contains(&g.id)).map(|r| r.title.clone()).collect())
                .unwrap_or_default();
            if !collections.is_empty() {
                ui.label(RichText::new(format!("Also in: {}", collections.join("; "))).size(11.5).color(p.muted));
            }
        });

        let game = g.game();
        if game.is_none() {
            card(ui, p, icon::INFO, "Eyefinder and this game", |ui| {
                ui.label(
                    RichText::new(
                        "Eyefinder does not read this game yet. Its releases are listed for identification and tracking; no research on its file formats has been recorded in this build.",
                    )
                    .color(p.muted),
                );
            });
        }
        if let Some(game) = game {
            // Identification: the files detection looks for
            card(ui, p, icon::FINGERPRINT, "How a copy is recognised", |ui| {
                for (platform, files, what) in ef_core::detection_markers(game) {
                    ui.horizontal_wrapped(|ui| {
                        pill(ui, platform.short(), p.on_accent, p.accent);
                        ui.label(RichText::new(what).size(12.0).family(semibold()));
                        ui.label(RichText::new(files.join(", ")).size(11.5).monospace().color(p.muted));
                    });
                }
                ui.label(
                    RichText::new("Two thirds of a set must be present. Console discs and ROMs are recognised by serial, IP.BIN or SHA-1 (see the Index).")
                        .size(11.5)
                        .color(p.muted),
                );
            });
            // Executables
            let exes = ef_core::edition::exe_names(game);
            let builds: Vec<ef_core::Edition> =
                ef_core::edition::known(game, ef_core::Platform::Pc).into_iter().filter(|e| !e.is_unknown()).collect();
            if !exes.is_empty() || !builds.is_empty() {
                card(ui, p, icon::CPU, "Executables and versions", |ui| {
                    if !exes.is_empty() {
                        ui.label(
                            RichText::new(format!("Executables examined: {}", exes.join(", "))).size(12.0).monospace(),
                        );
                    }
                    for b in builds {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(icon::DOT_OUTLINE).color(p.muted));
                            ui.label(RichText::new(&b.label).size(12.0));
                            ui.label(RichText::new(&b.id).size(11.0).monospace().color(p.muted));
                        });
                    }
                });
            }
        }

        // File types by kind
        for (kind, title) in KINDS {
            let rows: Vec<&&Entry> = entries.iter().filter(|e| e.kind == kind).collect();
            if rows.is_empty() {
                continue;
            }
            card(ui, p, kind_icon(kind), title, |ui| {
                for e in rows {
                    egui::Frame::new()
                        .fill(p.raised)
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                status_badge(ui, p, e.status);
                                ui.label(RichText::new(&e.data).size(12.5).family(semibold()));
                            });
                            ui.horizontal_wrapped(|ui| {
                                let plats: Vec<&str> = e.platforms.iter().map(|x| idx.platform_name(x)).collect();
                                ui.label(RichText::new(plats.join(", ")).size(11.0).color(p.accent_text));
                                if !e.files.is_empty() {
                                    ui.label(RichText::new(&e.files).size(11.0).monospace().color(p.muted));
                                }
                            });
                            if let Some(k) = &e.known {
                                ui.label(RichText::new(format!("Known: {}", plain(k))).size(11.5));
                            }
                            if let Some(n) = &e.needed {
                                ui.label(RichText::new(format!("Missing: {}", plain(n))).size(11.5).color(p.danger));
                            }
                            if let Some(c) = &e.confirmation {
                                ui.label(
                                    RichText::new(format!("{} Real files: {c}", icon::CHECK_CIRCLE))
                                        .size(11.5)
                                        .color(p.verified),
                                );
                            }
                            if let Some(n) = &e.notes {
                                ui.label(RichText::new(n).size(11.5).color(p.muted));
                            }
                            if !e.sources.is_empty() {
                                ui.label(
                                    RichText::new(format!("Sources: {}", e.sources.join("; ")))
                                        .size(11.0)
                                        .color(p.muted),
                                );
                            }
                        });
                    ui.add_space(3.0);
                }
            });
        }
        if entries.iter().any(|e| e.status == Status::Missing) {
            card(ui, p, icon::HAND_HEART, "How to help", |ui| {
                ui.label(
                    RichText::new(
                        "A report from a real copy settles most undocumented items without sharing game files: the file list (eyefinder-cli files), the first bytes and size of a file (Files view), or an export's manifest.json. See CONTRIBUTING.md.",
                    )
                    .size(12.0),
                );
            });
        }
        to_index
    }
}

fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "identification" => icon::FINGERPRINT,
        "container" => icon::DISC,
        "exe" => icon::CPU,
        "audio" => icon::SPEAKER_HIGH,
        "texture" => icon::IMAGE,
        "text" => icon::TEXT_AA,
        "map" => icon::MAP_TRIFOLD,
        "video" => icon::FILM_STRIP,
        "script" => icon::CODE,
        _ => icon::FILE,
    }
}

/// Markdown emphasis and code marks removed for display.
fn plain(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

fn card(ui: &mut Ui, p: &Palette, ic: &str, title: &str, body: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("{ic} {title}")).size(14.0).family(semibold()).color(p.accent_text));
            ui.add_space(4.0);
            body(ui);
        });
    ui.add_space(8.0);
}
