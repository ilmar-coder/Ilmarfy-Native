//! Tipos que espelham o JSON da API do servidor (handlers/spotify.go e handlers/auth.go).
//! Todos os campos têm padrão, para o app não quebrar se o servidor omitir algo.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    pub cover_url: String,
    pub play_count: i64,
    pub filename: String,
    /// Segundos
    pub duration: i64,
    /// Unix, segundos
    pub date_added: i64,
    pub restricted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub track_ids: Vec<String>,
}

/// Nome da playlist que o Ilmarfy web usa como "Favoritos"
pub const FAVORITES_PLAYLIST: &str = "Favoritos 2026";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub current: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RegisteredDevice {
    pub token: String,
    pub device: Device,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ApiVersion {
    pub api_version: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    #[serde(rename = "plainLyrics")]
    pub plain: String,
    #[serde(rename = "syncedLyrics")]
    pub synced: String,
    pub instrumental: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

impl Lyrics {
    /// Linhas sincronizadas no formato LRC ("[01:23.45] texto"), em ordem de tempo.
    pub fn synced_lines(&self) -> Vec<LyricLine> {
        let mut lines = Vec::new();
        for raw in self.synced.lines() {
            let mut rest = raw.trim();
            let mut stamps = Vec::new();
            // Uma linha pode ter vários tempos: "[00:10.00][01:20.00] refrão"
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(end) = stripped.find(']') else { break };
                if let Some(ms) = parse_lrc_time(&stripped[..end]) {
                    stamps.push(ms);
                }
                rest = stripped[end + 1..].trim_start();
            }
            for ms in stamps {
                lines.push(LyricLine {
                    time_ms: ms,
                    text: rest.to_string(),
                });
            }
        }
        lines.sort_by_key(|l| l.time_ms);
        lines
    }
}

fn parse_lrc_time(s: &str) -> Option<u64> {
    let (min, sec) = s.split_once(':')?;
    let min: u64 = min.trim().parse().ok()?;
    let sec: f64 = sec.trim().parse().ok()?;
    Some(min * 60_000 + (sec * 1000.0).round() as u64)
}

/// Faixa sugerida pelo Flow (ainda não está na biblioteca; toca pela prévia do YouTube)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowTrack {
    /// videoId do YouTube
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: String,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FlowResponse {
    pub seeds: Vec<String>,
    pub tracks: Vec<FlowTrack>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct BlockedArtist {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct DislikedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub at: i64,
}

/// Restritos do Flow: músicas recusadas e artistas bloqueados
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FlowRestricted {
    pub blocked_artists: Vec<BlockedArtist>,
    pub disliked_tracks: Vec<DislikedTrack>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DislikeScope {
    /// Só esta música
    Track,
    /// Bloqueia o artista no Flow
    Artist,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PlaysResult {
    pub accepted: u32,
    pub duplicates: u32,
    pub unknown: u32,
}

/// Play registrado no aparelho, aguardando envio ao servidor
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingPlay {
    pub id: String,
    pub track_id: String,
    pub played_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtistSummary {
    pub name: String,
    pub track_count: u32,
    /// Capa de uma das faixas, para usar enquanto não há foto do artista
    pub cover_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumSummary {
    pub name: String,
    pub artist: String,
    pub year: String,
    pub cover_url: String,
    pub track_count: u32,
}

/// Resultado de busca no YouTube Music (para adicionar à biblioteca)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct YtTrack {
    /// videoId do YouTube
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(rename = "albumId")]
    pub album_id: String,
    pub duration_ms: i64,
    pub duration_str: String,
    pub cover: String,
}

impl YtTrack {
    /// Como faixa do Flow, para tocar a prévia no mesmo player
    pub fn as_flow(&self) -> FlowTrack {
        FlowTrack {
            id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            cover: self.cover.clone(),
            duration_ms: self.duration_ms,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct YtAlbum {
    pub title: String,
    pub artist: String,
    pub cover: String,
    #[serde(deserialize_with = "null_as_empty")]
    pub tracks: Vec<YtTrack>,
}

/// Item da fila de downloads do servidor
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadTask {
    pub id: String,
    pub query: String,
    pub title: String,
    pub artist: String,
    /// queued, downloading, paused, completed, error, cancelled…
    pub status: String,
    pub progress: f64,
    pub speed: String,
    pub eta: String,
    pub error: String,
}

impl DownloadTask {
    pub fn is_active(&self) -> bool {
        !matches!(
            self.status.as_str(),
            "completed" | "done" | "error" | "failed" | "cancelled" | "canceled"
        )
    }
}

/// Campos editáveis de uma faixa
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrackEdit {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: String,
    #[serde(rename = "coverUrl")]
    pub cover_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuplicateCluster {
    pub key: String,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Duplicates {
    pub total_clusters: u32,
    /// O servidor manda `null` quando não há nenhuma
    #[serde(deserialize_with = "null_as_empty")]
    pub duplicates: Vec<DuplicateCluster>,
}

fn null_as_empty<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letra_sincronizada_com_varios_tempos() {
        let l = Lyrics {
            synced: "[00:12.50] Primeira\n[01:00.00][00:05.00] Refrão\nlixo sem tempo\n[xx:yy] inválida".into(),
            ..Default::default()
        };
        let lines = l.synced_lines();
        assert_eq!(
            lines,
            vec![
                LyricLine {
                    time_ms: 5_000,
                    text: "Refrão".into()
                },
                LyricLine {
                    time_ms: 12_500,
                    text: "Primeira".into()
                },
                LyricLine {
                    time_ms: 60_000,
                    text: "Refrão".into()
                },
            ]
        );
    }

    #[test]
    fn faixa_aceita_campos_faltando() {
        let t: Track = serde_json::from_str(r#"{"id":"1","title":"Roots"}"#).unwrap();
        assert_eq!(t.title, "Roots");
        assert!(!t.restricted && t.play_count == 0);
    }
}
