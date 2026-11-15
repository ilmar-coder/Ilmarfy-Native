//! Ponte UniFFI: expõe o ilmarfy-core ao Kotlin. Os tipos aqui são espelhos simples dos do núcleo
//! (registros sem lógica), para o núcleo não depender do UniFFI.
//! Todas as chamadas são bloqueantes: no Android, chame de `Dispatchers.IO`.

use ilmarfy_core as core;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

uniffi::setup_scaffolding!();

// ------------------------------------------------------------ Erros

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum IlmarfyError {
    #[error("{detail}")]
    Network { detail: String },
    /// Token inválido/revogado: o app deve voltar para a tela de login
    #[error("{detail}")]
    Unauthorized { detail: String },
    #[error("{detail}")]
    RateLimited { detail: String },
    #[error("{detail}")]
    NotFound { detail: String },
    #[error("{detail}")]
    Other { detail: String },
}

impl From<core::Error> for IlmarfyError {
    fn from(e: core::Error) -> Self {
        let detail = e.to_string();
        match e {
            core::Error::Network(_) => Self::Network { detail },
            core::Error::Unauthorized(m) => Self::Unauthorized { detail: m },
            core::Error::RateLimited(m) => Self::RateLimited { detail: m },
            core::Error::NotFound(_) => Self::NotFound { detail },
            _ => Self::Other { detail },
        }
    }
}

type Result<T> = std::result::Result<T, IlmarfyError>;

// ------------------------------------------------------------ Registros

#[derive(uniffi::Record, Clone)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    pub cover_url: String,
    pub play_count: i64,
    pub filename: String,
    pub duration_secs: i64,
    pub date_added: i64,
    pub restricted: bool,
}

impl From<core::Track> for Track {
    fn from(t: core::Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            year: t.year,
            cover_url: t.cover_url,
            play_count: t.play_count,
            filename: t.filename,
            duration_secs: t.duration,
            date_added: t.date_added,
            restricted: t.restricted,
        }
    }
}

impl From<Track> for core::Track {
    fn from(t: Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            year: t.year,
            cover_url: t.cover_url,
            play_count: t.play_count,
            filename: t.filename,
            duration: t.duration_secs,
            date_added: t.date_added,
            restricted: t.restricted,
        }
    }
}

fn tracks(v: Vec<core::Track>) -> Vec<Track> {
    v.into_iter().map(Track::from).collect()
}

#[derive(uniffi::Record)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub track_ids: Vec<String>,
}

impl From<core::Playlist> for Playlist {
    fn from(p: core::Playlist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            track_ids: p.track_ids,
        }
    }
}

#[derive(uniffi::Record)]
pub struct Artist {
    pub name: String,
    pub track_count: u32,
    pub cover_url: String,
}

#[derive(uniffi::Record)]
pub struct Album {
    pub name: String,
    pub artist: String,
    pub year: String,
    pub cover_url: String,
    pub track_count: u32,
}

#[derive(uniffi::Enum)]
pub enum TrackOrder {
    Title,
    Artist,
    RecentlyAdded,
    MostPlayed,
}

impl From<TrackOrder> for core::TrackOrder {
    fn from(o: TrackOrder) -> Self {
        match o {
            TrackOrder::Title => Self::Title,
            TrackOrder::Artist => Self::Artist,
            TrackOrder::RecentlyAdded => Self::RecentlyAdded,
            TrackOrder::MostPlayed => Self::MostPlayed,
        }
    }
}

/// De onde tocar: `local_path` (arquivo baixado) ou `url` + cabeçalho de autorização
#[derive(uniffi::Record)]
pub struct PlaybackSource {
    pub local_path: Option<String>,
    pub url: Option<String>,
    pub header_name: Option<String>,
    pub header_value: Option<String>,
}

#[derive(uniffi::Record)]
pub struct SyncReport {
    pub tracks_updated: bool,
    pub playlists_updated: bool,
    pub track_count: u32,
    pub plays_sent: u32,
}

#[derive(uniffi::Record)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

#[derive(uniffi::Record)]
pub struct Lyrics {
    pub plain: String,
    /// Vazio se a letra não for sincronizada
    pub synced: Vec<LyricLine>,
    pub instrumental: bool,
}

#[derive(uniffi::Record, Clone)]
pub struct FlowTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_url: String,
    pub duration_ms: i64,
}

impl From<core::FlowTrack> for FlowTrack {
    fn from(t: core::FlowTrack) -> Self {
        Self {
            video_id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            cover_url: t.cover,
            duration_ms: t.duration_ms,
        }
    }
}

impl From<FlowTrack> for core::FlowTrack {
    fn from(t: FlowTrack) -> Self {
        Self {
            id: t.video_id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            cover: t.cover_url,
            duration_ms: t.duration_ms,
        }
    }
}

#[derive(uniffi::Record)]
pub struct FlowFeed {
    pub seeds: Vec<String>,
    pub tracks: Vec<FlowTrack>,
}

#[derive(uniffi::Record)]
pub struct BlockedArtist {
    pub key: String,
    pub name: String,
}

#[derive(uniffi::Record)]
pub struct DislikedTrack {
    pub video_id: String,
    pub title: String,
    pub artist: String,
}

#[derive(uniffi::Record)]
pub struct FlowRestricted {
    pub blocked_artists: Vec<BlockedArtist>,
    pub disliked_tracks: Vec<DislikedTrack>,
}

impl From<core::FlowRestricted> for FlowRestricted {
    fn from(r: core::FlowRestricted) -> Self {
        Self {
            blocked_artists: r
                .blocked_artists
                .into_iter()
                .map(|a| BlockedArtist { key: a.key, name: a.name })
                .collect(),
            disliked_tracks: r
                .disliked_tracks
                .into_iter()
                .map(|t| DislikedTrack {
                    video_id: t.id,
                    title: t.title,
                    artist: t.artist,
                })
                .collect(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct DownloadFailure {
    pub track_id: String,
    pub message: String,
}

#[derive(uniffi::Record)]
pub struct PlaylistDownloadResult {
    pub downloaded: u32,
    pub failures: Vec<DownloadFailure>,
}

/// Progresso de downloads (implementado no Kotlin). Devolva false para cancelar.
#[uniffi::export(with_foreign)]
pub trait DownloadListener: Send + Sync {
    fn on_progress(&self, done_bytes: u64, total_bytes: u64) -> bool;
    /// Playlists: chamado antes de cada faixa
    fn on_track(&self, index: u32, total: u32, title: String) -> bool;
}

// ------------------------------------------------------------ Objeto principal

#[derive(uniffi::Object)]
pub struct IlmarfyCore {
    inner: RwLock<core::Ilmarfy>,
}

#[uniffi::export]
impl IlmarfyCore {
    /// `data_dir`: pasta privada do app. `token`: o salvo no login anterior (ou nenhum).
    #[uniffi::constructor]
    pub fn new(data_dir: String, server: String, token: Option<String>) -> Result<Arc<Self>> {
        let app = core::Ilmarfy::open(&PathBuf::from(data_dir), &server, token)?;
        Ok(Arc::new(Self { inner: RwLock::new(app) }))
    }

    // Conta

    /// Troca a senha por um token deste aparelho; guarde o token (a senha não é guardada).
    pub fn login(&self, password: String, device_name: String) -> Result<String> {
        Ok(self.inner.write().unwrap().login(&password, &device_name, "android")?)
    }

    pub fn logout(&self) -> Result<()> {
        Ok(self.inner.write().unwrap().logout()?)
    }

    pub fn is_logged_in(&self) -> bool {
        self.inner.read().unwrap().is_logged_in()
    }

    pub fn server_url(&self) -> String {
        self.inner.read().unwrap().api().base_url().to_string()
    }

    // Sincronização e plays

    pub fn sync(&self) -> Result<SyncReport> {
        let r = self.inner.read().unwrap().sync()?;
        Ok(SyncReport {
            tracks_updated: r.tracks_updated,
            playlists_updated: r.playlists_updated,
            track_count: r.track_count,
            plays_sent: r.plays_sent,
        })
    }

    pub fn last_sync(&self) -> Result<Option<i64>> {
        Ok(self.inner.read().unwrap().last_sync()?)
    }

    pub fn record_play(&self, track_id: String) -> Result<()> {
        Ok(self.inner.read().unwrap().record_play(&track_id)?)
    }

    pub fn flush_plays(&self) -> Result<u32> {
        Ok(self.inner.read().unwrap().flush_plays()?)
    }

    // Biblioteca (cache local; funciona offline)

    pub fn track_count(&self) -> Result<u32> {
        Ok(self.inner.read().unwrap().library().track_count()?)
    }

    pub fn track(&self, id: String) -> Result<Option<Track>> {
        Ok(self.inner.read().unwrap().library().track(&id)?.map(Track::from))
    }

    pub fn tracks(&self, order: TrackOrder) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().tracks(order.into(), false)?))
    }

    pub fn restricted_tracks(&self) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().restricted_tracks()?))
    }

    pub fn search(&self, query: String, limit: u32) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().search(&query, limit)?))
    }

    pub fn artists(&self) -> Result<Vec<Artist>> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .library()
            .artists()?
            .into_iter()
            .map(|a| Artist {
                name: a.name,
                track_count: a.track_count,
                cover_url: a.cover_url,
            })
            .collect())
    }

    pub fn artist_tracks(&self, artist: String) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().artist_tracks(&artist)?))
    }

    pub fn albums(&self, artist: String) -> Result<Vec<Album>> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .library()
            .albums(&artist)?
            .into_iter()
            .map(|a| Album {
                name: a.name,
                artist: a.artist,
                year: a.year,
                cover_url: a.cover_url,
                track_count: a.track_count,
            })
            .collect())
    }

    pub fn album_tracks(&self, artist: String, album: String) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().album_tracks(&artist, &album)?))
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .library()
            .playlists()?
            .into_iter()
            .map(Playlist::from)
            .collect())
    }

    pub fn playlist_tracks(&self, playlist_id: String) -> Result<Vec<Track>> {
        Ok(tracks(self.inner.read().unwrap().library().playlist_tracks(&playlist_id)?))
    }

    pub fn favorites(&self) -> Result<Vec<Track>> {
        let app = self.inner.read().unwrap();
        match app.library().favorites_playlist()? {
            Some(p) => Ok(tracks(app.library().playlist_tracks(&p.id)?)),
            None => Ok(Vec::new()),
        }
    }

    pub fn is_favorite(&self, track_id: String) -> Result<bool> {
        Ok(self.inner.read().unwrap().is_favorite(&track_id)?)
    }

    /// Devolve o novo estado (true = favorita)
    pub fn toggle_favorite(&self, track_id: String) -> Result<bool> {
        Ok(self.inner.read().unwrap().toggle_favorite(&track_id)?)
    }

    pub fn create_playlist(&self, name: String) -> Result<Playlist> {
        Ok(self.inner.read().unwrap().create_playlist(&name)?.into())
    }

    pub fn delete_playlist(&self, playlist_id: String) -> Result<()> {
        Ok(self.inner.read().unwrap().delete_playlist(&playlist_id)?)
    }

    pub fn add_to_playlist(&self, playlist_id: String, track_id: String) -> Result<()> {
        self.inner.read().unwrap().add_to_playlist(&playlist_id, &track_id)?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, playlist_id: String, track_id: String) -> Result<()> {
        Ok(self.inner.read().unwrap().remove_from_playlist(&playlist_id, &track_id)?)
    }

    pub fn lyrics(&self, track_id: String) -> Result<Option<Lyrics>> {
        Ok(self.inner.read().unwrap().api().lyrics(&track_id)?.map(|l| Lyrics {
            synced: l
                .synced_lines()
                .into_iter()
                .map(|x| LyricLine {
                    time_ms: x.time_ms,
                    text: x.text,
                })
                .collect(),
            plain: l.plain,
            instrumental: l.instrumental,
        }))
    }

    // Reprodução e offline

    pub fn source(&self, track: Track) -> Result<PlaybackSource> {
        Ok(match self.inner.read().unwrap().source(&track.into())? {
            core::Source::Local(p) => PlaybackSource {
                local_path: Some(p.to_string_lossy().into_owned()),
                url: None,
                header_name: None,
                header_value: None,
            },
            core::Source::Remote { url, header } => {
                let (name, value) = header.unzip();
                PlaybackSource {
                    local_path: None,
                    url: Some(url),
                    header_name: name,
                    header_value: value,
                }
            }
        })
    }

    /// Cabeçalho de autorização para o player (streaming e prévias do Flow)
    pub fn auth_header_value(&self) -> Option<String> {
        self.inner.read().unwrap().api().auth_header().map(|(_, v)| v)
    }

    pub fn preview_url(&self, video_id: String) -> String {
        self.inner.read().unwrap().api().preview_url(&video_id)
    }

    pub fn is_downloaded(&self, track_id: String) -> Result<bool> {
        Ok(self.inner.read().unwrap().is_downloaded(&track_id)?)
    }

    pub fn downloaded_ids(&self) -> Result<Vec<String>> {
        Ok(self.inner.read().unwrap().library().downloaded_ids()?)
    }

    pub fn downloads_size(&self) -> Result<u64> {
        Ok(self.inner.read().unwrap().library().downloads_size()?)
    }

    pub fn download(&self, track_id: String, listener: Arc<dyn DownloadListener>) -> Result<String> {
        let path = self.inner.read().unwrap().download(&track_id, |d, t| listener.on_progress(d, t))?;
        Ok(path.to_string_lossy().into_owned())
    }

    pub fn download_playlist(&self, playlist_id: String, listener: Arc<dyn DownloadListener>) -> Result<PlaylistDownloadResult> {
        let (downloaded, failures) = self
            .inner
            .read()
            .unwrap()
            .download_playlist(&playlist_id, |i, n, t| listener.on_track(i as u32, n as u32, t.title.clone()))?;
        Ok(PlaylistDownloadResult {
            downloaded,
            failures: failures
                .into_iter()
                .map(|(id, e)| DownloadFailure {
                    track_id: id,
                    message: e.to_string(),
                })
                .collect(),
        })
    }

    pub fn remove_download(&self, track_id: String) -> Result<()> {
        Ok(self.inner.read().unwrap().remove_download(&track_id)?)
    }

    // Flow

    pub fn flow(&self) -> Result<FlowFeed> {
        let f = self.inner.read().unwrap().api().flow()?;
        Ok(FlowFeed {
            seeds: f.seeds,
            tracks: f.tracks.into_iter().map(FlowTrack::from).collect(),
        })
    }

    pub fn flow_restricted(&self) -> Result<FlowRestricted> {
        Ok(self.inner.read().unwrap().api().flow_restricted()?.into())
    }

    /// `block_artist`: true bloqueia o artista; false recusa só a música
    pub fn flow_dislike(&self, track: FlowTrack, block_artist: bool) -> Result<FlowRestricted> {
        let scope = if block_artist {
            core::DislikeScope::Artist
        } else {
            core::DislikeScope::Track
        };
        Ok(self.inner.read().unwrap().api().flow_dislike(&track.into(), scope)?.into())
    }

    pub fn flow_restore_track(&self, video_id: String) -> Result<FlowRestricted> {
        Ok(self.inner.read().unwrap().api().flow_restore_track(&video_id)?.into())
    }

    pub fn flow_unblock_artist(&self, key: String) -> Result<FlowRestricted> {
        Ok(self.inner.read().unwrap().api().flow_unblock_artist(&key)?.into())
    }

    /// Pede ao servidor para baixar a faixa do Flow para a biblioteca
    pub fn flow_save(&self, track: FlowTrack) -> Result<()> {
        self.inner.read().unwrap().api().flow_save_to_library(&track.into())?;
        Ok(())
    }
}
