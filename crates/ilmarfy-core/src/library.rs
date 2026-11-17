//! Cache local da biblioteca em SQLite: funciona offline e responde às telas na hora.

use crate::error::Result;
use crate::models::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use unicode_normalization::UnicodeNormalization;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tracks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, year TEXT NOT NULL,
    cover_url TEXT NOT NULL, play_count INTEGER NOT NULL, filename TEXT NOT NULL,
    duration INTEGER NOT NULL, date_added INTEGER NOT NULL, restricted INTEGER NOT NULL,
    search TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS tracks_artist ON tracks(artist COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS playlists (id TEXT PRIMARY KEY, name TEXT NOT NULL, position INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS playlist_tracks (
    playlist_id TEXT NOT NULL, position INTEGER NOT NULL, track_id TEXT NOT NULL,
    PRIMARY KEY (playlist_id, position)
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS pending_plays (id TEXT PRIMARY KEY, track_id TEXT NOT NULL, played_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS downloads (
    track_id TEXT PRIMARY KEY, path TEXT NOT NULL, size INTEGER NOT NULL, downloaded_at INTEGER NOT NULL
);
";

/// Texto para busca: minúsculo e sem acentos ("Ação" -> "acao")
pub fn normalize(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
}

/// Ordem das listas de faixas
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackOrder {
    Title,
    Artist,
    RecentlyAdded,
    MostPlayed,
}

pub struct Library {
    conn: Mutex<Connection>,
}

const TRACK_COLS: &str = "id, title, artist, album, year, cover_url, play_count, filename, duration, date_added, restricted";

fn row_to_track(r: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        album: r.get(3)?,
        year: r.get(4)?,
        cover_url: r.get(5)?,
        play_count: r.get(6)?,
        filename: r.get(7)?,
        duration: r.get(8)?,
        date_added: r.get(9)?,
        restricted: r.get::<_, i64>(10)? != 0,
    })
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    /// Banco em memória (testes)
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().unwrap();
        Ok(f(&mut conn)?)
    }

    // ------------------------------------------------------------ Metadados (ETags etc.)

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| c.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional())
    }

    pub fn set_meta(&self, key: &str, value: Option<&str>) -> Result<()> {
        self.with(|c| match value {
            Some(v) => c.execute(
                "INSERT INTO meta(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![key, v],
            ),
            None => c.execute("DELETE FROM meta WHERE key = ?1", [key]),
        })?;
        Ok(())
    }

    // ------------------------------------------------------------ Escrita da sincronização

    /// Substitui a biblioteca inteira pela versão do servidor (numa transação)
    pub fn replace_tracks(&self, tracks: &[Track]) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM tracks", [])?;
            {
                let mut stmt = tx.prepare(&format!(
                    "INSERT OR REPLACE INTO tracks({TRACK_COLS}, search) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
                ))?;
                for t in tracks {
                    let search = normalize(&format!("{} {} {}", t.title, t.artist, t.album));
                    stmt.execute(params![
                        t.id,
                        t.title,
                        t.artist,
                        t.album,
                        t.year,
                        t.cover_url,
                        t.play_count,
                        t.filename,
                        t.duration,
                        t.date_added,
                        t.restricted as i64,
                        search
                    ])?;
                }
            }
            tx.commit()
        })
    }

    pub fn replace_playlists(&self, playlists: &[Playlist]) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM playlists", [])?;
            tx.execute("DELETE FROM playlist_tracks", [])?;
            for (i, p) in playlists.iter().enumerate() {
                insert_playlist(&tx, p, i as i64)?;
            }
            tx.commit()
        })
    }

    /// Atualiza uma playlist depois de uma alteração feita no servidor
    pub fn upsert_playlist(&self, playlist: &Playlist) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            let position: i64 = tx
                .query_row("SELECT position FROM playlists WHERE id = ?1", [&playlist.id], |r| r.get(0))
                .optional()?
                .unwrap_or_else(|| {
                    tx.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM playlists", [], |r| r.get(0))
                        .unwrap_or(0)
                });
            tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [&playlist.id])?;
            insert_playlist(&tx, playlist, position)?;
            tx.commit()
        })
    }

    pub fn remove_playlist(&self, playlist_id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [playlist_id])?;
            c.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])
        })?;
        Ok(())
    }

    // ------------------------------------------------------------ Consultas para as telas

    pub fn track_count(&self) -> Result<u32> {
        self.with(|c| c.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)))
    }

    pub fn track(&self, id: &str) -> Result<Option<Track>> {
        self.with(|c| {
            c.query_row(&format!("SELECT {TRACK_COLS} FROM tracks WHERE id = ?1"), [id], row_to_track)
                .optional()
        })
    }

    /// Todas as faixas. Restritas ficam de fora, como no Ilmarfy web, a não ser que `include_restricted`.
    pub fn tracks(&self, order: TrackOrder, include_restricted: bool) -> Result<Vec<Track>> {
        let order_by = match order {
            TrackOrder::Title => "title COLLATE NOCASE",
            TrackOrder::Artist => "artist COLLATE NOCASE, album COLLATE NOCASE, title COLLATE NOCASE",
            TrackOrder::RecentlyAdded => "date_added DESC",
            TrackOrder::MostPlayed => "play_count DESC, title COLLATE NOCASE",
        };
        let filter = if include_restricted { "" } else { "WHERE restricted = 0" };
        self.query_tracks(&format!("SELECT {TRACK_COLS} FROM tracks {filter} ORDER BY {order_by}"), [])
    }

    pub fn set_restricted(&self, track_id: &str, restricted: bool) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE tracks SET restricted = ?2 WHERE id = ?1",
                params![track_id, restricted as i64],
            )
            .map(|_| ())
        })
    }

    pub fn restricted_tracks(&self) -> Result<Vec<Track>> {
        self.query_tracks(
            &format!("SELECT {TRACK_COLS} FROM tracks WHERE restricted = 1 ORDER BY title COLLATE NOCASE"),
            [],
        )
    }

    /// Busca por título, artista ou álbum, ignorando acentos e maiúsculas; todas as palavras precisam aparecer
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let words: Vec<String> = normalize(query)
            .split_whitespace()
            .map(|w| format!("%{}%", w.replace('%', "")))
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let cond = vec!["search LIKE ?"; words.len()].join(" AND ");
        let sql = format!(
            "SELECT {TRACK_COLS} FROM tracks WHERE restricted = 0 AND {cond} ORDER BY play_count DESC, title COLLATE NOCASE LIMIT {limit}"
        );
        self.query_tracks(&sql, rusqlite::params_from_iter(words.iter()))
    }

    pub fn artists(&self) -> Result<Vec<ArtistSummary>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                // Capa do artista: a que mais se repete nas músicas dele (desempate: a mais tocada).
                // Uma capa errada numa música só não vira a cara do artista.
                "SELECT t.artist, COUNT(*),
                        COALESCE((SELECT c.cover_url FROM tracks c
                                  WHERE c.artist = t.artist COLLATE NOCASE AND c.restricted = 0 AND c.cover_url <> ''
                                  GROUP BY c.cover_url ORDER BY COUNT(*) DESC, SUM(c.play_count) DESC LIMIT 1), '')
                 FROM tracks t WHERE t.restricted = 0 GROUP BY t.artist COLLATE NOCASE ORDER BY t.artist COLLATE NOCASE",
            )?;
            stmt.query_map([], |r| {
                Ok(ArtistSummary {
                    name: r.get(0)?,
                    track_count: r.get(1)?,
                    cover_url: r.get(2)?,
                })
            })?
            .collect()
        })
    }

    pub fn artist_tracks(&self, artist: &str) -> Result<Vec<Track>> {
        self.query_tracks(
            &format!(
                "SELECT {TRACK_COLS} FROM tracks WHERE artist = ?1 COLLATE NOCASE AND restricted = 0 ORDER BY year DESC, album COLLATE NOCASE, title COLLATE NOCASE"
            ),
            [artist],
        )
    }

    pub fn albums(&self, artist: &str) -> Result<Vec<AlbumSummary>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT album, MAX(artist), MAX(year), MAX(cover_url), COUNT(*) FROM tracks
                 WHERE artist = ?1 COLLATE NOCASE AND restricted = 0 AND album <> ''
                 GROUP BY album COLLATE NOCASE ORDER BY MAX(year) DESC",
            )?;
            stmt.query_map([artist], |r| {
                Ok(AlbumSummary {
                    name: r.get(0)?,
                    artist: r.get(1)?,
                    year: r.get(2)?,
                    cover_url: r.get(3)?,
                    track_count: r.get(4)?,
                })
            })?
            .collect()
        })
    }

    pub fn album_tracks(&self, artist: &str, album: &str) -> Result<Vec<Track>> {
        self.query_tracks(
            &format!(
                "SELECT {TRACK_COLS} FROM tracks WHERE artist = ?1 COLLATE NOCASE AND album = ?2 COLLATE NOCASE AND restricted = 0 ORDER BY title COLLATE NOCASE"
            ),
            [artist, album],
        )
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name FROM playlists ORDER BY position")?;
            let mut lists: Vec<Playlist> = stmt
                .query_map([], |r| {
                    Ok(Playlist {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        track_ids: Vec::new(),
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            let mut stmt = c.prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position")?;
            for p in &mut lists {
                p.track_ids = stmt.query_map([&p.id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            }
            Ok(lists)
        })
    }

    /// Faixas da playlist na ordem dela (ids que não existem mais na biblioteca são ignorados)
    pub fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>> {
        let cols = TRACK_COLS.split(", ").map(|c| format!("t.{c}")).collect::<Vec<_>>().join(", ");
        self.query_tracks(
            &format!(
                "SELECT {cols} FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id WHERE pt.playlist_id = ?1 ORDER BY pt.position"
            ),
            [playlist_id],
        )
    }

    /// A playlist "Favoritos 2026" usada pelo Ilmarfy web
    pub fn favorites_playlist(&self) -> Result<Option<Playlist>> {
        Ok(self
            .playlists()?
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(FAVORITES_PLAYLIST)))
    }

    fn query_tracks<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Track>> {
        self.with(|c| c.prepare(sql)?.query_map(params, row_to_track)?.collect())
    }

    // ------------------------------------------------------------ Plays pendentes (offline)

    /// Registra um play no aparelho; ele vai para o servidor na próxima sincronização
    pub fn record_play(&self, track_id: &str, played_at: i64) -> Result<PendingPlay> {
        let play = PendingPlay {
            id: uuid::Uuid::new_v4().to_string(),
            track_id: track_id.to_string(),
            played_at,
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO pending_plays(id, track_id, played_at) VALUES(?1, ?2, ?3)",
                params![play.id, play.track_id, play.played_at],
            )?;
            // Reflete na hora nas listas "mais tocadas"
            c.execute("UPDATE tracks SET play_count = play_count + 1 WHERE id = ?1", [track_id])
        })?;
        Ok(play)
    }

    pub fn pending_plays(&self, limit: u32) -> Result<Vec<PendingPlay>> {
        self.with(|c| {
            c.prepare(&format!(
                "SELECT id, track_id, played_at FROM pending_plays ORDER BY played_at LIMIT {limit}"
            ))?
            .query_map([], |r| {
                Ok(PendingPlay {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    played_at: r.get(2)?,
                })
            })?
            .collect()
        })
    }

    pub fn remove_pending_plays(&self, ids: &[String]) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            for id in ids {
                tx.execute("DELETE FROM pending_plays WHERE id = ?1", [id])?;
            }
            tx.commit()
        })
    }

    // ------------------------------------------------------------ Downloads (offline)

    pub fn mark_downloaded(&self, track_id: &str, path: &Path, size: u64, at: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO downloads(track_id, path, size, downloaded_at) VALUES(?1, ?2, ?3, ?4)",
                params![track_id, path.to_string_lossy(), size as i64, at],
            )
        })?;
        Ok(())
    }

    pub fn unmark_downloaded(&self, track_id: &str) -> Result<Option<PathBuf>> {
        let path = self.downloaded_path(track_id)?;
        self.with(|c| c.execute("DELETE FROM downloads WHERE track_id = ?1", [track_id]))?;
        Ok(path)
    }

    pub fn downloaded_path(&self, track_id: &str) -> Result<Option<PathBuf>> {
        let path: Option<String> = self.with(|c| {
            c.query_row("SELECT path FROM downloads WHERE track_id = ?1", [track_id], |r| r.get(0))
                .optional()
        })?;
        Ok(path.map(PathBuf::from))
    }

    pub fn downloaded_ids(&self) -> Result<Vec<String>> {
        self.with(|c| c.prepare("SELECT track_id FROM downloads")?.query_map([], |r| r.get(0))?.collect())
    }

    /// Bytes ocupados pelas músicas baixadas
    pub fn downloads_size(&self) -> Result<u64> {
        let size: i64 = self.with(|c| c.query_row("SELECT COALESCE(SUM(size), 0) FROM downloads", [], |r| r.get(0)))?;
        Ok(size as u64)
    }
}

fn insert_playlist(tx: &rusqlite::Transaction<'_>, p: &Playlist, position: i64) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO playlists(id, name, position) VALUES(?1, ?2, ?3)",
        params![p.id, p.name, position],
    )?;
    let mut stmt = tx.prepare("INSERT OR REPLACE INTO playlist_tracks(playlist_id, position, track_id) VALUES(?1, ?2, ?3)")?;
    for (i, track_id) in p.track_ids.iter().enumerate() {
        stmt.execute(params![p.id, i as i64, track_id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str, artist: &str, album: &str, plays: i64, restricted: bool) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            year: "1996".into(),
            play_count: plays,
            filename: format!("{artist} - {title}.mp3"),
            date_added: plays,
            restricted,
            ..Default::default()
        }
    }

    fn sample() -> Library {
        let lib = Library::in_memory().unwrap();
        lib.replace_tracks(&[
            t("1", "Roots Bloody Roots", "Sepultura", "Roots", 10, false),
            t("2", "Ratamahatta", "Sepultura", "Roots", 3, false),
            t("3", "Território", "Sepultura", "Chaos A.D.", 7, false),
            t("4", "Walk", "Pantera", "Vulgar Display of Power", 20, false),
            t("5", "Música restrita", "Pantera", "", 99, true),
        ])
        .unwrap();
        lib
    }

    #[test]
    fn busca_ignora_acentos_e_exige_todas_as_palavras() {
        let lib = sample();
        let ids = |q: &str| lib.search(q, 50).unwrap().into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids("territorio"), vec!["3"]);
        assert_eq!(ids("SEPULTURA roots"), vec!["1", "2"]);
        assert!(ids("restrita").is_empty(), "restritas não aparecem na busca");
        assert!(ids("   ").is_empty());
    }

    #[test]
    fn artistas_albuns_e_ordem() {
        let lib = sample();
        let artists = lib.artists().unwrap();
        assert_eq!(
            artists.iter().map(|a| (a.name.as_str(), a.track_count)).collect::<Vec<_>>(),
            vec![("Pantera", 1), ("Sepultura", 3)]
        );
        let albums = lib.albums("sepultura").unwrap();
        assert_eq!(albums.len(), 2);
        assert_eq!(lib.album_tracks("Sepultura", "Roots").unwrap().len(), 2);
        let most = lib.tracks(TrackOrder::MostPlayed, false).unwrap();
        assert_eq!(most.first().unwrap().id, "4");
        assert_eq!(lib.tracks(TrackOrder::Title, true).unwrap().len(), 5);
        assert_eq!(lib.restricted_tracks().unwrap().len(), 1);
    }

    #[test]
    fn capa_do_artista_e_a_que_mais_se_repete() {
        let lib = Library::in_memory().unwrap();
        let with_cover = |id: &str, cover: &str, plays: i64| Track {
            cover_url: cover.into(),
            ..t(id, id, "Deep Purple", "", plays, false)
        };
        lib.replace_tracks(&[
            with_cover("1", "machine-head.jpg", 1),
            with_cover("2", "machine-head.jpg", 2),
            with_cover("3", "capa-errada-zzz.jpg", 50),
            with_cover("4", "", 0),
        ])
        .unwrap();
        assert_eq!(lib.artists().unwrap()[0].cover_url, "machine-head.jpg");
    }

    #[test]
    fn playlists_e_favoritos() {
        let lib = sample();
        lib.replace_playlists(&[
            Playlist {
                id: "p1".into(),
                name: "favoritos 2026".into(),
                track_ids: vec!["4".into(), "x-apagada".into(), "1".into()],
            },
            Playlist {
                id: "p2".into(),
                name: "Treino".into(),
                track_ids: vec![],
            },
        ])
        .unwrap();
        assert_eq!(
            lib.playlist_tracks("p1").unwrap().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["4", "1"]
        );
        assert_eq!(lib.favorites_playlist().unwrap().unwrap().id, "p1");

        lib.upsert_playlist(&Playlist {
            id: "p2".into(),
            name: "Treino".into(),
            track_ids: vec!["2".into()],
        })
        .unwrap();
        lib.upsert_playlist(&Playlist {
            id: "p3".into(),
            name: "Nova".into(),
            track_ids: vec![],
        })
        .unwrap();
        let lists = lib.playlists().unwrap();
        assert_eq!(lists.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["p1", "p2", "p3"]);
        assert_eq!(lists[1].track_ids, vec!["2"]);
        lib.remove_playlist("p3").unwrap();
        assert_eq!(lib.playlists().unwrap().len(), 2);
    }

    #[test]
    fn plays_pendentes_e_downloads() {
        let lib = sample();
        let p = lib.record_play("2", 1_700_000_000).unwrap();
        assert_eq!(lib.track("2").unwrap().unwrap().play_count, 4);
        assert_eq!(lib.pending_plays(10).unwrap(), vec![p.clone()]);
        lib.remove_pending_plays(&[p.id]).unwrap();
        assert!(lib.pending_plays(10).unwrap().is_empty());

        lib.mark_downloaded("1", Path::new("/tmp/1.mp3"), 1234, 1).unwrap();
        assert_eq!(lib.downloaded_ids().unwrap(), vec!["1"]);
        assert_eq!(lib.downloads_size().unwrap(), 1234);
        assert_eq!(lib.unmark_downloaded("1").unwrap(), Some(PathBuf::from("/tmp/1.mp3")));
        assert!(lib.downloaded_path("1").unwrap().is_none());
    }

    #[test]
    fn meta() {
        let lib = Library::in_memory().unwrap();
        lib.set_meta("etag", Some("\"a\"")).unwrap();
        assert_eq!(lib.meta("etag").unwrap().as_deref(), Some("\"a\""));
        lib.set_meta("etag", None).unwrap();
        assert!(lib.meta("etag").unwrap().is_none());
    }
}
