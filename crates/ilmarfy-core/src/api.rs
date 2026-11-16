//! Cliente HTTP da API do Ilmarfy (síncrono: chame fora da thread de interface).

use crate::error::{Error, Result};
use crate::models::*;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub const DEFAULT_SERVER: &str = "https://download.ilmar.com.br";

/// Resultado de uma busca com ETag
#[derive(Debug)]
pub enum Fetched<T> {
    /// O conteúdo não mudou desde o ETag informado
    NotModified,
    Updated {
        data: T,
        etag: Option<String>,
    },
}

#[derive(Clone)]
pub struct Api {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
    /// Downloads de faixas podem demorar; não usam o timeout curto
    download_agent: ureq::Agent,
}

fn make_agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .user_agent(concat!("Ilmarfy-Native/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Codifica um segmento de caminho de URL (nomes de arquivo têm espaços, acentos, etc.)
pub fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn network(e: ureq::Error) -> Error {
    Error::Network(e.to_string())
}

impl Api {
    pub fn new(base_url: &str) -> Self {
        Self {
            base: base_url.trim_end_matches('/').to_string(),
            token: None,
            agent: make_agent(Some(Duration::from_secs(60))),
            download_agent: make_agent(None),
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn set_token(&mut self, token: Option<String>) {
        self.token = token;
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Cabeçalho para o player tocar as faixas por streaming (ExoPlayer/rodio precisam dele)
    pub fn auth_header(&self) -> Option<(String, String)> {
        self.token.as_ref().map(|t| ("Authorization".to_string(), format!("Bearer {t}")))
    }

    pub fn track_stream_url(&self, track: &Track) -> String {
        format!("{}/api/spotify/tracks/{}", self.base, encode_path_segment(&track.filename))
    }

    /// Prévia (YouTube) de uma faixa do Flow
    pub fn preview_url(&self, video_id: &str) -> String {
        format!("{}/api/ytmusic/preview?id={}", self.base, encode_path_segment(video_id))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn authed<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        match &self.token {
            Some(t) => req.header("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    /// Converte status de erro em `Error`, lendo a mensagem `{"error": "..."}` do servidor
    fn check(mut resp: ureq::http::Response<ureq::Body>) -> Result<ureq::http::Response<ureq::Body>> {
        let status = resp.status().as_u16();
        if status < 400 {
            return Ok(resp);
        }
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or(body);
        Err(match status {
            401 => Error::Unauthorized(message),
            404 => Error::NotFound(message),
            429 => Error::RateLimited(message),
            _ => Error::Http { status, message },
        })
    }

    fn read_json<T: DeserializeOwned>(mut resp: ureq::http::Response<ureq::Body>) -> Result<T> {
        resp.body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_json()
            .map_err(|e| Error::Decode(e.to_string()))
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let resp = self.authed(self.agent.get(&self.url(path))).call().map_err(network)?;
        Self::read_json(Self::check(resp)?)
    }

    fn get_with_etag<T: DeserializeOwned>(&self, path: &str, etag: Option<&str>) -> Result<Fetched<T>> {
        let mut req = self.authed(self.agent.get(&self.url(path)));
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        let resp = Self::check(req.call().map_err(network)?)?;
        if resp.status().as_u16() == 304 {
            return Ok(Fetched::NotModified);
        }
        let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).map(String::from);
        Ok(Fetched::Updated {
            data: Self::read_json(resp)?,
            etag,
        })
    }

    fn send<T: DeserializeOwned>(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Result<T> {
        let url = self.url(path);
        let resp = match (method, body) {
            ("POST", Some(b)) => self.authed(self.agent.post(&url)).send_json(&b),
            ("POST", None) => self.authed(self.agent.post(&url)).send_empty(),
            ("PUT", Some(b)) => self.authed(self.agent.put(&url)).send_json(&b),
            ("PUT", None) => self.authed(self.agent.put(&url)).send_empty(),
            ("DELETE", _) => self.authed(self.agent.delete(&url)).call(),
            _ => unreachable!("método não suportado: {method}"),
        }
        .map_err(network)?;
        Self::read_json(Self::check(resp)?)
    }

    // ------------------------------------------------------------ Conta e dispositivos

    pub fn version(&self) -> Result<ApiVersion> {
        self.get("/api/version")
    }

    /// Troca a senha por um token deste aparelho. Guarde o token; a senha não deve ser armazenada.
    pub fn register_device(&self, password: &str, name: &str, platform: &str) -> Result<RegisteredDevice> {
        self.send(
            "POST",
            "/api/auth/device",
            Some(json!({ "password": password, "name": name, "platform": platform })),
        )
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        self.get("/api/auth/devices")
    }

    /// Revoga um aparelho; use "current" para sair deste
    pub fn revoke_device(&self, id: &str) -> Result<()> {
        self.send::<serde_json::Value>("DELETE", &format!("/api/auth/devices/{}", encode_path_segment(id)), None)?;
        Ok(())
    }

    // ------------------------------------------------------------ Biblioteca

    pub fn tracks(&self, etag: Option<&str>) -> Result<Fetched<Vec<Track>>> {
        self.get_with_etag("/api/spotify/tracks", etag)
    }

    pub fn playlists(&self, etag: Option<&str>) -> Result<Fetched<Vec<Playlist>>> {
        self.get_with_etag("/api/spotify/playlists", etag)
    }

    /// Alterna a faixa entre restrita e normal no servidor (o servidor só sabe inverter)
    pub fn toggle_restricted(&self, track_id: &str) -> Result<Track> {
        self.send(
            "PUT",
            &format!("/api/spotify/tracks/{}/restrict", encode_path_segment(track_id)),
            None,
        )
    }

    pub fn create_playlist(&self, name: &str) -> Result<Playlist> {
        self.send("POST", "/api/spotify/playlists", Some(json!({ "name": name })))
    }

    pub fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.send::<serde_json::Value>(
            "DELETE",
            &format!("/api/spotify/playlists/{}", encode_path_segment(playlist_id)),
            None,
        )?;
        Ok(())
    }

    pub fn add_to_playlist(&self, playlist_id: &str, track_id: &str) -> Result<Playlist> {
        let path = format!("/api/spotify/playlists/{}/tracks", encode_path_segment(playlist_id));
        self.send("POST", &path, Some(json!({ "track_id": track_id })))
    }

    pub fn remove_from_playlist(&self, playlist_id: &str, track_id: &str) -> Result<serde_json::Value> {
        let path = format!(
            "/api/spotify/playlists/{}/tracks/{}",
            encode_path_segment(playlist_id),
            encode_path_segment(track_id)
        );
        self.send("DELETE", &path, None)
    }

    /// Letra da faixa (None se não houver)
    pub fn lyrics(&self, track_id: &str) -> Result<Option<Lyrics>> {
        match self.get(&format!("/api/spotify/track-lyrics/{}", encode_path_segment(track_id))) {
            Ok(l) => Ok(Some(l)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Envia plays acumulados (o servidor ignora ids repetidos)
    pub fn send_plays(&self, plays: &[PendingPlay]) -> Result<PlaysResult> {
        self.send("POST", "/api/spotify/plays", Some(json!({ "plays": plays })))
    }

    // ------------------------------------------------------------ Flow

    pub fn flow(&self) -> Result<FlowResponse> {
        self.get("/api/spotify/flow")
    }

    pub fn flow_restricted(&self) -> Result<FlowRestricted> {
        self.get("/api/spotify/flow/feedback")
    }

    pub fn flow_dislike(&self, track: &FlowTrack, scope: DislikeScope) -> Result<FlowRestricted> {
        let scope = match scope {
            DislikeScope::Track => "track",
            DislikeScope::Artist => "artist",
        };
        self.send(
            "POST",
            "/api/spotify/flow/dislike",
            Some(json!({ "id": track.id, "title": track.title, "artist": track.artist, "scope": scope })),
        )
    }

    pub fn flow_restore_track(&self, video_id: &str) -> Result<FlowRestricted> {
        self.send(
            "DELETE",
            &format!("/api/spotify/flow/dislike/{}", encode_path_segment(video_id)),
            None,
        )
    }

    pub fn flow_unblock_artist(&self, key: &str) -> Result<FlowRestricted> {
        self.send(
            "DELETE",
            &format!("/api/spotify/flow/blocked-artists/{}", encode_path_segment(key)),
            None,
        )
    }

    /// Pede ao servidor para baixar uma faixa do Flow para a biblioteca
    pub fn flow_save_to_library(&self, track: &FlowTrack) -> Result<serde_json::Value> {
        self.send(
            "POST",
            "/api/spotify/download/start",
            Some(json!({
                "query": format!("https://www.youtube.com/watch?v={}", track.id),
                "title": track.title,
                "artist": track.artist,
                "duration_ms": track.duration_ms,
            })),
        )
    }

    // ------------------------------------------------------------ Arquivos

    /// Baixa o arquivo da faixa para `dest` (grava em `.part` e renomeia no fim).
    /// `progress(baixados, total)` é chamado durante o download; devolva `false` para cancelar.
    pub fn download_track(&self, track: &Track, dest: &Path, progress: impl FnMut(u64, u64) -> bool) -> Result<u64> {
        self.download_url(&self.track_stream_url(track), dest, progress)
    }

    /// Baixa a prévia (áudio do YouTube, via servidor) de uma faixa do Flow
    pub fn download_preview(&self, video_id: &str, dest: &Path, progress: impl FnMut(u64, u64) -> bool) -> Result<u64> {
        self.download_url(&self.preview_url(video_id), dest, progress)
    }

    /// Baixa uma URL do servidor (com o token) para `dest`, gravando em `.part` e renomeando no fim
    pub fn download_url(&self, url: &str, dest: &Path, mut progress: impl FnMut(u64, u64) -> bool) -> Result<u64> {
        let resp = self.authed(self.download_agent.get(url)).call().map_err(network)?;
        let mut resp = Self::check(resp)?;
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = dest.with_extension("part");
        let mut file = std::fs::File::create(&part)?;
        let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    drop(file);
                    let _ = std::fs::remove_file(&part);
                    return Err(Error::Network(e.to_string()));
                }
            };
            file.write_all(&buf[..n])?;
            done += n as u64;
            if !progress(done, total) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(Error::Network("download cancelado".into()));
            }
        }
        file.sync_all()?;
        drop(file);
        if total > 0 && done != total {
            let _ = std::fs::remove_file(&part);
            return Err(Error::Network(format!("download incompleto ({done} de {total} bytes)")));
        }
        std::fs::rename(&part, dest)?;
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codifica_nome_de_arquivo() {
        assert_eq!(
            encode_path_segment("Sepultura - Roots Bloody Roots.mp3"),
            "Sepultura%20-%20Roots%20Bloody%20Roots.mp3"
        );
        assert_eq!(encode_path_segment("Ação/É"), "A%C3%A7%C3%A3o%2F%C3%89");
    }

    #[test]
    fn url_de_streaming_e_cabecalho() {
        let api = Api::new("https://exemplo.com/").with_token("ilf_abc");
        let t = Track {
            filename: "A B.mp3".into(),
            ..Default::default()
        };
        assert_eq!(api.track_stream_url(&t), "https://exemplo.com/api/spotify/tracks/A%20B.mp3");
        assert_eq!(api.auth_header(), Some(("Authorization".into(), "Bearer ilf_abc".into())));
    }
}
