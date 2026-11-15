//! Testes contra o servidor real, só de leitura (não alteram biblioteca, playlists nem contagens).
//! `ILMARFY_TOKEN=$(cat ~/Library/Application\ Support/ilmarfy-native/token) cargo test -p ilmarfy-core -- --ignored --test-threads 1`

use ilmarfy_core::{DEFAULT_SERVER, Ilmarfy, Source, TrackOrder};
use std::path::PathBuf;

fn app(name: &str) -> (Ilmarfy, PathBuf) {
    let token = std::env::var("ILMARFY_TOKEN").expect("defina ILMARFY_TOKEN (token salvo pelo `ilmarfy login`)");
    let server = std::env::var("ILMARFY_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_string());
    let dir = std::env::temp_dir().join(format!("ilmarfy-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    (Ilmarfy::open(&dir, &server, Some(token)).unwrap(), dir)
}

#[test]
#[ignore]
fn versao_e_dispositivo_atual() {
    let (app, dir) = app("versao");
    let v = app.api().version().unwrap();
    assert!(v.features.iter().any(|f| f == "device_tokens"), "{:?}", v.features);
    assert!(
        app.api().devices().unwrap().iter().any(|d| d.current),
        "o token usado deve aparecer como atual"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore]
fn sincroniza_e_usa_etag_na_segunda_vez() {
    let (app, dir) = app("sync");
    let first = app.sync().unwrap();
    println!("primeira: {first:?}");
    assert!(first.tracks_updated && first.track_count > 0);
    assert!(!app.library().artists().unwrap().is_empty());

    let second = app.sync().unwrap();
    println!("segunda: {second:?}");
    assert!(
        !second.tracks_updated && !second.playlists_updated,
        "sem mudanças, o servidor deve responder 304"
    );
    assert_eq!(second.track_count, first.track_count);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore]
fn baixa_uma_faixa_e_toca_do_arquivo() {
    let (app, dir) = app("download");
    app.sync().unwrap();
    // A faixa mais curta, para o teste ser rápido
    let track = app
        .library()
        .tracks(TrackOrder::Title, false)
        .unwrap()
        .into_iter()
        .filter(|t| t.duration > 30)
        .min_by_key(|t| t.duration)
        .expect("biblioteca vazia");
    assert!(matches!(app.source(&track).unwrap(), Source::Remote { .. }));

    let path = app.download(&track.id, |_, _| true).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    println!("{} — {} ({} s): {} bytes", track.artist, track.title, track.duration, size);
    assert!(size > 50_000);
    assert_eq!(app.source(&track).unwrap(), Source::Local(path.clone()));

    app.remove_download(&track.id).unwrap();
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
#[ignore]
fn letras_e_restritos_do_flow() {
    let (app, dir) = app("leitura");
    app.sync().unwrap();
    let top = app.library().tracks(TrackOrder::MostPlayed, false).unwrap();
    let with_lyrics = top.iter().take(10).filter_map(|t| app.api().lyrics(&t.id).unwrap()).count();
    println!("letras encontradas nas 10 mais tocadas: {with_lyrics}");
    let r = app.api().flow_restricted().unwrap();
    println!(
        "restritos: {} artistas, {} músicas",
        r.blocked_artists.len(),
        r.disliked_tracks.len()
    );
    let _ = std::fs::remove_dir_all(dir);
}
