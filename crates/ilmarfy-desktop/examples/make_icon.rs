//! Gera o ícone do Ilmarfy (PNG 1024x1024 com transparência): `cargo run -p ilmarfy-desktop --example make_icon -- saida.png`
use image::{Rgba, RgbaImage};

const SIZE: u32 = 1024;
const SS: u32 = 4;

fn round_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    x >= x0 && x <= x1 && y >= y0 && y <= y1 && (x - cx).powi(2) + (y - cy).powi(2) <= r * r
}

/// Fones de ouvido: arco (metade de cima de um anel) + duas conchas arredondadas
fn headphones(x: f32, y: f32) -> bool {
    let (cx, cy) = (512.0, 560.0);
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    let arc = y <= cy && (215.0..=275.0).contains(&d);
    let side = round_rect(x, y, 237.0, cy - 10.0, 277.0, 640.0, 20.0) || round_rect(x, y, 747.0, cy - 10.0, 787.0, 640.0, 20.0);
    let cups = round_rect(x, y, 220.0, 560.0, 350.0, 780.0, 44.0) || round_rect(x, y, 674.0, 560.0, 804.0, 780.0, 44.0);
    arc || side || cups
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "icon.png".into());
    let mut img = RgbaImage::new(SIZE, SIZE);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0, 0.0, 0.0);
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                    if !round_rect(x, y, 100.0, 100.0, 924.0, 924.0, 185.0) {
                        continue;
                    }
                    let (cr, cg, cb) = if headphones(x, y) {
                        (255.0, 255.0, 255.0)
                    } else {
                        // Verde do Ilmarfy, um pouco mais escuro embaixo
                        let t = (y - 100.0) / 824.0;
                        (29.0 - 10.0 * t, 185.0 - 50.0 * t, 84.0 - 20.0 * t)
                    };
                    r += cr;
                    g += cg;
                    b += cb;
                    a += 1.0;
                }
            }
            if a > 0.0 {
                img.put_pixel(
                    px,
                    py,
                    Rgba([(r / a) as u8, (g / a) as u8, (b / a) as u8, (255.0 * a / (SS * SS) as f32) as u8]),
                );
            }
        }
    }
    img.save(&out).expect("salvar ícone");
}
