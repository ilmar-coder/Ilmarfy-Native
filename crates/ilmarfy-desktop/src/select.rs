//! Seleção de várias músicas (como no Spotify) e downloads de listas inteiras.
//!
//! Clique seleciona, ⌘/Ctrl+clique soma ou tira, Shift+clique pega o intervalo,
//! ⌘/Ctrl+A seleciona a lista toda e Esc limpa. Com músicas selecionadas aparece
//! uma barra com as ações para todas de uma vez.

use super::*;

/// Ação sobre todas as músicas selecionadas
pub(super) enum Batch {
    Play,
    PlayNext,
    Enqueue,
    Like,
    AddTo(String, String),
    Download,
    Export,
    Restrict(bool),
    Clear,
}

/// Adiciona várias músicas a uma playlist (pula as que já estão nela)
pub(super) fn add_many(c: &Ilmarfy, pid: &str, name: &str, ids: &[String]) -> Event {
    let already: HashSet<String> = c
        .library()
        .playlists()
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.id == pid)
        .map(|p| p.track_ids.into_iter().collect())
        .unwrap_or_default();
    let (mut added, mut failed) = (0, 0);
    for id in ids.iter().filter(|id| !already.contains(*id)) {
        match c.add_to_playlist(pid, id) {
            Ok(_) => added += 1,
            Err(_) => failed += 1,
        }
    }
    let skipped = ids.len() - added - failed;
    let mut msg = format!("{added} {} a \"{name}\"", plural(added, "música adicionada", "músicas adicionadas"));
    if skipped > 0 {
        msg += &format!(" ({skipped} já estavam)");
    }
    if failed > 0 {
        msg += &format!(", {failed} falharam");
    }
    Event::Message(msg)
}

/// Curte várias músicas (as já curtidas ficam de fora antes de chamar)
pub(super) fn like_many(c: &Ilmarfy, ids: &[String]) -> Event {
    let ok = ids.iter().filter(|id| c.toggle_favorite(id).is_ok()).count();
    Event::Message(format!("♥ {ok} {} nas curtidas", plural(ok, "música", "músicas")))
}

pub(super) fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 { one.to_string() } else { many.to_string() }
}

impl App {
    /// ⌘/Ctrl+A seleciona tudo; Esc limpa (só quando não está digitando)
    pub(super) fn selection_keys(&mut self, ui: &Ui, tracks: &[Track]) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (all, esc) = ui.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::A), i.key_pressed(egui::Key::Escape)));
        if all {
            self.selected = tracks.iter().map(|t| t.id.clone()).collect();
        }
        if esc {
            self.selected.clear();
            self.sel_anchor = None;
        }
    }

    /// Itens de menu para a seleção (usado no clique direito e no "…" da barra)
    pub(super) fn batch_menu(&self, ui: &mut Ui) -> Option<Batch> {
        ui.set_min_width(230.0);
        let mut out = None;
        let n = self.selected.len();
        ui.label(RichText::new(format!("{n} músicas selecionadas")).color(SUB));
        ui.separator();
        for (label, b) in [
            ("▶  Tocar", Batch::Play),
            ("⏭  Tocar a seguir", Batch::PlayNext),
            ("➕  Adicionar à fila", Batch::Enqueue),
            ("♥  Curtir", Batch::Like),
        ] {
            if ui.button(label).clicked() {
                out = Some(b);
            }
        }
        ui.menu_button("📃  Adicionar à playlist", |ui| {
            for p in &self.playlists {
                if ui.button(&p.name).clicked() {
                    out = Some(Batch::AddTo(p.id.clone(), p.name.clone()));
                }
            }
        });
        ui.separator();
        if ui.button("⬇  Baixar para ouvir offline").clicked() {
            out = Some(Batch::Download);
        }
        if ui.button("💾  Exportar para pendrive").clicked() {
            out = Some(Batch::Export);
        }
        ui.separator();
        let restricted_view = self.view == View::Restricted;
        if ui
            .button(if restricted_view {
                "↺  Tirar das restritas"
            } else {
                "🚫  Restringir"
            })
            .clicked()
        {
            out = Some(Batch::Restrict(!restricted_view));
        }
        if out.is_some() {
            ui.close();
        }
        out
    }

    /// Barra flutuante no rodapé da lista quando há músicas selecionadas
    pub(super) fn selection_bar(&mut self, ui: &mut Ui) -> Option<Batch> {
        if self.selected.is_empty() {
            return None;
        }
        let area = ui.max_rect();
        let mut out = None;
        egui::Area::new(egui::Id::unique("barra-selecao"))
            .order(egui::Order::Foreground)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(egui::pos2(area.center().x, area.bottom() - 16.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(0x28, 0x28, 0x28))
                    .corner_radius(12)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(160),
                    })
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let n = self.selected.len();
                            ui.label(RichText::new(format!("{n} {}", plural(n, "selecionada", "selecionadas"))).font(ui::font_bold(15.0)));
                            ui.add_space(10.0);
                            for (icon, tip, b) in [
                                ("▶", "Tocar", Batch::Play),
                                ("⏭", "Tocar a seguir", Batch::PlayNext),
                                ("➕", "Adicionar à fila", Batch::Enqueue),
                                ("♥", "Curtir", Batch::Like),
                                ("⬇", "Baixar para ouvir offline", Batch::Download),
                                ("💾", "Exportar para pendrive", Batch::Export),
                            ] {
                                if ui::icon_button(ui, icon, 19.0, false, tip).clicked() {
                                    out = Some(b);
                                }
                            }
                            let more = ui::icon_button(ui, "📃", 19.0, false, "Adicionar à playlist, restringir…");
                            egui::Popup::menu(&more).show(|ui| {
                                if let Some(b) = self.batch_menu(ui) {
                                    out = Some(b);
                                }
                            });
                            ui.add_space(6.0);
                            if ui::icon_button(ui, "×", 17.0, false, "Limpar seleção (Esc)").clicked() {
                                out = Some(Batch::Clear);
                            }
                        });
                    });
            });
        out
    }

    pub(super) fn batch_action(&mut self, ctx: &egui::Context, chosen: Vec<Track>, b: Batch) {
        if chosen.is_empty() {
            return;
        }
        let n = chosen.len();
        match b {
            Batch::Play => self.play_tracks(&chosen, 0, false),
            Batch::PlayNext => {
                if let Some(p) = &mut self.playback {
                    // Em ordem reversa: cada uma entra logo depois da atual
                    for t in chosen.into_iter().rev() {
                        p.play_next(Item::Track(t));
                    }
                }
                self.toast(ctx, format!("{n} {} a seguir", plural(n, "toca", "tocam")));
            }
            Batch::Enqueue => {
                if let Some(p) = &mut self.playback {
                    for t in chosen {
                        p.enqueue(Item::Track(t));
                    }
                }
                self.toast(ctx, format!("{n} {} à fila", plural(n, "adicionada", "adicionadas")));
            }
            Batch::Like => {
                let ids: Vec<String> = chosen
                    .iter()
                    .filter(|t| !self.favorites.contains(&t.id))
                    .map(|t| t.id.clone())
                    .collect();
                if ids.is_empty() {
                    self.toast(ctx, "Já estão todas nas curtidas");
                } else {
                    self.spawn(ctx, move |c| like_many(c, &ids));
                }
            }
            Batch::AddTo(pid, name) => {
                let ids: Vec<String> = chosen.iter().map(|t| t.id.clone()).collect();
                self.spawn(ctx, move |c| add_many(c, &pid, &name, &ids));
            }
            Batch::Download => {
                let key = format!("sel:{}", self.view_download_key());
                self.start_download(ctx, key, chosen);
            }
            Batch::Export => {
                self.export.scope = export::Scope::Selected;
                self.export.selected = chosen;
                self.go(View::Export);
            }
            Batch::Restrict(on) => {
                let ids: Vec<String> = chosen.iter().map(|t| t.id.clone()).collect();
                self.selected.clear();
                self.spawn(ctx, move |c| {
                    let ok = ids.iter().filter(|id| c.set_restricted(id, on).is_ok()).count();
                    Event::Message(if on {
                        format!("🚫 {ok} {} para as restritas", plural(ok, "música foi", "músicas foram"))
                    } else {
                        format!("↺ {ok} {} das restritas", plural(ok, "música saiu", "músicas saíram"))
                    })
                });
            }
            Batch::Clear => {
                self.selected.clear();
                self.sel_anchor = None;
            }
        }
    }

    /// Baixa uma lista de músicas em segundo plano, com andamento no botão `key`
    pub(super) fn start_download(&mut self, ctx: &egui::Context, key: String, tracks: Vec<Track>) {
        if self.downloading.contains(&key) {
            return;
        }
        let missing: Vec<Track> = tracks.into_iter().filter(|t| !self.downloaded.contains(&t.id)).collect();
        if missing.is_empty() {
            self.toast(ctx, "Já estão todas no computador");
            return;
        }
        self.downloading.insert(key.clone());
        let prog = self.dl_progress.clone();
        prog.lock().unwrap().insert(key.clone(), (0, missing.len()));
        self.toast(
            ctx,
            format!("Baixando {} {}…", missing.len(), plural(missing.len(), "música", "músicas")),
        );
        let ctx2 = ctx.clone();
        self.spawn(ctx, move |c| {
            let r = c.download_tracks(&missing, |i, n, _| {
                prog.lock().unwrap().insert(key.clone(), (i, n));
                ctx2.request_repaint();
                true
            });
            let msg = match r {
                Ok((n, fails)) if fails.is_empty() => format!("⬇ {n} {} no computador", plural(n as usize, "música", "músicas")),
                Ok((n, fails)) => format!("⬇ {n} baixadas, {} falharam", fails.len()),
                Err(e) => format!("Falha ao baixar: {e}"),
            };
            Event::DownloadDone(key, msg)
        });
    }

    /// Botão ⬇ do cabeçalho de qualquer lista (artista, playlist, curtidas, ranking…)
    pub(super) fn download_button(&mut self, ui: &mut Ui, tracks: &[Track]) {
        let key = self.view_download_key();
        let missing: Vec<&Track> = tracks.iter().filter(|t| !self.downloaded.contains(&t.id)).collect();
        ui.add_space(4.0);
        if self.downloading.contains(&key) {
            let (i, n) = self.dl_progress.lock().unwrap().get(&key).copied().unwrap_or((0, 0));
            ui.spinner();
            ui.label(RichText::new(format!("Baixando {} de {n}…", (i + 1).min(n.max(1)))).color(SUB));
        } else if !tracks.is_empty() && missing.is_empty() {
            ui.label(RichText::new("⬇ Disponível offline").color(GREEN));
        } else if !tracks.is_empty() {
            let tip = format!(
                "Baixar para ouvir offline: {} {} (≈ {})",
                missing.len(),
                plural(missing.len(), "música", "músicas"),
                export::fmt_size(export::estimate_bytes(&missing))
            );
            if ui::icon_button(ui, "⬇", 24.0, false, &tip).clicked() {
                let list = tracks.to_vec();
                self.start_download(ui.ctx(), key, list);
            }
        }
    }

    /// Ações da página Offline: baixar a biblioteca toda e exportar para pendrive
    pub(super) fn offline_actions(&mut self, ui: &mut Ui) {
        let key = "Offline:tudo".to_string();
        let missing: Vec<&Track> = self.tracks_cache.iter().filter(|t| !self.downloaded.contains(&t.id)).collect();
        ui.add_space(12.0);
        if self.downloading.contains(&key) {
            let (i, n) = self.dl_progress.lock().unwrap().get(&key).copied().unwrap_or((0, 0));
            ui.spinner();
            ui.label(RichText::new(format!("Baixando a biblioteca: {} de {n}…", (i + 1).min(n.max(1)))).color(SUB));
        } else if !missing.is_empty() {
            let label = format!(
                "⬇  Baixar todas ({} faltando, ≈ {})",
                missing.len(),
                export::fmt_size(export::estimate_bytes(&missing))
            );
            if ui::pill(ui, &label, TILE, TEXT).clicked() {
                let all = self.tracks_cache.clone();
                self.start_download(ui.ctx(), key, all);
            }
        }
        ui.add_space(8.0);
        if ui::pill(ui, "💾  Exportar para pendrive", TILE, TEXT).clicked() {
            self.export.scope = export::Scope::Library;
            self.go(View::Export);
        }
    }
}
