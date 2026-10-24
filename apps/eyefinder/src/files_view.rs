//! The centre view for a file picked in the sidebar's file tree: what it is, what Eyefinder can do
//! with it, its first bytes, and a byte-exact copy.

use std::io::{Read, Write};
use std::sync::Arc;

use ef_core::img::{ImgVfs, is_img_name};
use ef_core::vfs::Vfs;
use egui::{CornerRadius, Margin, RichText, Stroke, Ui, vec2};
use egui_phosphor::regular as icon;

use crate::app::{EyefinderApp, JobProgress, JobReport, TexFilter, View, accent_button, file_icon, human_bytes};
use crate::theme::{Palette, section_label, semibold};

/// Splits `models/gta3.img/hud.txd` into the archive and the entry, if the path runs through one.
fn split_archive(vfs: &dyn Vfs, path: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    for i in 1..parts.len() {
        let head = parts[..i].join("/");
        if is_img_name(&head) && vfs.exists(&head) {
            return Some((head, parts[i..].join("/")));
        }
    }
    None
}

fn kind_of(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    match l.rsplit('.').next().unwrap_or("") {
        "txd" => "RenderWare texture dictionary",
        "dff" => "RenderWare model (not viewable yet)",
        "col" => "Collision data (not viewable yet)",
        "ifp" => "Animation package (not viewable yet)",
        "img" => "IMG archive",
        "dir" => "IMG archive directory",
        "wav" => "WAVE audio",
        "mp3" => "MP3 audio",
        "adf" => "Vice City radio stream (obfuscated MP3)",
        "vb" => "PS2 audio stream (PS-ADPCM)",
        "gxt" => "Game text table (GTA2 GXT, or GTA III / Vice City GXT)",
        "fxt" => "GTA1 / London text table (FXT)",
        "sty" => "GTA2 style: tiles, sprites and palettes (STY)",
        "gry" => "GTA1 / London 8-bit style (GRY)",
        "g24" => "GTA1 / London 24-bit style (G24)",
        "fon" => "GTA1 / London font images (FON)",
        "sdt" => "Sample bank index (SDT)",
        "raw" => "Sample bank data or raw graphics (RAW)",
        "cmp" => "GTA1 / London compressed city map (CMP, not viewable yet)",
        "gmp" => "GTA2 map (GMP, not viewable yet)",
        "scr" => "GTA2 compiled mission script (not viewable)",
        "seq" | "mmp" | "ini" => "Text or data file",
        "smk" => "Smacker movie (not decoded)",
        "bik" => "Bink movie (not decoded)",
        "tga" => "Truevision TGA image",
        "str" => "Sega Dreamcast audio stream (STR)",
        "cdda" => "CD audio track (Red Book PCM)",
        "gb" | "gbc" | "cgb" => "Game Boy Color cartridge ROM",
        "gba" | "agb" => "Game Boy Advance cartridge ROM",
        "dat" | "ide" | "ipl" | "cfg" | "txt" => "Text or data file",
        "exe" => "Windows executable",
        "mpg" | "mpeg" => "MPEG movie",
        _ => "File",
    }
}

impl EyefinderApp {
    pub(crate) fn file_view(&mut self, ui: &mut Ui, p: &Palette, ctx: &egui::Context, path: &str) {
        let Some(sel) = self.selected_entry else { return };
        let Some((install, audio)) = self.current() else { return };
        let (install, audio) = (install.clone(), audio.clone());
        let vfs = install.vfs.clone();
        // the file system holding the file: the game, or an archive inside it
        let (fs, inner): (Arc<dyn Vfs>, String) = match split_archive(vfs.as_ref(), path) {
            Some((archive, entry)) => match ImgVfs::open(&vfs, &archive) {
                Ok(a) => (Arc::new(a), entry),
                Err(e) => {
                    ui.label(RichText::new(format!("{} {archive}: {e}", icon::WARNING)).color(p.danger));
                    return;
                }
            },
            None => (vfs.clone(), path.to_string()),
        };
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let size = fs.size(&inner);
        ui.horizontal(|ui| {
            ui.label(RichText::new(file_icon(&name)).size(30.0).color(p.accent_text));
            ui.vertical(|ui| {
                ui.label(RichText::new(&name).size(21.0).family(semibold()));
                ui.label(RichText::new(path).color(p.muted).size(12.5));
            });
        });
        ui.add_space(10.0);
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("file-facts").num_columns(2).spacing(vec2(18.0, 6.0)).show(ui, |ui| {
                    ui.label(RichText::new("Type").color(p.muted));
                    ui.label(kind_of(&name));
                    ui.end_row();
                    ui.label(RichText::new("Size").color(p.muted));
                    ui.label(size.map_or("unknown".into(), |s| format!("{} ({s} bytes)", human_bytes(s))));
                    ui.end_row();
                    ui.label(RichText::new("Stored in").color(p.muted));
                    ui.label(if Arc::ptr_eq(&fs, &vfs) { install.vfs.describe() } else { fs.describe() });
                    ui.end_row();
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    // what Eyefinder can show for this file
                    if let Some(cat) = self.textures_of(sel) {
                        let n = cat
                            .sources
                            .iter()
                            .position(|s| s.path.eq_ignore_ascii_case(path))
                            .map(|s| cat.assets.iter().filter(|a| a.source == s).count());
                        if let Some(n) = n
                            && ui.add(accent_button(p, format!("{} Show {n} textures", icon::IMAGE))).clicked()
                        {
                            self.settings.file_tree = false;
                            self.set_view(View::Textures(TexFilter::Dict(path.to_string())));
                        }
                    }
                    let audio_hits = audio.assets.iter().filter(|a| a.source.eq_ignore_ascii_case(path)).count();
                    if audio_hits > 0
                        && ui
                            .add(accent_button(p, format!("{} Show {audio_hits} audio items", icon::SPEAKER_HIGH)))
                            .clicked()
                    {
                        self.settings.file_tree = false;
                        self.search = name.clone();
                        self.set_view(View::Audio(None));
                    }
                    if ui
                        .button(format!("{} Save a copy...", icon::FLOPPY_DISK))
                        .on_hover_text("Copies the file byte for byte")
                        .clicked()
                        && let Some(dest) = rfd::FileDialog::new().set_file_name(&name).save_file()
                    {
                        let (fs2, inner2, dest2) = (fs.clone(), inner.clone(), dest.clone());
                        let total = size.unwrap_or(0) as usize;
                        let c2 = ctx.clone();
                        let out_dir = dest.parent().map(|d| d.join("_")).unwrap_or_default();
                        self.start_job("Saving a copy".into(), out_dir, total, ctx, move |progress, cancel| {
                            let mut src = fs2.open(&inner2).map_err(|e| e.to_string())?;
                            let tmp = dest2.with_extension("partial");
                            let mut out =
                                std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
                            let mut buf = vec![0u8; 1 << 20];
                            let mut done = 0usize;
                            loop {
                                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                                    drop(out);
                                    let _ = std::fs::remove_file(&tmp);
                                    return Ok(JobReport { written: 0, failed: vec![], cancelled: true });
                                }
                                let n = src.read(&mut buf).map_err(|e| e.to_string())?;
                                if n == 0 {
                                    break;
                                }
                                out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                                done += n;
                                *progress.lock().unwrap() =
                                    JobProgress { done, total, current: dest2.display().to_string() };
                                c2.request_repaint();
                            }
                            out.flush().map_err(|e| e.to_string())?;
                            drop(out);
                            std::fs::rename(&tmp, &dest2).map_err(|e| e.to_string())?;
                            Ok(JobReport { written: 1, failed: vec![], cancelled: false })
                        });
                    }
                });
            });
        // GTA1/London FXT and GTA2 GXT: show the decoded text.
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".fxt") || lower.ends_with(".gxt") {
            if self.text_cache.as_ref().is_none_or(|(k, _)| k != path) {
                let parsed = fs.read_all(&inner).map_err(|e| e.to_string()).and_then(|d| {
                    ef_core::text::parse_named(&name, &d)
                        .unwrap_or_else(|| Err("not a GTA1/London FXT or GTA2 GXT file".into()))
                });
                self.text_cache = Some((path.to_string(), parsed));
            }
            if let Some((_, parsed)) = &self.text_cache {
                ui.add_space(12.0);
                section_label(ui, p, "Text");
                ui.add_space(4.0);
                match parsed {
                    Err(e) => {
                        ui.label(RichText::new(e).color(p.muted));
                    }
                    Ok(t) => {
                        for n in &t.notes {
                            ui.label(RichText::new(format!("{} {n}", icon::INFO)).size(12.0).color(p.muted));
                        }
                        egui::Frame::new()
                            .fill(p.panel)
                            .stroke(Stroke::new(1.0, p.border))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::vertical().id_salt("text-entries").max_height(320.0).show_rows(
                                    ui,
                                    18.0,
                                    t.entries.len(),
                                    |ui, rows| {
                                        for (k, v) in &t.entries[rows] {
                                            ui.horizontal(|ui| {
                                                ui.add_sized(
                                                    vec2(90.0, 16.0),
                                                    egui::Label::new(
                                                        RichText::new(k).monospace().size(12.0).color(p.accent_text),
                                                    ),
                                                );
                                                ui.label(RichText::new(v.replace('\n', " · ")).size(12.5));
                                            });
                                        }
                                    },
                                );
                            });
                    }
                }
            }
        }
        ui.add_space(12.0);
        section_label(ui, p, "First bytes");
        ui.add_space(4.0);
        let mut head = vec![0u8; 512];
        let n = fs.open(&inner).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        head.truncate(n);
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (row, chunk) in head.chunks(16).enumerate() {
                        let hex: String = chunk.iter().map(|b| format!("{b:02X} ")).collect();
                        let text: String =
                            chunk.iter().map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '·' }).collect();
                        ui.label(
                            RichText::new(format!("{:08X}  {hex:<48} {text}", row * 16))
                                .monospace()
                                .size(12.0)
                                .color(if row % 2 == 0 { p.text } else { p.muted }),
                        );
                    }
                    if n == 0 {
                        ui.label(RichText::new("The file is empty or could not be read.").color(p.muted));
                    }
                });
            });
    }
}
