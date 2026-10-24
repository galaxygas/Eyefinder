//! The build database around the Index page: its status, the Database menu (import, update check,
//! export, folder), and the editor that adds, corrects and hides entries in the user's own file.
//! Loading, importing and downloading run on worker threads.

use std::sync::{Arc, Mutex};
use std::thread;

use ef_core::database::{self, Available, Base, Database, Entry, Imported, Source};
use egui::{RichText, Ui, vec2};
use egui_phosphor::regular as icon;
use indexmap::IndexMap;

use crate::app::{EyefinderApp, accent_button, modal_frame, open_in_file_manager, pill};
use crate::index_view::EntryAction;
use crate::theme::{Palette, semibold};

/// A database job running on a worker thread.
pub(crate) struct DbTask {
    pub title: String,
    pub state: Arc<Mutex<DbTaskState>>,
}

pub(crate) enum DbTaskState {
    Running,
    /// A newer pack is on GitHub: ask before installing it.
    Offer(Available),
    Done(String),
    Failed(String),
}

/// The entry being added or corrected.
pub(crate) struct EntryEditor {
    pub entry: Entry,
    pub new: bool,
    pub games: String,
    pub platforms: String,
    pub serials: String,
    pub sources: String,
    pub source_text: String,
    pub source_url: String,
    pub fields: Vec<(String, String)>,
    pub error: Option<String>,
}

fn list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from).collect()
}

impl EntryEditor {
    fn of(entry: Entry, new: bool) -> EntryEditor {
        EntryEditor {
            games: entry.games.join(", "),
            platforms: entry.platforms.join(", "),
            serials: entry.serials.join(", "),
            sources: entry.sources.join(", "),
            fields: entry.fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            source_text: String::new(),
            source_url: String::new(),
            entry,
            new,
            error: None,
        }
    }
    /// The entry as edited, and the new source if one was typed.
    fn result(&self, db: &Database) -> (Entry, IndexMap<String, Source>) {
        let mut e = self.entry.clone();
        e.games = list(&self.games);
        e.platforms = list(&self.platforms);
        e.serials = list(&self.serials);
        e.sources = list(&self.sources);
        e.fields = self.fields.iter().filter(|(k, _)| !k.trim().is_empty()).cloned().collect();
        let mut new = IndexMap::new();
        if !self.source_text.trim().is_empty() || !self.source_url.trim().is_empty() {
            let key = database::new_source_key(db);
            new.insert(
                key.clone(),
                Source {
                    text: self.source_text.trim().into(),
                    url: self.source_url.trim().into(),
                    origin: String::new(),
                },
            );
            e.sources.push(key);
        }
        (e, new)
    }
}

/// Loads the database on a worker thread and repaints when it is ready.
pub(crate) fn load_in_background(ctx: &egui::Context) {
    let ctx = ctx.clone();
    let _ = thread::Builder::new().name("database".into()).spawn(move || {
        database::reload();
        ctx.request_repaint();
    });
}

impl EyefinderApp {
    /// The version, layers and edits of the database; its problems and notices, if any.
    pub(crate) fn database_status(&mut self, ui: &mut Ui, p: &Palette, db: &Database) {
        ui.horizontal_wrapped(|ui| {
            let base = match db.base {
                Base::BuiltIn => "built in".to_string(),
                Base::Pack => format!("installed pack (built-in {})", db.built_in_version),
            };
            ui.label(RichText::new(format!("Database {} · {base}", db.version)).size(11.5).color(p.muted));
            if !db.user_files.is_empty() {
                let edits = db.entries.iter().filter(|e| matches!(e.layer, database::Layer::User(_))).count();
                pill(
                    ui,
                    &format!(
                        "your files: {} · {edits} added or corrected · {} hidden",
                        db.user_files.join(", "),
                        db.hidden.len()
                    ),
                    p.accent_text,
                    p.accent_soft,
                );
            }
        });
        for n in &db.notices {
            ui.label(RichText::new(format!("{} {n}", icon::INFO)).size(12.0).color(p.spec));
        }
        if !db.problems.is_empty() {
            egui::CollapsingHeader::new(
                RichText::new(format!("{} {} problems in the database files", icon::WARNING, db.problems.len()))
                    .color(p.danger),
            )
            .id_salt("db-problems")
            .show(ui, |ui| {
                for pr in &db.problems {
                    ui.label(RichText::new(format!("{}: {}", pr.file, pr.message)).size(11.5));
                }
            });
        }
    }

    /// The Database menu button.
    pub(crate) fn database_menu(&mut self, ui: &mut Ui, p: &Palette, db: &Arc<Database>) {
        let busy = self.db_task.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            ui.menu_button(format!("{} Database", icon::DATABASE), |ui| {
                if ui
                    .button(format!("{} Import database file…", icon::FILE_ARROW_DOWN))
                    .on_hover_text("A .toml file of entries (added to your files) or a complete database pack .zip")
                    .clicked()
                {
                    ui.close();
                    if let Some(f) =
                        rfd::FileDialog::new().add_filter("Eyefinder database", &["toml", "zip"]).pick_file()
                    {
                        self.run_db_task("Importing the database file", ui.ctx(), move || {
                            let r = database::import(database::paths(), &f).map_err(|e| e.to_string())?;
                            database::reload();
                            Ok(DbTaskState::Done(match r {
                                Imported::UserFile(n) => format!("Added as your file {n}."),
                                Imported::Pack { version } => format!("Database {version} installed."),
                            }))
                        });
                    }
                }
                if ui
                    .button(format!("{} Check for a database update", icon::CLOUD_ARROW_DOWN))
                    .on_hover_text("Looks for a newer database pack on Eyefinder's GitHub releases")
                    .clicked()
                {
                    ui.close();
                    let current = db.version.clone();
                    self.run_db_task("Checking for a database update", ui.ctx(), move || {
                        Ok(match database::check_update(&current).map_err(|e| e.to_string())? {
                            Some(a) => DbTaskState::Offer(a),
                            None => DbTaskState::Done(format!("The database is up to date ({current}).")),
                        })
                    });
                }
                if ui
                    .button(format!("{} Export my additions…", icon::EXPORT))
                    .on_hover_text("Your added, corrected and hidden entries as one .toml, to share or contribute")
                    .clicked()
                {
                    ui.close();
                    if let Some(dest) = rfd::FileDialog::new()
                        .add_filter("Eyefinder database", &["toml"])
                        .set_file_name("my-eyefinder-additions.toml")
                        .save_file()
                    {
                        match Database::export_user(database::paths(), &dest) {
                            Ok(n) => self.toast(format!("{n} entries exported to {}", dest.display())),
                            Err(e) => self.toast(format!("Export failed: {e}")),
                        }
                    }
                }
                if let Some(root) = database::paths().root()
                    && ui.button(format!("{} Open the database folder", icon::FOLDER_OPEN)).clicked()
                {
                    ui.close();
                    let _ = std::fs::create_dir_all(root.join("user"));
                    open_in_file_manager(&root);
                }
                if ui.button(format!("{} Reload", icon::ARROW_CLOCKWISE)).clicked() {
                    ui.close();
                    load_in_background(ui.ctx());
                }
                ui.separator();
                ui.hyperlink_to(
                    "How to edit the database",
                    "https://github.com/galaxygas/Eyefinder/blob/main/docs/DATABASE.md",
                );
                let _ = p;
            });
        });
    }

    fn run_db_task(
        &mut self,
        title: &str,
        ctx: &egui::Context,
        work: impl FnOnce() -> Result<DbTaskState, String> + Send + 'static,
    ) {
        let state = Arc::new(Mutex::new(DbTaskState::Running));
        let (s, c) = (state.clone(), ctx.clone());
        let _ = thread::Builder::new().name("database-task".into()).spawn(move || {
            let r = work().unwrap_or_else(DbTaskState::Failed);
            *s.lock().unwrap() = r;
            c.request_repaint();
        });
        self.db_task = Some(DbTask { title: title.into(), state });
    }

    /// Carries out a click on the Index page.
    pub(crate) fn entry_action(&mut self, a: EntryAction, db: &Database, ctx: &egui::Context) {
        let paths = database::paths();
        let result = match a {
            EntryAction::Add { game, platform } => {
                let entry = Entry {
                    id: database::new_entry_id(db, &game),
                    kind: "retail".into(),
                    games: vec![game],
                    platforms: platform.into_iter().collect(),
                    ..Entry::default()
                };
                self.db_editor = Some(EntryEditor::of(entry, true));
                return;
            }
            EntryAction::Edit(id) => {
                if let Some(e) = db.entry(&id) {
                    self.db_editor = Some(EntryEditor::of(e.clone(), false));
                }
                return;
            }
            EntryAction::Hide(id) => database::hide_entry(paths, &id).map(|_| "Hidden in your copy."),
            EntryAction::Revert(id) => database::revert_entry(paths, &id).map(|_| "Your change was undone."),
        };
        match result {
            Ok(msg) => self.toast(msg),
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
        load_in_background(ctx);
    }

    /// The modal of a running database task.
    pub(crate) fn db_task_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(task) = &self.db_task else { return };
        let mut close = false;
        let mut install = None;
        egui::Modal::new("db-task".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(RichText::new(format!("{} {}", icon::DATABASE, task.title)).size(18.0).family(semibold()));
            ui.add_space(10.0);
            match &*task.state.lock().unwrap() {
                DbTaskState::Running => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Working…");
                    });
                }
                DbTaskState::Offer(a) => {
                    ui.label(format!("Database {} is available (release {}).", a.version, a.tag));
                    if a.sha256_url.is_none() {
                        ui.label(RichText::new("The release publishes no SHA-256 checksum.").size(12.0).color(p.spec));
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(accent_button(p, format!("{} Install {}", icon::DOWNLOAD_SIMPLE, a.version)))
                            .clicked()
                        {
                            install = Some(a.clone());
                        }
                        if ui.button("Not now").clicked() {
                            close = true;
                        }
                    });
                }
                DbTaskState::Done(m) => {
                    ui.label(m);
                    ui.add_space(10.0);
                    if ui.add(accent_button(p, "OK").min_size(vec2(80.0, 30.0))).clicked() {
                        close = true;
                    }
                }
                DbTaskState::Failed(m) => {
                    ui.label(RichText::new(m).color(p.danger));
                    ui.add_space(10.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
            }
        });
        if let Some(a) = install {
            self.run_db_task("Installing the database update", ctx, move || {
                let v = database::download_and_install(database::paths(), &a).map_err(|e| e.to_string())?;
                database::reload();
                Ok(DbTaskState::Done(format!("Database {v} installed.")))
            });
        } else if close {
            self.db_task = None;
        }
    }

    /// The entry editor.
    pub(crate) fn entry_editor_modal(&mut self, p: &Palette, ctx: &egui::Context) {
        let Some(ed) = &mut self.db_editor else { return };
        let Some(db) = database::loaded() else { return };
        let mut save = false;
        let mut close = false;
        let m = egui::Modal::new("db-editor".into()).frame(modal_frame(p)).show(ctx, |ui| {
            ui.set_width(640.0);
            let title = if ed.new { "Add a build or version" } else { "Correct an entry" };
            ui.label(RichText::new(format!("{} {title}", icon::PENCIL_SIMPLE)).size(18.0).family(semibold()));
            ui.label(
                RichText::new(
                    "Saved in your own database file, which survives updates. Give only what a source says: leave \
                     unknown fields empty, or write \"not documented\".",
                )
                .size(12.0)
                .color(p.muted),
            );
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.62).show(ui, |ui| {
                egui::Grid::new("db-editor-grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    let e = &mut ed.entry;
                    let label = |ui: &mut Ui, t: &str| {
                        ui.label(RichText::new(t).size(12.0).family(semibold()).color(p.muted));
                    };
                    label(ui, "Title");
                    ui.add(
                        egui::TextEdit::singleline(&mut e.title)
                            .hint_text("e.g. Retail disc v1.60")
                            .desired_width(460.0),
                    );
                    ui.end_row();
                    label(ui, "Kind");
                    egui::ComboBox::from_id_salt("db-kind").selected_text(db.kind_label(&e.kind)).show_ui(ui, |ui| {
                        for (k, l) in &db.kinds {
                            ui.selectable_value(&mut e.kind, k.clone(), l);
                        }
                    });
                    ui.end_row();
                    label(ui, "Games");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut ed.games).desired_width(300.0));
                        egui::ComboBox::from_id_salt("db-add-game").selected_text("Add").show_ui(ui, |ui| {
                            for g in &db.games {
                                if ui.selectable_label(false, &g.title).clicked() && !list(&ed.games).contains(&g.id) {
                                    ed.games = list(&ed.games)
                                        .into_iter()
                                        .chain([g.id.clone()])
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                }
                            }
                        });
                    });
                    ui.end_row();
                    label(ui, "Platforms");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut ed.platforms)
                                .hint_text("empty = all or not named")
                                .desired_width(300.0),
                        );
                        egui::ComboBox::from_id_salt("db-add-platform").selected_text("Add").show_ui(ui, |ui| {
                            for (id, name) in &db.platforms {
                                if ui.selectable_label(false, name).clicked() && !list(&ed.platforms).contains(id) {
                                    ed.platforms = list(&ed.platforms)
                                        .into_iter()
                                        .chain([id.clone()])
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                }
                            }
                        });
                    });
                    ui.end_row();
                    let text = |ui: &mut Ui, l: &str, v: &mut String, hint: &str| {
                        label(ui, l);
                        ui.add(egui::TextEdit::singleline(v).hint_text(hint).desired_width(460.0));
                        ui.end_row();
                    };
                    text(ui, "Platform text", &mut e.platform_text, "as the source writes it");
                    text(ui, "Region", &mut e.region, "e.g. PAL (Europe, Australia)");
                    text(ui, "Date", &mut e.date, "YYYY-MM-DD, YYYY-MM or YYYY, only when the source gives one date");
                    text(ui, "Date text", &mut e.date_text, "conflicting dates, ranges, comments");
                    text(ui, "Version", &mut e.version, "e.g. 1.60");
                    text(ui, "Serials", &mut ed.serials, "comma-separated, e.g. SLES-50330");
                    text(
                        ui,
                        "Serial / ID text",
                        &mut e.id_text,
                        "serials, IDs, sizes, hashes as the source gives them",
                    );
                    label(ui, "Notes");
                    ui.add(egui::TextEdit::multiline(&mut e.notes).desired_rows(3).desired_width(460.0));
                    ui.end_row();
                    label(ui, "Confidence");
                    egui::ComboBox::from_id_salt("db-conf")
                        .selected_text(if e.confidence.is_empty() { "—" } else { e.confidence.as_str() })
                        .show_ui(ui, |ui| {
                            for c in ["", "confirmed", "single-source", "unverified", "mixed"] {
                                ui.selectable_value(
                                    &mut e.confidence,
                                    c.to_string(),
                                    if c.is_empty() { "—" } else { c },
                                );
                            }
                        });
                    ui.end_row();
                    text(ui, "Sources", &mut ed.sources, "source keys, comma-separated, e.g. 3-45");
                    text(ui, "New source", &mut ed.source_text, "who says so: site, page title, date");
                    text(ui, "New source URL", &mut ed.source_url, "https://…");
                    label(ui, "Other fields");
                    ui.vertical(|ui| {
                        let mut remove = None;
                        for (i, (k, v)) in ed.fields.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(k).hint_text("name").desired_width(140.0));
                                ui.add(egui::TextEdit::singleline(v).hint_text("value").desired_width(270.0));
                                if ui.small_button(icon::X).clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            ed.fields.remove(i);
                        }
                        if ui.small_button(format!("{} Add a field", icon::PLUS)).clicked() {
                            ed.fields.push((String::new(), String::new()));
                        }
                    });
                    ui.end_row();
                });
            });
            if let Some(err) = &ed.error {
                ui.label(RichText::new(err).color(p.danger));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(accent_button(p, format!("{} Save", icon::FLOPPY_DISK)).min_size(vec2(100.0, 32.0))).clicked()
                {
                    save = true;
                }
                if ui.add(egui::Button::new("Cancel").min_size(vec2(90.0, 32.0))).clicked() {
                    close = true;
                }
            });
        });
        if m.should_close() {
            close = true;
        }
        if save {
            let (entry, new_sources) = ed.result(&db);
            match database::check_entry(&db, &entry, &new_sources) {
                Err(msg) => ed.error = Some(msg),
                Ok(()) => match database::save_user_entry(database::paths(), entry, new_sources) {
                    Ok(()) => {
                        self.db_editor = None;
                        self.toast("Saved in your database file.");
                        load_in_background(ctx);
                    }
                    Err(e) => ed.error = Some(format!("Could not save: {e}")),
                },
            }
        } else if close {
            self.db_editor = None;
        }
    }
}
