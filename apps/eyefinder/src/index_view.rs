//! The Index page: every Grand Theft Auto game, platform, release, edition and version in the
//! release index (`ef_core::index`), with how Eyefinder identifies each one.

use ef_core::index::{self, IndexGame, Release};
use egui::{CornerRadius, Margin, RichText, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

use crate::app::{EyefinderApp, Page, collapsible, pill, support_badge};
use crate::theme::{Palette, section_label, semibold};

/// How a release is identified, in words.
pub(crate) fn identified_by(r: &Release) -> String {
    if r.ip_bin.is_some() {
        "IP.BIN product, version, date".into()
    } else if r.media == "rom" {
        "ROM SHA-1".into()
    } else if r.platform == "pc" && !r.files.is_empty() {
        "Track sizes and data-track SHA-1".into()
    } else if !r.serial.is_empty() && matches!(r.platform.as_str(), "ps1" | "ps2") {
        "Disc serial (SYSTEM.CNF)".into()
    } else if !r.serial.is_empty() {
        "Listed (serial / title ID)".into()
    } else {
        "Listed".into()
    }
}

fn year(d: Option<&str>) -> &str {
    d.and_then(|d| d.get(..4)).unwrap_or("—")
}

impl EyefinderApp {
    pub(crate) fn index_page(&mut self, ui: &mut Ui, p: &Palette) {
        let idx = index::index();
        let releases: usize = idx.games.iter().map(|g| g.releases.len()).sum();
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("{} Release index", icon::LIST_MAGNIFYING_GLASS)).size(20.0).family(semibold()),
            );
            ui.label(
                RichText::new(format!(
                    "{} games · {releases} releases · {} sources",
                    idx.games.len() - 1,
                    idx.sources.len()
                ))
                .color(p.muted),
            );
        });
        ui.label(
            RichText::new(
                "Every release the cited sources list (redump and No-Intro DATs, redump's PC DAT, Wikipedia, titledb). \
                 Eyefinder uses it to name the copy you add. Nothing here is filled in by hand: what no source lists is \
                 named under Gaps.",
            )
            .size(12.5)
            .color(p.muted),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(icon::MAGNIFYING_GLASS);
            ui.add(
                egui::TextEdit::singleline(&mut self.index_search)
                    .hint_text("Search titles, regions, serials, versions")
                    .desired_width(300.0),
            );
            let current = self.index_platform.as_deref().map_or("All platforms", |id| idx.platform_name(id));
            egui::ComboBox::from_id_salt("index-platform").selected_text(current).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.index_platform, None, "All platforms");
                for id in idx.platforms.keys() {
                    if idx.releases().any(|(_, r)| &r.platform == id)
                        || idx.games.iter().any(|g| g.first_on(id).is_some())
                    {
                        ui.selectable_value(&mut self.index_platform, Some(id.clone()), idx.platform_name(id));
                    }
                }
            });
            ui.checkbox(&mut self.index_eyefinder_only, "Only games Eyefinder reads");
        });
        ui.add_space(8.0);
        let needle = self.index_search.to_lowercase();
        let platform = self.index_platform.clone();
        let mut open_docs = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut games: Vec<&IndexGame> = idx.games.iter().collect();
            games.sort_by_key(|g| (g.id == "collections", g.first_release.clone().unwrap_or_else(|| "9999".into())));
            for g in games {
                if self.index_eyefinder_only && g.eyefinder.is_none() {
                    continue;
                }
                let matches = |r: &Release| {
                    platform.as_deref().is_none_or(|pl| r.platform == pl)
                        && (needle.is_empty()
                            || r.title.to_lowercase().contains(&needle)
                            || r.serial.to_lowercase().contains(&needle)
                            || r.region.to_lowercase().contains(&needle)
                            || r.version.to_lowercase().contains(&needle)
                            || g.title.to_lowercase().contains(&needle))
                };
                let shown: Vec<&Release> = g.releases.iter().filter(|r| matches(r)).collect();
                let dated = platform.as_deref().is_none_or(|pl| g.first_on(pl).is_some());
                if shown.is_empty() && !(dated && (needle.is_empty() || g.title.to_lowercase().contains(&needle))) {
                    continue;
                }
                if self.game_card(ui, p, g, &shown, platform.as_deref()) {
                    open_docs = Some(g.id.clone());
                }
                ui.add_space(8.0);
            }
            ui.add_space(6.0);
            section_label(ui, p, "Gaps: releases no reachable source lists");
            for gap in &idx.gaps {
                ui.horizontal_wrapped(|ui| {
                    pill(ui, &gap.platforms, p.text, p.border);
                    ui.label(RichText::new(&gap.text).size(12.0).color(p.muted));
                });
            }
            ui.add_space(6.0);
            section_label(ui, p, "Sources");
            for (id, s) in &idx.sources {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(id).size(11.5).family(semibold()));
                    ui.hyperlink_to(RichText::new(&s.name).size(11.5), &s.url);
                    ui.label(RichText::new(format!("pin {} · {}", s.pin, s.license)).size(11.0).color(p.muted));
                });
            }
        });
        if let Some(id) = open_docs {
            self.docs_game = id;
            self.docs_scroll = true;
            self.page = Page::Documentation;
        }
    }

    /// One game: title, dates, companies, a Documentation button, and its releases by platform.
    /// Returns whether the Documentation button was clicked.
    fn game_card(&self, ui: &mut Ui, p: &Palette, g: &IndexGame, shown: &[&Release], platform: Option<&str>) -> bool {
        let idx = index::index();
        let mut docs = false;
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&g.title).size(16.0).family(semibold()));
                    ui.label(RichText::new(year(g.first_release.as_deref())).color(p.accent_text).family(semibold()));
                    match g.game() {
                        Some(game) => {
                            let best = ef_core::Platform::ALL
                                .iter()
                                .map(|&pl| ef_core::support_summary(game, pl).0)
                                .max()
                                .unwrap_or(ef_core::SupportLevel::DetectedOnly);
                            support_badge(ui, p, best).on_hover_text("Eyefinder's best support on any platform");
                        }
                        None => pill(ui, "Not read by Eyefinder yet", p.muted, p.raised),
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(format!("{} Documentation", icon::BOOK_OPEN_TEXT))
                            .on_hover_text("Everything Eyefinder knows about this game's files")
                            .clicked()
                        {
                            docs = true;
                        }
                    });
                });
                if !g.developer.is_empty() || !g.publisher.is_empty() {
                    ui.label(
                        RichText::new(format!(
                            "Developer: {} · Publisher: {}",
                            or_dash(&g.developer),
                            or_dash(&g.publisher)
                        ))
                        .size(12.0)
                        .color(p.muted),
                    );
                }
                let mut platforms = g.platforms();
                if let Some(pl) = platform {
                    platforms.retain(|x| x == pl);
                }
                for pl in platforms {
                    let rels: Vec<&&Release> = shown.iter().filter(|r| r.platform == pl).collect();
                    let dates: Vec<String> = g
                        .platform_dates
                        .iter()
                        .filter(|d| d.platform == pl)
                        .map(|d| if d.region.is_empty() { d.date.clone() } else { format!("{} {}", d.region, d.date) })
                        .collect();
                    let title = format!(
                        "{} — {}",
                        idx.platform_name(&pl),
                        if dates.is_empty() { "date not in the sources".into() } else { dates.join(", ") }
                    );
                    collapsible(
                        ui,
                        p,
                        ("index", g.id.as_str(), pl.as_str()),
                        icon::DISC,
                        &title,
                        &rels.len().to_string(),
                        false,
                        |ui| {
                            if rels.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "No disc, ROM or store listing of this platform is in the sources (see Gaps).",
                                    )
                                    .size(12.0)
                                    .color(p.muted),
                                );
                                return;
                            }
                            egui::Grid::new(("rel", g.id.as_str(), pl.as_str()))
                                .striped(true)
                                .spacing(vec2(12.0, 4.0))
                                .show(ui, |ui| {
                                    for h in ["Release", "Serial / ID", "Media", "Identified by", "Source"] {
                                        ui.label(RichText::new(h).size(11.0).family(semibold()).color(p.muted));
                                    }
                                    ui.end_row();
                                    for r in rels {
                                        ui.label(RichText::new(r.label()).size(12.0)).on_hover_text(&r.title);
                                        ui.label(
                                            RichText::new(if r.serial.is_empty() { "—" } else { &r.serial })
                                                .size(11.5)
                                                .monospace(),
                                        );
                                        let size: u64 = r.files.iter().map(|f| f.size).sum();
                                        let media = if size > 0 {
                                            format!("{} · {}", r.media, crate::app::human_bytes(size))
                                        } else {
                                            r.media.clone()
                                        };
                                        ui.label(RichText::new(media).size(11.5).color(p.muted));
                                        ui.label(RichText::new(identified_by(r)).size(11.5));
                                        let src = idx.sources.get(&r.source);
                                        ui.label(RichText::new(&r.source).size(11.0).color(p.muted)).on_hover_text(
                                            src.map_or(String::new(), |s| format!("{}\n{}", s.name, s.url)),
                                        );
                                        ui.end_row();
                                        if !r.updates.is_empty() {
                                            ui.label(
                                                RichText::new(format!(
                                                    "  updates: {}",
                                                    r.updates
                                                        .iter()
                                                        .map(|u| format!("v{} {}", u.version, u.date))
                                                        .collect::<Vec<_>>()
                                                        .join(", ")
                                                ))
                                                .size(11.0)
                                                .color(p.muted),
                                            );
                                            ui.end_row();
                                        }
                                    }
                                });
                        },
                    );
                }
                if !g.builds.is_empty() && platform.is_none_or(|pl| pl == "pc") {
                    let builds: Vec<String> = g.builds.iter().map(|b| b.label.clone()).collect();
                    ui.label(
                        RichText::new(format!("{} PC executable builds identified: {}", icon::CPU, builds.join(", ")))
                            .size(12.0)
                            .color(p.muted),
                    );
                }
            });
        docs
    }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}
