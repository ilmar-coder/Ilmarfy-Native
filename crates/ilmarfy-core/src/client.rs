//! `Ilmarfy`: o ponto de entrada que os apps usam (API + cache local + offline).

use crate::api::{Api, Fetched};
use crate::error::{Error, Result};
use crate::library::Library;
use crate::models::*;
use std::path::{Path, PathBuf};

const ETAG_TRACKS: &str = "etag:tracks";
const ETAG_PLAYLISTS: &str = "etag:playlists";
const LAST_SYNC: &str = "last_sync";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// De onde tocar uma faixa
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Arquivo baixado no aparelho
    Local(PathBuf),
    /// Streaming; o player precisa enviar o cabeçalho
    Remote { url: String, header: Option<(String, String)> },
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SyncReport {
    pub tracks_updated: bool,
    pub playlists_updated: bool,
    pub track_count: u32,
    pub plays_sent: u32,
}

pub struct Ilmarfy {
    api: Api,
    library: Library,
    music_dir: PathBuf,
}

impl Ilmarfy {
    /// `data_dir`: pasta privada do app (banco e músicas baixadas). `token`: o salvo no login, se houver.
    pub fn open(data_dir: &Path, server: &str, token: Option<String>) -> Result<Self> {
        let mut api = Api::new(server);
        api.set_token(token);
        Ok(Self {
            api,
            library: Library::open(&data_dir.join("library.sqlite"))?,
            music_dir: data_dir.join("music"),
        })
    }

    pub fn api(&self) -> &Api {
        &self.api
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    // ------------------------------------------------------------ Conta

    /// Faz login neste aparelho e devolve o token, que o app deve guardar com segurança
    /// (Keystore no Android, Keychain no Mac). A senha não é guardada.
    pub fn login(&mut self, password: &str, device_name: &str, platform: &str) -> Result<String> {
        let reg = self.api.register_device(password, device_name, platform)?;
        self.api.set_token(Some(reg.token.clone()));
        Ok(reg.token)
    }

    /// Sai: revoga o token no servidor (se der) e esquece localmente
    pub fn logout(&mut self) -> Result<()> {
        let result = self.api.revoke_device("current");
        self.api.set_token(None);
        match result {
            Ok(()) | Err(Error::Unauthorized(_)) | Err(Error::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Revoga o token deste aparelho no servidor sem mexer no estado local
    /// (para quem guarda o núcleo compartilhado e vai descartá-lo em seguida)
    pub fn revoke_this_device(&self) -> Result<()> {
        match self.api.revoke_device("current") {
            Ok(()) | Err(Error::Unauthorized(_)) | Err(Error::NotFound(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn is_logged_in(&self) -> bool {
        self.api.auth_header().is_some()
    }

    // ------------------------------------------------------------ Sincronização

    /// Envia plays pendentes e baixa biblioteca/playlists só se mudaram (ETag).
    pub fn sync(&self) -> Result<SyncReport> {
        let mut report = SyncReport {
            plays_sent: self.flush_plays()?,
            ..Default::default()
        };

        // Banco vazio (ex.: dados apagados) ignora o ETag salvo para forçar o download
        let empty = self.library.track_count()? == 0;
        let tag = if empty { None } else { self.library.meta(ETAG_TRACKS)? };
        if let Fetched::Updated { data, etag } = self.api.tracks(tag.as_deref())? {
            self.library.replace_tracks(&data)?;
            self.library.set_meta(ETAG_TRACKS, etag.as_deref())?;
            report.tracks_updated = true;
        }

        let tag = if self.library.playlists()?.is_empty() {
            None
        } else {
            self.library.meta(ETAG_PLAYLISTS)?
        };
        if let Fetched::Updated { data, etag } = self.api.playlists(tag.as_deref())? {
            self.library.replace_playlists(&data)?;
            self.library.set_meta(ETAG_PLAYLISTS, etag.as_deref())?;
            report.playlists_updated = true;
        }

        self.library.set_meta(LAST_SYNC, Some(&now().to_string()))?;
        report.track_count = self.library.track_count()?;
        Ok(report)
    }

    pub fn last_sync(&self) -> Result<Option<i64>> {
        Ok(self.library.meta(LAST_SYNC)?.and_then(|v| v.parse().ok()))
    }

    /// Registra que a faixa foi ouvida (vale offline; vai ao servidor na próxima sincronização)
    pub fn record_play(&self, track_id: &str) -> Result<()> {
        self.library.record_play(track_id, now())?;
        Ok(())
    }

    /// Envia os plays pendentes em lotes. Sem conexão, mantém tudo para a próxima vez.
    pub fn flush_plays(&self) -> Result<u32> {
        let mut sent = 0;
        loop {
            let batch = self.library.pending_plays(500)?;
            if batch.is_empty() {
                return Ok(sent);
            }
            match self.api.send_plays(&batch) {
                Ok(res) => {
                    sent += res.accepted;
                    // Aceitos, repetidos e de faixas apagadas: nenhum precisa ser reenviado
                    self.library
                        .remove_pending_plays(&batch.iter().map(|p| p.id.clone()).collect::<Vec<_>>())?;
                }
                Err(Error::Network(_)) => return Ok(sent),
                Err(e) => return Err(e),
            }
        }
    }

    // ------------------------------------------------------------ Reprodução e offline

    /// Arquivo baixado se existir; senão, streaming
    pub fn source(&self, track: &Track) -> Result<Source> {
        if let Some(path) = self.library.downloaded_path(&track.id)? {
            if path.is_file() {
                return Ok(Source::Local(path));
            }
            // Arquivo sumiu (limpeza do sistema, cartão removido…)
            self.library.unmark_downloaded(&track.id)?;
        }
        Ok(Source::Remote {
            url: self.api.track_stream_url(track),
            header: self.api.auth_header(),
        })
    }

    fn local_file(&self, track: &Track) -> PathBuf {
        let ext = Path::new(&track.filename).extension().and_then(|e| e.to_str()).unwrap_or("mp3");
        // Nome pelo id: evita caracteres problemáticos e colisões
        let safe_id: String = track
            .id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        self.music_dir.join(format!("{safe_id}.{ext}"))
    }

    pub fn is_downloaded(&self, track_id: &str) -> Result<bool> {
        Ok(self.library.downloaded_path(track_id)?.is_some_and(|p| p.is_file()))
    }

    /// Baixa a faixa para ouvir offline. `progress(baixados, total)`; devolva false para cancelar.
    pub fn download(&self, track_id: &str, progress: impl FnMut(u64, u64) -> bool) -> Result<PathBuf> {
        let track = self
            .library
            .track(track_id)?
            .ok_or_else(|| Error::NotFound(format!("faixa {track_id}")))?;
        if self.is_downloaded(track_id)? {
            return Ok(self.library.downloaded_path(track_id)?.unwrap());
        }
        let dest = self.local_file(&track);
        let size = self.api.download_track(&track, &dest, progress)?;
        self.library.mark_downloaded(track_id, &dest, size, now())?;
        Ok(dest)
    }

    /// Baixa todas as faixas da playlist que ainda não estão no aparelho.
    /// `on_track(índice, total, faixa)` é chamado antes de cada uma; devolva false para parar.
    /// Devolve quantas foram baixadas agora; erros de faixas isoladas não interrompem as demais.
    pub fn download_playlist(
        &self,
        playlist_id: &str,
        on_track: impl FnMut(usize, usize, &Track) -> bool,
    ) -> Result<(u32, Vec<(String, Error)>)> {
        let tracks = self.library.playlist_tracks(playlist_id)?;
        self.download_tracks(&tracks, on_track)
    }

    /// Baixa as faixas (de qualquer lista ou seleção) que ainda não estão no aparelho.
    /// Mesmo contrato de [`Self::download_playlist`].
    pub fn download_tracks(
        &self,
        tracks: &[Track],
        mut on_track: impl FnMut(usize, usize, &Track) -> bool,
    ) -> Result<(u32, Vec<(String, Error)>)> {
        let mut downloaded = 0;
        let mut failures = Vec::new();
        for (i, t) in tracks.iter().enumerate() {
            if !on_track(i, tracks.len(), t) {
                break;
            }
            if self.is_downloaded(&t.id)? {
                continue;
            }
            match self.download(&t.id, |_, _| true) {
                Ok(_) => downloaded += 1,
                Err(e) => failures.push((t.id.clone(), e)),
            }
        }
        Ok((downloaded, failures))
    }

    pub fn remove_download(&self, track_id: &str) -> Result<()> {
        if let Some(path) = self.library.unmark_downloaded(track_id)? {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ Playlists e favoritos

    pub fn create_playlist(&self, name: &str) -> Result<Playlist> {
        let p = self.api.create_playlist(name)?;
        self.library.upsert_playlist(&p)?;
        Ok(p)
    }

    pub fn delete_playlist(&self, playlist_id: &str) -> Result<()> {
        self.api.delete_playlist(playlist_id)?;
        self.library.remove_playlist(playlist_id)
    }

    pub fn add_to_playlist(&self, playlist_id: &str, track_id: &str) -> Result<Playlist> {
        let p = self.api.add_to_playlist(playlist_id, track_id)?;
        self.library.upsert_playlist(&p)?;
        Ok(p)
    }

    /// Move a faixa para antes de `before` (ou para o fim). Usa a lista completa da playlist,
    /// já que a tela pode estar mostrando só parte dela.
    pub fn move_in_playlist(&self, playlist_id: &str, track_id: &str, before: Option<&str>) -> Result<Playlist> {
        let current = self
            .library
            .playlists()?
            .into_iter()
            .find(|p| p.id == playlist_id)
            .ok_or_else(|| Error::NotFound("playlist".into()))?;
        let order = moved(&current.track_ids, track_id, before);
        let p = self.api.reorder_playlist(playlist_id, &order)?;
        self.library.upsert_playlist(&p)?;
        Ok(p)
    }

    pub fn remove_from_playlist(&self, playlist_id: &str, track_id: &str) -> Result<()> {
        self.api.remove_from_playlist(playlist_id, track_id)?;
        // A rota de remoção não devolve a playlist; atualiza a cópia local na mão
        if let Some(mut p) = self.library.playlists()?.into_iter().find(|p| p.id == playlist_id) {
            p.track_ids.retain(|id| id != track_id);
            self.library.upsert_playlist(&p)?;
        }
        Ok(())
    }

    /// Marca ou desmarca a faixa como restrita (some da biblioteca e vai para "Restritas").
    pub fn set_restricted(&self, track_id: &str, restricted: bool) -> Result<()> {
        if let Some(t) = self.library.track(track_id)?
            && t.restricted == restricted
        {
            return Ok(());
        }
        let mut t = self.api.toggle_restricted(track_id)?;
        if t.restricted != restricted {
            // a cópia local estava desatualizada: o servidor inverteu para o lado errado
            t = self.api.toggle_restricted(track_id)?;
        }
        self.library.set_restricted(track_id, t.restricted)
    }

    pub fn is_favorite(&self, track_id: &str) -> Result<bool> {
        Ok(self
            .library
            .favorites_playlist()?
            .is_some_and(|p| p.track_ids.iter().any(|id| id == track_id)))
    }

    /// Liga/desliga favorito (cria a playlist "Favoritos 2026" se não existir). Devolve o novo estado.
    pub fn toggle_favorite(&self, track_id: &str) -> Result<bool> {
        let fav = match self.library.favorites_playlist()? {
            Some(p) => p,
            None => self.create_playlist(FAVORITES_PLAYLIST)?,
        };
        if fav.track_ids.iter().any(|id| id == track_id) {
            self.remove_from_playlist(&fav.id, track_id)?;
            Ok(false)
        } else {
            self.add_to_playlist(&fav.id, track_id)?;
            Ok(true)
        }
    }
}

/// `ids` com `id` tirado de onde estava e posto antes de `before` (ou no fim)
fn moved(ids: &[String], id: &str, before: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().filter(|x| *x != id).cloned().collect();
    if !ids.iter().any(|x| x == id) {
        return ids.to_vec();
    }
    let at = before.and_then(|b| out.iter().position(|x| x == b)).unwrap_or(out.len());
    out.insert(at, id.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_coloca_antes_da_faixa_alvo() {
        let ids: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(moved(&ids, "d", Some("b")), ["a", "d", "b", "c"]);
        assert_eq!(moved(&ids, "a", Some("d")), ["b", "c", "a", "d"]);
        assert_eq!(moved(&ids, "b", None), ["a", "c", "d", "b"]);
        assert_eq!(moved(&ids, "x", Some("a")), ids);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ilmarfy-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn source_cai_para_streaming_quando_o_arquivo_some() {
        let dir = temp_dir("source");
        let app = Ilmarfy::open(&dir, "https://exemplo.com", Some("ilf_x".into())).unwrap();
        let track = Track {
            id: "t/1".into(),
            filename: "A B.mp3".into(),
            ..Default::default()
        };
        app.library().replace_tracks(std::slice::from_ref(&track)).unwrap();

        let file = app.local_file(&track);
        assert_eq!(file.file_name().unwrap(), "t_1.mp3", "id vira nome de arquivo seguro");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, b"mp3").unwrap();
        app.library().mark_downloaded("t/1", &file, 3, 0).unwrap();
        assert_eq!(app.source(&track).unwrap(), Source::Local(file.clone()));

        std::fs::remove_file(&file).unwrap();
        match app.source(&track).unwrap() {
            Source::Remote { url, header } => {
                assert_eq!(url, "https://exemplo.com/api/spotify/tracks/A%20B.mp3");
                assert_eq!(header.unwrap().1, "Bearer ilf_x");
            }
            other => panic!("esperava streaming, veio {other:?}"),
        }
        assert!(!app.is_downloaded("t/1").unwrap(), "registro limpo quando o arquivo some");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn plays_ficam_guardados_sem_conexao() {
        let dir = temp_dir("plays");
        // Porta fechada: simula estar offline
        let app = Ilmarfy::open(&dir, "http://127.0.0.1:9", Some("ilf_x".into())).unwrap();
        app.library()
            .replace_tracks(&[Track {
                id: "a".into(),
                ..Default::default()
            }])
            .unwrap();
        app.record_play("a").unwrap();
        assert_eq!(app.flush_plays().unwrap(), 0);
        assert_eq!(app.library().pending_plays(10).unwrap().len(), 1, "o play continua pendente");
        let _ = std::fs::remove_dir_all(dir);
    }
}
