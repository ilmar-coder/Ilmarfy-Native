//! CLI para usar e testar o ilmarfy-core contra o servidor real.
//! Uso: `ilmarfy login`, `ilmarfy sync`, `ilmarfy stats`, `ilmarfy search sepultura`…

use ilmarfy_core::{DEFAULT_SERVER, Ilmarfy, TrackOrder};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Ilmarfy (linha de comando)

  login [servidor]          entra neste computador (a senha é pedida sem aparecer)
  logout                    sai e revoga o token deste computador
  sync                      sincroniza biblioteca e playlists, envia plays pendentes
  stats                     resumo do cache local
  search <texto>            busca faixas (ignora acentos)
  artists                   lista artistas
  playlists                 lista playlists
  playlist <id>             faixas de uma playlist
  download <track_id>       baixa uma faixa para ouvir offline
  download-playlist <id>    baixa a playlist inteira
  remove-download <id>      apaga a faixa baixada
  play <track_id>           registra que a faixa foi ouvida (vai ao servidor no sync)
  favorite <track_id>       liga/desliga favorito
  lyrics <track_id>         mostra a letra
  flow                      sugestões do Flow
  restricted                Restritos do Flow
  devices                   aparelhos autorizados

Variáveis: ILMARFY_DATA (pasta de dados), ILMARFY_SERVER";

fn data_dir() -> PathBuf {
    std::env::var_os("ILMARFY_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("ilmarfy-native"))
}

fn token_file() -> PathBuf {
    data_dir().join("token")
}

fn server_file() -> PathBuf {
    data_dir().join("server")
}

fn saved(path: PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn server() -> String {
    std::env::var("ILMARFY_SERVER")
        .ok()
        .or_else(|| saved(server_file()))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

fn write_private(path: &PathBuf, content: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn fmt_duration(secs: i64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let arg = args.get(1).cloned().unwrap_or_default();
    let mut app = Ilmarfy::open(&data_dir(), &server(), saved(token_file()))?;

    // Comandos que só leem o cache local funcionam sem login (e offline)
    let offline_ok = matches!(
        cmd,
        "login" | "help" | "stats" | "search" | "artists" | "playlists" | "playlist" | "play"
    );
    if !offline_ok && !app.is_logged_in() {
        return Err("faça login primeiro: ilmarfy login".into());
    }

    match cmd {
        "login" => {
            if !arg.is_empty() {
                write_private(&server_file(), &arg)?;
                app = Ilmarfy::open(&data_dir(), &arg, None)?;
            }
            let v = app.api().version()?;
            println!("Servidor: {} (API v{})", app.api().base_url(), v.api_version);
            let name = format!("{} (CLI)", hostname());
            let password = rpassword::prompt_password("Senha do Ilmarfy: ")?;
            let token = app.login(&password, &name, std::env::consts::OS)?;
            write_private(&token_file(), &token)?;
            println!("Pronto: \"{name}\" autorizado. Token salvo em {}", token_file().display());
        }
        "logout" => {
            app.logout()?;
            let _ = std::fs::remove_file(token_file());
            println!("Saiu e revogou o token deste computador.");
        }
        "sync" => {
            let r = app.sync()?;
            println!(
                "Faixas: {} ({}) • Playlists: {} • Plays enviados: {}",
                r.track_count,
                if r.tracks_updated { "atualizadas" } else { "sem mudanças" },
                if r.playlists_updated { "atualizadas" } else { "sem mudanças" },
                r.plays_sent
            );
        }
        "stats" => {
            let lib = app.library();
            println!("Faixas: {}", lib.track_count()?);
            println!("Artistas: {}", lib.artists()?.len());
            println!("Playlists: {}", lib.playlists()?.len());
            println!("Restritas: {}", lib.restricted_tracks()?.len());
            println!(
                "Baixadas: {} ({:.1} MB)",
                lib.downloaded_ids()?.len(),
                lib.downloads_size()? as f64 / 1_048_576.0
            );
            println!("Plays pendentes: {}", lib.pending_plays(100_000)?.len());
            println!("Mais tocadas:");
            for t in lib.tracks(TrackOrder::MostPlayed, false)?.iter().take(5) {
                println!("  {:>4}×  {} — {}", t.play_count, t.artist, t.title);
            }
            match app.last_sync()? {
                Some(ts) => println!("Última sincronização: unix {ts}"),
                None => println!("Ainda não sincronizado: rode `ilmarfy sync`"),
            }
        }
        "search" => {
            for t in app.library().search(&args[1..].join(" "), 30)? {
                println!("{}  {} — {}  [{}]", t.id, t.artist, t.title, fmt_duration(t.duration));
            }
        }
        "artists" => {
            for a in app.library().artists()? {
                println!("{:>4}  {}", a.track_count, a.name);
            }
        }
        "playlists" => {
            for p in app.library().playlists()? {
                println!("{}  {} ({} faixas)", p.id, p.name, p.track_ids.len());
            }
        }
        "playlist" => {
            for t in app.library().playlist_tracks(&arg)? {
                let off = if app.is_downloaded(&t.id)? { "⬇" } else { " " };
                println!("{off} {}  {} — {}", t.id, t.artist, t.title);
            }
        }
        "download" => {
            let mut last = 0;
            let path = app.download(&arg, |done, total| {
                let pct = (done * 100).checked_div(total).unwrap_or(0);
                if pct >= last + 10 {
                    last = pct;
                    eprint!("\r{pct}%");
                }
                true
            })?;
            eprintln!("\rBaixada: {}", path.display());
        }
        "download-playlist" => {
            let (n, failures) = app.download_playlist(&arg, |i, total, t| {
                eprintln!("[{}/{}] {} — {}", i + 1, total, t.artist, t.title);
                true
            })?;
            println!("{n} faixas baixadas agora; {} falharam", failures.len());
            for (id, e) in failures {
                println!("  {id}: {e}");
            }
        }
        "remove-download" => {
            app.remove_download(&arg)?;
            println!("Removida do aparelho.");
        }
        "play" => {
            app.record_play(&arg)?;
            println!("Play registrado (vai ao servidor no próximo sync).");
        }
        "favorite" => {
            let on = app.toggle_favorite(&arg)?;
            println!(
                "{}",
                if on {
                    "Adicionada aos favoritos."
                } else {
                    "Removida dos favoritos."
                }
            );
        }
        "lyrics" => match app.api().lyrics(&arg)? {
            Some(l) if !l.synced_lines().is_empty() => {
                for line in l.synced_lines() {
                    println!("[{}] {}", fmt_duration((line.time_ms / 1000) as i64), line.text);
                }
            }
            Some(l) => println!("{}", l.plain),
            None => println!("Sem letra."),
        },
        "flow" => {
            let f = app.api().flow()?;
            println!("Baseado em: {}", f.seeds.join(", "));
            for t in f.tracks {
                println!("{}  {} — {}", t.id, t.artist, t.title);
            }
        }
        "restricted" => {
            let r = app.api().flow_restricted()?;
            println!(
                "Artistas: {}",
                r.blocked_artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
            );
            for t in r.disliked_tracks {
                println!("  {} — {}", t.artist, t.title);
            }
        }
        "devices" => {
            for d in app.api().devices()? {
                println!("{}  {} [{}]{}", d.id, d.name, d.platform, if d.current { "  ← este" } else { "" });
            }
        }
        _ => println!("{USAGE}"),
    }
    Ok(())
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Computador".into())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Erro: {e}");
            ExitCode::FAILURE
        }
    }
}
