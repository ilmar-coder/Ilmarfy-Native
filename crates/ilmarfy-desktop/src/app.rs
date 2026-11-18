//! Interface do Ilmarfy para desktop (egui), no estilo Spotify.

use crate::playback::{Item, Playback, Status};
use crate::ui::{self, BG, BLACK, CARD, DIM, GREEN, HOVER, SUB, TEXT, TILE};
use crate::updater;
use eframe::egui::{self, Align, Align2, Color32, Layout, Rect, RichText, Sense, Ui, Vec2};
use ilmarfy_core::{
    AlbumSummary, ArtistSummary, DEFAULT_SERVER, DislikeScope, FlowRestricted, FlowTrack, Ilmarfy, LyricLine, Playlist, Repeat, Track,
    TrackOrder,
};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

#[path = "admin.rs"]
mod admin;

const ROW_H: f32 = 56.0;

/// Item da barra lateral sendo arrastado (para mudar a ordem)
#[derive(Clone)]
struct DragSide {
    key: String,
}

enum SideCover {
    Special(Color32, Color32, &'static str),
    Mosaic(Vec<String>, String),
}

/// Um item de "Sua Biblioteca"
struct SideEntry {
    /// Chave estável para lembrar a ordem ("fav", "pl:<id>", …)
    key: String,
    label: String,
    subtitle: String,
    view: View,
    cover: SideCover,
    /// Aceita músicas arrastadas? None = não; Some(None) = curtidas; Some(Some((id, nome))) = playlist
    drop: Option<Option<(String, String)>>,
}

/// Música sendo arrastada (para soltar numa playlist da barra lateral ou reordenar)
#[derive(Clone)]
struct DragTrack {
    id: String,
    title: String,
}

#[derive(Clone, Debug, PartialEq, Hash)]
enum View {
    Home,
    Search,
    Songs,
    Artists,
    Artist(String),
    Playlist(String),
    Favorites,
    Offline,
    Restricted,
    Ranking,
    Add,
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
    UpdateChecked(Result<Option<updater::Release>, String>, bool),
    UpdateInstalled(Result<std::process::Command, String>),
    Admin(admin::AdminEvent),
}

#[derive(PartialEq, Clone, Copy)]
enum SidePanel {
    None,
    Lyrics,
    Queue,
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
    Restrict(bool),
    Edit,
    Fix,
    Delete,
    /// Mover na playlist: (playlist, faixa arrastada, antes de qual)
    MoveTo(String, String, Option<String>),
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
    covers: HashMap<String, String>,
    top: Vec<Track>,
    recent: Vec<Track>,
    view_tracks: Vec<Track>,
    view_key: Option<View>,
    artists: Vec<ArtistSummary>,
    albums: Vec<AlbumSummary>,
    playlists: Vec<Playlist>,
    downloaded: HashSet<String>,
    favorites: HashSet<String>,
    favorites_id: Option<String>,
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
    update: Option<updater::Release>,
    restricted_count: usize,
    admin: admin::AdminState,
    /// Ordem escolhida para "Sua Biblioteca" (guardada neste computador)
    sidebar_order: Vec<String>,
    update_busy: bool,
    update_hidden: bool,
    update_status: String,
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
    ui::fmt_duration(d.as_secs())
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Computador".into())
}

fn local_hour() -> u32 {
    std::process::Command::new("date")
        .arg("+%H")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(12)
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        ui::setup_style(&cc.egui_ctx);
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
            covers: HashMap::new(),
            top: Vec::new(),
            recent: Vec::new(),
            view_tracks: Vec::new(),
            view_key: None,
            artists: Vec::new(),
            albums: Vec::new(),
            playlists: Vec::new(),
            downloaded: HashSet::new(),
            favorites: HashSet::new(),
            favorites_id: None,
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
            update: None,
            restricted_count: 0,
            admin: Default::default(),
            sidebar_order: Vec::new(),
            update_busy: false,
            update_hidden: false,
            update_status: String::new(),
        };
        app.sidebar_order = std::fs::read(app.data_dir.join("sidebar_order.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        app.setup_media_controls();
        #[cfg(not(test))]
        app.check_update(&cc.egui_ctx, false);
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

    fn check_update(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.update_status = "Procurando atualizações…".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::UpdateChecked(updater::check(), manual));
            ctx.request_repaint();
        });
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        let Some(rel) = self.update.clone() else { return };
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.update_status = format!("Baixando e instalando a versão {}…", rel.version);
        let (tx, ctx, dir) = (self.tx.clone(), ctx.clone(), self.data_dir.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::UpdateInstalled(updater::install(&rel, &dir)));
            ctx.request_repaint();
        });
    }

    /// Envia plays pendentes esperando no máximo 2 s (offline/rede lenta)
    fn flush_plays_quick(&self) {
        if let Some(c) = self.core.clone() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = c.flush_plays();
                let _ = tx.send(());
            });
            let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
        }
    }

    fn update_window(&mut self, ctx: &egui::Context) {
        let Some(rel) = self.update.clone() else { return };
        if self.update_hidden {
            return;
        }
        let (mut now, mut later) = (false, false);
        egui::Window::new("Atualização disponível")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(format!("Ilmarfy {} → {}", env!("CARGO_PKG_VERSION"), rel.version)).font(ui::font_bold(16.0)));
                if !rel.notes.is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                        ui.label(RichText::new(&rel.notes).color(SUB));
                    });
                }
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{:.1} MB", rel.size as f64 / 1_048_576.0)).color(DIM));
                ui.add_space(10.0);
                if self.update_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(&self.update_status);
                    });
                } else {
                    ui.horizontal(|ui| {
                        now = ui::pill(ui, "Atualizar agora", GREEN, Color32::BLACK).clicked();
                        later = ui::pill(ui, "Depois", TILE, TEXT).clicked();
                    });
                    if cfg!(target_os = "linux") {
                        ui.label(RichText::new("O sistema vai pedir sua senha para instalar.").color(DIM));
                    }
                }
            });
        if now {
            self.install_update(ctx);
        } else if later {
            self.update_hidden = true;
        }
    }

    fn spawn(&self, ctx: &egui::Context, job: impl FnOnce(&Ilmarfy) -> Event + Send + 'static) {
        let Some(core) = self.core.clone() else { return };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&core));
            let _ = tx.send(Event::Changed);
            ctx.request_repaint();
        });
    }

    fn sync(&mut self, ctx: &egui::Context) {
        if self.syncing {
            return;
        }
        self.syncing = true;
        self.spawn(ctx, |c| {
            Event::Synced(c.sync().map(|r| format!("{} músicas", r.track_count)).map_err(|e| {
                if matches!(e, ilmarfy_core::Error::Unauthorized(_)) {
                    "DESCONECTADO".to_string()
                } else {
                    e.to_string()
                }
            }))
        });
    }

    fn go(&mut self, view: View) {
        if self.view != view {
            self.history.push(self.view.clone());
            self.view = view;
        }
    }

    fn back(&mut self) {
        if let Some(v) = self.history.pop() {
            self.view = v;
        }
    }

    fn reload(&mut self) {
        let Some(core) = &self.core else { return };
        let lib = core.library();
        self.tracks_cache = lib.tracks(TrackOrder::Title, false).unwrap_or_default();
        self.restricted_count = lib.restricted_tracks().map(|t| t.len()).unwrap_or(0);
        self.covers = lib
            .tracks(TrackOrder::Title, true)
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id, t.cover_url))
            .collect();
        self.top = lib
            .tracks(TrackOrder::MostPlayed, false)
            .unwrap_or_default()
            .into_iter()
            .take(14)
            .collect();
        self.recent = lib
            .tracks(TrackOrder::RecentlyAdded, false)
            .unwrap_or_default()
            .into_iter()
            .take(14)
            .collect();
        self.artists = lib.artists().unwrap_or_default();
        let fav = lib.favorites_playlist().ok().flatten();
        self.favorites_id = fav.as_ref().map(|p| p.id.clone());
        self.favorites = fav.map(|p| p.track_ids.into_iter().collect()).unwrap_or_default();
        // A playlist de favoritos aparece como "Músicas curtidas", não na lista de playlists
        self.playlists = lib
            .playlists()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| Some(&p.id) != self.favorites_id.as_ref())
            .collect();
        self.downloaded = lib.downloaded_ids().unwrap_or_default().into_iter().collect();
        self.view_key = None;
        self.loaded_version = self.lib_version;
    }

    fn playlist_covers(&self, p: &Playlist) -> Vec<String> {
        p.track_ids
            .iter()
            .filter_map(|id| self.covers.get(id))
            .filter(|c| !c.is_empty())
            .take(12)
            .cloned()
            .collect()
    }

    /// Faixas da tela atual (recalculadas quando a tela ou a biblioteca muda)
    fn tracks_for_view(&mut self) {
        if self.view_key.as_ref() == Some(&self.view) && self.view != View::Search {
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
            View::Favorites => self
                .favorites_id
                .as_ref()
                .map(|id| lib.playlist_tracks(id).unwrap_or_default())
                .unwrap_or_default(),
            View::Restricted => lib.restricted_tracks().unwrap_or_default(),
            View::Ranking => lib
                .tracks(TrackOrder::MostPlayed, false)
                .unwrap_or_default()
                .into_iter()
                .filter(|t| t.play_count > 0)
                .take(100)
                .collect(),
            View::Offline => self
                .tracks_cache
                .iter()
                .filter(|t| self.downloaded.contains(&t.id))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        self.view_key = Some(self.view.clone());
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Synced(r) => {
                    self.syncing = false;
                    match r {
                        Ok(s) => self.status_line = s,
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
                Event::UpdateChecked(r, manual) => {
                    self.update_busy = false;
                    match r {
                        Ok(Some(rel)) => {
                            self.update_status = format!("Versão {} disponível", rel.version);
                            self.update_hidden = false;
                            self.update = Some(rel);
                        }
                        Ok(None) => {
                            self.update_status = "Você está na versão mais recente".into();
                            if manual {
                                self.toast(ctx, "Você está na versão mais recente");
                            }
                        }
                        Err(e) => {
                            self.update_status = e.clone();
                            if manual {
                                self.toast(ctx, e);
                            }
                        }
                    }
                }
                Event::Admin(a) => self.admin_event(ctx, a),
                Event::UpdateInstalled(r) => {
                    self.update_busy = false;
                    match r {
                        Ok(mut relaunch) => {
                            self.flush_plays_quick();
                            let _ = relaunch.spawn();
                            std::process::exit(0);
                        }
                        Err(e) => {
                            self.update_status = e.clone();
                            self.toast(ctx, e);
                        }
                    }
                }
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

    fn current_track_id(&self) -> Option<String> {
        self.playback
            .as_ref()
            .and_then(|p| p.current())
            .and_then(|i| i.track_id())
            .map(String::from)
    }

    fn is_playing(&self) -> bool {
        self.playback.as_ref().is_some_and(|p| *p.status() == Status::Playing)
    }

    // ------------------------------------------------------------ Login

    fn login_ui(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.18).max(20.0));
            ui.label(RichText::new("🎧").size(56.0).color(GREEN));
            ui.label(RichText::new("Ilmarfy").font(ui::font_bold(40.0)));
            ui.label(RichText::new("Entre uma vez; o app guarda só um token deste computador.").color(SUB));
            ui.add_space(24.0);
            ui.set_max_width(380.0);
            for (label, value, password) in [("Servidor", 0, false), ("Nome deste computador", 1, false), ("Senha", 2, true)] {
                ui.label(RichText::new(label).font(ui::font_bold(13.0)));
                let field = match value {
                    0 => &mut self.login_server,
                    1 => &mut self.login_device,
                    _ => &mut self.login_password,
                };
                let r = ui.add(
                    egui::TextEdit::singleline(field)
                        .password(password)
                        .desired_width(f32::INFINITY)
                        .margin(Vec2::new(10.0, 8.0)),
                );
                if password && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.start_login(ui.ctx());
                }
                ui.add_space(6.0);
            }
            if let Some(e) = &self.login_error {
                ui.colored_label(Color32::from_rgb(255, 107, 107), e);
            }
            ui.add_space(10.0);
            let label = if self.login_busy { "Entrando…" } else { "Entrar" };
            if ui::pill(ui, label, GREEN, Color32::BLACK).clicked() {
                self.start_login(ui.ctx());
            }
        });
    }

    fn start_login(&mut self, ctx: &egui::Context) {
        if self.login_busy || self.login_password.is_empty() {
            return;
        }
        self.login_busy = true;
        let (server, device, password, dir) = (
            self.login_server.trim().to_string(),
            self.login_device.clone(),
            self.login_password.clone(),
            self.data_dir.clone(),
        );
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let res = Ilmarfy::open(&dir, &server, None)
                .and_then(|mut c| c.login(&password, &device, std::env::consts::OS).map(|t| (t, Box::new(c))))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::LoggedIn(res));
            ctx.request_repaint();
        });
    }

    // ------------------------------------------------------------ Barra lateral

    fn nav_item(&mut self, ui: &mut Ui, icon: &str, label: &str, view: View) {
        let selected = self.view == view;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
        let color = if selected || resp.hovered() { TEXT } else { SUB };
        ui.painter().text(
            rect.left_center() + Vec2::new(14.0, 0.0),
            Align2::LEFT_CENTER,
            icon,
            ui::font(20.0),
            color,
        );
        ui.painter().text(
            rect.left_center() + Vec2::new(50.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            ui::font_bold(15.0),
            color,
        );
        if resp.clicked() {
            self.go(view);
        }
    }

    /// Item da biblioteca: capa pequena + nome + descrição
    fn library_item(&mut self, ui: &mut Ui, label: &str, subtitle: &str, view: View, cover: impl FnOnce(&mut Ui, Rect)) -> egui::Response {
        let selected = self.view == view;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 60.0), Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
        if selected || resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, if selected { HOVER } else { CARD });
        }
        let cover_rect = Rect::from_min_size(rect.min + Vec2::new(8.0, 6.0), Vec2::splat(48.0));
        cover(ui, cover_rect);
        let x = cover_rect.right() + 12.0;
        let w = rect.right() - x - 8.0;
        let title_color = if selected { GREEN } else { TEXT };
        paint_truncated(ui, egui::pos2(x, rect.center().y - 9.0), label, ui::font_bold(15.0), title_color, w);
        paint_truncated(ui, egui::pos2(x, rect.center().y + 11.0), subtitle, ui::font(13.0), SUB, w);
        if resp.clicked() {
            self.go(view);
        }
        resp
    }

    fn sidebar_entries(&self) -> Vec<SideEntry> {
        let special = |key: &str, label: &str, subtitle: String, view: View, top, bottom, sym| SideEntry {
            key: key.into(),
            label: label.into(),
            subtitle,
            view,
            cover: SideCover::Special(top, bottom, sym),
            drop: None,
        };
        let mut items = vec![
            SideEntry {
                drop: Some(None),
                ..special(
                    "fav",
                    "Músicas curtidas",
                    format!("Playlist • {} músicas", self.favorites.len()),
                    View::Favorites,
                    Color32::from_rgb(0x45, 0x0A, 0xF5),
                    Color32::from_rgb(0xC4, 0xEF, 0xD9),
                    "♥",
                )
            },
            special(
                "flow",
                "Ilmarfy Flow",
                "Descobertas para você".into(),
                View::Flow,
                Color32::from_rgb(0x1D, 0xB9, 0x54),
                Color32::from_rgb(0x0B, 0x4D, 0x7A),
                "🌊",
            ),
            special(
                "offline",
                "Offline",
                format!("{} músicas no computador", self.downloaded.len()),
                View::Offline,
                Color32::from_rgb(0x0E, 0x7C, 0x4F),
                Color32::from_rgb(0x06, 0x2E, 0x1E),
                "⬇",
            ),
            special(
                "ranking",
                "Mais tocadas",
                "Ranking • top 100".into(),
                View::Ranking,
                Color32::from_rgb(0xE9, 0x5F, 0x2A),
                Color32::from_rgb(0x5A, 0x14, 0x3C),
                "🏆",
            ),
            special(
                "restricted",
                "Restritas",
                format!("{} músicas guardadas", self.restricted_count),
                View::Restricted,
                Color32::from_rgb(0x8A, 0x1C, 0x2B),
                Color32::from_rgb(0x2A, 0x0A, 0x10),
                "🚫",
            ),
        ];
        for p in &self.playlists {
            items.push(SideEntry {
                key: format!("pl:{}", p.id),
                label: p.name.clone(),
                subtitle: format!("Playlist • {} músicas", p.track_ids.len()),
                view: View::Playlist(p.id.clone()),
                cover: SideCover::Mosaic(self.playlist_covers(p), p.name.clone()),
                drop: Some(Some((p.id.clone(), p.name.clone()))),
            });
        }
        items.push(special(
            "artists",
            "Artistas",
            format!("{} artistas", self.artists.len()),
            View::Artists,
            Color32::from_rgb(0x8D, 0x67, 0xAB),
            Color32::from_rgb(0x2E, 0x1F, 0x3B),
            "👤",
        ));
        items.push(special(
            "songs",
            "Todas as músicas",
            format!("{} músicas", self.tracks_cache.len()),
            View::Songs,
            Color32::from_rgb(0xE9, 0x14, 0x29),
            Color32::from_rgb(0x4A, 0x06, 0x0D),
            "♪",
        ));
        // Ordem escolhida pelo usuário; itens novos (ex.: playlist nova) ficam no fim
        let rank = |k: &str| self.sidebar_order.iter().position(|o| o == k).unwrap_or(usize::MAX);
        items.sort_by_key(|e| rank(&e.key));
        items
    }

    fn sidebar_items(&mut self, ui: &mut Ui) {
        let entries = self.sidebar_entries();
        let keys: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
        let mut new_order: Option<Vec<String>> = None;
        for e in entries {
            let cover = e.cover;
            let resp = self.library_item(ui, &e.label, &e.subtitle, e.view, |ui, r| match &cover {
                SideCover::Special(top, bottom, sym) => paint_special(ui, r, *top, *bottom, sym),
                SideCover::Mosaic(covers, seed) => ui::mosaic_at(ui, r, covers, seed, 4.0),
            });
            if let Some(target) = e.drop {
                self.accept_track_drop(ui, &resp, target);
            }
            resp.dnd_set_drag_payload(DragSide { key: e.key.clone() });
            if resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                drag_label(ui.ctx(), &e.label);
            }
            if let Some(pos) = ui.ctx().pointer_hover_pos()
                && resp.dnd_hover_payload::<DragSide>().is_some_and(|d| d.key != e.key)
            {
                let upper = pos.y < resp.rect.center().y;
                let y = if upper { resp.rect.top() } else { resp.rect.bottom() };
                ui.painter()
                    .hline(resp.rect.left() + 8.0..=resp.rect.right() - 8.0, y, egui::Stroke::new(2.0, GREEN));
                if let Some(d) = resp.dnd_release_payload::<DragSide>() {
                    new_order = Some(reordered(&keys, &d.key, &e.key, upper));
                }
            }
        }
        if let Some(order) = new_order {
            if let Ok(b) = serde_json::to_vec(&order) {
                let _ = std::fs::write(self.data_dir.join("sidebar_order.json"), b);
            }
            self.sidebar_order = order;
        }
    }

    /// Aceita uma música arrastada sobre um item da biblioteca (playlist ou curtidas)
    fn accept_track_drop(&mut self, ui: &Ui, resp: &egui::Response, playlist: Option<(String, String)>) {
        // Só olha músicas: pegar o pacote de outro tipo (ex.: item da barra) faria o egui descartá-lo
        if resp.dnd_hover_payload::<DragTrack>().is_none() {
            return;
        }
        ui.painter()
            .rect_stroke(resp.rect, 6.0, egui::Stroke::new(2.0, GREEN), egui::StrokeKind::Inside);
        let Some(drag) = resp.dnd_release_payload::<DragTrack>() else {
            return;
        };
        let ctx = ui.ctx().clone();
        let drag = (*drag).clone();
        match playlist {
            Some((pid, name)) => self.spawn(&ctx, move |c| match c.add_to_playlist(&pid, &drag.id) {
                Ok(_) => Event::Message(format!("\"{}\" adicionada a \"{name}\"", drag.title)),
                Err(e) => Event::Message(e.to_string()),
            }),
            None => {
                if self.favorites.contains(&drag.id) {
                    self.toast(&ctx, format!("\"{}\" já está nas curtidas", drag.title));
                } else {
                    self.spawn(&ctx, move |c| match c.toggle_favorite(&drag.id) {
                        Ok(_) => Event::Message(format!("♥ \"{}\" nas curtidas", drag.title)),
                        Err(e) => Event::Message(e.to_string()),
                    });
                }
            }
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        // Bloco de cima: logo e navegação
        egui::Frame::new()
            .fill(BG)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(8, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("🎧 Ilmarfy").font(ui::font_bold(22.0)).color(GREEN));
                });
                ui.add_space(8.0);
                self.nav_item(ui, "🏠", "Início", View::Home);
                self.nav_item(ui, "🔍", "Buscar", View::Search);
                let n = self.active_downloads();
                let label = if n > 0 {
                    format!("Adicionar músicas ({n} ⬇)")
                } else {
                    "Adicionar músicas".to_string()
                };
                self.nav_item(ui, "➕", &label, View::Add);
            });
        ui.add_space(8.0);
        // Bloco de baixo: sua biblioteca
        egui::Frame::new()
            .fill(BG)
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(8, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("📚  Sua Biblioteca").font(ui::font_bold(15.0)).color(SUB));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::icon_button(ui, "⚙", 16.0, self.view == View::Settings, "Configurações").clicked() {
                            self.go(View::Settings);
                        }
                        if ui::icon_button(ui, "+", 16.0, false, "Nova playlist").clicked() {
                            self.new_playlist = Some(String::new());
                        }
                    });
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.sidebar_items(ui);
                    ui.add_space(8.0);
                    let status = if self.syncing {
                        "Sincronizando…".to_string()
                    } else {
                        self.status_line.clone()
                    };
                    ui.label(RichText::new(status).small().color(DIM));
                });
            });
    }

    // ------------------------------------------------------------ Telas

    fn page_header(&mut self, ui: &mut Ui, eyebrow: &str, title: &str, subtitle: &str, color: Color32, cover: impl FnOnce(&mut Ui)) {
        // Faixa com gradiente da cor da página, como no Spotify
        let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.max_rect().width(), 260.0));
        paint_gradient(ui, rect, color.gamma_multiply(0.75), BG);
        if !self.history.is_empty() {
            let back = Rect::from_min_size(rect.min + Vec2::new(16.0, 14.0), Vec2::splat(32.0));
            let resp = ui.interact(back, ui.scope_id().with("back"), Sense::click());
            ui.painter().circle_filled(back.center(), 16.0, Color32::from_black_alpha(160));
            ui.painter()
                .text(back.center(), Align2::CENTER_CENTER, "‹", ui::font_bold(22.0), TEXT);
            if resp.on_hover_text("Voltar").clicked() {
                self.back();
            }
        }
        ui.add_space(64.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            cover(ui);
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.add_space(70.0);
                ui.label(RichText::new(eyebrow).font(ui::font_bold(13.0)));
                // Maior tamanho de título que cabe na largura disponível (como no Spotify)
                let avail = ui.available_width() - 24.0;
                let size = [72.0, 56.0, 44.0, 34.0, 28.0]
                    .into_iter()
                    .find(|&sz| ui.fonts_mut(|f| f.layout_no_wrap(title.to_string(), ui::font_bold(sz), TEXT).size().x) <= avail)
                    .unwrap_or(28.0);
                ui.add(egui::Label::new(RichText::new(title).font(ui::font_bold(size))).truncate());
                ui.label(RichText::new(subtitle).color(SUB));
            });
        });
        ui.add_space(18.0);
    }

    /// Botão verde grande de tocar + aleatório (+ extra opcional)
    fn action_row(&mut self, ui: &mut Ui, tracks: &[Track], extra: impl FnOnce(&mut Self, &mut Ui)) {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let playing_this = self.is_playing() && self.current_track_id().is_some_and(|id| tracks.iter().any(|t| t.id == id));
            if ui::circle_button(ui, if playing_this { "⏸" } else { "▶" }, 56.0, GREEN, Color32::BLACK).clicked() {
                if playing_this {
                    if let Some(p) = &mut self.playback {
                        p.toggle();
                    }
                } else {
                    self.play_tracks(tracks, 0, false);
                }
            }
            ui.add_space(8.0);
            if ui::icon_button(ui, "🔀", 26.0, false, "Tocar em ordem aleatória").clicked() {
                self.play_tracks(tracks, 0, true);
            }
            extra(self, ui);
        });
        ui.add_space(10.0);
    }

    /// Tabela de músicas virtualizada: # | Título | Álbum | ♥ | duração
    fn track_table(&mut self, ui: &mut Ui, tracks: Vec<Track>) {
        let width = ui.available_width() - 24.0;
        let album_w = (width * 0.30).clamp(120.0, 360.0);
        let title_w = width - 48.0 - album_w - 110.0;
        let ranking = self.view == View::Ranking;
        // cabeçalho (mesmas colunas das linhas)
        let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
        let row = head.shrink2(Vec2::new(12.0, 0.0));
        let cy = row.center().y;
        ui.painter()
            .text(egui::pos2(row.left() + 28.0, cy), Align2::CENTER_CENTER, "#", ui::font(13.0), SUB);
        paint_truncated(ui, egui::pos2(row.left() + 52.0, cy), "Título", ui::font(13.0), SUB, title_w);
        paint_truncated(
            ui,
            egui::pos2(row.left() + 52.0 + title_w, cy),
            if ranking { "Reproduções" } else { "Álbum" },
            ui::font(13.0),
            SUB,
            album_w,
        );
        ui.painter()
            .text(egui::pos2(row.right() - 16.0, cy), Align2::RIGHT_CENTER, "🕒", ui::font(13.0), SUB);
        let sep = ui.cursor().min.y;
        ui.painter().hline(
            ui.max_rect().left() + 24.0..=ui.max_rect().right() - 24.0,
            sep,
            egui::Stroke::new(1.0, Color32::from_white_alpha(25)),
        );
        ui.add_space(8.0);

        let current = self.current_track_id();
        let playing = self.is_playing();
        let mut action: Option<(usize, TrackAction)> = None;
        // Playlist que aceita reordenar arrastando (a busca e as outras listas não têm ordem própria)
        let reorder = match &self.view {
            View::Playlist(id) => Some(id.clone()),
            View::Favorites => self.favorites_id.clone(),
            _ => None,
        };
        let dragging = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
        // Cada página (artista, playlist…) tem a própria rolagem: abre do topo e,
        // ao voltar, fica onde estava. Sem isso a rolagem de uma passava para a outra.
        egui::ScrollArea::vertical()
            .id_salt(("faixas", &self.view))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, tracks.len(), |ui, range| {
                // Rolagem automática ao arrastar perto das bordas
                if dragging && let Some(pos) = ui.ctx().pointer_hover_pos() {
                    let clip = ui.clip_rect();
                    if clip.x_range().contains(pos.x) {
                        if pos.y < clip.top() + 48.0 && pos.y > clip.top() - 8.0 {
                            ui.scroll_with_delta(Vec2::new(0.0, 10.0));
                        } else if pos.y > clip.bottom() - 48.0 && pos.y < clip.bottom() + 8.0 {
                            ui.scroll_with_delta(Vec2::new(0.0, -10.0));
                        }
                        ui.ctx().request_repaint();
                    }
                }
                for i in range {
                    let t = &tracks[i];
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
                    resp.dnd_set_drag_payload(DragTrack {
                        id: t.id.clone(),
                        title: t.title.clone(),
                    });
                    if resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        drag_label(ui.ctx(), &t.title);
                    }
                    if let Some(pid) = &reorder
                        && let Some(pos) = ui.ctx().pointer_hover_pos()
                        && resp.dnd_hover_payload::<DragTrack>().is_some_and(|d| d.id != t.id)
                    {
                        let upper = pos.y < rect.center().y;
                        let y = if upper { rect.top() } else { rect.bottom() };
                        ui.painter()
                            .hline(rect.left() + 12.0..=rect.right() - 12.0, y, egui::Stroke::new(2.0, GREEN));
                        if let Some(d) = resp.dnd_release_payload::<DragTrack>() {
                            let before = if upper {
                                Some(t.id.clone())
                            } else {
                                tracks.get(i + 1).map(|n| n.id.clone())
                            };
                            if before.as_deref() != Some(d.id.as_str()) {
                                action = Some((i, TrackAction::MoveTo(pid.clone(), d.id.clone(), before)));
                            }
                        }
                    }
                    let row = rect.shrink2(Vec2::new(12.0, 0.0));
                    let is_current = current.as_deref() == Some(t.id.as_str());
                    if resp.hovered() {
                        ui.painter().rect_filled(row, 6.0, Color32::from_white_alpha(18));
                    }
                    let p = ui.painter();
                    let cy = row.center().y;
                    // coluna #
                    let idx_center = egui::pos2(row.left() + 28.0, cy);
                    if resp.hovered() {
                        p.text(idx_center, Align2::CENTER_CENTER, "▶", ui::font(15.0), TEXT);
                    } else if is_current {
                        p.text(
                            idx_center,
                            Align2::CENTER_CENTER,
                            if playing { "♫" } else { "♪" },
                            ui::font(16.0),
                            GREEN,
                        );
                    } else {
                        let (f, c) = if ranking && i < 3 {
                            (ui::font_bold(17.0), GREEN)
                        } else {
                            (ui::font(14.0), SUB)
                        };
                        p.text(idx_center, Align2::CENTER_CENTER, (i + 1).to_string(), f, c);
                    }
                    // capa + título/artista
                    let cover = Rect::from_min_size(egui::pos2(row.left() + 52.0, cy - 20.0), Vec2::splat(40.0));
                    ui::cover_at(ui, cover, &t.cover_url, &format!("{}{}", t.artist, t.album), 4.0);
                    let tx = cover.right() + 12.0;
                    let tw = title_w - 56.0;
                    let title_color = if is_current { GREEN } else { TEXT };
                    paint_truncated(ui, egui::pos2(tx, cy - 9.0), &t.title, ui::font_bold(15.0), title_color, tw);
                    let artist = if self.downloaded.contains(&t.id) {
                        format!("⬇ {}", t.artist)
                    } else {
                        t.artist.clone()
                    };
                    paint_truncated(ui, egui::pos2(tx, cy + 10.0), &artist, ui::font(13.0), SUB, tw);
                    // álbum
                    let ax = row.left() + 52.0 + title_w;
                    if ranking {
                        paint_truncated(
                            ui,
                            egui::pos2(ax, cy),
                            &format!("{} ▶", fmt_thousands(t.play_count)),
                            ui::font_bold(14.0),
                            if i < 3 { GREEN } else { TEXT },
                            album_w - 16.0,
                        );
                    } else {
                        paint_truncated(ui, egui::pos2(ax, cy), &t.album, ui::font(13.0), SUB, album_w - 16.0);
                    }
                    // favorito e duração
                    let p = ui.painter();
                    if self.favorites.contains(&t.id) {
                        p.text(
                            egui::pos2(row.right() - 70.0, cy),
                            Align2::CENTER_CENTER,
                            "♥",
                            ui::font(15.0),
                            GREEN,
                        );
                    }
                    p.text(
                        egui::pos2(row.right() - 16.0, cy),
                        Align2::RIGHT_CENTER,
                        ui::fmt_duration(t.duration.max(0) as u64),
                        ui::font(13.0),
                        SUB,
                    );

                    if resp.double_clicked() || resp.clicked() {
                        action = Some((i, TrackAction::Play));
                    }
                    resp.context_menu(|ui| {
                        ui.set_min_width(220.0);
                        for (label, a) in [
                            ("⏭  Tocar a seguir", TrackAction::PlayNext),
                            ("➕  Adicionar à fila", TrackAction::Enqueue),
                        ] {
                            if ui.button(label).clicked() {
                                action = Some((i, a));
                            }
                        }
                        let fav = if self.favorites.contains(&t.id) {
                            "♡  Remover das curtidas"
                        } else {
                            "♥  Curtir"
                        };
                        if ui.button(fav).clicked() {
                            action = Some((i, TrackAction::Favorite));
                        }
                        ui.menu_button("📃  Adicionar à playlist", |ui| {
                            for p in &self.playlists {
                                if ui.button(&p.name).clicked() {
                                    action = Some((i, TrackAction::AddTo(p.id.clone(), p.name.clone())));
                                }
                            }
                        });
                        ui.separator();
                        if self.downloaded.contains(&t.id) {
                            if ui.button("🗑  Remover do computador").clicked() {
                                action = Some((i, TrackAction::RemoveDownload));
                            }
                        } else if ui.button("⬇  Baixar para ouvir offline").clicked() {
                            action = Some((i, TrackAction::Download));
                        }
                        if ui.button("👤  Ir para o artista").clicked() {
                            action = Some((i, TrackAction::GoArtist));
                        }
                        ui.separator();
                        let (label, on) = if t.restricted {
                            ("↺  Tirar das restritas", false)
                        } else {
                            ("🚫  Restringir", true)
                        };
                        if ui.button(label).clicked() {
                            action = Some((i, TrackAction::Restrict(on)));
                        }
                        ui.separator();
                        if ui.button("✏  Editar informações").clicked() {
                            action = Some((i, TrackAction::Edit));
                        }
                        if ui.button("✨  Corrigir capa e álbum").clicked() {
                            action = Some((i, TrackAction::Fix));
                        }
                        if ui.button("🗑  Apagar do servidor").clicked() {
                            action = Some((i, TrackAction::Delete));
                        }
                    });
                }
                ui.add_space(24.0);
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
                Ok(on) => Event::Message(if on {
                    format!("♥ \"{}\" nas curtidas", t.title)
                } else {
                    format!("\"{}\" saiu das curtidas", t.title)
                }),
                Err(e) => Event::Message(e.to_string()),
            }),
            TrackAction::AddTo(pid, name) => self.spawn(ctx, move |c| match c.add_to_playlist(&pid, &t.id) {
                Ok(_) => Event::Message(format!("Adicionada a \"{name}\"")),
                Err(e) => Event::Message(e.to_string()),
            }),
            TrackAction::Download => {
                self.toast(ctx, format!("Baixando \"{}\"…", t.title));
                self.spawn(ctx, move |c| match c.download(&t.id, |_, _| true) {
                    Ok(_) => Event::Message(format!("⬇ \"{}\" disponível offline", t.title)),
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
            TrackAction::Restrict(on) => self.restrict(ctx, t, on),
            TrackAction::Edit => self.start_edit(t),
            TrackAction::Fix => self.fix_metadata(ctx, t),
            TrackAction::Delete => self.ask_delete(t),
            TrackAction::MoveTo(pid, id, before) => {
                // Mostra a nova ordem na hora; o servidor confirma em seguida
                if let Some(from) = self.view_tracks.iter().position(|x| x.id == id) {
                    let item = self.view_tracks.remove(from);
                    let at = before
                        .as_ref()
                        .and_then(|b| self.view_tracks.iter().position(|x| &x.id == b))
                        .unwrap_or(self.view_tracks.len());
                    self.view_tracks.insert(at, item);
                }
                self.spawn(ctx, move |c| match c.move_in_playlist(&pid, &id, before.as_deref()) {
                    Ok(_) => Event::Changed,
                    Err(e) => Event::Message(format!("Não deu para mover: {e}")),
                });
            }
        }
    }

    fn restrict(&mut self, ctx: &egui::Context, t: Track, on: bool) {
        self.spawn(ctx, move |c| match c.set_restricted(&t.id, on) {
            Ok(()) => Event::Message(if on {
                format!("🚫 \"{}\" foi para Restritas", t.title)
            } else {
                format!("\"{}\" voltou para a biblioteca", t.title)
            }),
            Err(e) => Event::Message(e.to_string()),
        });
    }

    fn home(&mut self, ui: &mut Ui) {
        let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.max_rect().width(), 300.0));
        paint_gradient(ui, rect, Color32::from_rgb(0x1E, 0x4D, 0x33), BG);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                let greet = match local_hour() {
                    5..=11 => "Bom dia",
                    12..=17 => "Boa tarde",
                    _ => "Boa noite",
                };
                ui.label(RichText::new(greet).font(ui::font_bold(32.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(24.0);
                    if ui::pill(ui, if self.syncing { "Sincronizando…" } else { "⟳  Sincronizar" }, TILE, TEXT).clicked() {
                        let ctx = ui.ctx().clone();
                        self.sync(&ctx);
                    }
                    let all = self.tracks_cache.clone();
                    if ui::pill(ui, &format!("🔀  Aleatório • {} músicas", all.len()), GREEN, Color32::BLACK).clicked() {
                        self.play_tracks(&all, 0, true);
                    }
                });
            });
            ui.add_space(16.0);
            self.quick_grid(ui);
            let (top, recent) = (self.top.clone(), self.recent.clone());
            self.shelf(ui, "Mais tocadas", top);
            self.shelf(ui, "Adicionadas recentemente", recent);
            ui.add_space(24.0);
        });
    }

    /// Atalhos em grade (como o topo da home do Spotify)
    fn quick_grid(&mut self, ui: &mut Ui) {
        enum Tile {
            Special(&'static str, Color32, Color32, &'static str, View),
            Playlist(Playlist),
        }
        let mut tiles = vec![
            Tile::Special(
                "Músicas curtidas",
                Color32::from_rgb(0x45, 0x0A, 0xF5),
                Color32::from_rgb(0xC4, 0xEF, 0xD9),
                "♥",
                View::Favorites,
            ),
            Tile::Special(
                "Ilmarfy Flow",
                Color32::from_rgb(0x1D, 0xB9, 0x54),
                Color32::from_rgb(0x0B, 0x4D, 0x7A),
                "🌊",
                View::Flow,
            ),
            Tile::Special(
                "Mais tocadas",
                Color32::from_rgb(0xE9, 0x5F, 0x2A),
                Color32::from_rgb(0x5A, 0x14, 0x3C),
                "🏆",
                View::Ranking,
            ),
        ];
        tiles.extend(self.playlists.iter().cloned().map(Tile::Playlist));
        let cols = if ui.available_width() > 1100.0 {
            4
        } else if ui.available_width() > 760.0 {
            3
        } else {
            2
        };
        let gap = 10.0;
        let w = (ui.available_width() - 48.0 - gap * (cols as f32 - 1.0)) / cols as f32;
        for row in tiles.chunks(cols) {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                for tile in row {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 64.0), Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        6.0,
                        if resp.hovered() {
                            Color32::from_white_alpha(45)
                        } else {
                            Color32::from_white_alpha(25)
                        },
                    );
                    let cover = Rect::from_min_size(rect.min, Vec2::splat(64.0));
                    let (name, view) = match tile {
                        Tile::Special(name, a, b, sym, view) => {
                            paint_special(ui, cover, *a, *b, sym);
                            (name.to_string(), view.clone())
                        }
                        Tile::Playlist(p) => {
                            let covers = self.playlist_covers(p);
                            ui::mosaic_at(ui, cover, &covers, &p.name, 6.0);
                            (p.name.clone(), View::Playlist(p.id.clone()))
                        }
                    };
                    paint_truncated(
                        ui,
                        egui::pos2(cover.right() + 14.0, rect.center().y),
                        &name,
                        ui::font_bold(15.0),
                        TEXT,
                        w - 90.0,
                    );
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                    if resp.clicked() {
                        self.go(view);
                    }
                    ui.add_space(gap - ui.spacing().item_spacing.x);
                }
            });
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
    }

    /// Prateleira horizontal de cartões (capa grande + título + artista)
    fn shelf(&mut self, ui: &mut Ui, title: &str, list: Vec<Track>) {
        if list.is_empty() {
            return;
        }
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(title).font(ui::font_bold(22.0)));
        });
        ui.add_space(8.0);
        let mut play = None;
        egui::ScrollArea::horizontal()
            .id_salt(title)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    for (i, t) in list.iter().enumerate() {
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(176.0, 236.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 8.0, HOVER);
                        }
                        let cover = Rect::from_min_size(rect.min + Vec2::new(12.0, 12.0), Vec2::splat(152.0));
                        ui::cover_at(ui, cover, &t.cover_url, &format!("{}{}", t.artist, t.album), 6.0);
                        if resp.hovered() {
                            let c = cover.right_bottom() - Vec2::new(26.0, 26.0);
                            ui.painter().circle_filled(c, 22.0, GREEN);
                            ui.painter()
                                .text(c + Vec2::new(2.0, 0.0), Align2::CENTER_CENTER, "▶", ui::font(18.0), Color32::BLACK);
                        }
                        paint_truncated(
                            ui,
                            egui::pos2(cover.left(), cover.bottom() + 18.0),
                            &t.title,
                            ui::font_bold(15.0),
                            TEXT,
                            152.0,
                        );
                        paint_truncated(
                            ui,
                            egui::pos2(cover.left(), cover.bottom() + 38.0),
                            &t.artist,
                            ui::font(13.0),
                            SUB,
                            152.0,
                        );
                        if resp.clicked() {
                            play = Some(i);
                        }
                    }
                });
            });
        if let Some(i) = play {
            self.play_tracks(&list, i, false);
        }
    }

    fn list_page(&mut self, ui: &mut Ui) {
        let tracks = self.view_tracks.clone();
        let mins = tracks.iter().map(|t| t.duration.max(0) as u64).sum::<u64>() / 60;
        let summary = format!("{} músicas • {} h {} min", tracks.len(), mins / 60, mins % 60);
        match self.view.clone() {
            View::Playlist(id) => {
                let Some(p) = self.playlists.iter().find(|p| p.id == id).cloned() else {
                    ui.label("Playlist não encontrada.");
                    return;
                };
                let covers = self.playlist_covers(&p);
                let color = ui::seed_color(&p.name);
                self.page_header(ui, "Playlist", &p.name, &summary, color, |ui| {
                    ui::mosaic(ui, &covers, &p.name, 200.0, 6.0);
                });
                let offline = tracks.iter().filter(|t| self.downloaded.contains(&t.id)).count();
                self.action_row(ui, &tracks, |app, ui| {
                    let key = format!("playlist:{id}");
                    if app.downloading.contains(&key) {
                        ui.spinner();
                        ui.label(RichText::new("Baixando para offline…").color(SUB));
                    } else if !tracks.is_empty() && offline == tracks.len() {
                        ui.label(RichText::new("⬇ Disponível offline").color(GREEN));
                    } else if ui::icon_button(ui, "⬇", 24.0, false, "Baixar para ouvir offline").clicked() {
                        app.downloading.insert(key);
                        let name = p.name.clone();
                        let pid = id.clone();
                        app.spawn(ui.ctx(), move |c| match c.download_playlist(&pid, |_, _, _| true) {
                            Ok((n, fails)) => Event::Message(format!(
                                "\"{name}\": {n} músicas baixadas{}",
                                if fails.is_empty() {
                                    String::new()
                                } else {
                                    format!(", {} falharam", fails.len())
                                }
                            )),
                            Err(e) => Event::Message(format!("Falha ao baixar: {e}")),
                        });
                    }
                });
            }
            View::Favorites => {
                self.page_header(
                    ui,
                    "Playlist",
                    "Músicas curtidas",
                    &summary,
                    Color32::from_rgb(0x50, 0x38, 0xA0),
                    |ui| {
                        ui::special_cover(
                            ui,
                            200.0,
                            Color32::from_rgb(0x45, 0x0A, 0xF5),
                            Color32::from_rgb(0xC4, 0xEF, 0xD9),
                            "♥",
                        );
                    },
                );
                self.action_row(ui, &tracks, |_, _| {});
            }
            View::Ranking => {
                let plays: i64 = tracks.iter().map(|t| t.play_count).sum();
                self.page_header(
                    ui,
                    "Ranking",
                    "Mais tocadas",
                    &format!("{} músicas • {} reproduções", tracks.len(), fmt_thousands(plays)),
                    Color32::from_rgb(0xE9, 0x5F, 0x2A),
                    |ui| {
                        ui::special_cover(
                            ui,
                            200.0,
                            Color32::from_rgb(0xE9, 0x5F, 0x2A),
                            Color32::from_rgb(0x5A, 0x14, 0x3C),
                            "🏆",
                        );
                    },
                );
                self.action_row(ui, &tracks, |_, _| {});
            }
            View::Restricted => {
                self.page_header(
                    ui,
                    "Fora da biblioteca",
                    "Restritas",
                    &summary,
                    Color32::from_rgb(0x8A, 0x1C, 0x2B),
                    |ui| {
                        ui::special_cover(
                            ui,
                            200.0,
                            Color32::from_rgb(0x8A, 0x1C, 0x2B),
                            Color32::from_rgb(0x2A, 0x0A, 0x10),
                            "🚫",
                        );
                    },
                );
                self.action_row(ui, &tracks, |_, _| {});
            }
            View::Offline => {
                self.page_header(
                    ui,
                    "No computador",
                    "Offline",
                    &summary,
                    Color32::from_rgb(0x0E, 0x7C, 0x4F),
                    |ui| {
                        ui::special_cover(
                            ui,
                            200.0,
                            Color32::from_rgb(0x0E, 0x7C, 0x4F),
                            Color32::from_rgb(0x06, 0x2E, 0x1E),
                            "⬇",
                        );
                    },
                );
                self.action_row(ui, &tracks, |_, _| {});
            }
            View::Songs => {
                self.page_header(
                    ui,
                    "Biblioteca",
                    "Todas as músicas",
                    &summary,
                    Color32::from_rgb(0xA0, 0x1E, 0x2B),
                    |ui| {
                        ui::special_cover(
                            ui,
                            200.0,
                            Color32::from_rgb(0xE9, 0x14, 0x29),
                            Color32::from_rgb(0x4A, 0x06, 0x0D),
                            "♪",
                        );
                    },
                );
                self.action_row(ui, &tracks, |_, _| {});
            }
            View::Artist(name) => {
                let cover_url = tracks
                    .iter()
                    .map(|t| t.cover_url.clone())
                    .find(|c| !c.is_empty())
                    .unwrap_or_default();
                let summary = format!("{} músicas • {} álbuns", tracks.len(), self.albums.len());
                self.page_header(ui, "Artista", &name, &summary, ui::seed_color(&name), |ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(200.0), Sense::hover());
                    ui::cover_at(ui, rect, &cover_url, &name, 8.0);
                });
                self.action_row(ui, &tracks, |_, _| {});
            }
            _ => {}
        }
        self.track_table(ui, tracks);
    }

    fn artists_page(&mut self, ui: &mut Ui) {
        let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.max_rect().width(), 200.0));
        paint_gradient(ui, rect, Color32::from_rgb(0x4A, 0x37, 0x5C), BG);
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new("Artistas").font(ui::font_bold(32.0)));
        });
        ui.add_space(12.0);
        let artists = self.artists.clone();
        let card = 176.0;
        let cols = ((ui.available_width() - 48.0) / card).floor().max(1.0) as usize;
        let rows = artists.len().div_ceil(cols);
        let mut go = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 230.0, rows, |ui, range| {
                for r in range {
                    ui.horizontal(|ui| {
                        ui.add_space(14.0);
                        for a in artists.iter().skip(r * cols).take(cols) {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(card, 226.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 8.0, HOVER);
                            }
                            let cover = Rect::from_min_size(rect.min + Vec2::new(12.0, 12.0), Vec2::splat(152.0));
                            ui::cover_at(ui, cover, &a.cover_url, &a.name, 6.0);
                            paint_truncated(
                                ui,
                                egui::pos2(cover.left(), cover.bottom() + 18.0),
                                &a.name,
                                ui::font_bold(15.0),
                                TEXT,
                                152.0,
                            );
                            paint_truncated(
                                ui,
                                egui::pos2(cover.left(), cover.bottom() + 38.0),
                                &if a.track_count == 1 {
                                    "1 música".to_string()
                                } else {
                                    format!("{} músicas", a.track_count)
                                },
                                ui::font(13.0),
                                SUB,
                                152.0,
                            );
                            if resp.clicked() {
                                go = Some(a.name.clone());
                            }
                        }
                    });
                }
            });
        if let Some(a) = go {
            self.go(View::Artist(a));
        }
    }

    fn search_page(&mut self, ui: &mut Ui) {
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let w = (ui.available_width() - 48.0).min(460.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("🔍  O que você quer ouvir?")
                    .desired_width(w)
                    .font(ui::font(16.0))
                    .margin(Vec2::new(16.0, 10.0)),
            );
            if r.changed() {
                self.view_key = None;
            }
        });
        ui.add_space(16.0);
        let tracks = self.view_tracks.clone();
        if self.search.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new("Busque na sua biblioteca por música, artista ou álbum (sem se preocupar com acentos).").color(SUB));
            });
            return;
        }
        if tracks.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new(format!("Nada encontrado para \"{}\".", self.search)).color(SUB));
            });
            return;
        }
        self.track_table(ui, tracks);
    }

    fn flow_page(&mut self, ui: &mut Ui) {
        if self.flow.is_none() && !self.flow_loading {
            self.load_flow(ui.ctx());
        }
        let tracks = match &self.flow {
            Some(Ok((_, t))) => t.clone(),
            _ => Vec::new(),
        };
        let seeds = match &self.flow {
            Some(Ok((s, _))) => format!("Baseado em {}", s.join(", ")),
            _ => "Músicas que você ainda não tem, a partir dos seus artistas".into(),
        };
        self.page_header(
            ui,
            "Feito para você",
            "Ilmarfy Flow",
            &seeds,
            Color32::from_rgb(0x1D, 0x7A, 0x6E),
            |ui| {
                ui::special_cover(
                    ui,
                    200.0,
                    Color32::from_rgb(0x1D, 0xB9, 0x54),
                    Color32::from_rgb(0x0B, 0x4D, 0x7A),
                    "🌊",
                );
            },
        );
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            if ui::circle_button(ui, "▶", 56.0, GREEN, Color32::BLACK).clicked()
                && !tracks.is_empty()
                && let Some(p) = &mut self.playback
            {
                p.play_items(tracks.iter().cloned().map(Item::Flow).collect(), 0, false);
            }
            ui.add_space(8.0);
            if ui::icon_button(ui, "⟳", 24.0, false, "Sortear de novo").clicked() {
                self.load_flow(ui.ctx());
            }
            let n = self
                .restricted
                .as_ref()
                .map(|r| r.blocked_artists.len() + r.disliked_tracks.len())
                .unwrap_or(0);
            if ui::pill(ui, &format!("🚫  Restritos ({n})"), TILE, TEXT).clicked() {
                self.show_restricted = true;
            }
        });
        ui.add_space(10.0);
        if self.flow_loading {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.spinner();
                ui.label(RichText::new("Sintonizando seu Flow…").color(SUB));
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
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                    let row = rect.shrink2(Vec2::new(12.0, 0.0));
                    if resp.hovered() {
                        ui.painter().rect_filled(row, 6.0, Color32::from_white_alpha(18));
                    }
                    let cy = row.center().y;
                    let playing = current.as_deref() == Some(format!("f:{}", t.id).as_str());
                    let p = ui.painter();
                    p.text(
                        egui::pos2(row.left() + 28.0, cy),
                        Align2::CENTER_CENTER,
                        if resp.hovered() { "▶".into() } else { (i + 1).to_string() },
                        ui::font(14.0),
                        if playing { GREEN } else { SUB },
                    );
                    let cover = Rect::from_min_size(egui::pos2(row.left() + 52.0, cy - 20.0), Vec2::splat(40.0));
                    ui::cover_at(ui, cover, &t.cover, &t.artist, 4.0);
                    let tw = row.width() - 52.0 - 56.0 - 150.0;
                    paint_truncated(
                        ui,
                        egui::pos2(cover.right() + 12.0, cy - 9.0),
                        &t.title,
                        ui::font_bold(15.0),
                        if playing { GREEN } else { TEXT },
                        tw,
                    );
                    paint_truncated(ui, egui::pos2(cover.right() + 12.0, cy + 10.0), &t.artist, ui::font(13.0), SUB, tw);
                    if resp.clicked() {
                        act = Some((i, 0));
                    }
                    // Botões à direita, alinhados ao centro da linha
                    let mut buttons = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(Rect::from_min_max(egui::pos2(row.right() - 140.0, row.top()), row.right_bottom()))
                            .layout(Layout::right_to_left(Align::Center)),
                    );
                    if ui::icon_button(&mut buttons, "🚫", 17.0, false, "Não quero mais este artista no Flow").clicked() {
                        act = Some((i, 3));
                    }
                    if ui::icon_button(&mut buttons, "👎", 17.0, false, "Não curti esta música").clicked() {
                        act = Some((i, 2));
                    }
                    if ui::icon_button(&mut buttons, "➕", 17.0, false, "Adicionar à biblioteca").clicked() {
                        act = Some((i, 1));
                    }
                }
                ui.add_space(24.0);
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
                    // Também sai da fila de reprodução (e pula, se for a que está tocando)
                    if let Some(p) = &mut self.playback {
                        p.remove_where(|i| match i {
                            Item::Flow(f) => f.id == t.id || (block && f.artist.eq_ignore_ascii_case(&t.artist)),
                            _ => false,
                        });
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
        let mut restore: Option<(bool, String)> = None;
        egui::Window::new("Restritos do Flow")
            .open(&mut open)
            .default_size([440.0, 500.0])
            .collapsible(false)
            .show(ctx, |ui| {
                let Some(r) = &self.restricted else {
                    ui.spinner();
                    return;
                };
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new(format!("Artistas ({})", r.blocked_artists.len())).font(ui::font_bold(15.0)));
                    for a in &r.blocked_artists {
                        ui.horizontal(|ui| {
                            ui.label(&a.name);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("↩ Restaurar").clicked() {
                                    restore = Some((true, a.key.clone()));
                                }
                            });
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new(format!("Músicas ({})", r.disliked_tracks.len())).font(ui::font_bold(15.0)));
                    for t in &r.disliked_tracks {
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(format!("{} — {}", t.title, t.artist)).truncate());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("↩ Restaurar").clicked() {
                                    restore = Some((false, t.id.clone()));
                                }
                            });
                        });
                    }
                });
            });
        self.show_restricted = open;
        if let Some((artist, key)) = restore {
            self.spawn(ctx, move |c| {
                let res = if artist {
                    c.api().flow_unblock_artist(&key)
                } else {
                    c.api().flow_restore_track(&key)
                };
                match res {
                    Ok(r) => Event::Restricted(r),
                    Err(e) => Event::Message(e.to_string()),
                }
            });
        }
    }

    fn settings_page(&mut self, ui: &mut Ui) {
        ui.add_space(24.0);
        let Some(core) = self.core.clone() else { return };
        egui::Frame::new().inner_margin(egui::Margin::symmetric(24, 0)).show(ui, |ui| {
            ui.label(RichText::new("Configurações").font(ui::font_bold(32.0)));
            ui.add_space(16.0);
            egui::Grid::new("settings").num_columns(2).spacing([32.0, 12.0]).show(ui, |ui| {
                let rows = [
                    ("Servidor", core.api().base_url().to_string()),
                    ("Versão", env!("CARGO_PKG_VERSION").to_string()),
                    ("Músicas", core.library().track_count().unwrap_or(0).to_string()),
                    (
                        "Offline",
                        format!(
                            "{} músicas ({:.1} MB)",
                            self.downloaded.len(),
                            core.library().downloads_size().unwrap_or(0) as f64 / 1_048_576.0
                        ),
                    ),
                    ("Dados", self.data_dir.display().to_string()),
                ];
                for (k, v) in rows {
                    ui.label(RichText::new(k).color(SUB));
                    ui.label(v);
                    ui.end_row();
                }
            });
            if !self.update_status.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(&self.update_status).color(SUB));
            }
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if ui::pill(ui, "⟳  Sincronizar agora", TILE, TEXT).clicked() {
                    let ctx = ui.ctx().clone();
                    self.sync(&ctx);
                }
                let label = if self.update.is_some() {
                    "⬇  Instalar atualização"
                } else {
                    "Procurar atualizações"
                };
                if ui::pill(ui, label, TILE, TEXT).clicked() {
                    let ctx = ui.ctx().clone();
                    if self.update.is_some() {
                        self.update_hidden = false;
                    } else {
                        self.check_update(&ctx, true);
                    }
                }
                if ui::pill(ui, "Sair deste computador", Color32::from_rgb(0xB4, 0x32, 0x32), TEXT).clicked() {
                    let _ = core.revoke_this_device();
                    self.logout_local();
                }
            });
            self.maintenance_section(ui);
        });
    }

    // ------------------------------------------------------------ Player

    fn player_bar(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let third = full.width() / 3.0;
        let left = Rect::from_min_max(full.min, egui::pos2(full.left() + third, full.bottom()));
        let center = Rect::from_min_max(
            egui::pos2(left.right(), full.top()),
            egui::pos2(full.right() - third, full.bottom()),
        );
        let right = Rect::from_min_max(egui::pos2(center.right(), full.top()), full.max);

        let item = self.playback.as_ref().and_then(|p| p.current().cloned());
        // Esquerda: capa, título, artista e curtir
        if let Some(item) = &item {
            let cy = left.center().y;
            let cover = Rect::from_min_size(egui::pos2(left.left() + 8.0, cy - 28.0), Vec2::splat(56.0));
            ui::cover_at(ui, cover, item.cover(), &format!("{}{}", item.artist(), item.album()), 4.0);
            let tw = left.width() - 56.0 - 100.0;
            paint_truncated(
                ui,
                egui::pos2(cover.right() + 14.0, cy - 10.0),
                item.title(),
                ui::font_bold(14.0),
                TEXT,
                tw,
            );
            let artist = if matches!(item, Item::Flow(_)) {
                format!("Flow • {}", item.artist())
            } else {
                item.artist().to_string()
            };
            paint_truncated(ui, egui::pos2(cover.right() + 14.0, cy + 10.0), &artist, ui::font(12.0), SUB, tw);
            if let Some(id) = item.track_id().map(String::from) {
                let fav = self.favorites.contains(&id);
                let heart = Rect::from_center_size(egui::pos2(left.right() - 28.0, cy), Vec2::splat(28.0));
                let resp = ui.interact(heart, ui.scope_id().with("fav"), Sense::click());
                ui.painter().text(
                    heart.center(),
                    Align2::CENTER_CENTER,
                    if fav { "♥" } else { "♡" },
                    ui::font(20.0),
                    if fav {
                        GREEN
                    } else if resp.hovered() {
                        TEXT
                    } else {
                        SUB
                    },
                );
                // Restringir a música que está tocando (ou devolvê-la à biblioteca)
                let restricted = self
                    .core
                    .as_ref()
                    .and_then(|c| c.library().track(&id).ok().flatten())
                    .is_some_and(|t| t.restricted);
                let ban = Rect::from_center_size(egui::pos2(left.right() - 60.0, cy), Vec2::splat(28.0));
                let ban_resp = ui.interact(ban, ui.scope_id().with("ban"), Sense::click());
                ui.painter().text(
                    ban.center(),
                    Align2::CENTER_CENTER,
                    "🚫",
                    ui::font(16.0),
                    if restricted {
                        Color32::from_rgb(0xFF, 0x52, 0x52)
                    } else if ban_resp.hovered() {
                        TEXT
                    } else {
                        DIM
                    },
                );
                let ban_tip = if restricted { "Tirar das restritas" } else { "Restringir" };
                if ban_resp.on_hover_text(ban_tip).clicked()
                    && let Item::Track(t) = item
                {
                    self.restrict(ui.ctx(), t.clone(), !restricted);
                }
                if resp.on_hover_text(if fav { "Remover das curtidas" } else { "Curtir" }).clicked() {
                    self.spawn(ui.ctx(), move |c| match c.toggle_favorite(&id) {
                        Ok(_) => Event::Changed,
                        Err(e) => Event::Message(e.to_string()),
                    });
                }
            }
        }

        // Centro: controles numa linha e a barra de progresso embaixo
        let Some(p) = &mut self.playback else { return };
        // Cada botão num ponto fixo em relação ao centro, para ficar alinhado com a barra
        let (cx, cy) = (center.center().x, center.top() + 30.0);
        let slot = |ui: &mut Ui, dx: f32, size: f32| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_center_size(egui::pos2(cx + dx, cy), Vec2::splat(size)))
                    .layout(Layout::left_to_right(Align::Center)),
            )
        };
        if ui::icon_button(&mut slot(ui, -96.0, 25.0), "🔀", 17.0, p.shuffle(), "Aleatório").clicked() {
            p.toggle_shuffle();
        }
        if ui::icon_button(&mut slot(ui, -50.0, 28.0), "⏮", 20.0, false, "Anterior").clicked() {
            p.previous();
        }
        let icon = match p.status() {
            Status::Playing => "⏸",
            Status::Loading => "…",
            _ => "▶",
        };
        if ui::circle_button(&mut slot(ui, 0.0, 36.0), icon, 36.0, TEXT, Color32::BLACK).clicked() {
            p.toggle();
        }
        if ui::icon_button(&mut slot(ui, 50.0, 28.0), "⏭", 20.0, false, "Próxima").clicked() {
            p.next();
        }
        let (rep_icon, rep_on) = match p.repeat() {
            Repeat::Off => ("🔁", false),
            Repeat::All => ("🔁", true),
            Repeat::One => ("🔂", true),
        };
        if ui::icon_button(&mut slot(ui, 96.0, 25.0), rep_icon, 17.0, rep_on, "Repetir").clicked() {
            p.cycle_repeat();
        }

        let (pos, dur) = (p.position(), p.duration());
        let bar_w = (center.width() - 120.0).min(520.0);
        let bar_row = Rect::from_center_size(egui::pos2(center.center().x, center.top() + 64.0), Vec2::new(bar_w + 100.0, 16.0));
        ui.painter().text(
            egui::pos2(bar_row.left() + 40.0, bar_row.center().y),
            Align2::RIGHT_CENTER,
            fmt(pos),
            ui::font(11.0),
            SUB,
        );
        ui.painter().text(
            egui::pos2(bar_row.right() - 40.0, bar_row.center().y),
            Align2::LEFT_CENTER,
            fmt(dur),
            ui::font(11.0),
            SUB,
        );
        let mut b = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(bar_row.center(), Vec2::new(bar_w, 16.0))));
        let frac = if dur.as_secs_f32() > 0.0 {
            pos.as_secs_f32() / dur.as_secs_f32()
        } else {
            0.0
        };
        if let Some(f) = ui::progress(&mut b, frac, bar_w) {
            p.seek(dur.mul_f32(f));
        }

        // Direita: letra, fila e volume
        let mut r = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right.shrink2(Vec2::new(16.0, 0.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let mut b2 = r.new_child(egui::UiBuilder::new().layout(Layout::right_to_left(Align::Center)));
        if let Some(v) = ui::progress(&mut b2, p.volume(), 100.0) {
            p.set_volume(v);
        }
        r.add_space(108.0);
        let vol_icon = if p.volume() == 0.0 { "🔇" } else { "🔊" };
        r.label(RichText::new(vol_icon).color(SUB));
        if ui::icon_button(&mut r, "☰", 18.0, self.side_panel == SidePanel::Queue, "Fila").clicked() {
            self.side_panel = if self.side_panel == SidePanel::Queue {
                SidePanel::None
            } else {
                SidePanel::Queue
            };
        }
        if ui::icon_button(&mut r, "🎤", 17.0, self.side_panel == SidePanel::Lyrics, "Letra").clicked() {
            self.side_panel = if self.side_panel == SidePanel::Lyrics {
                SidePanel::None
            } else {
                SidePanel::Lyrics
            };
        }
    }

    fn side_panel_ui(&mut self, ui: &mut Ui) {
        ui.add_space(12.0);
        match self.side_panel {
            SidePanel::Lyrics => {
                ui.label(RichText::new("Letra").font(ui::font_bold(18.0)));
                ui.add_space(8.0);
                let pos = self.playback.as_ref().map(|p| p.position()).unwrap_or_default();
                match &self.lyrics {
                    Some((_, Some(lines), _)) => {
                        let current = lines.iter().rposition(|l| l.time_ms <= pos.as_millis() as u64);
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            for (i, l) in lines.iter().enumerate() {
                                let active = Some(i) == current;
                                let past = current.is_some_and(|c| i < c);
                                let color = if active {
                                    TEXT
                                } else if past {
                                    SUB
                                } else {
                                    DIM
                                };
                                let r = ui.label(
                                    RichText::new(if l.text.is_empty() { "♪" } else { &l.text })
                                        .font(ui::font_bold(19.0))
                                        .color(color),
                                );
                                if active {
                                    r.scroll_to_me(Some(Align::Center));
                                }
                            }
                        });
                    }
                    Some((_, None, plain)) => {
                        egui::ScrollArea::vertical().show(ui, |ui| ui.label(RichText::new(plain.as_str()).font(ui::font(15.0))));
                    }
                    None => {
                        ui.label(RichText::new("Sem letra (prévia do Flow ou nada tocando).").color(SUB));
                    }
                }
            }
            SidePanel::Queue => {
                ui.label(RichText::new("Fila").font(ui::font_bold(18.0)));
                ui.add_space(8.0);
                let Some(p) = &mut self.playback else { return };
                let up = p.upcoming();
                let mut jump = None;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (i, it) in up.iter().enumerate() {
                        if i == 1 {
                            ui.add_space(6.0);
                            ui.label(RichText::new("A seguir").font(ui::font_bold(14.0)).color(SUB));
                        }
                        let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 6.0, Color32::from_white_alpha(18));
                        }
                        let cover = Rect::from_min_size(rect.min + Vec2::new(4.0, 6.0), Vec2::splat(40.0));
                        ui::cover_at(ui, cover, it.cover(), &format!("{}{}", it.artist(), it.album()), 4.0);
                        let w = rect.width() - 60.0;
                        paint_truncated(
                            ui,
                            egui::pos2(cover.right() + 10.0, rect.center().y - 9.0),
                            it.title(),
                            ui::font_bold(14.0),
                            if i == 0 { GREEN } else { TEXT },
                            w,
                        );
                        paint_truncated(
                            ui,
                            egui::pos2(cover.right() + 10.0, rect.center().y + 10.0),
                            it.artist(),
                            ui::font(12.0),
                            SUB,
                            w,
                        );
                        if resp.clicked() {
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

fn paint_gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (pos, c) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// `keys` com `moving` tirado do lugar e posto antes (`upper`) ou depois de `target`
fn reordered(keys: &[String], moving: &str, target: &str, upper: bool) -> Vec<String> {
    let mut order: Vec<String> = keys.iter().filter(|k| *k != moving).cloned().collect();
    let at = order.iter().position(|k| k == target).unwrap_or(order.len());
    let at = if upper { at } else { (at + 1).min(order.len()) };
    order.insert(at, moving.to_string());
    order
}

/// Etiqueta que acompanha o cursor enquanto se arrasta algo
fn drag_label(ctx: &egui::Context, text: &str) {
    let Some(pos) = ctx.pointer_hover_pos() else { return };
    egui::Area::new(egui::Id::unique("arrastando"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + Vec2::new(14.0, 10.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(0x2E, 0x77, 0xD0))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).font(ui::font_bold(13.0)).color(TEXT));
                });
        });
}

fn paint_special(ui: &Ui, rect: Rect, top: Color32, bottom: Color32, symbol: &str) {
    paint_gradient(ui, rect, top, bottom);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, symbol, ui::font(rect.height() * 0.42), TEXT);
}

/// Texto numa linha, cortado com "…" se passar da largura; `pos` é o começo à esquerda, centro vertical
fn paint_truncated(ui: &Ui, pos: egui::Pos2, s: &str, font: egui::FontId, color: Color32, max_w: f32) {
    if max_w <= 10.0 {
        return;
    }
    let mut job = egui::text::LayoutJob::single_section(s.to_string(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(pos - Vec2::new(0.0, galley.size().y / 2.0), galley, color);
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
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(BG))
                .show(ui, |ui| self.login_ui(ui));
            return;
        }
        egui::Panel::bottom("player")
            .exact_size(96.0)
            .frame(egui::Frame::new().fill(BLACK))
            .show(ui, |ui| self.player_bar(ui));
        egui::Panel::left("sidebar")
            .exact_size(300.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(BLACK).inner_margin(egui::Margin {
                left: 8,
                right: 0,
                top: 8,
                bottom: 0,
            }))
            .show(ui, |ui| self.sidebar(ui));
        if self.side_panel != SidePanel::None {
            egui::Panel::right("side")
                .exact_size(360.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(BLACK).inner_margin(egui::Margin {
                    left: 0,
                    right: 8,
                    top: 8,
                    bottom: 0,
                }))
                .show(ui, |ui| {
                    egui::Frame::new().fill(BG).corner_radius(8).inner_margin(16).show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        self.side_panel_ui(ui);
                    });
                });
        }
        self.tracks_for_view();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BLACK).inner_margin(8))
            .show(ui, |ui| {
                egui::Frame::new().fill(BG).corner_radius(8).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    ui.set_clip_rect(ui.max_rect());
                    match self.view.clone() {
                        View::Home => self.home(ui),
                        View::Search => self.search_page(ui),
                        View::Artists => self.artists_page(ui),
                        View::Flow => self.flow_page(ui),
                        View::Settings => self.settings_page(ui),
                        View::Add => self.add_page(ui),
                        _ => self.list_page(ui),
                    }
                });
            });
        if self.show_restricted {
            self.restricted_window(&ctx);
        }
        self.update_window(&ctx);
        self.admin_windows(&ctx);
        self.poll_downloads(&ctx);
        if let Some(name) = &mut self.new_playlist {
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("Nova playlist")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(name).hint_text("Nome da playlist").desired_width(280.0));
                    ui.horizontal(|ui| {
                        create = ui::pill(ui, "Criar", GREEN, Color32::BLACK).clicked() && !name.trim().is_empty();
                        cancel = ui::pill(ui, "Cancelar", TILE, TEXT).clicked();
                    });
                });
            if create {
                let n = name.trim().to_string();
                self.new_playlist = None;
                self.spawn(&ctx, move |c| match c.create_playlist(&n) {
                    Ok(_) => Event::Message(format!("Playlist \"{n}\" criada")),
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
                    .anchor(Align2::CENTER_BOTTOM, [0.0, -112.0])
                    .show(&ctx, |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(0x2E, 0x77, 0xD0))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(16, 10))
                            .show(ui, |ui| {
                                ui.label(RichText::new(msg).font(ui::font_bold(14.0)).color(TEXT));
                            });
                    });
            }
        }
    }

    fn on_exit(&mut self) {
        self.flush_plays_quick();
        // Encerra já: no Linux, desmontar o MPRIS (D-Bus) e a saída de áudio pode travar a janela
        #[cfg(not(test))]
        std::process::exit(0);
    }
}

/// 12345 -> "12.345"
fn fmt_thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    //! Janela renderizada fora da tela com dados reais (só leitura; volume zerado):
    //! `ILMARFY_TOKEN=… SNAP_DIR=/tmp/x cargo test -p ilmarfy-desktop telas -- --ignored --nocapture`
    use super::*;
    use egui_kittest::Harness;
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
        for _ in 0..20 {
            h.step();
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    #[test]
    fn reordena_a_barra_lateral() {
        let k: Vec<String> = ["fav", "flow", "ranking", "songs"].iter().map(|s| s.to_string()).collect();
        assert_eq!(reordered(&k, "ranking", "fav", true), ["ranking", "fav", "flow", "songs"]);
        assert_eq!(reordered(&k, "fav", "ranking", false), ["flow", "ranking", "fav", "songs"]);
        assert_eq!(reordered(&k, "flow", "songs", false), ["fav", "ranking", "songs", "flow"]);
    }

    #[test]
    #[ignore]
    fn telas_com_dados_reais() {
        let token = std::env::var("ILMARFY_TOKEN").expect("ILMARFY_TOKEN");
        let dir = std::env::temp_dir().join(format!("ilmarfy-desktop-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_private(&token_file(&dir), &token);
        let mut h = Harness::builder()
            .with_size([1440.0, 900.0])
            .build_eframe(|cc| App::new(cc, dir.clone()));
        wait(&mut h, 60, |a| !a.syncing && !a.tracks_cache.is_empty());
        assert!(h.state().tracks_cache.len() > 100, "biblioteca deveria ter sincronizado");
        snap(&mut h, "d1-inicio");

        h.state_mut().go(View::Songs);
        wait(&mut h, 2, |_| false);
        snap(&mut h, "d2-musicas");

        // Toca a primeira faixa com volume zero
        {
            let tracks = h.state().view_tracks.clone();
            let app = h.state_mut();
            if let Some(p) = &mut app.playback {
                p.set_volume(0.0);
            }
            app.play_tracks(&tracks, 0, false);
        }
        wait(&mut h, 60, |a| a.playback.as_ref().is_some_and(|p| *p.status() == Status::Playing));
        if h.state().playback.as_ref().is_some_and(|p| p.has_output()) {
            assert_eq!(h.state().playback.as_ref().map(|p| p.status().clone()), Some(Status::Playing));
        }
        h.state_mut().side_panel = SidePanel::Lyrics;
        wait(&mut h, 3, |_| false);
        snap(&mut h, "d3-tocando");

        let first = h.state().playlists.first().map(|p| p.id.clone());
        if let Some(id) = first {
            h.state_mut().side_panel = SidePanel::None;
            h.state_mut().go(View::Playlist(id));
            wait(&mut h, 3, |_| false);
            snap(&mut h, "d4-playlist");
            // Arrastar: segura a 1ª música, passa pela 2ª e por uma playlist da barra lateral,
            // e solta num lugar neutro (o topo), para não mudar nada no servidor
            let ev = |h: &mut Harness<'_, App>, e: egui::Event| {
                h.input_mut().events.push(e);
                h.step();
            };
            let mv = |h: &mut Harness<'_, App>, x: f32, y: f32| {
                for _ in 0..3 {
                    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
                    h.step();
                }
            };
            mv(&mut h, 700.0, 452.0);
            ev(
                &mut h,
                egui::Event::PointerButton {
                    pos: egui::pos2(700.0, 452.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            );
            for y in [460.0, 480.0, 500.0, 525.0] {
                mv(&mut h, 700.0, y);
            }
            snap(&mut h, "d4b-arrastando-na-lista");
            for (x, y) in [(500.0, 600.0), (300.0, 640.0), (150.0, 630.0)] {
                mv(&mut h, x, y);
            }
            snap(&mut h, "d4c-arrastando-na-barra");
            mv(&mut h, 720.0, 30.0);
            ev(
                &mut h,
                egui::Event::PointerButton {
                    pos: egui::pos2(720.0, 30.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            );
            wait(&mut h, 1, |_| false);
        }

        h.state_mut().go(View::Artists);
        wait(&mut h, 3, |_| false);
        snap(&mut h, "d5-artistas");
        // Barra lateral: arrasta "Mais tocadas" para cima de "Músicas curtidas"
        {
            let mv = |h: &mut Harness<'_, App>, x: f32, y: f32| {
                for _ in 0..3 {
                    h.input_mut().events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
                    h.step();
                }
            };
            let press = |h: &mut Harness<'_, App>, x: f32, y: f32, pressed: bool| {
                h.input_mut().events.push(egui::Event::PointerButton {
                    pos: egui::pos2(x, y),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
                h.step();
            };
            mv(&mut h, 150.0, 489.0);
            press(&mut h, 150.0, 489.0, true);
            for y in [470.0, 420.0, 360.0, 300.0, 282.0] {
                mv(&mut h, 150.0, y);
            }
            snap(&mut h, "d5b-arrastando-barra-lateral");
            press(&mut h, 150.0, 282.0, false);
            wait(&mut h, 1, |_| false);
            snap(&mut h, "d5c-barra-reordenada");
            assert_eq!(h.state().sidebar_order.first().map(String::as_str), Some("ranking"));
        }

        h.state_mut().go(View::Flow);
        wait(&mut h, 40, |a| a.flow.is_some());
        snap(&mut h, "d6-flow");
        h.state_mut().go(View::Restricted);
        h.run_steps(5);
        snap(&mut h, "d7-restritas");
        h.state_mut().go(View::Ranking);
        h.run_steps(5);
        snap(&mut h, "d8-ranking");
        // Adicionar músicas: só busca (não manda nada para a fila)
        h.state_mut().go(View::Add);
        h.state_mut().admin.query = "Cachorro Grande Lunático".into();
        let ctx = h.ctx.clone();
        h.state_mut().run_search(&ctx);
        wait(&mut h, 60, |a| !a.admin.loading);
        snap(&mut h, "d9-adicionar");
        if let Some(p) = &mut h.state_mut().playback {
            p.stop();
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
