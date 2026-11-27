//! Exportar para pendrive: copia as músicas para uma pasta (o pendrive do carro),
//! com nomes que qualquer rádio aceita (FAT32) e nome, artista, álbum e capa gravados
//! dentro de cada MP3 (é isso que o rádio mostra na tela).
//!
//! Tudo vai para `<destino>/Ilmarfy`. Um manifesto (`.ilmarfy-export.json`) lembra o que já
//! foi copiado: exportar de novo só copia o que é novo ou mudou, e "apagar o que saiu"
//! só mexe em arquivos que a própria exportação criou.

use super::*;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const ROOT: &str = "Ilmarfy";
const MANIFEST: &str = ".ilmarfy-export.json";
/// Muitos rádios leem no máximo ~255 arquivos por pasta
const FOLDER_LIMIT: usize = 250;

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Layout {
    /// Artista/Álbum/Título.mp3
    #[default]
    Artist,
    /// Uma pasta por playlist (ou artista escolhido), faixas numeradas na ordem da lista
    Playlist,
}

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Scope {
    #[default]
    Library,
    Chosen,
    Selected,
}

/// Uma lista que entra na exportação
pub(super) struct Source {
    pub name: String,
    pub tracks: Vec<Track>,
    /// Gera um .m3u (playlists e seleção, não a biblioteca)
    pub m3u: bool,
}

pub(super) struct PlanItem {
    pub track: Track,
    /// Caminho relativo a `<destino>/Ilmarfy`, com "/"
    pub rel: String,
}

pub(super) struct Plan {
    pub root: PathBuf,
    pub items: Vec<PlanItem>,
    /// (nome do .m3u, caminhos relativos na ordem da playlist)
    pub playlists: Vec<(String, Vec<(String, Track)>)>,
}

#[derive(Default)]
pub(super) struct Report {
    pub copied: usize,
    pub skipped: usize,
    pub retagged: usize,
    pub removed: usize,
    pub failed: Vec<String>,
    pub stopped: Option<String>,
}

#[derive(Default)]
pub(super) struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
    pub pause: AtomicBool,
    pub current: Mutex<String>,
    pub report: Mutex<Option<Report>>,
}

#[derive(Default)]
pub(super) struct ExportState {
    pub dest: String,
    pub scope: Scope,
    pub layout: Layout,
    pub tags: bool,
    pub prune: bool,
    pub playlists: HashSet<String>,
    pub artists: HashSet<String>,
    pub artist_filter: String,
    /// Faixas vindas da seleção ("Exportar selecionadas")
    pub selected: Vec<Track>,
    pub job: Option<Arc<Progress>>,
    pub started: f64,
    pub eject_msg: Option<String>,
}

// ------------------------------------------------------------------ nomes

/// Nome seguro para FAT32 e para rádios: sem caracteres proibidos, sem emoji, curto
pub(super) fn safe_name(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let ok = match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => false,
            c if c.is_control() => false,
            // Latim (com acentos) e pontuação comum; o resto (emoji, ideogramas) muitos rádios não mostram
            c => (c as u32) < 0x250 || ('\u{2018}'..='\u{201D}').contains(&c),
        };
        let c = match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '\'',
            c => c,
        };
        out.push(if ok { c } else { ' ' });
    }
    let mut out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.chars().count() > max {
        out = out.chars().take(max).collect();
    }
    let out = out.trim_end_matches(['.', ' ']).trim_start_matches('.').to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"];
    if out.is_empty() {
        "Sem nome".into()
    } else if reserved.contains(&out.to_uppercase().as_str()) {
        format!("{out}_")
    } else {
        out
    }
}

/// Monta o plano: que arquivo vai para onde. FAT32 não diferencia maiúsculas, então
/// nomes iguais ignorando caixa ganham " (2)", " (3)"…
pub(super) fn plan(dest: &Path, sources: &[Source], layout: Layout) -> Plan {
    let root = dest.join(ROOT);
    let mut items: Vec<PlanItem> = Vec::new();
    let mut by_id: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    let mut unique = |rel: String| {
        let (stem, ext) = rel.rsplit_once('.').unwrap_or((&rel, "mp3"));
        let mut candidate = rel.clone();
        let mut n = 2;
        while !taken.insert(candidate.to_lowercase()) {
            candidate = format!("{stem} ({n}).{ext}");
            n += 1;
        }
        candidate
    };
    let mut playlists = Vec::new();

    match layout {
        Layout::Artist => {
            let mut all: Vec<&Track> = sources.iter().flat_map(|s| &s.tracks).collect();
            // Ordem de gravação = ordem em que o rádio toca (FAT32 não ordena)
            all.sort_by_cached_key(|t| (t.artist.to_lowercase(), t.album.to_lowercase(), t.title.to_lowercase()));
            for t in all {
                if by_id.contains_key(&t.id) {
                    continue;
                }
                let album = if t.album.trim().is_empty() { "Sem álbum" } else { &t.album };
                let rel = unique(format!(
                    "{}/{}/{}.mp3",
                    safe_name(&t.artist, 60),
                    safe_name(album, 60),
                    safe_name(&t.title, 80)
                ));
                by_id.insert(t.id.clone(), rel.clone());
                items.push(PlanItem { track: t.clone(), rel });
            }
            for s in sources.iter().filter(|s| s.m3u) {
                let list = s
                    .tracks
                    .iter()
                    .filter_map(|t| by_id.get(&t.id).map(|r| (r.clone(), t.clone())))
                    .collect();
                playlists.push((safe_name(&s.name, 60), list));
            }
        }
        Layout::Playlist => {
            for s in sources {
                let folder = safe_name(&s.name, 60);
                let parts = s.tracks.len().div_ceil(FOLDER_LIMIT).max(1);
                let mut seen = HashSet::new();
                for (i, t) in s.tracks.iter().filter(|t| seen.insert(t.id.clone())).enumerate() {
                    // Listas enormes viram "Nome 1", "Nome 2"… para caber no limite do rádio
                    let dir = if parts > 1 {
                        format!("{folder} {}", i / FOLDER_LIMIT + 1)
                    } else {
                        folder.clone()
                    };
                    let rel = unique(format!(
                        "{dir}/{:03} - {} - {}.mp3",
                        i + 1,
                        safe_name(&t.artist, 40),
                        safe_name(&t.title, 70)
                    ));
                    items.push(PlanItem { track: t.clone(), rel });
                }
            }
        }
    }
    Plan { root, items, playlists }
}

// ------------------------------------------------------------------ manifesto

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Manifest {
    files: BTreeMap<String, Entry>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
struct Entry {
    id: String,
    size: u64,
    /// Assinatura das informações gravadas no arquivo (muda se a capa/título mudarem)
    tags: String,
}

fn tag_sig(t: &Track, with_tags: bool) -> String {
    if !with_tags {
        return String::new();
    }
    format!("{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}", t.title, t.artist, t.album, t.year, t.cover_url)
}

fn load_manifest(root: &Path) -> Manifest {
    std::fs::read(root.join(MANIFEST))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_manifest(root: &Path, m: &Manifest) {
    if let Ok(b) = serde_json::to_vec_pretty(m) {
        let _ = std::fs::write(root.join(MANIFEST), b);
    }
}

// ------------------------------------------------------------------ capa e tags

/// Capa em JPEG 500x500 (alguns rádios não abrem PNG, WebP nem JPEG progressivo/grande)
fn fetch_cover(url: &str, cache: &mut HashMap<String, Option<Vec<u8>>>) -> Option<Vec<u8>> {
    if url.is_empty() {
        return None;
    }
    if let Some(c) = cache.get(url) {
        return c.clone();
    }
    let get = || -> Option<Vec<u8>> {
        let mut resp = ureq::get(url).call().ok()?;
        let bytes = resp.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().ok()?;
        let img = image::load_from_memory(&bytes).ok()?;
        let img = img.resize_to_fill(500, 500, image::imageops::FilterType::Triangle).to_rgb8();
        let mut out = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&img)
            .ok()?;
        Some(out.into_inner())
    };
    let c = get();
    if cache.len() > 300 {
        cache.clear();
    }
    cache.insert(url.to_string(), c.clone());
    c
}

fn write_tags(path: &Path, t: &Track, cover: Option<Vec<u8>>) -> std::result::Result<(), String> {
    use id3::{Tag, TagLike, Version, frame::Picture, frame::PictureType};
    let mut tag = Tag::new();
    tag.set_title(t.title.replace("\\'", "'"));
    tag.set_artist(&t.artist);
    tag.set_album_artist(&t.artist);
    if !t.album.is_empty() {
        tag.set_album(&t.album);
    }
    if let Ok(y) = t.year.get(..4).unwrap_or("").parse::<i32>() {
        tag.set_year(y);
    }
    if let Some(data) = cover {
        tag.add_frame(Picture {
            mime_type: "image/jpeg".into(),
            picture_type: PictureType::CoverFront,
            description: String::new(),
            data,
        });
    }
    // ID3v2.3: a versão que rádios de carro leem
    tag.write_to_path(path, Version::Id3v23).map_err(|e| e.to_string())
}

fn is_disk_full(e: &str) -> bool {
    let e = e.to_lowercase();
    e.contains("no space") || e.contains("storage full") || e.contains("disk full") || e.contains("os error 28")
}

// ------------------------------------------------------------------ execução

pub(super) fn run(core: &Ilmarfy, plan: Plan, with_tags: bool, prune: bool, p: &Progress) -> Report {
    let mut r = Report::default();
    if let Err(e) = std::fs::create_dir_all(&plan.root) {
        r.stopped = Some(format!("Não deu para criar a pasta: {e}"));
        return r;
    }
    let old = load_manifest(&plan.root);
    let mut manifest = Manifest::default();
    // Mantém no manifesto o que já existia e não está nesta exportação (a menos que vá apagar)
    if !prune {
        manifest.files = old.files.clone();
    }
    p.total.store(plan.items.len(), Ordering::Relaxed);
    let mut covers = HashMap::new();

    for (i, item) in plan.items.iter().enumerate() {
        while p.pause.load(Ordering::Relaxed) && !p.cancel.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if p.cancel.load(Ordering::Relaxed) {
            r.stopped = Some("Exportação cancelada".into());
            break;
        }
        *p.current.lock().unwrap() = format!("{} — {}", item.track.artist, item.track.title);
        let dest = plan.root.join(&item.rel);
        let sig = tag_sig(&item.track, with_tags);
        let size_now = std::fs::metadata(&dest).map(|m| m.len()).ok();
        let prev = old.files.get(&item.rel);
        let same_file = prev.is_some_and(|e| e.id == item.track.id && Some(e.size) == size_now);

        let result: std::result::Result<bool, String> = (|| {
            if same_file && prev.is_some_and(|e| e.tags == sig) {
                return Ok(false); // já está lá, igual
            }
            if !same_file {
                if let Some(dir) = dest.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                // Cópia offline se houver; senão baixa do servidor direto para o pendrive
                match core
                    .library()
                    .downloaded_path(&item.track.id)
                    .ok()
                    .flatten()
                    .filter(|f| f.is_file())
                {
                    Some(local) => {
                        std::fs::copy(&local, &dest).map_err(|e| e.to_string())?;
                    }
                    None => {
                        core.api()
                            .download_track(&item.track, &dest, |_, _| !p.cancel.load(Ordering::Relaxed))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            if with_tags {
                let cover = fetch_cover(&item.track.cover_url, &mut covers);
                write_tags(&dest, &item.track, cover)?;
            }
            Ok(true)
        })();

        match result {
            Ok(false) => r.skipped += 1,
            Ok(true) if same_file => r.retagged += 1,
            Ok(true) => r.copied += 1,
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                if is_disk_full(&e) {
                    r.stopped = Some("O pendrive encheu. Libere espaço ou exporte menos músicas.".into());
                    break;
                }
                r.failed.push(format!("{} — {}: {e}", item.track.artist, item.track.title));
            }
        }
        if let Ok(m) = std::fs::metadata(&dest) {
            manifest.files.insert(
                item.rel.clone(),
                Entry {
                    id: item.track.id.clone(),
                    size: m.len(),
                    tags: sig,
                },
            );
        }
        p.done.store(i + 1, Ordering::Relaxed);
        // Salva de tempos em tempos: se tirarem o pendrive no meio, a próxima continua daí
        if i % 25 == 24 {
            save_manifest(&plan.root, &manifest);
        }
    }

    if r.stopped.is_none() {
        // Playlists .m3u (caminhos relativos, como os rádios esperam)
        for (name, list) in &plan.playlists {
            let mut s = String::from("#EXTM3U\r\n");
            for (rel, t) in list {
                s += &format!("#EXTINF:{},{} - {}\r\n{}\r\n", t.duration.max(0), t.artist, t.title, rel);
            }
            let _ = std::fs::write(plan.root.join(format!("{name}.m3u")), s);
        }
        if prune {
            let keep: HashSet<&str> = plan.items.iter().map(|i| i.rel.as_str()).collect();
            for rel in old.files.keys().filter(|k| !keep.contains(k.as_str())) {
                let f = plan.root.join(rel);
                if std::fs::remove_file(&f).is_ok() {
                    r.removed += 1;
                    // Pastas que ficaram vazias (só as de dentro de Ilmarfy)
                    let mut dir = f.parent().map(Path::to_path_buf);
                    while let Some(d) = dir {
                        if d == plan.root || std::fs::remove_dir(&d).is_err() {
                            break;
                        }
                        dir = d.parent().map(Path::to_path_buf);
                    }
                }
            }
        }
    }
    save_manifest(&plan.root, &manifest);
    r
}

/// Pendrives e cartões conectados (para escolher com um clique)
pub(super) fn removable_drives() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut scan = |base: PathBuf| {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for e in rd.flatten() {
                let path = e.path();
                // No Mac o disco do sistema aparece em /Volumes como link para "/"
                if std::fs::canonicalize(&path).is_ok_and(|c| c == Path::new("/")) {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "Recovery" || name.starts_with("com.apple") {
                    continue;
                }
                if path.is_dir() {
                    out.push((name, path));
                }
            }
        }
    };
    if cfg!(target_os = "macos") {
        scan(PathBuf::from("/Volumes"));
    } else {
        let user = std::env::var("USER").unwrap_or_default();
        scan(PathBuf::from(format!("/run/media/{user}")));
        scan(PathBuf::from(format!("/media/{user}")));
    }
    out
}

/// Ejeta o pendrive (Mac: diskutil; Linux: gio/udisks)
pub(super) fn eject(path: &Path) -> std::result::Result<(), String> {
    let out = if cfg!(target_os = "macos") {
        std::process::Command::new("diskutil").arg("eject").arg(path).output()
    } else {
        std::process::Command::new("gio").args(["mount", "-e"]).arg(path).output()
    };
    match out {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Estimativa de tamanho: os MP3 do servidor têm ~128 kbps (16 KB/s)
pub(super) fn estimate_bytes(tracks: &[&Track]) -> u64 {
    tracks.iter().map(|t| t.duration.max(0) as u64 * 16_000).sum()
}

pub(super) fn fmt_size(b: u64) -> String {
    if b >= 1_000_000_000 {
        format!("{:.1} GB", b as f64 / 1e9).replace('.', ",")
    } else {
        format!("{} MB", b / 1_000_000)
    }
}

// ------------------------------------------------------------------ tela

impl App {
    pub(super) fn export_sources(&self) -> Vec<Source> {
        let Some(core) = &self.core else { return Vec::new() };
        let lib = core.library();
        match self.export.scope {
            Scope::Library => vec![Source {
                name: "Biblioteca".into(),
                tracks: self.tracks_cache.clone(),
                m3u: false,
            }],
            Scope::Selected => vec![Source {
                name: "Seleção".into(),
                tracks: self.export.selected.clone(),
                m3u: true,
            }],
            Scope::Chosen => {
                let mut out = Vec::new();
                for p in self.playlists.iter().filter(|p| self.export.playlists.contains(&p.id)) {
                    out.push(Source {
                        name: self.playlist_label(p),
                        tracks: lib.playlist_tracks(&p.id).unwrap_or_default(),
                        m3u: true,
                    });
                }
                let mut artists: Vec<&String> = self.export.artists.iter().collect();
                artists.sort();
                for a in artists {
                    out.push(Source {
                        name: a.clone(),
                        tracks: lib.artist_tracks(a).unwrap_or_default(),
                        m3u: false,
                    });
                }
                out
            }
        }
    }

    fn playlist_label(&self, p: &Playlist) -> String {
        if Some(&p.id) == self.favorites_id.as_ref() {
            "Músicas curtidas".into()
        } else {
            p.name.clone()
        }
    }

    pub(super) fn export_page(&mut self, ui: &mut Ui) {
        let rect = Rect::from_min_size(ui.max_rect().min, Vec2::new(ui.max_rect().width(), 200.0));
        paint_gradient(ui, rect, Color32::from_rgb(0x1E, 0x50, 0x8C), BG);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 24)).show(ui, |ui| {
                ui.label(RichText::new("Exportar para pendrive").font(ui::font_bold(32.0)));
                ui.label(
                    RichText::new(
                        "Copia as músicas com nome, artista, álbum e capa gravados em cada arquivo — é o que o rádio do carro mostra.",
                    )
                    .color(SUB),
                );
                ui.add_space(20.0);
                if let Some(job) = self.export.job.clone() {
                    self.export_progress(ui, &job);
                } else {
                    self.export_form(ui);
                }
            });
        });
    }

    fn section(ui: &mut Ui, n: &str, title: &str) {
        ui.add_space(14.0);
        ui.label(RichText::new(format!("{n}  {title}")).font(ui::font_bold(18.0)));
        ui.add_space(6.0);
    }

    fn export_form(&mut self, ui: &mut Ui) {
        // 1. Destino
        Self::section(ui, "1", "Para onde");
        let drives = removable_drives();
        if drives.is_empty() {
            ui.label(RichText::new("Nenhum pendrive encontrado. Conecte um ou escolha uma pasta.").color(SUB));
        }
        ui.horizontal_wrapped(|ui| {
            for (name, path) in &drives {
                let on = Path::new(&self.export.dest) == path.as_path();
                if ui::pill(
                    ui,
                    &format!("💾  {name}"),
                    if on { GREEN } else { TILE },
                    if on { Color32::BLACK } else { TEXT },
                )
                .clicked()
                {
                    self.export.dest = path.display().to_string();
                }
            }
            if ui::pill(ui, "📁  Escolher pasta…", TILE, TEXT).clicked()
                && let Some(dir) = rfd::FileDialog::new().set_title("Escolha o pendrive ou a pasta").pick_folder()
            {
                self.export.dest = dir.display().to_string();
            }
        });
        if !self.export.dest.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("As músicas vão para {}", Path::new(&self.export.dest).join(ROOT).display())).color(SUB));
        }

        // 2. O que
        Self::section(ui, "2", "O que exportar");
        let total = self.tracks_cache.len();
        ui.radio_value(
            &mut self.export.scope,
            Scope::Library,
            format!("Biblioteca inteira ({total} músicas)"),
        );
        ui.radio_value(&mut self.export.scope, Scope::Chosen, "Escolher playlists e artistas");
        if !self.export.selected.is_empty() {
            let n = self.export.selected.len();
            ui.radio_value(&mut self.export.scope, Scope::Selected, format!("As {n} músicas selecionadas"));
        }
        if self.export.scope == Scope::Chosen {
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                cols[0].label(RichText::new("Playlists").font(ui::font_bold(15.0)));
                egui::ScrollArea::vertical()
                    .id_salt("exp-pl")
                    .max_height(260.0)
                    .show(&mut cols[0], |ui| {
                        let pls = self.playlists.clone();
                        for p in &pls {
                            let mut on = self.export.playlists.contains(&p.id);
                            let label = format!("{} ({})", self.playlist_label(p), p.track_ids.len());
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    self.export.playlists.insert(p.id.clone());
                                } else {
                                    self.export.playlists.remove(&p.id);
                                }
                            }
                        }
                    });
                cols[1].label(RichText::new("Artistas").font(ui::font_bold(15.0)));
                cols[1].add(egui::TextEdit::singleline(&mut self.export.artist_filter).hint_text("🔍 Filtrar artistas"));
                egui::ScrollArea::vertical()
                    .id_salt("exp-ar")
                    .max_height(230.0)
                    .show(&mut cols[1], |ui| {
                        let f = self.export.artist_filter.to_lowercase();
                        let artists = self.artists.clone();
                        for a in artists.iter().filter(|a| f.is_empty() || a.name.to_lowercase().contains(&f)) {
                            let mut on = self.export.artists.contains(&a.name);
                            if ui.checkbox(&mut on, format!("{} ({})", a.name, a.track_count)).changed() {
                                if on {
                                    self.export.artists.insert(a.name.clone());
                                } else {
                                    self.export.artists.remove(&a.name);
                                }
                            }
                        }
                    });
            });
        }

        // 3. Organização
        Self::section(ui, "3", "Como organizar as pastas");
        ui.radio_value(
            &mut self.export.layout,
            Layout::Artist,
            "Por artista: Artista / Álbum / música  (melhor para a biblioteca inteira)",
        );
        ui.radio_value(
            &mut self.export.layout,
            Layout::Playlist,
            "Por playlist: uma pasta para cada playlist ou artista, músicas numeradas na ordem",
        );
        ui.add_space(8.0);
        ui.checkbox(&mut self.export.tags, "Gravar nome, artista, álbum e capa em cada música");
        ui.checkbox(
            &mut self.export.prune,
            "Apagar do pendrive as músicas que não estão nesta exportação (só as que o Ilmarfy copiou)",
        );

        // Resumo + botão
        let sources = self.export_sources();
        let mut seen = HashSet::new();
        let unique: Vec<&Track> = sources
            .iter()
            .flat_map(|s| &s.tracks)
            .filter(|t| seen.insert(t.id.clone()))
            .collect();
        let (count, size) = (unique.len(), estimate_bytes(&unique));
        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{count} músicas • ≈ {}", fmt_size(size))).font(ui::font_bold(16.0)));
            ui.add_space(16.0);
            let ready = !self.export.dest.is_empty() && count > 0;
            let btn = ui::pill(
                ui,
                "💾  Exportar",
                if ready { GREEN } else { TILE },
                if ready { Color32::BLACK } else { DIM },
            );
            if btn.clicked() && ready {
                self.start_export(ui.ctx(), sources);
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Exportar de novo depois só copia o que for novo ou mudou. Se o pendrive já tiver músicas, elas ficam onde estão.",
            )
            .color(DIM),
        );
    }

    pub(super) fn start_export(&mut self, ctx: &egui::Context, sources: Vec<Source>) {
        let Some(core) = self.core.clone() else { return };
        let plan = plan(Path::new(&self.export.dest), &sources, self.export.layout);
        let job = Arc::new(Progress::default());
        job.total.store(plan.items.len(), Ordering::Relaxed);
        self.export.job = Some(job.clone());
        self.export.started = ctx.input(|i| i.time);
        self.export.eject_msg = None;
        let (tags, prune) = (self.export.tags, self.export.prune);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = run(&core, plan, tags, prune, &job);
            *job.report.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }

    fn export_progress(&mut self, ui: &mut Ui, job: &Arc<Progress>) {
        let done = job.done.load(Ordering::Relaxed);
        let total = job.total.load(Ordering::Relaxed).max(1);
        let finished = job.report.lock().unwrap().is_some();
        if !finished {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
            let paused = job.pause.load(Ordering::Relaxed);
            ui.label(RichText::new(if paused { "Pausado" } else { "Exportando…" }).font(ui::font_bold(20.0)));
            ui.add_space(8.0);
            ui::progress(ui, done as f32 / total as f32, (ui.available_width() - 40.0).min(640.0));
            ui.label(RichText::new(format!("{done} de {total}")).color(SUB));
            ui.label(RichText::new(job.current.lock().unwrap().clone()).color(DIM));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui::pill(ui, if paused { "▶  Continuar" } else { "⏸  Pausar" }, TILE, TEXT).clicked() {
                    job.pause.store(!paused, Ordering::Relaxed);
                }
                if ui::pill(ui, "×  Cancelar", TILE, TEXT).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Pode usar o Ilmarfy enquanto isso. Não tire o pendrive até terminar.").color(DIM));
            return;
        }
        let guard = job.report.lock().unwrap();
        let r = guard.as_ref().unwrap();
        match &r.stopped {
            Some(why) => ui.label(RichText::new(why).font(ui::font_bold(20.0)).color(Color32::from_rgb(255, 107, 107))),
            None => ui.label(RichText::new("♪ Pronto para o carro").font(ui::font_bold(22.0)).color(GREEN)),
        };
        ui.add_space(8.0);
        let mut lines = vec![format!("{} copiadas", r.copied)];
        if r.retagged > 0 {
            lines.push(format!("{} com informações atualizadas", r.retagged));
        }
        if r.skipped > 0 {
            lines.push(format!("{} já estavam lá", r.skipped));
        }
        if r.removed > 0 {
            lines.push(format!("{} apagadas", r.removed));
        }
        ui.label(RichText::new(lines.join(" • ")).color(SUB));
        let failed = r.failed.clone();
        drop(guard);
        if !failed.is_empty() {
            ui.add_space(6.0);
            egui::CollapsingHeader::new(format!("{} não foram copiadas", failed.len())).show(ui, |ui| {
                for f in &failed {
                    ui.label(RichText::new(f).color(SUB));
                }
            });
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let dest = PathBuf::from(&self.export.dest);
            let is_drive = removable_drives().iter().any(|(_, p)| *p == dest);
            if is_drive && ui::pill(ui, "⬆  Ejetar pendrive", GREEN, Color32::BLACK).clicked() {
                self.export.eject_msg = Some(match eject(&dest) {
                    Ok(()) => "Pode tirar o pendrive.".into(),
                    Err(e) => format!("Não deu para ejetar: {e}"),
                });
            }
            if ui::pill(ui, "Nova exportação", TILE, TEXT).clicked() {
                self.export.job = None;
            }
        });
        if let Some(m) = &self.export.eject_msg {
            ui.add_space(6.0);
            ui.label(RichText::new(m).color(SUB));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, artist: &str, album: &str, title: &str) -> Track {
        Track {
            id: id.into(),
            artist: artist.into(),
            album: album.into(),
            title: title.into(),
            duration: 200,
            ..Default::default()
        }
    }

    #[test]
    fn nomes_seguros_para_fat32() {
        assert_eq!(safe_name("AC/DC", 60), "AC DC");
        assert_eq!(safe_name("What? Me: \"Worry\"*", 60), "What Me Worry");
        assert_eq!(safe_name("Ação ★ Reação", 60), "Ação Reação");
        assert_eq!(safe_name("...", 60), "Sem nome");
        assert_eq!(safe_name("Fim.", 60), "Fim");
        assert_eq!(safe_name("con", 60), "con_");
        assert_eq!(safe_name("abcdefghij", 4), "abcd");
        assert_eq!(safe_name("Don\u{2019}t", 60), "Don't");
    }

    #[test]
    fn plano_por_artista_sem_repetir_e_sem_colisao() {
        let a = t("1", "Ghost", "Meliora", "Cirice");
        let b = t("2", "ghost", "meliora", "CIRICE"); // outro id, mesmo nome ignorando caixa
        let src = vec![
            Source {
                name: "Rock".into(),
                tracks: vec![a.clone(), b.clone()],
                m3u: true,
            },
            Source {
                name: "Biblioteca".into(),
                tracks: vec![a.clone()],
                m3u: false,
            },
        ];
        let p = plan(Path::new("/x"), &src, Layout::Artist);
        let rels: Vec<_> = p.items.iter().map(|i| i.rel.as_str()).collect();
        assert_eq!(rels.len(), 2, "a mesma faixa em duas listas é copiada uma vez só");
        assert_eq!(rels[0].to_lowercase(), "ghost/meliora/cirice.mp3");
        assert!(rels[1].ends_with(" (2).mp3"), "{rels:?}");
        assert_eq!(p.playlists.len(), 1);
        assert_eq!(p.playlists[0].1.len(), 2);
        assert_eq!(p.root, Path::new("/x/Ilmarfy"));
    }

    #[test]
    fn plano_por_playlist_numera_e_divide_pastas_grandes() {
        let tracks: Vec<Track> = (0..260).map(|i| t(&i.to_string(), "A", "B", &format!("M{i}"))).collect();
        let src = vec![Source {
            name: "Todas".into(),
            tracks,
            m3u: false,
        }];
        let p = plan(Path::new("/x"), &src, Layout::Playlist);
        assert_eq!(p.items[0].rel, "Todas 1/001 - A - M0.mp3");
        assert_eq!(p.items[259].rel, "Todas 2/260 - A - M259.mp3");
    }

    #[test]
    fn exporta_com_tags_e_nao_recopia() {
        let dir = std::env::temp_dir().join(format!("ilmarfy-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join(ROOT);
        // Simula uma exportação anterior: arquivo lá + manifesto
        let item = t("1", "Ghost", "Meliora", "Cirice");
        let p = plan(
            &dir,
            &[Source {
                name: "x".into(),
                tracks: vec![item.clone()],
                m3u: true,
            }],
            Layout::Artist,
        );
        let f = root.join(&p.items[0].rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"fake-mp3-data").unwrap();
        let mut m = Manifest::default();
        m.files.insert(
            p.items[0].rel.clone(),
            Entry {
                id: "1".into(),
                size: 13,
                tags: String::new(),
            },
        );
        save_manifest(&root, &m);
        let core = Ilmarfy::open(&dir.join("data"), "http://127.0.0.1:9", None).unwrap();
        let prog = Progress::default();
        let r = run(&core, p, false, false, &prog);
        assert_eq!((r.copied, r.skipped, r.failed.len()), (0, 1, 0), "não recopia o que já está lá");
        assert!(root.join("x.m3u").is_file());
        // Com tags: regrava as informações no arquivo que já existe
        let p = plan(
            &dir,
            &[Source {
                name: "x".into(),
                tracks: vec![item],
                m3u: false,
            }],
            Layout::Artist,
        );
        let r = run(&core, p, true, false, &prog);
        assert_eq!(r.retagged, 1, "{:?}", r.failed);
        let tag = id3::Tag::read_from_path(&f).unwrap();
        use id3::TagLike;
        assert_eq!(tag.title(), Some("Cirice"));
        assert_eq!(tag.album(), Some("Meliora"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
