//! The Index page: every Grand Theft Auto game with every build, version, patch, edition, port,
//! demo, prototype and leak in the build database (`ef_core::database`), next to the dump records of
//! the release index (`ef_core::index`) that Eyefinder uses to identify copies.

use std::collections::BTreeSet;
use std::sync::Arc;

use ef_core::database::{Database, Entry, Layer};
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

/// The platform group of entries that name no platform (or all of them).
const ANY_PLATFORM: &str = "";

/// Games that are not one release: shown after the games.
const GROUPS: [&str; 4] = ["collections", "cancelled", "media", "series"];

/// What the filters let through, computed once per frame.
struct Filter {
    needle: String,
    platform: Option<String>,
    kind: Option<String>,
}

impl Filter {
    fn entry(&self, e: &Entry, game_title: &str) -> bool {
        self.platform.as_deref().is_none_or(|pl| e.platforms.iter().any(|x| x == pl))
            && self.kind.as_deref().is_none_or(|k| e.kind == k)
            && (self.needle.is_empty() || e.matches(&self.needle) || game_title.to_lowercase().contains(&self.needle))
    }
    fn release(&self, r: &Release, g: &IndexGame) -> bool {
        self.kind.as_deref().is_none_or(|k| k == "dump")
            && self.platform.as_deref().is_none_or(|pl| r.platform == pl)
            && (self.needle.is_empty()
                || r.title.to_lowercase().contains(&self.needle)
                || r.serial.to_lowercase().contains(&self.needle)
                || r.region.to_lowercase().contains(&self.needle)
                || r.version.to_lowercase().contains(&self.needle)
                || g.title.to_lowercase().contains(&self.needle))
    }
}

/// The colour and words of a confidence marker.
fn confidence(p: &Palette, c: &str) -> (egui::Color32, &'static str) {
    match c {
        "confirmed" => (p.verified, "Confirmed by a primary source or several independent ones"),
        "single-source" => (p.spec, "From one community source, or the sources disagree"),
        "unverified" => (p.unsupported, "Rumoured or unverified"),
        "mixed" => (p.spec, "Partly confirmed: see the confidence text"),
        _ => (p.muted, "No confidence given"),
    }
}

impl EyefinderApp {
    pub(crate) fn index_page(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(db) = ef_core::database::loaded() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Loading the build database…").color(p.muted));
            });
            return;
        };
        let idx = index::index();
        let dumps: usize = idx.games.iter().map(|g| g.releases.len()).sum();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{} Index", icon::LIST_MAGNIFYING_GLASS)).size(20.0).family(semibold()));
            ui.label(
                RichText::new(format!(
                    "{} games · {} builds and versions · {dumps} dump records · {} sources",
                    db.games.iter().filter(|g| !GROUPS.contains(&g.id.as_str())).count(),
                    db.entries.len(),
                    db.sources.len() + idx.sources.len()
                ))
                .color(p.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.database_menu(ui, p, &db));
        });
        ui.label(
            RichText::new(
                "Every documented build, version, patch, edition, port, demo, prototype and leak, with its sources \
                 (the build database), and every disc, ROM and store record of the dump databases (redump, No-Intro, \
                 titledb) that Eyefinder uses to name the copy you add. Nothing is estimated: what no source gives \
                 says \"not documented\".",
            )
            .size(12.5)
            .color(p.muted),
        );
        self.database_status(ui, p, &db);
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(icon::MAGNIFYING_GLASS);
            ui.add(
                egui::TextEdit::singleline(&mut self.index_search)
                    .hint_text("Search titles, versions, serials, regions, notes")
                    .desired_width(300.0),
            );
            let current = self.index_platform.as_deref().map_or("All platforms", |id| db.platform_name(id));
            egui::ComboBox::from_id_salt("index-platform").selected_text(current).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.index_platform, None, "All platforms");
                for id in db.platforms.keys() {
                    if db.entries.iter().any(|e| e.platforms.contains(id))
                        || idx.releases().any(|(_, r)| &r.platform == id)
                    {
                        ui.selectable_value(&mut self.index_platform, Some(id.clone()), db.platform_name(id));
                    }
                }
            });
            let kind = self.index_kind.as_deref().map_or("Everything", |k| db.kind_label(k));
            egui::ComboBox::from_id_salt("index-kind").selected_text(kind).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.index_kind, None, "Everything");
                for (id, label) in &db.kinds {
                    if db.entries.iter().any(|e| &e.kind == id) {
                        ui.selectable_value(&mut self.index_kind, Some(id.clone()), label);
                    }
                }
            });
            ui.checkbox(&mut self.index_eyefinder_only, "Only games Eyefinder reads");
        });
        ui.add_space(8.0);
        let filter = Filter {
            needle: self.index_search.to_lowercase(),
            platform: self.index_platform.clone(),
            kind: self.index_kind.clone(),
        };
        let mut open_docs = None;
        let mut actions = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for gid in game_order(&db) {
                let Some(info) = db.game(&gid) else { continue };
                if self.index_eyefinder_only && info.eyefinder.is_none() {
                    continue;
                }
                let ig = idx.game(&gid);
                let entries: Vec<&Entry> = db.entries_of(&gid).filter(|e| filter.entry(e, &info.title)).collect();
                let releases: Vec<&Release> =
                    ig.map(|g| g.releases.iter().filter(|r| filter.release(r, g)).collect()).unwrap_or_default();
                let named = filter.needle.is_empty() || info.title.to_lowercase().contains(&filter.needle);
                let unfiltered = filter.platform.is_none() && filter.kind.is_none();
                if entries.is_empty() && releases.is_empty() && !(named && unfiltered) {
                    continue;
                }
                let card = Card { db: &db, gid: &gid, index_game: ig, entries: &entries, releases: &releases };
                if self.game_card(ui, p, &card, &filter, &mut actions) {
                    open_docs = Some(gid.clone());
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
            section_label(ui, p, "Dump record sources");
            for (id, s) in &idx.sources {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(id).size(11.5).family(semibold()));
                    ui.hyperlink_to(RichText::new(&s.name).size(11.5), &s.url);
                    ui.label(RichText::new(format!("pin {} · {}", s.pin, s.license)).size(11.0).color(p.muted));
                });
            }
        });
        let ctx = ui.ctx().clone();
        for a in actions {
            self.entry_action(a, &db, &ctx);
        }
        if let Some(id) = open_docs {
            self.docs_game = id;
            self.docs_scroll = true;
            self.page = Page::Documentation;
        }
    }

    /// One game: title, support per platform, history, and its entries and dump records by platform.
    /// Returns whether the Documentation button was clicked.
    fn game_card(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        c: &Card,
        filter: &Filter,
        actions: &mut Vec<EntryAction>,
    ) -> bool {
        let db = c.db;
        let info = db.game(c.gid).expect("listed game");
        let game = info.eyefinder.as_deref().and_then(ef_core::Game::from_cli_id);
        let mut docs = false;
        let mut platforms = card_platforms(c);
        if let Some(pl) = &filter.platform {
            platforms.retain(|x| x == pl);
        }
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&info.title).size(16.0).family(semibold()));
                    let first = c.index_game.and_then(|g| g.first_release.clone()).or_else(|| first_date(c));
                    ui.label(RichText::new(year(first.as_deref())).color(p.accent_text).family(semibold()));
                    // Support is per platform: a level earned on one platform says nothing about another.
                    let mut unread = Vec::new();
                    for pl in platforms.iter().filter(|pl| !pl.is_empty()) {
                        match (game, index::platform_of(pl)) {
                            (Some(game), Some(eplat)) => {
                                ui.label(RichText::new(eplat.short()).size(11.5).color(p.muted));
                                platform_badge(ui, p, game, eplat);
                            }
                            _ => unread.push(db.platform_name(pl)),
                        }
                    }
                    if GROUPS.contains(&c.gid) {
                    } else if game.is_none() {
                        pill(ui, "Not read by Eyefinder yet", p.muted, p.raised);
                    } else if !unread.is_empty() {
                        pill(ui, &format!("Not read by Eyefinder: {}", unread.join(", ")), p.muted, p.raised);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if c.index_game.is_some()
                            && ui
                                .button(format!("{} Documentation", icon::BOOK_OPEN_TEXT))
                                .on_hover_text("Everything Eyefinder knows about this game's files")
                                .clicked()
                        {
                            docs = true;
                        }
                        if ui
                            .button(format!("{} Add entry", icon::PLUS))
                            .on_hover_text(
                                "Add a build, version or release that is missing (saved in your own database file)",
                            )
                            .clicked()
                        {
                            actions
                                .push(EntryAction::Add { game: c.gid.to_string(), platform: filter.platform.clone() });
                        }
                    });
                });
                if let Some(g) = c.index_game.filter(|g| !g.developer.is_empty() || !g.publisher.is_empty()) {
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
                let sections: Vec<_> = db.sections_of(c.gid).filter(|s| !s.text.trim().is_empty()).collect();
                if !sections.is_empty() && filter.needle.is_empty() {
                    collapsible(
                        ui,
                        p,
                        ("index-notes", c.gid),
                        icon::NOTEBOOK,
                        "History, notes and known gaps",
                        &sections.len().to_string(),
                        false,
                        |ui| {
                            for s in sections {
                                ui.label(RichText::new(&s.title).size(12.5).family(semibold()));
                                ui.label(RichText::new(plain_markdown(&s.text)).size(12.0));
                                ui.add_space(6.0);
                            }
                        },
                    );
                }
                for pl in platforms {
                    let entries: Vec<&Entry> = c
                        .entries
                        .iter()
                        .copied()
                        .filter(|e| if pl.is_empty() { e.platforms.is_empty() } else { e.platforms.contains(&pl) })
                        .collect();
                    let rels: Vec<&Release> = c.releases.iter().copied().filter(|r| r.platform == pl).collect();
                    let title = if pl.is_empty() {
                        "All or unnamed platforms".to_string()
                    } else {
                        let dates: Vec<String> = c
                            .index_game
                            .map(|g| {
                                g.platform_dates
                                    .iter()
                                    .filter(|d| d.platform == pl)
                                    .map(|d| {
                                        if d.region.is_empty() {
                                            d.date.clone()
                                        } else {
                                            format!("{} {}", d.region, d.date)
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        if dates.is_empty() {
                            db.platform_name(&pl).to_string()
                        } else {
                            format!("{} — {}", db.platform_name(&pl), dates.join(", "))
                        }
                    };
                    let open = !filter.needle.is_empty();
                    collapsible(
                        ui,
                        p,
                        ("index", c.gid, pl.as_str(), open),
                        if pl.is_empty() { icon::STACK } else { icon::DISC },
                        &title,
                        &(entries.len() + rels.len()).to_string(),
                        open,
                        |ui| {
                            if !pl.is_empty() && !GROUPS.contains(&c.gid) {
                                ui.horizontal_wrapped(|ui| match (game, index::platform_of(&pl)) {
                                    (Some(game), Some(eplat)) => {
                                        platform_badge(ui, p, game, eplat);
                                        ui.label(
                                            RichText::new(ef_core::support_summary(game, eplat).1)
                                                .size(12.0)
                                                .color(p.muted),
                                        );
                                    }
                                    _ => pill(ui, "Not read by Eyefinder on this platform", p.muted, p.raised),
                                });
                            }
                            self.entry_groups(ui, p, db, &entries, actions);
                            if !rels.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(format!(
                                        "Preserved dumps (redump, No-Intro, titledb) · {}",
                                        rels.len()
                                    ))
                                    .size(12.0)
                                    .family(semibold())
                                    .color(p.muted),
                                );
                                releases_grid(ui, p, c.gid, &pl, &rels);
                            }
                        },
                    );
                }
                if let Some(g) = c.index_game.filter(|g| !g.builds.is_empty())
                    && filter.platform.as_deref().is_none_or(|pl| pl == "pc")
                {
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

    /// The entries of one platform, grouped by kind in the database's kind order.
    fn entry_groups(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        db: &Database,
        entries: &[&Entry],
        actions: &mut Vec<EntryAction>,
    ) {
        for (kind, label) in &db.kinds {
            let mut group: Vec<&Entry> = entries.iter().copied().filter(|e| &e.kind == kind).collect();
            if group.is_empty() {
                continue;
            }
            group.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{label} · {}", group.len())).size(12.0).family(semibold()).color(p.muted));
            for e in group {
                self.entry_row(ui, p, db, e, actions);
            }
        }
    }

    /// One entry: a compact line that opens into every field and source.
    fn entry_row(&mut self, ui: &mut Ui, p: &Palette, db: &Database, e: &Entry, actions: &mut Vec<EntryAction>) {
        let open = self.index_open.contains(&e.id);
        let (dot, why) = confidence(p, &e.confidence);
        let resp = ui
            .horizontal_wrapped(|ui| {
                let caret = if open { icon::CARET_DOWN } else { icon::CARET_RIGHT };
                let r = ui.add(egui::Label::new(RichText::new(caret).color(p.muted)).sense(egui::Sense::click()));
                ui.label(RichText::new("●").color(dot).size(11.0)).on_hover_text(why);
                let title = ui.add(
                    egui::Label::new(RichText::new(if e.title.is_empty() { &e.id } else { &e.title }).size(12.5))
                        .sense(egui::Sense::click()),
                );
                let mut meta = Vec::new();
                if !e.version.is_empty() {
                    meta.push(format!("v{}", e.version.trim_start_matches(['v', 'V'])));
                }
                if !e.region.is_empty() && e.region != "—" {
                    meta.push(short(&e.region, 40));
                }
                if !e.when().is_empty() {
                    meta.push(short(e.when(), 40));
                }
                if !e.serials.is_empty() {
                    meta.push(e.serials.join(", "));
                }
                if !meta.is_empty() {
                    ui.label(RichText::new(meta.join(" · ")).size(11.5).color(p.muted));
                }
                if matches!(e.layer, Layer::User(_)) {
                    pill(ui, "edited locally", p.accent_text, p.accent_soft);
                }
                r.clicked() || title.clicked()
            })
            .inner;
        if resp {
            if open {
                self.index_open.remove(&e.id);
            } else {
                self.index_open.insert(e.id.clone());
            }
        }
        if !open {
            return;
        }
        egui::Frame::new()
            .fill(p.raised)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new(("entry", e.id.as_str())).num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    let mut row = |k: &str, v: &str| {
                        if !v.is_empty() {
                            ui.label(RichText::new(k).size(11.5).family(semibold()).color(p.muted));
                            ui.label(RichText::new(plain_markdown(v)).size(12.0));
                            ui.end_row();
                        }
                    };
                    row("Platform", &e.platform_text);
                    row("Region", &e.region);
                    row("Date", &e.date_text);
                    row("Version", &e.version);
                    row("Serial / ID", &e.id_text);
                    row("Game", &e.game_text);
                    row("Notes", &e.notes);
                    for (k, v) in &e.fields {
                        row(k, v);
                    }
                    row("Confidence", &e.confidence_text);
                    row("Sources", &e.sources_text);
                    let section = db.section(&e.section).map(|s| s.title.clone()).unwrap_or_default();
                    row("Section", &section);
                });
                for key in &e.sources {
                    if let Some(s) = db.sources.get(key) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(format!("[{key}]")).size(11.0).family(semibold()).color(p.muted));
                            if s.url.is_empty() {
                                ui.label(RichText::new(&s.text).size(11.5));
                            } else {
                                ui.hyperlink_to(RichText::new(short(&s.text, 160)).size(11.5), &s.url)
                                    .on_hover_text(&s.url);
                            }
                        });
                    }
                }
                let dumps = db.dumps_of(e);
                if !dumps.is_empty() {
                    let names: Vec<&str> = dumps.iter().map(|(_, r)| r.title.as_str()).collect();
                    ui.label(
                        RichText::new(format!("{} Dump records with this serial: {}", icon::DISC, names.join("; ")))
                            .size(11.5)
                            .color(p.muted),
                    );
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("id {}", e.id)).size(10.5).color(p.muted));
                    if !e.origin.is_empty() {
                        ui.label(
                            RichText::new(format!("· research file line {}", e.origin.trim_start_matches("md:")))
                                .size(10.5)
                                .color(p.muted),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if matches!(e.layer, Layer::User(_)) && ui.small_button("Undo my change").clicked() {
                            actions.push(EntryAction::Revert(e.id.clone()));
                        }
                        if ui
                            .small_button(format!("{} Hide", icon::EYE_SLASH))
                            .on_hover_text("Hide this entry (only in your copy)")
                            .clicked()
                        {
                            actions.push(EntryAction::Hide(e.id.clone()));
                        }
                        if ui.small_button(format!("{} Edit", icon::PENCIL_SIMPLE)).clicked() {
                            actions.push(EntryAction::Edit(e.id.clone()));
                        }
                    });
                });
            });
    }
}

/// A game card's data.
struct Card<'a> {
    db: &'a Arc<Database>,
    gid: &'a str,
    index_game: Option<&'static IndexGame>,
    entries: &'a [&'a Entry],
    releases: &'a [&'a Release],
}

/// What a click on the Index page asks for, done after drawing.
pub(crate) enum EntryAction {
    Add { game: String, platform: Option<String> },
    Edit(String),
    Hide(String),
    Revert(String),
}

/// Games in release order, then the groups.
fn game_order(db: &Database) -> Vec<String> {
    let idx = index::index();
    let mut v: Vec<(bool, String, usize, String)> = db
        .games
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let first = idx
                .game(&g.id)
                .and_then(|x| x.first_release.clone())
                .or_else(|| db.entries_of(&g.id).filter(|e| e.date.len() >= 4).map(|e| e.date.clone()).min())
                .unwrap_or_else(|| "9999".into());
            (GROUPS.contains(&g.id.as_str()), first, i, g.id.clone())
        })
        .collect();
    v.sort();
    v.into_iter().map(|t| t.3).collect()
}

/// Platforms of a card: dated ones first (index order), then the rest, then entries naming none.
fn card_platforms(c: &Card) -> Vec<String> {
    let mut out: Vec<String> = c.index_game.map(|g| g.platforms()).unwrap_or_default();
    let mut rest: BTreeSet<&str> = BTreeSet::new();
    for e in c.entries {
        rest.extend(e.platforms.iter().map(String::as_str));
    }
    for r in c.releases {
        rest.insert(&r.platform);
    }
    for pl in c.db.platforms.keys() {
        if rest.contains(pl.as_str()) && !out.contains(pl) {
            out.push(pl.clone());
        }
    }
    if c.entries.iter().any(|e| e.platforms.is_empty()) {
        out.push(ANY_PLATFORM.into());
    }
    out
}

fn first_date(c: &Card) -> Option<String> {
    c.entries.iter().filter(|e| e.date.len() >= 4).map(|e| e.date.clone()).min()
}

/// Dated entries in date order, the rest after them in file order.
fn sort_key(e: &Entry) -> (bool, &str) {
    (e.date.is_empty(), e.date.as_str())
}

fn releases_grid(ui: &mut Ui, p: &Palette, gid: &str, pl: &str, rels: &[&Release]) {
    let idx = index::index();
    egui::Grid::new(("rel", gid, pl)).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
        for h in ["Release", "Serial / ID", "Media", "Identified by", "Source"] {
            ui.label(RichText::new(h).size(11.0).family(semibold()).color(p.muted));
        }
        ui.end_row();
        for r in rels {
            ui.label(RichText::new(r.label()).size(12.0)).on_hover_text(&r.title);
            ui.label(RichText::new(if r.serial.is_empty() { "—" } else { &r.serial }).size(11.5).monospace());
            let size: u64 = r.files.iter().map(|f| f.size).sum();
            let media =
                if size > 0 { format!("{} · {}", r.media, crate::app::human_bytes(size)) } else { r.media.clone() };
            ui.label(RichText::new(media).size(11.5).color(p.muted));
            ui.label(RichText::new(identified_by(r)).size(11.5));
            let src = idx.sources.get(&r.source);
            ui.label(RichText::new(&r.source).size(11.0).color(p.muted))
                .on_hover_text(src.map_or(String::new(), |s| format!("{}\n{}", s.name, s.url)));
            ui.end_row();
            if !r.updates.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "  updates: {}",
                        r.updates.iter().map(|u| format!("v{} {}", u.version, u.date)).collect::<Vec<_>>().join(", ")
                    ))
                    .size(11.0)
                    .color(p.muted),
                );
                ui.end_row();
            }
        }
    });
}

/// Eyefinder's support level for `game` on this one platform.
fn platform_badge(ui: &mut Ui, p: &Palette, game: ef_core::Game, platform: ef_core::Platform) {
    let (level, text) = ef_core::support_summary(game, platform);
    support_badge(ui, p, level).on_hover_text(format!("{} on {}: {text}", game.short(), platform.name()));
}

/// Markdown emphasis and code marks removed, for plain labels.
pub(crate) fn plain_markdown(s: &str) -> String {
    s.replace("**", "").replace('`', "").replace("\\|", "|")
}

fn short(s: &str, n: usize) -> String {
    let s = plain_markdown(s);
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "—" } else { s }
}
