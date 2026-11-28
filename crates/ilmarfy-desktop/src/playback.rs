//! Reprodução no desktop: fila (PlayQueue do núcleo), cache local de arquivos e saída de áudio (rodio).
//! Cada faixa é baixada para o cache antes de tocar (as baixadas para offline tocam direto);
//! a próxima da fila já vai sendo baixada enquanto a atual toca.

use ilmarfy_core::{FlowTrack, Ilmarfy, PlayQueue, Repeat, Source, Track};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum Item {
    Track(Track),
    Flow(FlowTrack),
}

impl Item {
    pub fn key(&self) -> String {
        match self {
            Item::Track(t) => format!("t:{}", t.id),
            Item::Flow(f) => format!("f:{}", f.id),
        }
    }
    pub fn title(&self) -> &str {
        match self {
            Item::Track(t) => &t.title,
            Item::Flow(f) => &f.title,
        }
    }
    pub fn artist(&self) -> &str {
        match self {
            Item::Track(t) => &t.artist,
            Item::Flow(f) => &f.artist,
        }
    }
    pub fn album(&self) -> &str {
        match self {
            Item::Track(t) => &t.album,
            Item::Flow(f) => &f.album,
        }
    }
    pub fn cover(&self) -> &str {
        match self {
            Item::Track(t) => &t.cover_url,
            Item::Flow(f) => &f.cover,
        }
    }
    pub fn duration(&self) -> Duration {
        match self {
            Item::Track(t) => Duration::from_secs(t.duration.max(0) as u64),
            Item::Flow(f) => Duration::from_millis(f.duration_ms.max(0) as u64),
        }
    }
    pub fn track_id(&self) -> Option<&str> {
        match self {
            Item::Track(t) => Some(&t.id),
            Item::Flow(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Loading,
    Playing,
    Paused,
}

pub struct Playback {
    core: Arc<Ilmarfy>,
    _sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    items: HashMap<String, Item>,
    queue: PlayQueue,
    current: Option<String>,
    status: Status,
    duration: Duration,
    volume: f32,
    cache_dir: PathBuf,
    fetch_tx: Sender<(String, Result<PathBuf, String>)>,
    fetch_rx: Receiver<(String, Result<PathBuf, String>)>,
    fetching: HashSet<String>,
    ready: HashMap<String, PathBuf>,
    listened: Duration,
    counted: bool,
    last_tick: Instant,
    failures: u32,
    /// Avisos para a interface
    pub message: Option<String>,
}

/// Arquivos do cache de streaming acima disto são apagados (os mais antigos primeiro)
const CACHE_LIMIT_BYTES: u64 = 400 * 1024 * 1024;

impl Playback {
    pub fn new(core: Arc<Ilmarfy>, cache_dir: PathBuf) -> Self {
        let (fetch_tx, fetch_rx) = channel();
        let _ = std::fs::create_dir_all(&cache_dir);
        trim_cache(&cache_dir);
        // Sem placa de som (ex.: testes), o app abre do mesmo jeito; tenta de novo na hora de tocar
        let (sink, player) = open_output().map(|(s, p)| (Some(s), Some(p))).unwrap_or((None, None));
        Self {
            core,
            _sink: sink,
            player,
            items: HashMap::new(),
            queue: PlayQueue::new(Vec::new(), 0),
            current: None,
            status: Status::Idle,
            duration: Duration::ZERO,
            volume: 1.0,
            cache_dir,
            fetch_tx,
            fetch_rx,
            fetching: HashSet::new(),
            ready: HashMap::new(),
            listened: Duration::ZERO,
            counted: false,
            last_tick: Instant::now(),
            failures: 0,
            message: None,
        }
    }

    // ------------------------------------------------------------ Estado

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn has_output(&self) -> bool {
        self.player.is_some()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn current(&self) -> Option<&Item> {
        self.current.as_ref().and_then(|k| self.items.get(k))
    }

    pub fn current_key(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn position(&self) -> Duration {
        self.player
            .as_ref()
            .map(|p| p.get_pos())
            .unwrap_or_default()
            .min(self.duration.max(Duration::from_secs(1)))
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn shuffle(&self) -> bool {
        self.queue.shuffle()
    }

    pub fn repeat(&self) -> Repeat {
        self.queue.repeat()
    }

    /// Próximas da fila (a atual incluída)
    pub fn upcoming(&self) -> Vec<Item> {
        self.queue.upcoming().iter().filter_map(|k| self.items.get(*k).cloned()).collect()
    }

    // ------------------------------------------------------------ Comandos

    pub fn play_items(&mut self, list: Vec<Item>, start: usize, shuffle: bool) {
        if list.is_empty() {
            return;
        }
        let keys: Vec<String> = list.iter().map(Item::key).collect();
        self.items = list.into_iter().map(|i| (i.key(), i)).collect();
        let start = if shuffle && start == 0 {
            pseudo_random(keys.len())
        } else {
            start.min(keys.len() - 1)
        };
        let repeat = self.queue.repeat();
        self.queue = PlayQueue::new(keys, start);
        self.queue.set_repeat(repeat);
        self.queue.set_shuffle(shuffle);
        self.failures = 0;
        self.load_current();
    }

    pub fn play_next(&mut self, item: Item) {
        let key = item.key();
        self.items.insert(key.clone(), item);
        if self.current.is_none() {
            self.queue = PlayQueue::new(vec![key], 0);
            self.load_current();
        } else {
            self.queue.play_next(key);
        }
    }

    pub fn enqueue(&mut self, item: Item) {
        let key = item.key();
        self.items.insert(key.clone(), item);
        if self.current.is_none() {
            self.queue = PlayQueue::new(vec![key], 0);
            self.load_current();
        } else {
            self.queue.append(key);
        }
    }

    pub fn toggle(&mut self) {
        let Some(p) = &self.player else { return };
        match self.status {
            Status::Playing => {
                p.pause();
                self.status = Status::Paused;
            }
            Status::Paused => {
                p.play();
                self.status = Status::Playing;
            }
            Status::Idle if self.current.is_some() => self.load_current(),
            _ => {}
        }
    }

    pub fn next(&mut self) {
        if self.queue.next(true).is_some() {
            self.load_current();
        }
    }

    /// Volta ao começo da música; se já está no começo, vai para a anterior
    pub fn previous(&mut self) {
        if self.position() > Duration::from_secs(3) {
            self.seek(Duration::ZERO);
        } else if self.queue.previous().is_some() {
            self.load_current();
        }
    }

    /// Tira da fila tudo o que `drop` apontar (ex.: músicas recusadas no Flow).
    /// Se a que está tocando sair, pula para a próxima que ficou; sem nenhuma, para.
    pub fn remove_where(&mut self, drop: impl Fn(&Item) -> bool) {
        let keys: HashSet<String> = self.items.iter().filter(|(_, i)| drop(i)).map(|(k, _)| k.clone()).collect();
        if keys.is_empty() {
            return;
        }
        if self.current.as_ref().is_some_and(|c| keys.contains(c)) {
            let mut moved = false;
            for _ in 0..self.queue.len() {
                match self.queue.next(true) {
                    Some(k) if !keys.contains(k) => {
                        moved = true;
                        break;
                    }
                    Some(_) => continue,
                    None => break,
                }
            }
            if moved {
                self.load_current();
            } else {
                self.stop();
            }
        }
        for k in &keys {
            if self.current.as_deref() != Some(k.as_str()) {
                self.queue.remove(k);
                self.items.remove(k);
            }
        }
    }

    pub fn jump_to(&mut self, key: &str) {
        if self.queue.jump_to(key) {
            self.load_current();
        }
    }

    pub fn seek(&mut self, to: Duration) {
        if let Some(p) = &self.player {
            let _ = p.try_seek(to);
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(p) = &self.player {
            p.set_volume(self.volume * self.volume); // curva mais natural ao ouvido
        }
    }

    pub fn toggle_shuffle(&mut self) {
        let on = !self.queue.shuffle();
        self.queue.set_shuffle(on);
    }

    pub fn cycle_repeat(&mut self) {
        self.queue.set_repeat(match self.queue.repeat() {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        });
    }

    pub fn stop(&mut self) {
        if let Some(p) = &self.player {
            p.clear();
        }
        self.status = Status::Idle;
        self.current = None;
        self.queue = PlayQueue::new(Vec::new(), 0);
    }

    // ------------------------------------------------------------ Carregamento

    fn load_current(&mut self) {
        let Some(key) = self.queue.current().map(String::from) else {
            self.status = Status::Idle;
            return;
        };
        self.current = Some(key.clone());
        self.listened = Duration::ZERO;
        self.counted = false;
        if let Some(p) = &self.player {
            p.clear();
        }
        match self.ready.get(&key).cloned() {
            Some(path) if path.is_file() => self.start(&path),
            _ => {
                self.status = Status::Loading;
                self.fetch(&key);
            }
        }
    }

    /// Garante o arquivo da faixa: o baixado para offline, o do cache, ou baixa agora em segundo plano
    fn fetch(&mut self, key: &str) {
        if self.ready.contains_key(key) || self.fetching.contains(key) {
            return;
        }
        let Some(item) = self.items.get(key).cloned() else { return };
        if let Item::Track(t) = &item
            && let Ok(Source::Local(path)) = self.core.source(t)
        {
            self.ready.insert(key.to_string(), path);
            return;
        }
        let dest = self.cache_file(&item);
        if dest.is_file() {
            touch(&dest);
            self.ready.insert(key.to_string(), dest);
            return;
        }
        self.fetching.insert(key.to_string());
        let (core, tx, key) = (self.core.clone(), self.fetch_tx.clone(), key.to_string());
        std::thread::spawn(move || {
            let res = match &item {
                Item::Track(t) => core.api().download_track(t, &dest, |_, _| true),
                Item::Flow(f) => core.api().download_preview(&f.id, &dest, |_, _| true),
            };
            let _ = tx.send((key, res.map(|_| dest).map_err(|e| e.to_string())));
        });
    }

    fn cache_file(&self, item: &Item) -> PathBuf {
        let (name, ext) = match item {
            Item::Track(t) => (
                t.id.clone(),
                Path::new(&t.filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("mp3")
                    .to_string(),
            ),
            Item::Flow(f) => (format!("flow-{}", f.id), "m4a".to_string()),
        };
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        self.cache_dir.join(format!("{safe}.{ext}"))
    }

    /// Abre a saída de áudio se ainda não estiver aberta (ex.: falhou na criação, fone conectado depois)
    fn ensure_output(&mut self) -> Result<(), String> {
        if self.player.is_some() {
            return Ok(());
        }
        let (sink, player) = open_output()?;
        player.set_volume(self.volume * self.volume);
        self._sink = Some(sink);
        self.player = Some(player);
        Ok(())
    }

    fn start(&mut self, path: &Path) {
        if let Err(e) = self.ensure_output() {
            self.message = Some(format!("Sem saída de áudio: {e}"));
            self.status = Status::Idle;
            return;
        }
        let Some(player) = &self.player else { return };
        let decoded = std::fs::File::open(path)
            .map_err(|e| e.to_string())
            .and_then(|f| rodio::Decoder::try_from(f).map_err(|e| e.to_string()));
        match decoded {
            Ok(source) => {
                use rodio::Source as _;
                let item_duration = self.current().map(Item::duration).unwrap_or_default();
                self.duration = source.total_duration().filter(|d| !d.is_zero()).unwrap_or(item_duration);
                player.clear();
                player.append(source);
                player.set_volume(self.volume * self.volume);
                player.play();
                self.status = Status::Playing;
                self.failures = 0;
                self.last_tick = Instant::now();
                // Já baixa a próxima
                if let Some(next) = self.queue.upcoming().get(1).map(|s| s.to_string()) {
                    self.fetch(&next);
                }
            }
            Err(e) => {
                // Arquivo corrompido no cache: apaga para baixar de novo na próxima vez
                if path.starts_with(&self.cache_dir) {
                    let _ = std::fs::remove_file(path);
                }
                if let Some(k) = &self.current {
                    self.ready.remove(k);
                }
                self.fail(format!("Não foi possível tocar: {e}"));
            }
        }
    }

    fn fail(&mut self, msg: String) {
        self.failures += 1;
        self.message = Some(msg);
        if self.failures <= 3 && self.queue.next(true).is_some() {
            self.load_current();
        } else {
            self.status = Status::Idle;
        }
    }

    /// Chamado a cada quadro: resultados de download, fim da faixa e contagem de plays.
    /// Devolve true se a faixa atual mudou.
    pub fn tick(&mut self) -> bool {
        let before = self.current.clone();
        while let Ok((key, res)) = self.fetch_rx.try_recv() {
            self.fetching.remove(&key);
            match res {
                Ok(path) => {
                    self.ready.insert(key.clone(), path.clone());
                    // Mantém o cache no limite durante o uso (antes só ao abrir o app, e ele crescia
                    // sem parar numa sessão longa). Nunca apaga um arquivo que a fila vai usar.
                    let keep: HashSet<PathBuf> = self.ready.values().cloned().collect();
                    trim_cache_keeping(&self.cache_dir, &keep);
                    if self.current.as_deref() == Some(&key) && self.status == Status::Loading {
                        self.start(&path);
                    }
                }
                Err(e) if self.current.as_deref() == Some(&key) => {
                    let offline = e.contains("conexão");
                    self.fail(if offline {
                        "Sem internet: essa música não está baixada".into()
                    } else {
                        e
                    });
                }
                Err(_) => {}
            }
        }

        let now = Instant::now();
        let elapsed = now - self.last_tick;
        self.last_tick = now;
        if self.status == Status::Playing {
            self.listened += elapsed;
            if !self.counted && self.listened >= Duration::from_secs(30).min(self.duration / 2) {
                self.counted = true;
                if let Some(id) = self.current().and_then(Item::track_id).map(String::from) {
                    let _ = self.core.record_play(&id);
                }
            }
            if self.player.as_ref().is_some_and(|p| p.empty()) {
                // Fim da faixa
                if self.queue.next(false).is_some() {
                    self.load_current();
                } else {
                    self.status = Status::Idle;
                }
            }
        }
        before != self.current
    }
}

fn open_output() -> Result<(rodio::MixerDeviceSink, rodio::Player), String> {
    let builder = rodio::DeviceSinkBuilder::from_default_device().map_err(|e| e.to_string())?;
    let mut sink = builder.open_sink_or_fallback().map_err(|e| e.to_string())?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    Ok((sink, player))
}

fn pseudo_random(n: usize) -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    nanos as usize % n.max(1)
}

/// Marca o arquivo como usado agora (o limpador apaga primeiro os mais antigos)
fn touch(path: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

fn trim_cache(dir: &Path) {
    trim_cache_keeping(dir, &HashSet::new());
}

/// Apaga os arquivos mais antigos do cache até caber no limite, sem tocar nos de `keep`
fn trim_cache_keeping(dir: &Path, keep: &HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            m.is_file()
                .then(|| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= CACHE_LIMIT_BYTES {
            break;
        }
        if keep.contains(&path) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CACHE_LIMIT_BYTES, trim_cache_keeping};
    use ilmarfy_core::{DEFAULT_SERVER, Ilmarfy, TrackOrder};
    use rodio::Source;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn recusar_no_flow_tira_da_fila_e_pula_a_atual() {
        use super::{Item, Playback};
        use ilmarfy_core::FlowTrack;
        let dir = std::env::temp_dir().join(format!("ilmarfy-fila-{}", std::process::id()));
        let core = std::sync::Arc::new(Ilmarfy::open(&dir, "http://127.0.0.1:9", None).unwrap());
        let mut p = Playback::new(core, dir.join("cache"));
        let f = |id: &str, artist: &str| {
            Item::Flow(FlowTrack {
                id: id.into(),
                title: id.into(),
                artist: artist.into(),
                ..Default::default()
            })
        };
        p.play_items(vec![f("a", "X"), f("b", "Y"), f("c", "X"), f("d", "Z")], 0, false);
        assert_eq!(p.current_key(), Some("f:a"));
        // 👎 numa que está na fila
        p.remove_where(|i| matches!(i, Item::Flow(t) if t.id == "b"));
        let keys: Vec<String> = p.upcoming().iter().map(Item::key).collect();
        assert_eq!(keys, ["f:a", "f:c", "f:d"]);
        // 🚫 no artista da que está tocando: pula para a próxima que sobrou
        p.remove_where(|i| matches!(i, Item::Flow(t) if t.artist == "X"));
        assert_eq!(p.current_key(), Some("f:d"));
        let keys: Vec<String> = p.upcoming().iter().map(Item::key).collect();
        assert_eq!(keys, ["f:d"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Baixa uma faixa da biblioteca (MP3) e uma prévia do Flow (MP4 do YouTube) e decodifica as duas.
    /// `ILMARFY_TOKEN=… cargo test -p ilmarfy-desktop decodifica -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn decodifica_mp3_e_previa_do_flow() {
        let dir = std::env::temp_dir().join(format!("ilmarfy-decode-{}", std::process::id()));
        let core = Ilmarfy::open(&dir, DEFAULT_SERVER, Some(std::env::var("ILMARFY_TOKEN").unwrap())).unwrap();
        core.sync().unwrap();
        let t = core
            .library()
            .tracks(TrackOrder::Title, false)
            .unwrap()
            .into_iter()
            .filter(|t| t.duration > 30)
            .min_by_key(|t| t.duration)
            .unwrap();
        let mp3 = dir.join("a.mp3");
        core.api().download_track(&t, &mp3, |_, _| true).unwrap();
        let d = rodio::Decoder::try_from(std::fs::File::open(&mp3).unwrap()).expect("decodificar mp3");
        println!("mp3: {} — {} ({:?}, {} Hz)", t.artist, t.title, d.total_duration(), d.sample_rate());
        assert!(d.total_duration().is_some_and(|x| x.as_secs() > 20));

        let flow = core.api().flow().unwrap();
        let f = flow.tracks.first().expect("flow vazio");
        let m4a = dir.join("b.m4a");
        core.api().download_preview(&f.id, &m4a, |_, _| true).unwrap();
        let mut d = rodio::Decoder::try_from(std::fs::File::open(&m4a).unwrap()).expect("decodificar prévia");
        let samples = d.by_ref().take(48_000).count();
        println!(
            "prévia: {} — {} ({:?}, {} amostras lidas)",
            f.artist,
            f.title,
            d.total_duration(),
            samples
        );
        assert!(samples > 10_000, "a prévia deveria ter áudio decodificável");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cache_respeita_o_limite_e_poupa_a_fila() {
        let dir = std::env::temp_dir().join(format!("ilmarfy-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mb = vec![0u8; 1024 * 1024];
        let limit_mb = (CACHE_LIMIT_BYTES / (1024 * 1024)) as usize;
        // limite + 20 arquivos de 1 MB; o mais antigo é o "na fila"
        let mut files = Vec::new();
        for i in 0..limit_mb + 20 {
            let f = dir.join(format!("{i:04}.mp3"));
            std::fs::write(&f, &mb).unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + i as u64);
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
            files.push(f);
        }
        let keep: HashSet<PathBuf> = [files[0].clone()].into();
        trim_cache_keeping(&dir, &keep);
        let total: u64 = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert!(total <= CACHE_LIMIT_BYTES, "ficou {total}");
        assert!(files[0].is_file(), "o arquivo da fila não pode sumir");
        assert!(!files[1].is_file(), "os mais antigos saem primeiro");
        assert!(files.last().unwrap().is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
