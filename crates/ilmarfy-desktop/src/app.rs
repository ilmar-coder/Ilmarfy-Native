//! Interface do Ilmarfy para desktop (egui).

use crate::playback::{Item, Playback, Status};
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Ui, Vec2};
use ilmarfy_core::{
    AlbumSummary, ArtistSummary, DEFAULT_SERVER, DislikeScope, FlowRestricted, FlowTrack, Ilmarfy, LyricLine, Playlist, Repeat, Track,
    TrackOrder,
};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub const GREEN: Color32 = Color32::from_rgb(0x1D, 0xB9, 0x54);
const SUBTEXT: Color32 = Color32::from_rgb(0xB3, 0xB3, 0xB3);
const ROW_H: f32 = 48.0;

#[derive(Clone, PartialEq)]
enum View {
    Home,
    Search,
    Songs,
    Artists,
    Artist(String),
    Playlists,
    Playlist(String),
    Favorites,
    Offline,
    Flow,
    Settings,
}

/// Sementes e faixas do Flow, ou o erro
type FlowResult = Result<(Vec<String>, Vec<FlowTrack>), String>;

/// Resultados de tarefas em segundo plano (rede)
enum Event {
    Synced(Result<String, String>),
    LoggedIn(Result<(String, Box<Ilmarfy>), String>),
    Flow(FlowResult),
    Restricted(FlowRestricted),
    Lyrics(String, Option<Vec<LyricLine>>, String),
    Message(String),
    Changed,
}

pub struct App {
    data_dir: PathBuf,
    core: Option<Arc<Ilmarfy>>,
    playback: Option<Playback>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    media: Option<MediaControls>,
    media_rx: Receiver<MediaControlEvent>,
    media_tx: Sender<MediaControlEvent>,
    view: View,
    history: Vec<View>,
    // dados em cache (recarregados quando a biblioteca muda)
    lib_version: u64,
    loaded_version: u64,
    tracks_cache: Vec<Track>,
    view_tracks: Vec<Track>,
    view_key: Option<View>,
    artists: Vec<ArtistSummary>,
    albums: Vec<AlbumSummary>,
    playlists: Vec<Playlist>,
    downloaded: HashSet<String>,
    favorites: HashSet<String>,
    // telas
    search: String,
    syncing: bool,
    status_line: String,
    toast: Option<(String, f64)>,
    login_server: String,
    login_device: String,
    login_password: String,
    login_busy: bool,
    login_error: Option<String>,
    flow: Option<FlowResult>,
    flow_loading: bool,
    restricted: Option<FlowRestricted>,
    show_restricted: bool,
    side_panel: SidePanel,
    lyrics: Option<(String, Option<Vec<LyricLine>>, String)>,
    downloading: HashSet<String>,
    new_playlist: Option<String>,
}

#[derive(PartialEq, Clone, Copy)]
enum SidePanel {
    None,
    Lyrics,
    Queue,
}

fn token_file(dir: &std::path::Path) -> PathBuf {
    dir.join("token")
}
fn server_file(dir: &std::path::Path) -> PathBuf {
    dir.join("server")
}

fn read_trim(p: PathBuf) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_private(path: &std::path::Path, content: &str) {
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

fn fmt(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Computador".into())
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        setup_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let (media_tx, media_rx) = channel();
        let server = read_trim(server_file(&data_dir)).unwrap_or_else(|| DEFAULT_SERVER.to_string());
        let token = read_trim(token_file(&data_dir));
        let core = token.and_then(|t| Ilmarfy::open(&data_dir, &server, Some(t)).ok()).map(Arc::new);
        let mut app = Self {
            playback: core.clone().map(|c| Playback::new(c, data_dir.join("cache"))),
            data_dir,
            core,
            tx,
            rx,
            media: None,
            media_rx,
            media_tx,
            view: View::Home,
            history: Vec::new(),
            lib_version: 1,
            loaded_version: 0,
            tracks_cache: Vec::new(),
            view_tracks: Vec::new(),
            view_key: None,
            artists: Vec::new(),
            albums: Vec::new(),
            playlists: Vec::new(),
            downloaded: HashSet::new(),
            favorites: HashSet::new(),
            search: String::new(),
            syncing: false,
            status_line: String::new(),
            toast: None,
            login_server: server,
            login_device: format!("{} (desktop)", hostname()),
            login_password: String::new(),
            login_busy: false,
            login_error: None,
            flow: None,
            flow_loading: false,
            restricted: None,
            show_restricted: false,
            side_panel: SidePanel::None,
            lyrics: None,
            downloading: HashSet::new(),
            new_playlist: None,
        };
        app.setup_media_controls();
        if app.core.is_some() {
            app.sync(&cc.egui_ctx);
        }
        app
    }

    /// Teclas de mídia e "Tocando agora" do sistema (Mac) / MPRIS (Linux)
    fn setup_media_controls(&mut self) {
        let config = souvlaki::PlatformConfig {
            display_name: "Ilmarfy",
            dbus_name: "ilmarfy",
            hwnd: None,
        };
        if let Ok(mut controls) = MediaControls::new(config) {
            let tx = self.media_tx.clone();
            if controls
                .attach(move |e| {
                    let _ = tx.send(e);
                })
                .is_ok()
            {
                self.media = Some(controls);
            }
        }
    }

    fn toast(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time) + 4.0));
    }

    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Ilmarfy) -> Event + Send + 'static) {
        let Some(core) = self.core.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&core));
            ctx.request_repaint();
        });
    }

    fn sync(&mut self, ctx: &egui::Context) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.spawn(ctx, |c| {
            Event::Synced(
                c.sync()
                    .map(|r| format!("{} músicas • {} plays enviados", r.track_count, r.plays_sent))
                    .map_err(|e| {
                        if matches!(e, ilmarfy_core::Error::Unauthorized(_)) {
                            "DESCONECTADO".to_string()
                        } else {
                            e.to_string()
                        }
                    }),
            )
        });
    }

    fn go(&mut self, view: View) {
        if self.view != view {
            self.history.push(self.view.clone());
            self.view = view;
        }
    }

    fn reload(&mut self) {
        let Some(core) = &self.core else { return };
        let lib = core.library();
        self.tracks_cache = lib.tracks(TrackOrder::Title, false).unwrap_or_default();
        self.artists = lib.artists().unwrap_or_default();
        self.playlists = lib.playlists().unwrap_or_default();
        self.downloaded = lib.downloaded_ids().unwrap_or_default().into_iter().collect();
        self.favorites = lib
            .favorites_playlist()
            .ok()
            .flatten()
            .map(|p| p.track_ids.into_iter().collect())
            .unwrap_or_default();
        self.view_key = None;
        self.loaded_version = self.lib_version;
    }

    /// Faixas da tela atual (recalculadas quando a tela ou a biblioteca muda)
    fn tracks_for_view(&mut self) {
        let key = match &self.view {
            View::Search => View::Search,
            v => v.clone(),
        };
        if self.view_key.as_ref() == Some(&key) && key != View::Search {
            return;
        }
        let Some(core) = &self.core else { return };
        let lib = core.library();
        self.view_tracks = match &self.view {
            View::Songs => self.tracks_cache.clone(),
            View::Search => {
                if self.search.trim().is_empty() {
                    Vec::new()
                } else {
                    lib.search(&self.search, 300).unwrap_or_default()
                }
            }
            View::Artist(a) => {
                self.albums = lib.albums(a).unwrap_or_default();
                lib.artist_tracks(a).unwrap_or_default()
            }
            View::Playlist(id) => lib.playlist_tracks(id).unwrap_or_default(),
            View::Favorites => lib
                .favorites_playlist()
                .ok()
                .flatten()
                .map(|p| lib.playlist_tracks(&p.id).unwrap_or_default())
                .unwrap_or_default(),
            View::Offline => self
                .tracks_cache
                .iter()
                .filter(|t| self.downloaded.contains(&t.id))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        self.view_key = Some(key);
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Synced(r) => {
                    self.syncing = false;
                    match r {
                        Ok(s) => {
                            self.status_line = s;
                            self.lib_version += 1;
                        }
                        Err(e) if e == "DESCONECTADO" => {
                            self.logout_local();
                            self.login_error = Some("Este computador foi desconectado. Entre de novo.".into());
                        }
                        Err(e) => self.status_line = format!("Sem sincronizar: {e}"),
                    }
                }
                Event::LoggedIn(r) => {
                    self.login_busy = false;
                    self.login_password.clear();
                    match r {
                        Ok((token, core)) => {
                            write_private(&token_file(&self.data_dir), &token);
                            write_private(&server_file(&self.data_dir), &self.login_server);
                            let core = Arc::new(*core);
                            self.playback = Some(Playback::new(core.clone(), self.data_dir.join("cache")));
                            self.core = Some(core);
                            self.login_error = None;
                            self.sync(ctx);
                        }
                        Err(e) => self.login_error = Some(e),
                    }
                }
                Event::Flow(r) => {
                    self.flow_loading = false;
                    self.flow = Some(r);
                }
                Event::Restricted(r) => self.restricted = Some(r),
                Event::Lyrics(id, lines, plain) => self.lyrics = Some((id, lines, plain)),
                Event::Message(m) => self.toast(ctx, m),
                Event::Changed => self.lib_version += 1,
            }
        }
    }

    fn logout_local(&mut self) {
        if let Some(p) = &mut self.playback {
            p.stop();
        }
        let _ = std::fs::remove_file(token_file(&self.data_dir));
        self.core = None;
        self.playback = None;
    }

    fn handle_media_keys(&mut self) {
        while let Ok(e) = self.media_rx.try_recv() {
            let Some(p) = &mut self.playback else { continue };
            match e {
                MediaControlEvent::Play | MediaControlEvent::Pause | MediaControlEvent::Toggle => p.toggle(),
                MediaControlEvent::Next => p.next(),
                MediaControlEvent::Previous => p.previous(),
                MediaControlEvent::Stop => p.stop(),
                MediaControlEvent::SetPosition(MediaPosition(d)) => p.seek(d),
                MediaControlEvent::SetVolume(v) => p.set_volume(v as f32),
                _ => {}
            }
        }
    }

    fn update_media_info(&mut self, changed: bool) {
        let (Some(media), Some(p)) = (&mut self.media, &self.playback) else {
            return;
        };
        if changed && let Some(item) = p.current() {
            let _ = media.set_metadata(MediaMetadata {
                title: Some(item.title()),
                artist: Some(item.artist()),
                album: Some(item.album()),
                cover_url: Some(item.cover()).filter(|c| !c.is_empty()),
                duration: Some(item.duration()),
            });
        }
        let progress = Some(MediaPosition(p.position()));
        let _ = media.set_playback(match p.status() {
            Status::Playing => MediaPlayback::Playing { progress },
            Status::Paused => MediaPlayback::Paused { progress },
            _ => MediaPlayback::Stopped,
        });
    }

    fn load_lyrics(&mut self, ctx: &egui::Context) {
        let Some(id) = self
            .playback
            .as_ref()
            .and_then(|p| p.current())
            .and_then(|i| i.track_id())
            .map(String::from)
        else {
            self.lyrics = None;
            return;
        };
        if self.lyrics.as_ref().is_some_and(|l| l.0 == id) {
            return;
        }
        self.lyrics = Some((id.clone(), None, "Carregando…".into()));
        self.spawn(ctx, move |c| match c.api().lyrics(&id) {
            Ok(Some(l)) => {
                let lines = l.synced_lines();
                Event::Lyrics(id, (!lines.is_empty()).then_some(lines), l.plain)
            }
            Ok(None) => Event::Lyrics(id, None, "Sem letra para esta música.".into()),
            Err(e) => Event::Lyrics(id, None, e.to_string()),
        });
    }

    fn play_tracks(&mut self, tracks: &[Track], start: usize, shuffle: bool) {
        if let Some(p) = &mut self.playback {
            p.play_items(tracks.iter().cloned().map(Item::Track).collect(), start, shuffle);
        }
    }

    // ------------------------------------------------------------ Telas

    fn login_ui(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.2);
            ui.label(RichText::new("🎧").size(48.0).color(GREEN));
            ui.label(RichText::new("Ilmarfy").size(36.0).strong());
            ui.label(RichText::new("Entre uma vez; o app guarda só um token deste computador.").color(SUBTEXT));
            ui.add_space(20.0);
            ui.set_max_width(380.0);
            ui.label("Servidor");
            ui.add(egui::TextEdit::singleline(&mut self.login_server).desired_width(f32::INFINITY));
            ui.label("Nome deste computador");
            ui.add(egui::TextEdit::singleline(&mut self.login_device).desired_width(f32::INFINITY));
            ui.label("Senha");
            let pw = ui.add(
                egui::TextEdit::singleline(&mut self.login_password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            let enter = pw.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if let Some(e) = &self.login_error {
                ui.colored_label(Color32::from_rgb(255, 107, 107), e);
            }
            ui.add_space(8.0);
            let btn = egui::Button::new(
                RichText::new(if self.login_busy { "Entrando…" } else { "Entrar" })
                    .strong()
                    .color(Color32::BLACK),
            )
            .fill(GREEN)
            .min_size(Vec2::new(380.0, 36.0));
            if (ui.add_enabled(!self.login_busy && !self.login_password.is_empty(), btn).clicked() || enter)
                && !self.login_password.is_empty()
            {
                self.login_busy = true;
                let (server, device, password, dir) = (
                    self.login_server.trim().to_string(),
                    self.login_device.clone(),
                    self.login_password.clone(),
                    self.data_dir.clone(),
                );
                let (tx, ctx) = (self.tx.clone(), ui.ctx().clone());
                std::thread::spawn(move || {
                    let res = Ilmarfy::open(&dir, &server, None)
                        .and_then(|mut c| c.login(&password, &device, std::env::consts::OS).map(|t| (t, Box::new(c))))
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Event::LoggedIn(res));
                    ctx.request_repaint();
                });
            }
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);
        ui.label(RichText::new("🎧 Ilmarfy").size(22.0).strong().color(GREEN));
        ui.add_space(12.0);
        let entries = [
            (View::Home, "🏠  Início"),
            (View::Search, "🔍  Buscar"),
            (View::Songs, "🎵  Músicas"),
            (View::Artists, "👤  Artistas"),
            (View::Playlists, "📃  Playlists"),
            (View::Favorites, "♥  Favoritos"),
            (View::Offline, "⬇  Offline"),
            (View::Flow, "🌊  Flow"),
            (View::Settings, "⚙  Configurações"),
        ];
        for (view, label) in entries {
            let selected = std::mem::discriminant(&self.view) == std::mem::discriminant(&view)
                || matches!(
                    (&self.view, &view),
                    (View::Artist(_), View::Artists) | (View::Playlist(_), View::Playlists)
                );
            // Item do menu alinhado à esquerda, com destaque no selecionado
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
            if selected || resp.hovered() {
                let fill = if selected {
                    Color32::from_rgb(40, 40, 40)
                } else {
                    Color32::from_rgb(30, 30, 30)
                };
                ui.painter().rect_filled(rect, 6.0, fill);
            }
            let color = if selected {
                GREEN
            } else if resp.hovered() {
                Color32::WHITE
            } else {
                SUBTEXT
            };
            ui.painter().text(
                rect.left_center() + Vec2::new(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(15.0),
                color,
            );
            if resp.clicked() {
                self.go(view);
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(&self.status_line).small().color(SUBTEXT));
            if self.syncing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Sincronizando…").small());
                });
            }
        });
    }

    fn header(&mut self, ui: &mut Ui, title: &str, subtitle: &str) -> bool {
        let mut back = false;
        ui.horizontal(|ui| {
            if !self.history.is_empty() && matches!(self.view, View::Artist(_) | View::Playlist(_)) && ui.button("←").clicked() {
                back = true;
            }
            ui.label(RichText::new(title).size(28.0).strong());
        });
        if !subtitle.is_empty() {
            ui.label(RichText::new(subtitle).color(SUBTEXT));
        }
        ui.add_space(6.0);
        back
    }

    fn play_buttons(&mut self, ui: &mut Ui, tracks: &[Track]) {
        ui.horizontal(|ui| {
            let enabled = !tracks.is_empty();
            if ui
                .add_enabled(
                    enabled,
                    egui::Button::new(RichText::new("▶ Tocar").color(Color32::BLACK).strong()).fill(GREEN),
                )
                .clicked()
            {
                self.play_tracks(tracks, 0, false);
            }
            if ui.add_enabled(enabled, egui::Button::new("🔀 Aleatório")).clicked() {
                self.play_tracks(tracks, 0, true);
            }
        });
    }

    /// Lista virtualizada (só desenha as linhas visíveis), com menu de contexto
    fn track_list(&mut self, ui: &mut Ui, tracks: Vec<Track>) {
        let current = self
            .playback
            .as_ref()
            .and_then(|p| p.current())
            .and_then(|i| i.track_id())
            .map(String::from);
        let mut action: Option<(usize, TrackAction)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, tracks.len(), |ui, range| {
                for i in range {
                    let t = &tracks[i];
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 4.0, Color32::from_white_alpha(10));
                    }
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(rect.shrink2(Vec2::new(6.0, 4.0)))
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    cover(&mut child, &t.cover_url, 38.0);
                    child.add_space(8.0);
                    child.vertical(|ui| {
                        ui.set_width((rect.width() - 220.0).max(100.0));
                        let playing = current.as_deref() == Some(t.id.as_str());
                        ui.add(
                            egui::Label::new(RichText::new(&t.title).strong().color(if playing { GREEN } else { Color32::WHITE }))
                                .truncate(),
                        );
                        let dl = if self.downloaded.contains(&t.id) { "⬇ " } else { "" };
                        ui.add(
                            egui::Label::new(RichText::new(format!("{dl}{} • {}", t.artist, t.album)).small().color(SUBTEXT)).truncate(),
                        );
                    });
                    child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(fmt(Duration::from_secs(t.duration.max(0) as u64))).color(SUBTEXT));
                        if self.favorites.contains(&t.id) {
                            ui.label(RichText::new("♥").color(GREEN));
                        }
                    });
                    if resp.double_clicked() || resp.clicked() {
                        action = Some((i, TrackAction::Play));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("▶ Tocar").clicked() {
                            action = Some((i, TrackAction::Play));
                        }
                        if ui.button("⏭ Tocar a seguir").clicked() {
                            action = Some((i, TrackAction::PlayNext));
                        }
                        if ui.button("➕ Adicionar à fila").clicked() {
                            action = Some((i, TrackAction::Enqueue));
                        }
                        let fav = if self.favorites.contains(&t.id) {
                            "♡ Desfavoritar"
                        } else {
                            "♥ Favoritar"
                        };
                        if ui.button(fav).clicked() {
                            action = Some((i, TrackAction::Favorite));
                        }
                        ui.menu_button("📃 Adicionar à playlist", |ui| {
                            for p in &self.playlists {
                                if ui.button(&p.name).clicked() {
                                    action = Some((i, TrackAction::AddTo(p.id.clone(), p.name.clone())));
                                }
                            }
                        });
                        if self.downloaded.contains(&t.id) {
                            if ui.button("🗑 Remover do computador").clicked() {
                                action = Some((i, TrackAction::RemoveDownload));
                            }
                        } else if ui.button("⬇ Baixar para ouvir offline").clicked() {
                            action = Some((i, TrackAction::Download));
                        }
                        if ui.button("👤 Ir para o artista").clicked() {
                            action = Some((i, TrackAction::GoArtist));
                        }
                    });
                }
            });
        if let Some((i, a)) = action {
            self.track_action(ui.ctx(), &tracks, i, a);
        }
    }

    fn track_action(&mut self, ctx: &egui::Context, tracks: &[Track], i: usize, action: TrackAction) {
        let t = tracks[i].clone();
        match action {
            TrackAction::Play => self.play_tracks(tracks, i, false),
            TrackAction::PlayNext => {
                if let Some(p) = &mut self.playback {
                    p.play_next(Item::Track(t));
                }
                self.toast(ctx, "Toca a seguir");
            }
            TrackAction::Enqueue => {
                if let Some(p) = &mut self.playback {
                    p.enqueue(Item::Track(t));
                }
                self.toast(ctx, "Adicionada à fila");
            }
            TrackAction::Favorite => self.spawn(ctx, move |c| match c.toggle_favorite(&t.id) {
                Ok(on) => {
                    let _ = c.library();
                    Event::Message(if on {
                        format!("♥ {} nos favoritos", t.title)
                    } else {
                        format!("{} saiu dos favoritos", t.title)
                    })
                }
                Err(e) => Event::Message(e.to_string()),
            }),
            TrackAction::AddTo(pid, name) => self.spawn(ctx, move |c| match c.add_to_playlist(&pid, &t.id) {
                Ok(_) => Event::Message(format!("Adicionada a \"{name}\"")),
                Err(e) => Event::Message(e.to_string()),
            }),
            TrackAction::Download => {
                self.downloading.insert(t.id.clone());
                self.spawn(ctx, move |c| match c.download(&t.id, |_, _| true) {
                    Ok(_) => Event::Message(format!("⬇ {} disponível offline", t.title)),
                    Err(e) => Event::Message(format!("Falha ao baixar: {e}")),
                });
            }
            TrackAction::RemoveDownload => {
                if let Some(c) = &self.core {
                    let _ = c.remove_download(&t.id);
                }
                self.lib_version += 1;
            }
            TrackAction::GoArtist => self.go(View::Artist(t.artist)),
        }
        // Favoritos, playlists e downloads mudam a biblioteca: recarrega quando a tarefa terminar
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            let _ = tx.send(Event::Changed);
        });
    }

    fn home(&mut self, ui: &mut Ui) {
        let hour = chrono_hour();
        let greet = match hour {
            5..=11 => "Bom dia",
            12..=17 => "Boa tarde",
            _ => "Boa noite",
        };
        self.header(ui, greet, "");
        let all = self.tracks_cache.clone();
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!("🔀 Aleatório: {} músicas", all.len()))
                            .color(Color32::BLACK)
                            .strong(),
                    )
                    .fill(GREEN),
                )
                .clicked()
            {
                self.play_tracks(&all, 0, true);
            }
            if ui.button(if self.syncing { "Sincronizando…" } else { "⟳ Sincronizar" }).clicked() {
                let ctx = ui.ctx().clone();
                self.sync(&ctx);
            }
        });
        let (top, recent) = match &self.core {
            Some(c) => (
                c.library()
                    .tracks(TrackOrder::MostPlayed, false)
                    .unwrap_or_default()
                    .into_iter()
                    .take(12)
                    .collect::<Vec<_>>(),
                c.library()
                    .tracks(TrackOrder::RecentlyAdded, false)
                    .unwrap_or_default()
                    .into_iter()
                    .take(12)
                    .collect::<Vec<_>>(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if !self.playlists.is_empty() {
                section(ui, "Suas playlists");
                let lists = self.playlists.clone();
                ui.horizontal_wrapped(|ui| {
                    for p in lists {
                        let r = ui.add_sized(
                            [150.0, 60.0],
                            egui::Button::new(RichText::new(format!("{}\n{} faixas", p.name, p.track_ids.len()))),
                        );
                        if r.clicked() {
                            self.go(View::Playlist(p.id.clone()));
                        }
                    }
                });
            }
            for (title, list) in [("Mais tocadas", top), ("Adicionadas recentemente", recent)] {
                section(ui, title);
                egui::ScrollArea::horizontal().id_salt(title).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, t) in list.iter().enumerate() {
                            let r = ui
                                .vertical(|ui| {
                                    ui.set_width(140.0);
                                    cover(ui, &t.cover_url, 140.0);
                                    ui.add(egui::Label::new(RichText::new(&t.title).strong()).truncate());
                                    ui.add(egui::Label::new(RichText::new(&t.artist).small().color(SUBTEXT)).truncate());
                                })
                                .response
                                .interact(Sense::click());
                            if r.clicked() {
                                self.play_tracks(&list, i, false);
                            }
                        }
                    });
                });
            }
        });
    }

    fn artists_view(&mut self, ui: &mut Ui) {
        self.header(ui, "Artistas", &format!("{} artistas", self.artists.len()));
        let artists = self.artists.clone();
        let mut go = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, artists.len(), |ui, range| {
                for a in &artists[range] {
                    let r = ui
                        .horizontal(|ui| {
                            ui.set_height(ROW_H - 4.0);
                            cover(ui, &a.cover_url, 38.0);
                            ui.label(RichText::new(&a.name).strong());
                            ui.label(RichText::new(format!("{} faixas", a.track_count)).color(SUBTEXT));
                        })
                        .response
                        .interact(Sense::click());
                    if r.clicked() {
                        go = Some(a.name.clone());
                    }
                }
            });
        if let Some(a) = go {
            self.go(View::Artist(a));
        }
    }

    fn playlists_view(&mut self, ui: &mut Ui) {
        self.header(ui, "Playlists", &format!("{} playlists", self.playlists.len()));
        if ui.button("➕ Nova playlist").clicked() {
            self.new_playlist = Some(String::new());
        }
        ui.add_space(6.0);
        let lists = self.playlists.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for p in lists {
                let offline = p.track_ids.iter().filter(|id| self.downloaded.contains(*id)).count();
                let label = format!(
                    "{}   —   {} faixas{}",
                    p.name,
                    p.track_ids.len(),
                    if offline > 0 {
                        format!(" • {offline} offline")
                    } else {
                        String::new()
                    }
                );
                if ui.add_sized([ui.available_width(), 36.0], egui::Button::new(label)).clicked() {
                    self.go(View::Playlist(p.id.clone()));
                }
            }
        });
    }

    fn playlist_view(&mut self, ui: &mut Ui, id: &str) {
        let Some(p) = self.playlists.iter().find(|p| p.id == id).cloned() else {
            ui.label("Playlist não encontrada.");
            return;
        };
        let tracks = self.view_tracks.clone();
        let offline = tracks.iter().filter(|t| self.downloaded.contains(&t.id)).count();
        if self.header(ui, &p.name, &format!("{} faixas • {offline} offline", tracks.len())) {
            self.view = self.history.pop().unwrap_or(View::Playlists);
            return;
        }
        ui.horizontal(|ui| {
            self.play_buttons(ui, &tracks);
            let key = format!("playlist:{id}");
            if self.downloading.contains(&key) {
                ui.spinner();
                ui.label("Baixando para offline…");
            } else if offline < tracks.len() && ui.button("⬇ Baixar para ouvir offline").clicked() {
                self.downloading.insert(key.clone());
                let (pid, name) = (id.to_string(), p.name.clone());
                self.spawn(ui.ctx(), move |c| match c.download_playlist(&pid, |_, _, _| true) {
                    Ok((n, fails)) => Event::Message(format!(
                        "\"{name}\": {n} faixas baixadas{}",
                        if fails.is_empty() {
                            String::new()
                        } else {
                            format!(", {} falharam", fails.len())
                        }
                    )),
                    Err(e) => Event::Message(format!("Falha ao baixar: {e}")),
                });
            } else if offline == tracks.len() && !tracks.is_empty() {
                ui.label(RichText::new("⬇ Disponível offline").color(GREEN));
            }
        });
        self.track_list(ui, tracks);
    }

    fn artist_view(&mut self, ui: &mut Ui, name: &str) {
        let tracks = self.view_tracks.clone();
        if self.header(ui, name, &format!("{} faixas • {} álbuns", tracks.len(), self.albums.len())) {
            self.view = self.history.pop().unwrap_or(View::Artists);
            return;
        }
        self.play_buttons(ui, &tracks);
        self.track_list(ui, tracks);
    }

    fn simple_list(&mut self, ui: &mut Ui, title: &str) {
        let tracks = self.view_tracks.clone();
        self.header(ui, title, &format!("{} faixas", tracks.len()));
        self.play_buttons(ui, &tracks);
        self.track_list(ui, tracks);
    }

    fn search_view(&mut self, ui: &mut Ui) {
        self.header(ui, "Buscar", "Música, artista ou álbum (ignora acentos; funciona offline)");
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Digite para buscar…")
                .desired_width(f32::INFINITY),
        );
        if r.changed() {
            self.view_key = None;
        }
        ui.add_space(6.0);
        let tracks = self.view_tracks.clone();
        if !self.search.trim().is_empty() && tracks.is_empty() {
            ui.label(RichText::new("Nada encontrado.").color(SUBTEXT));
        }
        self.track_list(ui, tracks);
    }

    fn flow_view(&mut self, ui: &mut Ui) {
        self.header(
            ui,
            "Ilmarfy Flow",
            "Músicas que você ainda não tem, a partir dos seus artistas e de parecidos",
        );
        if self.flow.is_none() && !self.flow_loading {
            self.load_flow(ui.ctx());
        }
        let tracks = match &self.flow {
            Some(Ok((seeds, t))) => {
                ui.label(RichText::new(format!("Baseado em: {}", seeds.join(", "))).color(SUBTEXT));
                t.clone()
            }
            _ => Vec::new(),
        };
        ui.horizontal(|ui| {
            let play = egui::Button::new(RichText::new("▶ Tocar Flow").color(Color32::BLACK).strong()).fill(GREEN);
            if ui.add_enabled(!tracks.is_empty(), play).clicked()
                && let Some(p) = &mut self.playback
            {
                p.play_items(tracks.iter().cloned().map(Item::Flow).collect(), 0, false);
            }
            if ui.button("⟳ Atualizar").clicked() {
                self.load_flow(ui.ctx());
            }
            let n = self
                .restricted
                .as_ref()
                .map(|r| r.blocked_artists.len() + r.disliked_tracks.len())
                .unwrap_or(0);
            if ui.button(format!("🚫 Restritos ({n})")).clicked() {
                self.show_restricted = true;
            }
        });
        if self.flow_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Sintonizando seu Flow…");
            });
            return;
        }
        if let Some(Err(e)) = &self.flow {
            ui.colored_label(Color32::from_rgb(255, 107, 107), e);
        }
        let current = self.playback.as_ref().and_then(|p| p.current_key().map(String::from));
        let mut act: Option<(usize, u8)> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, tracks.len(), |ui, range| {
                for i in range {
                    let t = &tracks[i];
                    ui.horizontal(|ui| {
                        ui.set_height(ROW_H - 4.0);
                        cover(ui, &t.cover, 38.0);
                        let playing = current.as_deref() == Some(&format!("f:{}", t.id));
                        let r = ui
                            .vertical(|ui| {
                                ui.set_width((ui.available_width() - 140.0).max(100.0));
                                ui.add(
                                    egui::Label::new(RichText::new(&t.title).strong().color(if playing { GREEN } else { Color32::WHITE }))
                                        .truncate(),
                                );
                                ui.add(egui::Label::new(RichText::new(&t.artist).small().color(SUBTEXT)).truncate());
                            })
                            .response
                            .interact(Sense::click());
                        if r.clicked() {
                            act = Some((i, 0));
                        }
                        if ui.button("➕").on_hover_text("Adicionar à biblioteca").clicked() {
                            act = Some((i, 1));
                        }
                        if ui.button("👎").on_hover_text("Não curti esta música").clicked() {
                            act = Some((i, 2));
                        }
                        if ui.button("🚫").on_hover_text("Bloquear artista no Flow").clicked() {
                            act = Some((i, 3));
                        }
                    });
                }
            });
        if let Some((i, a)) = act {
            let t = tracks[i].clone();
            match a {
                0 => {
                    if let Some(p) = &mut self.playback {
                        p.play_items(tracks.iter().cloned().map(Item::Flow).collect(), i, false);
                    }
                }
                1 => self.spawn(ui.ctx(), move |c| match c.api().flow_save_to_library(&t) {
                    Ok(_) => Event::Message(format!("\"{}\" vai ser baixada para a biblioteca", t.title)),
                    Err(e) => Event::Message(e.to_string()),
                }),
                _ => {
                    let block = a == 3;
                    if let Some(Ok((_, list))) = &mut self.flow {
                        list.retain(|x| x.id != t.id && !(block && x.artist.eq_ignore_ascii_case(&t.artist)));
                    }
                    let scope = if block { DislikeScope::Artist } else { DislikeScope::Track };
                    self.toast(
                        ui.ctx(),
                        if block {
                            format!("🚫 {} foi para os Restritos", t.artist)
                        } else {
                            format!("👎 \"{}\" foi para os Restritos", t.title)
                        },
                    );
                    self.spawn(ui.ctx(), move |c| match c.api().flow_dislike(&t, scope) {
                        Ok(r) => Event::Restricted(r),
                        Err(e) => Event::Message(e.to_string()),
                    });
                }
            }
        }
    }

    fn load_flow(&mut self, ctx: &egui::Context) {
        self.flow_loading = true;
        self.spawn(ctx, |c| {
            Event::Flow(c.api().flow().map(|f| (f.seeds, f.tracks)).map_err(|e| e.to_string()))
        });
        self.spawn(ctx, |c| match c.api().flow_restricted() {
            Ok(r) => Event::Restricted(r),
            Err(e) => Event::Message(e.to_string()),
        });
    }

    fn restricted_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_restricted;
        let mut restore: Option<(bool, String, String)> = None;
        egui::Window::new("Restritos do Flow")
            .open(&mut open)
            .default_size([420.0, 480.0])
            .show(ctx, |ui| {
                let Some(r) = &self.restricted else {
                    ui.spinner();
                    return;
                };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new(format!("Artistas ({})", r.blocked_artists.len())).strong());
                    for a in &r.blocked_artists {
                        ui.horizontal(|ui| {
                            ui.label(&a.name);
                            if ui.small_button("↩ Restaurar").clicked() {
                                restore = Some((true, a.key.clone(), a.name.clone()));
                            }
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new(format!("Músicas ({})", r.disliked_tracks.len())).strong());
                    for t in &r.disliked_tracks {
                        ui.horizontal(|ui| {
                            ui.label(format!("{} — {}", t.title, t.artist));
                            if ui.small_button("↩ Restaurar").clicked() {
                                restore = Some((false, t.id.clone(), t.title.clone()));
                            }
                        });
                    }
                });
            });
        self.show_restricted = open;
        if let Some((artist, key, label)) = restore {
            self.spawn(ctx, move |c| {
                let res = if artist {
                    c.api().flow_unblock_artist(&key)
                } else {
                    c.api().flow_restore_track(&key)
                };
                match res {
                    Ok(r) => {
                        let _ = label;
                        Event::Restricted(r)
                    }
                    Err(e) => Event::Message(e.to_string()),
                }
            });
        }
    }

    fn settings_view(&mut self, ui: &mut Ui) {
        self.header(ui, "Configurações", "");
        let Some(core) = self.core.clone() else { return };
        egui::Grid::new("settings").num_columns(2).spacing([24.0, 8.0]).show(ui, |ui| {
            ui.label(RichText::new("Servidor").color(SUBTEXT));
            ui.label(core.api().base_url());
            ui.end_row();
            ui.label(RichText::new("Versão").color(SUBTEXT));
            ui.label(env!("CARGO_PKG_VERSION"));
            ui.end_row();
            ui.label(RichText::new("Músicas").color(SUBTEXT));
            ui.label(core.library().track_count().unwrap_or(0).to_string());
            ui.end_row();
            ui.label(RichText::new("Offline").color(SUBTEXT));
            ui.label(format!(
                "{} músicas ({:.1} MB)",
                self.downloaded.len(),
                core.library().downloads_size().unwrap_or(0) as f64 / 1_048_576.0
            ));
            ui.end_row();
            ui.label(RichText::new("Dados").color(SUBTEXT));
            ui.label(self.data_dir.display().to_string());
            ui.end_row();
        });
        ui.add_space(12.0);
        if ui.button("⟳ Sincronizar agora").clicked() {
            let ctx = ui.ctx().clone();
            self.sync(&ctx);
        }
        ui.add_space(24.0);
        if ui
            .add(egui::Button::new(RichText::new("Sair deste computador").color(Color32::WHITE)).fill(Color32::from_rgb(180, 50, 50)))
            .clicked()
        {
            let _ = core.revoke_this_device();
            self.logout_local();
        }
    }

    fn player_bar(&mut self, ui: &mut Ui) {
        let Some(p) = &mut self.playback else { return };
        let Some(item) = p.current().cloned() else {
            ui.centered_and_justified(|ui| ui.label(RichText::new("Escolha uma música para tocar").color(SUBTEXT)));
            return;
        };
        let (pos, dur) = (p.position(), p.duration());
        ui.columns(3, |cols| {
            cols[0].horizontal(|ui| {
                cover(ui, item.cover(), 56.0);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(item.title()).strong()).truncate());
                    let artist = if matches!(item, Item::Flow(_)) {
                        format!("Flow • {}", item.artist())
                    } else {
                        item.artist().to_string()
                    };
                    ui.add(egui::Label::new(RichText::new(artist).small().color(SUBTEXT)).truncate());
                });
            });
            cols[1].vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    let w = 5.0 * 36.0;
                    ui.add_space((ui.available_width() - w) / 2.0);
                    if ui
                        .add(egui::Button::new(RichText::new("🔀").color(if p.shuffle() { GREEN } else { SUBTEXT })).frame(false))
                        .clicked()
                    {
                        p.toggle_shuffle();
                    }
                    if ui.add(egui::Button::new(RichText::new("⏮").size(18.0)).frame(false)).clicked() {
                        p.previous();
                    }
                    let icon = match p.status() {
                        Status::Playing => "⏸",
                        Status::Loading => "⏳",
                        _ => "▶",
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(icon).size(22.0).color(Color32::BLACK))
                                .fill(Color32::WHITE)
                                .corner_radius(18.0)
                                .min_size(Vec2::splat(36.0)),
                        )
                        .clicked()
                    {
                        p.toggle();
                    }
                    if ui.add(egui::Button::new(RichText::new("⏭").size(18.0)).frame(false)).clicked() {
                        p.next();
                    }
                    let rep = match p.repeat() {
                        Repeat::Off => RichText::new("🔁").color(SUBTEXT),
                        Repeat::All => RichText::new("🔁").color(GREEN),
                        Repeat::One => RichText::new("🔂").color(GREEN),
                    };
                    if ui.add(egui::Button::new(rep).frame(false)).clicked() {
                        p.cycle_repeat();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(fmt(pos)).small().color(SUBTEXT));
                    let mut frac = if dur.as_secs_f32() > 0.0 {
                        pos.as_secs_f32() / dur.as_secs_f32()
                    } else {
                        0.0
                    };
                    ui.spacing_mut().slider_width = (ui.available_width() - 50.0).max(60.0);
                    let r = ui.add(egui::Slider::new(&mut frac, 0.0..=1.0).show_value(false));
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        p.seek(dur.mul_f32(frac));
                    }
                    ui.label(RichText::new(fmt(dur)).small().color(SUBTEXT));
                });
            });
            cols[2].with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut vol = p.volume();
                ui.spacing_mut().slider_width = 90.0;
                if ui.add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false)).changed() {
                    p.set_volume(vol);
                }
                ui.label("🔊");
                for (panel, label) in [(SidePanel::Queue, "☰ Fila"), (SidePanel::Lyrics, "🎤 Letra")] {
                    if ui.selectable_label(self.side_panel == panel, label).clicked() {
                        self.side_panel = if self.side_panel == panel { SidePanel::None } else { panel };
                    }
                }
            });
        });
    }

    fn side_panel_ui(&mut self, ui: &mut Ui) {
        match self.side_panel {
            SidePanel::Lyrics => {
                ui.heading("Letra");
                let pos = self.playback.as_ref().map(|p| p.position()).unwrap_or_default();
                match &self.lyrics {
                    Some((_, Some(lines), _)) => {
                        let current = lines.iter().rposition(|l| l.time_ms <= pos.as_millis() as u64);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            for (i, l) in lines.iter().enumerate() {
                                let active = Some(i) == current;
                                let r = ui.label(
                                    RichText::new(if l.text.is_empty() { "♪" } else { &l.text })
                                        .size(17.0)
                                        .color(if active { GREEN } else { SUBTEXT })
                                        .strong(),
                                );
                                if active {
                                    r.scroll_to_me(Some(Align::Center));
                                }
                            }
                        });
                    }
                    Some((_, None, plain)) => {
                        egui::ScrollArea::vertical().show(ui, |ui| ui.label(plain.as_str()));
                    }
                    None => {
                        ui.label(RichText::new("Sem letra (prévia do Flow ou nada tocando).").color(SUBTEXT));
                    }
                }
            }
            SidePanel::Queue => {
                ui.heading("A seguir");
                let Some(p) = &mut self.playback else { return };
                let up = p.upcoming();
                let mut jump = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (i, it) in up.iter().enumerate() {
                        let text =
                            RichText::new(format!("{} — {}", it.title(), it.artist())).color(if i == 0 { GREEN } else { Color32::WHITE });
                        if ui.add(egui::Label::new(text).truncate().sense(Sense::click())).clicked() {
                            jump = Some(it.key());
                        }
                    }
                });
                if let Some(k) = jump {
                    p.jump_to(&k);
                }
            }
            SidePanel::None => {}
        }
    }
}

enum TrackAction {
    Play,
    PlayNext,
    Enqueue,
    Favorite,
    AddTo(String, String),
    Download,
    RemoveDownload,
    GoArtist,
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title).size(19.0).strong());
    ui.add_space(4.0);
}

fn cover(ui: &mut Ui, url: &str, size: f32) {
    if url.is_empty() {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        ui.painter().rect_filled(rect, 4.0, Color32::from_gray(40));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "♪",
            egui::FontId::proportional(size * 0.4),
            SUBTEXT,
        );
    } else {
        ui.add(egui::Image::new(url).fit_to_exact_size(Vec2::splat(size)).corner_radius(4.0));
    }
}

fn chrono_hour() -> u32 {
    // Hora local sem dependências: usa o `date` do sistema
    std::process::Command::new("date")
        .arg("+%H")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(12)
}

fn setup_style(ctx: &egui::Context) {
    // Ícones/emoji (🎧 🔀 ⏮ …) e, para títulos em outros alfabetos (japonês etc.), as fontes do sistema
    use egui::epaint::text::{FontData, FontInsert, FontPriority, InsertFontFamily};
    for (name, data) in [
        ("emoji-icons", epaint_default_fonts::EMOJI_ICON),
        ("noto-emoji", epaint_default_fonts::NOTO_EMOJI_REGULAR),
    ] {
        ctx.add_font(FontInsert {
            name: name.into(),
            data: FontData::from_static(data),
            families: [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
                .into_iter()
                .map(|family| InsertFontFamily {
                    family,
                    priority: FontPriority::Lowest,
                })
                .collect(),
        });
    }
    ctx.add_font_provider(Arc::new(egui_system_font_provider::SystemFontProvider::new()));
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|s| {
        s.visuals.panel_fill = Color32::from_rgb(18, 18, 18);
        s.visuals.window_fill = Color32::from_rgb(24, 24, 24);
        s.visuals.selection.bg_fill = Color32::from_rgb(0x1D, 0x6B, 0x3A);
        s.visuals.hyperlink_color = GREEN;
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
    });
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.handle_media_keys();
        if self.core.is_some() && self.loaded_version != self.lib_version {
            self.reload();
        }
        let changed = self.playback.as_mut().map(|p| p.tick()).unwrap_or(false);
        // Carrega a letra quando a faixa muda ou ao abrir o painel de letra
        if changed || (self.side_panel == SidePanel::Lyrics && self.lyrics.is_none()) {
            self.load_lyrics(ctx);
        }
        self.update_media_info(changed);
        if let Some(msg) = self.playback.as_mut().and_then(|p| p.message.take()) {
            self.toast(ctx, msg);
        }
        // Redesenha só o necessário: rápido enquanto toca, devagar quando parado
        let playing = self
            .playback
            .as_ref()
            .is_some_and(|p| matches!(p.status(), Status::Playing | Status::Loading));
        ctx.request_repaint_after(if playing {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(2)
        });
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.core.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.login_ui(ui));
            return;
        }
        egui::Panel::bottom("player").exact_size(92.0).show(ui, |ui| {
            ui.add_space(8.0);
            self.player_bar(ui);
        });
        egui::Panel::left("sidebar")
            .exact_size(200.0)
            .resizable(false)
            .show(ui, |ui| self.sidebar(ui));
        if self.side_panel != SidePanel::None {
            egui::Panel::right("side")
                .exact_size(340.0)
                .resizable(false)
                .show(ui, |ui| self.side_panel_ui(ui));
        }
        self.tracks_for_view();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            match self.view.clone() {
                View::Home => self.home(ui),
                View::Search => self.search_view(ui),
                View::Songs => self.simple_list(ui, "Músicas"),
                View::Artists => self.artists_view(ui),
                View::Artist(a) => self.artist_view(ui, &a),
                View::Playlists => self.playlists_view(ui),
                View::Playlist(id) => self.playlist_view(ui, &id),
                View::Favorites => self.simple_list(ui, "Favoritos"),
                View::Offline => self.simple_list(ui, "Offline"),
                View::Flow => self.flow_view(ui),
                View::Settings => self.settings_view(ui),
            }
        });
        if self.show_restricted {
            self.restricted_window(&ctx);
        }
        if let Some(name) = &mut self.new_playlist {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("Nova playlist").collapsible(false).show(&ctx, |ui| {
                ui.text_edit_singleline(name);
                ui.horizontal(|ui| {
                    create = ui.button("Criar").clicked() && !name.trim().is_empty();
                    cancel = ui.button("Cancelar").clicked();
                });
            });
            if create {
                let n = name.trim().to_string();
                self.new_playlist = None;
                self.spawn(&ctx, move |c| match c.create_playlist(&n) {
                    Ok(_) => Event::Changed,
                    Err(e) => Event::Message(e.to_string()),
                });
            } else if cancel {
                self.new_playlist = None;
            }
        }
        if let Some((msg, until)) = &self.toast {
            if ctx.input(|i| i.time) > *until {
                self.toast = None;
            } else {
                egui::Area::new(egui::Id::unique("toast"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -110.0])
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg));
                    });
            }
        }
    }

    fn on_exit(&mut self) {
        // Envia plays pendentes ao fechar (sem travar se estiver offline)
        if let Some(c) = &self.core {
            let _ = c.flush_plays();
        }
    }
}

#[cfg(test)]
mod tests {
    //! Janela renderizada fora da tela com dados reais (só leitura; volume zerado):
    //! `ILMARFY_TOKEN=… SNAP_DIR=/tmp/x cargo test -p ilmarfy-desktop -- --ignored --nocapture`
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use std::time::Instant;

    fn snap(h: &mut Harness<'_, App>, name: &str) {
        let dir = std::env::var("SNAP_DIR").unwrap_or_else(|_| std::env::temp_dir().display().to_string());
        h.render().expect("render").save(format!("{dir}/{name}.png")).unwrap();
    }

    fn wait(h: &mut Harness<'_, App>, secs: u64, done: impl Fn(&App) -> bool) {
        let start = Instant::now();
        while !done(h.state()) && start.elapsed().as_secs() < secs {
            h.step();
            std::thread::sleep(Duration::from_millis(100));
        }
        for _ in 0..15 {
            h.step();
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    #[test]
    #[ignore]
    fn telas_com_dados_reais() {
        let token = std::env::var("ILMARFY_TOKEN").expect("ILMARFY_TOKEN");
        let dir = std::env::temp_dir().join(format!("ilmarfy-desktop-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_private(&token_file(&dir), &token);
        let mut h = Harness::builder()
            .with_size([1200.0, 760.0])
            .build_eframe(|cc| App::new(cc, dir.clone()));
        wait(&mut h, 60, |a| !a.syncing && !a.tracks_cache.is_empty());
        assert!(h.state().tracks_cache.len() > 100, "biblioteca deveria ter sincronizado");
        snap(&mut h, "d1-inicio");

        h.get_by_label("🎵  Músicas").click();
        wait(&mut h, 2, |_| false);
        snap(&mut h, "d2-musicas");

        // Toca a primeira faixa com volume zero e confere que começou
        {
            let tracks = h.state().view_tracks.clone();
            let app = h.state_mut();
            if let Some(p) = &mut app.playback {
                p.set_volume(0.0);
            }
            app.play_tracks(&tracks, 0, false);
        }
        wait(&mut h, 60, |a| a.playback.as_ref().is_some_and(|p| *p.status() == Status::Playing));
        // Sem placa de som (ambiente de teste) o player não toca; com placa, tem que estar tocando
        if h.state().playback.as_ref().is_some_and(|p| p.has_output()) {
            assert_eq!(h.state().playback.as_ref().map(|p| p.status().clone()), Some(Status::Playing));
        }
        h.state_mut().side_panel = SidePanel::Lyrics;
        wait(&mut h, 3, |_| false);
        snap(&mut h, "d3-tocando");

        let first = h.state().playlists.first().map(|p| p.id.clone());
        if let Some(id) = first {
            h.state_mut().go(View::Playlist(id));
            wait(&mut h, 2, |_| false);
            snap(&mut h, "d4-playlist");
        }

        h.state_mut().go(View::Flow);
        wait(&mut h, 40, |a| a.flow.is_some());
        snap(&mut h, "d5-flow");
        if let Some(p) = &mut h.state_mut().playback {
            p.stop();
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
