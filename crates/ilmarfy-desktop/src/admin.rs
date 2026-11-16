//! Administração da biblioteca (o que antes só o Ilmarfy web fazia): adicionar músicas
//! pelo YouTube Music, fila de downloads do servidor, editar/corrigir/apagar faixas,
//! duplicadas e backup.

use super::*;
use ilmarfy_core::{DownloadTask, Duplicates, TrackEdit, YtTrack};

const ADD_TOP: Color32 = Color32::from_rgb(0x27, 0x56, 0xC9);
const ADD_BOTTOM: Color32 = Color32::from_rgb(0x0F, 0x1E, 0x4A);
const DANGER: Color32 = Color32::from_rgb(0xB4, 0x32, 0x32);

#[derive(Default)]
pub(super) struct AdminState {
    pub(super) query: String,
    pub(super) loading: bool,
    /// Título da lista de resultados ("Resultados para…", nome do álbum, "Playlist")
    pub(super) title: String,
    pub(super) results: Vec<YtTrack>,
    pub(super) error: Option<String>,
    /// ids do YouTube já mandados para a fila nesta sessão
    pub(super) queued: HashSet<String>,
    /// Link de um vídeo avulso: (link, artista, título)
    pub(super) link: Option<(String, String, String)>,
    pub(super) downloads: Vec<DownloadTask>,
    pub(super) next_poll: f64,
    pub(super) edit: Option<(Track, TrackEdit)>,
    pub(super) delete: Option<Track>,
    pub(super) dups: Option<Duplicates>,
    pub(super) dups_loading: bool,
    pub(super) dups_confirm: bool,
}

pub(super) enum AdminEvent {
    Results(Result<(String, Vec<YtTrack>), String>),
    Downloads(Vec<DownloadTask>),
    Duplicates(Result<Duplicates, String>),
}

fn is_link(q: &str) -> bool {
    q.starts_with("http://") || q.starts_with("https://")
}

fn is_playlist_link(q: &str) -> bool {
    is_link(q) && (q.contains("list=") || q.contains("playlist"))
}

fn status_label(s: &str) -> &str {
    match s {
        "queued" => "Na fila",
        "downloading" => "Baixando",
        "processing" | "converting" => "Processando",
        "paused" => "Pausado",
        "completed" | "done" => "Pronto",
        "error" | "failed" => "Erro",
        "cancelled" | "canceled" => "Cancelado",
        other => other,
    }
}

/// Data de hoje (UTC) como AAAA-MM-DD, sem depender de bibliotecas de data
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl App {
    pub(super) fn admin_event(&mut self, ctx: &egui::Context, ev: AdminEvent) {
        match ev {
            AdminEvent::Results(r) => {
                self.admin.loading = false;
                match r {
                    Ok((title, list)) => {
                        self.admin.title = title;
                        self.admin.results = list;
                        self.admin.error = None;
                    }
                    Err(e) => self.admin.error = Some(e),
                }
            }
            AdminEvent::Downloads(list) => {
                // Algum download terminou? Atualiza a biblioteca para a música aparecer
                let finished = self
                    .admin
                    .downloads
                    .iter()
                    .filter(|old| old.is_active())
                    .any(|old| list.iter().any(|n| n.id == old.id && !n.is_active()));
                self.admin.downloads = list;
                if finished {
                    self.sync(ctx);
                }
            }
            AdminEvent::Duplicates(r) => {
                self.admin.dups_loading = false;
                match r {
                    Ok(d) => self.admin.dups = Some(d),
                    Err(e) => self.toast(ctx, e),
                }
            }
        }
    }

    pub(super) fn active_downloads(&self) -> usize {
        self.admin.downloads.iter().filter(|d| d.is_active()).count()
    }

    /// Consulta a fila do servidor: a cada 2 s com downloads ativos ou na página, senão a cada 30 s
    pub(super) fn poll_downloads(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now < self.admin.next_poll {
            return;
        }
        let fast = self.view == View::Add || self.active_downloads() > 0;
        let every = if fast { 2.0 } else { 30.0 };
        self.admin.next_poll = now + every;
        ctx.request_repaint_after(Duration::from_secs_f64(every));
        let Some(core) = self.core.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Ok(list) = core.api().downloads() {
                let _ = tx.send(Event::Admin(AdminEvent::Downloads(list)));
                ctx.request_repaint();
            }
        });
    }

    fn admin_send(&self, ctx: &egui::Context, job: impl FnOnce(&Ilmarfy) -> AdminEvent + Send + 'static) {
        let Some(core) = self.core.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Admin(job(&core)));
            ctx.request_repaint();
        });
    }

    pub(super) fn run_search(&mut self, ctx: &egui::Context) {
        let q = self.admin.query.trim().to_string();
        if q.is_empty() {
            return;
        }
        if is_link(&q) && !is_playlist_link(&q) {
            self.admin.link = Some((q, String::new(), String::new()));
            return;
        }
        self.admin.link = None;
        self.admin.loading = true;
        self.admin.error = None;
        self.admin_send(ctx, move |c| {
            AdminEvent::Results(if is_playlist_link(&q) {
                c.api()
                    .yt_playlist(&q)
                    .map(|l| ("Playlist do YouTube".to_string(), l))
                    .map_err(|e| e.to_string())
            } else {
                c.api()
                    .yt_search(&q)
                    .map(|l| (format!("Resultados para \"{q}\""), l))
                    .map_err(|e| e.to_string())
            })
        });
    }

    fn open_album(&mut self, ctx: &egui::Context, album_id: String) {
        self.admin.loading = true;
        self.admin_send(ctx, move |c| {
            AdminEvent::Results(
                c.api()
                    .yt_album(&album_id)
                    .map(|a| {
                        let title = format!("Álbum: {} — {}", a.title, a.artist);
                        // As faixas do álbum vêm sem capa/álbum; completa com os dados do álbum
                        let tracks = a
                            .tracks
                            .into_iter()
                            .map(|mut t| {
                                if t.cover.is_empty() {
                                    t.cover = a.cover.clone();
                                }
                                if t.album.is_empty() {
                                    t.album = a.title.clone();
                                }
                                if t.artist.is_empty() {
                                    t.artist = a.artist.clone();
                                }
                                t
                            })
                            .collect();
                        (title, tracks)
                    })
                    .map_err(|e| e.to_string()),
            )
        });
    }

    fn in_library(&self, t: &YtTrack) -> bool {
        self.tracks_cache
            .iter()
            .any(|l| l.title.eq_ignore_ascii_case(t.title.trim()) && l.artist.eq_ignore_ascii_case(t.artist.trim()))
    }

    fn queue_tracks(&mut self, ctx: &egui::Context, tracks: Vec<YtTrack>) {
        let tracks: Vec<YtTrack> = tracks
            .into_iter()
            .filter(|t| !self.admin.queued.contains(&t.id) && !self.in_library(t))
            .collect();
        if tracks.is_empty() {
            self.toast(ctx, "Essas músicas já estão na biblioteca ou na fila");
            return;
        }
        for t in &tracks {
            self.admin.queued.insert(t.id.clone());
        }
        self.admin.next_poll = 0.0;
        let n = tracks.len();
        self.spawn(ctx, move |c| {
            let mut fails = 0;
            for t in &tracks {
                if c.api().add_yt_track(t).is_err() {
                    fails += 1;
                }
            }
            Event::Message(match (n, fails) {
                (1, 0) => format!("⬇ \"{}\" foi para a fila de downloads", tracks[0].title),
                (_, 0) => format!("⬇ {n} músicas foram para a fila de downloads"),
                (_, f) => format!("{} na fila, {f} recusadas (já existem?)", n - f),
            })
        });
    }

    // ------------------------------------------------------------ Página "Adicionar músicas"

    pub(super) fn add_page(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.page_header(
                ui,
                "Biblioteca",
                "Adicionar músicas",
                "Busque no YouTube Music, ou cole o link de uma música ou playlist",
                ADD_TOP,
                |ui| {
                    ui::special_cover(ui, 200.0, ADD_TOP, ADD_BOTTOM, "➕");
                },
            );
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                let w = (ui.available_width() - 160.0).min(560.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.admin.query)
                        .hint_text("🔍  Artista, música, álbum ou link")
                        .desired_width(w)
                        .font(ui::font(16.0))
                        .margin(Vec2::new(16.0, 10.0)),
                );
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui::pill(ui, "Buscar", GREEN, Color32::BLACK).clicked() || enter {
                    self.run_search(&ctx);
                }
            });
            if let Some((url, artist, title)) = &mut self.admin.link {
                let mut send = false;
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("Link de um vídeo: diga de quem é a música").color(SUB));
                });
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.add(egui::TextEdit::singleline(artist).hint_text("Artista").desired_width(200.0));
                    ui.add(egui::TextEdit::singleline(title).hint_text("Título").desired_width(260.0));
                    send = ui::pill(ui, "⬇  Adicionar", GREEN, Color32::BLACK).clicked();
                });
                if send {
                    let (url, artist, title) = (url.clone(), artist.trim().to_string(), title.trim().to_string());
                    if artist.is_empty() || title.is_empty() {
                        self.toast(&ctx, "Preencha artista e título");
                    } else {
                        self.admin.link = None;
                        self.admin.query.clear();
                        self.admin.next_poll = 0.0;
                        self.spawn(&ctx, move |c| match c.api().start_download(&url, &title, &artist, 0) {
                            Ok(()) => Event::Message(format!("⬇ \"{title}\" foi para a fila de downloads")),
                            Err(e) => Event::Message(e.to_string()),
                        });
                    }
                }
            }
            ui.add_space(20.0);
            self.downloads_section(ui);
            self.results_section(ui);
            ui.add_space(24.0);
        });
    }

    fn downloads_section(&mut self, ui: &mut Ui) {
        if self.admin.downloads.is_empty() {
            return;
        }
        let active = self.active_downloads();
        section_title(ui, &format!("Fila de downloads ({active} em andamento)"));
        let mut act: Option<(String, &'static str)> = None;
        for d in self.admin.downloads.iter().take(12) {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::hover());
            let row = rect.shrink2(Vec2::new(24.0, 0.0));
            let cy = row.center().y;
            let name = if d.title.is_empty() {
                d.query.clone()
            } else {
                format!("{} — {}", d.title, d.artist)
            };
            let text_w = row.width() * 0.45;
            paint_truncated(ui, egui::pos2(row.left(), cy - 9.0), &name, ui::font_bold(14.0), TEXT, text_w);
            let detail = if !d.error.is_empty() {
                d.error.clone()
            } else if d.status == "downloading" {
                [d.speed.as_str(), d.eta.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" • ")
            } else {
                String::new()
            };
            let status_color = match d.status.as_str() {
                "completed" | "done" => GREEN,
                "error" | "failed" => Color32::from_rgb(0xFF, 0x52, 0x52),
                _ => SUB,
            };
            paint_truncated(
                ui,
                egui::pos2(row.left(), cy + 10.0),
                &format!("{}  {detail}", status_label(&d.status)),
                ui::font(12.0),
                status_color,
                text_w,
            );
            // barra de progresso
            let bar = Rect::from_min_size(
                egui::pos2(row.left() + text_w + 16.0, cy - 2.0),
                Vec2::new(row.width() - text_w - 16.0 - 90.0, 4.0),
            );
            ui.painter().rect_filled(bar, 2.0, Color32::from_rgb(0x4D, 0x4D, 0x4D));
            let frac = (d.progress / 100.0).clamp(0.0, 1.0) as f32;
            let fill = Rect::from_min_size(bar.min, Vec2::new(bar.width() * frac, 4.0));
            ui.painter().rect_filled(fill, 2.0, if d.is_active() { GREEN } else { SUB });
            if d.is_active() {
                let mut b = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_max(egui::pos2(row.right() - 80.0, row.top()), row.right_bottom()))
                        .layout(Layout::right_to_left(Align::Center)),
                );
                if ui::icon_button(&mut b, "✖", 15.0, false, "Cancelar").clicked() {
                    act = Some((d.id.clone(), "cancel"));
                }
                let (icon, action, tip) = if d.status == "paused" {
                    ("▶", "resume", "Continuar")
                } else {
                    ("⏸", "pause", "Pausar")
                };
                if ui::icon_button(&mut b, icon, 15.0, false, tip).clicked() {
                    act = Some((d.id.clone(), action));
                }
            }
        }
        if let Some((id, action)) = act {
            self.admin.next_poll = 0.0;
            self.spawn(ui.ctx(), move |c| match c.api().download_action(&id, action) {
                Ok(()) => Event::Changed,
                Err(e) => Event::Message(e.to_string()),
            });
        }
        ui.add_space(16.0);
    }

    fn results_section(&mut self, ui: &mut Ui) {
        if self.admin.loading {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.spinner();
                ui.label(RichText::new("Buscando no YouTube Music…").color(SUB));
            });
            return;
        }
        if let Some(e) = &self.admin.error {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.colored_label(Color32::from_rgb(255, 107, 107), format!("Não deu para buscar: {e}"));
            });
            return;
        }
        if self.admin.results.is_empty() {
            if !self.admin.title.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    ui.label(RichText::new("Nada encontrado.").color(SUB));
                });
            }
            return;
        }
        let results = self.admin.results.clone();
        let mut add_all = false;
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(&self.admin.title).font(ui::font_bold(22.0)));
            ui.add_space(12.0);
            if results.len() > 1 {
                add_all = ui::pill(ui, &format!("⬇  Adicionar todas ({})", results.len()), TILE, TEXT).clicked();
            }
        });
        ui.add_space(8.0);
        let current = self.playback.as_ref().and_then(|p| p.current_key().map(String::from));
        let mut act: Option<(usize, u8)> = None;
        for (i, t) in results.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
            let row = rect.shrink2(Vec2::new(12.0, 0.0));
            if resp.hovered() {
                ui.painter().rect_filled(row, 6.0, Color32::from_white_alpha(18));
            }
            let cy = row.center().y;
            let playing = current.as_deref() == Some(format!("f:{}", t.id).as_str());
            ui.painter().text(
                egui::pos2(row.left() + 28.0, cy),
                Align2::CENTER_CENTER,
                if resp.hovered() { "▶".into() } else { (i + 1).to_string() },
                ui::font(14.0),
                if playing { GREEN } else { SUB },
            );
            let cover = Rect::from_min_size(egui::pos2(row.left() + 52.0, cy - 20.0), Vec2::splat(40.0));
            ui::cover_at(ui, cover, &t.cover, &t.artist, 4.0);
            let width = row.width() - 52.0 - 56.0;
            let tw = width * 0.45;
            paint_truncated(
                ui,
                egui::pos2(cover.right() + 12.0, cy - 9.0),
                &t.title,
                ui::font_bold(15.0),
                if playing { GREEN } else { TEXT },
                tw,
            );
            paint_truncated(ui, egui::pos2(cover.right() + 12.0, cy + 10.0), &t.artist, ui::font(13.0), SUB, tw);
            // álbum (clicável quando há id)
            let ax = cover.right() + 12.0 + tw + 16.0;
            let aw = width - tw - 16.0 - 200.0;
            if !t.album.is_empty() && aw > 60.0 {
                let album_rect = Rect::from_min_size(egui::pos2(ax, cy - 10.0), Vec2::new(aw, 20.0));
                let a_resp = ui.interact(album_rect, ui.scope_id().with(("album", i)), Sense::click());
                let color = if a_resp.hovered() && !t.album_id.is_empty() { TEXT } else { SUB };
                paint_truncated(ui, egui::pos2(ax, cy), &t.album, ui::font(13.0), color, aw);
                if !t.album_id.is_empty() && a_resp.on_hover_text("Ver o álbum inteiro").clicked() {
                    act = Some((i, 2));
                }
            }
            ui.painter().text(
                egui::pos2(row.right() - 110.0, cy),
                Align2::RIGHT_CENTER,
                &t.duration_str,
                ui::font(13.0),
                SUB,
            );
            if resp.clicked() {
                act = Some((i, 0));
            }
            let mut b = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_max(egui::pos2(row.right() - 90.0, row.top()), row.right_bottom()))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            b.add_space(8.0);
            if self.in_library(t) {
                b.label(RichText::new("✔ Na biblioteca").color(GREEN).font(ui::font(12.0)));
            } else if self.admin.queued.contains(&t.id) {
                b.label(RichText::new("⬇ Na fila").color(SUB).font(ui::font(12.0)));
            } else if ui::icon_button(&mut b, "➕", 18.0, false, "Adicionar à biblioteca").clicked() {
                act = Some((i, 1));
            }
        }
        let ctx = ui.ctx().clone();
        if add_all {
            self.queue_tracks(&ctx, results.clone());
        }
        if let Some((i, a)) = act {
            match a {
                0 => {
                    if let Some(p) = &mut self.playback {
                        p.play_items(results.iter().map(|t| Item::Flow(t.as_flow())).collect(), i, false);
                    }
                }
                1 => self.queue_tracks(&ctx, vec![results[i].clone()]),
                _ => self.open_album(&ctx, results[i].album_id.clone()),
            }
        }
    }

    // ------------------------------------------------------------ Editar e apagar faixas

    pub(super) fn start_edit(&mut self, t: Track) {
        let edit = TrackEdit {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            year: t.year.clone(),
            cover_url: t.cover_url.clone(),
        };
        self.admin.edit = Some((t, edit));
    }

    pub(super) fn ask_delete(&mut self, t: Track) {
        self.admin.delete = Some(t);
    }

    pub(super) fn fix_metadata(&mut self, ctx: &egui::Context, t: Track) {
        self.toast(ctx, format!("Procurando capa e álbum de \"{}\"…", t.title));
        self.spawn(ctx, move |c| match c.api().fix_metadata(&t.id) {
            Ok(n) => {
                let _ = c.sync();
                Event::Message(format!("✨ \"{}\" atualizada ({})", n.title, n.album))
            }
            Err(e) => Event::Message(format!("Não achei dados para \"{}\": {e}", t.title)),
        });
    }

    pub(super) fn admin_windows(&mut self, ctx: &egui::Context) {
        self.edit_window(ctx);
        self.delete_window(ctx);
        self.dups_window(ctx);
    }

    fn edit_window(&mut self, ctx: &egui::Context) {
        let Some((track, edit)) = &mut self.admin.edit else { return };
        let (mut save, mut cancel) = (false, false);
        egui::Window::new("Editar informações")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(120.0), Sense::hover());
                    ui::cover_at(ui, r, &edit.cover_url, &format!("{}{}", edit.artist, edit.album), 6.0);
                    ui.add_space(12.0);
                    egui::Grid::new("edit").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        for (label, value) in [
                            ("Título", &mut edit.title),
                            ("Artista", &mut edit.artist),
                            ("Álbum", &mut edit.album),
                            ("Ano", &mut edit.year),
                            ("Capa (link)", &mut edit.cover_url),
                        ] {
                            ui.label(RichText::new(label).color(SUB));
                            ui.add(egui::TextEdit::singleline(value).desired_width(300.0));
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    save = ui::pill(ui, "Salvar", GREEN, Color32::BLACK).clicked();
                    cancel = ui::pill(ui, "Cancelar", TILE, TEXT).clicked();
                });
            });
        if save {
            let (track, edit) = (track.clone(), edit.clone());
            self.admin.edit = None;
            self.spawn(ctx, move |c| match c.api().update_track(&track.id, &edit, &track.filename) {
                Ok(_) => {
                    let _ = c.sync();
                    Event::Message(format!("\"{}\" atualizada", edit.title))
                }
                Err(e) => Event::Message(e.to_string()),
            });
        } else if cancel {
            self.admin.edit = None;
        }
    }

    fn delete_window(&mut self, ctx: &egui::Context) {
        let Some(t) = self.admin.delete.clone() else { return };
        let (mut yes, mut no) = (false, false);
        egui::Window::new("Apagar música")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new(format!("Apagar \"{}\" de {}?", t.title, t.artist)).font(ui::font_bold(16.0)));
                ui.label(
                    RichText::new("O arquivo é removido do servidor e da biblioteca em todos os aparelhos. Não dá para desfazer.")
                        .color(SUB),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    yes = ui::pill(ui, "Apagar", DANGER, TEXT).clicked();
                    no = ui::pill(ui, "Cancelar", TILE, TEXT).clicked();
                });
            });
        if yes {
            self.admin.delete = None;
            self.spawn(ctx, move |c| match c.api().delete_track(&t.id) {
                Ok(()) => {
                    let _ = c.remove_download(&t.id);
                    let _ = c.sync();
                    Event::Message(format!("\"{}\" foi apagada", t.title))
                }
                Err(e) => Event::Message(e.to_string()),
            });
        } else if no {
            self.admin.delete = None;
        }
    }

    // ------------------------------------------------------------ Manutenção (Configurações)

    pub(super) fn maintenance_section(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.add_space(28.0);
        ui.label(RichText::new("Manutenção da biblioteca").font(ui::font_bold(20.0)));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let label = if self.admin.dups_loading {
                "Procurando…"
            } else {
                "Procurar músicas duplicadas"
            };
            if ui::pill(ui, label, TILE, TEXT).clicked() && !self.admin.dups_loading {
                self.admin.dups_loading = true;
                self.admin.dups_confirm = false;
                self.admin_send(&ctx, |c| AdminEvent::Duplicates(c.api().duplicates().map_err(|e| e.to_string())));
            }
            if ui::pill(ui, "Salvar backup da biblioteca", TILE, TEXT).clicked() {
                let dir = dirs::download_dir().unwrap_or_else(|| self.data_dir.clone());
                self.spawn(&ctx, move |c| match c.api().backup() {
                    Ok(v) => {
                        let path = dir.join(format!("ilmarfy-backup-{}.json", today()));
                        match serde_json::to_vec_pretty(&v)
                            .map_err(|e| e.to_string())
                            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
                        {
                            Ok(()) => Event::Message(format!("Backup salvo em {}", path.display())),
                            Err(e) => Event::Message(format!("Falha ao salvar o backup: {e}")),
                        }
                    }
                    Err(e) => Event::Message(e.to_string()),
                });
            }
        });
    }

    fn dups_window(&mut self, ctx: &egui::Context) {
        let Some(d) = self.admin.dups.clone() else { return };
        let (mut close, mut clean) = (false, false);
        egui::Window::new("Músicas duplicadas")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(520.0);
                if d.duplicates.is_empty() {
                    ui.label(RichText::new("Nenhuma duplicada. A biblioteca está limpa! ✨").color(SUB));
                } else {
                    let extra: usize = d.duplicates.iter().map(|c| c.tracks.len() - 1).sum();
                    ui.label(
                        RichText::new(format!(
                            "{} músicas aparecem mais de uma vez ({extra} cópias a mais). A limpeza mantém a mais tocada de cada uma.",
                            d.duplicates.len()
                        ))
                        .color(SUB),
                    );
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for cluster in &d.duplicates {
                            let first = &cluster.tracks[0];
                            ui.label(RichText::new(format!("{} — {}", first.title, first.artist)).font(ui::font_bold(14.0)));
                            for t in &cluster.tracks {
                                ui.label(
                                    RichText::new(format!(
                                        "    {} • {} • {} reproduções",
                                        if t.album.is_empty() { "sem álbum" } else { &t.album },
                                        ui::fmt_duration(t.duration.max(0) as u64),
                                        t.play_count
                                    ))
                                    .color(SUB),
                                );
                            }
                            ui.add_space(4.0);
                        }
                    });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if !d.duplicates.is_empty() {
                        if self.admin.dups_confirm {
                            clean = ui::pill(ui, "Confirmar: apagar as cópias", DANGER, TEXT).clicked();
                        } else if ui::pill(ui, "Apagar as cópias", DANGER, TEXT).clicked() {
                            self.admin.dups_confirm = true;
                        }
                    }
                    close = ui::pill(ui, "Fechar", TILE, TEXT).clicked();
                });
            });
        if clean {
            self.admin.dups = None;
            self.admin.dups_confirm = false;
            self.spawn(ctx, |c| match c.api().cleanup_duplicates() {
                Ok(v) => {
                    let _ = c.sync();
                    let n = v.get("deleted").and_then(|n| n.as_u64()).unwrap_or(0);
                    Event::Message(format!("Limpeza feita: {n} cópias apagadas"))
                }
                Err(e) => Event::Message(e.to_string()),
            });
        } else if close {
            self.admin.dups = None;
            self.admin.dups_confirm = false;
        }
    }
}

fn section_title(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(text).font(ui::font_bold(22.0)));
    });
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_links() {
        assert!(is_playlist_link("https://music.youtube.com/playlist?list=PL123"));
        assert!(is_playlist_link("https://www.youtube.com/watch?v=abc&list=PL1"));
        assert!(!is_playlist_link("https://youtu.be/abc"));
        assert!(is_link("https://youtu.be/abc"));
        assert!(!is_link("Cachorro Grande"));
    }

    #[test]
    fn data_de_hoje_no_formato_certo() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("20"));
    }
}
