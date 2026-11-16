//! Diagnóstico da saída de áudio: `cargo run -p ilmarfy-desktop --example audio_check -- [arquivo.mp3]`
//! Toca 2 s com volume zero e confere se a posição avança.
fn main() {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink().expect("abrir saída");
    sink.log_on_drop(false);
    println!("saída: {:?}", sink.config());
    let Some(path) = std::env::args().nth(1) else { return };
    let player = rodio::Player::connect_new(sink.mixer());
    player.set_volume(0.0);
    let source = rodio::Decoder::try_from(std::fs::File::open(&path).expect("abrir arquivo")).expect("decodificar");
    player.append(source);
    player.play();
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!("posição depois de 2 s: {:?} (tocando: {})", player.get_pos(), !player.empty());
}
