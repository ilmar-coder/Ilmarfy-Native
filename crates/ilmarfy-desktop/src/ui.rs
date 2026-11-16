//! Componentes visuais no estilo Spotify: cores, fontes, capas (com capa gerada quando falta),
//! mosaico de playlist, botões redondos e barra de progresso.

use eframe::egui::{self, Align2, Color32, CornerRadius, FontFamily, FontId, Mesh, Pos2, Rect, Response, Sense, Shape, Ui, Vec2};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x12, 0x12, 0x12);
pub const BLACK: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const CARD: Color32 = Color32::from_rgb(0x18, 0x18, 0x18);
pub const HOVER: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2A);
pub const TILE: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);
pub const TEXT: Color32 = Color32::WHITE;
pub const SUB: Color32 = Color32::from_rgb(0xB3, 0xB3, 0xB3);
pub const DIM: Color32 = Color32::from_rgb(0x7A, 0x7A, 0x7A);
pub const GREEN: Color32 = Color32::from_rgb(0x1E, 0xD7, 0x60);

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_bold(size: f32) -> FontId {
    FontId::new(size, bold())
}

/// Fonte do sistema parecida com a do Spotify (Avenir Next no Mac, Noto Sans no Fedora),
/// com as fontes do egui e de emoji como reserva para ícones.
pub fn setup_fonts(ctx: &egui::Context) {
    use egui::epaint::text::{FontData, FontDefinitions};
    let mut defs = FontDefinitions::default();
    let mut fallback = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for (name, data) in [
        ("emoji-icons", epaint_default_fonts::EMOJI_ICON),
        ("noto-emoji", epaint_default_fonts::NOTO_EMOJI_REGULAR),
    ] {
        defs.font_data.insert(name.into(), Arc::new(FontData::from_static(data)));
        fallback.push(name.into());
    }

    // (arquivo, índice na coleção) da fonte normal e da negrito
    type Face = (&'static str, u32);
    let candidates: [(Face, Face); 4] = [
        (
            ("/System/Library/Fonts/Avenir Next.ttc", 7),
            ("/System/Library/Fonts/Avenir Next.ttc", 0),
        ),
        (
            ("/usr/share/fonts/google-noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/google-noto/NotoSans-Bold.ttf", 0),
        ),
        (
            ("/usr/share/fonts/noto/NotoSans-Regular.ttf", 0),
            ("/usr/share/fonts/noto/NotoSans-Bold.ttf", 0),
        ),
        (
            ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf", 0),
            ("/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf", 0),
        ),
    ];
    let load = |(path, index): (&str, u32)| std::fs::read(path).ok().map(|bytes| FontData::from_blob(Arc::new(bytes), index));
    let (mut regular, mut bold_name) = ("Ubuntu-Light".to_string(), "Ubuntu-Light".to_string());
    if let Some((r, b)) = candidates.into_iter().find_map(|(r, b)| Some((load(r)?, load(b)?))) {
        defs.font_data.insert("ui-regular".into(), Arc::new(r));
        defs.font_data.insert("ui-bold".into(), Arc::new(b));
        regular = "ui-regular".into();
        bold_name = "ui-bold".into();
    }
    let with = |first: &str| {
        std::iter::once(first.to_string())
            .chain(fallback.iter().cloned())
            .collect::<Vec<_>>()
    };
    defs.families.insert(FontFamily::Proportional, with(&regular));
    defs.families.insert(bold(), with(&bold_name));
    ctx.set_fonts(defs);
    // Títulos em outros alfabetos (japonês etc.)
    ctx.add_font_provider(Arc::new(egui_system_font_provider::SystemFontProvider::new()));
}

pub fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    // Sempre escuro, como o Spotify (antes seguia o tema claro do macOS e o texto ficava ilegível)
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = Color32::from_rgb(0x28, 0x28, 0x28);
        v.extreme_bg_color = Color32::from_rgb(0x24, 0x24, 0x24);
        v.override_text_color = Some(TEXT);
        v.selection.bg_fill = Color32::from_rgb(0x1D, 0x6B, 0x3A);
        v.hyperlink_color = GREEN;
        v.widgets.inactive.weak_bg_fill = TILE;
        v.widgets.inactive.bg_fill = TILE;
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x3A, 0x3A, 0x3A);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0x44, 0x44, 0x44);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = CornerRadius::same(6);
            w.fg_stroke.color = TEXT;
        }
        v.widgets.noninteractive.fg_stroke.color = SUB;
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.text_styles.insert(egui::TextStyle::Body, font(14.0));
        s.text_styles.insert(egui::TextStyle::Button, font(14.0));
        s.text_styles.insert(egui::TextStyle::Small, font(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, font_bold(22.0));
    });
}

// ------------------------------------------------------------ Capas

fn hash(s: &str) -> u64 {
    s.bytes()
        .fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
}

/// Cor viva derivada do texto (mesmo artista/álbum => mesma cor)
pub fn seed_color(seed: &str) -> Color32 {
    let h = hash(seed);
    let hue = (h % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.55, 0.62, 1.0).into()
}

fn gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    for (pos, c) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, c);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// Capa gerada quando a música não tem: gradiente com a cor do artista e uma nota musical
pub fn placeholder(ui: &Ui, rect: Rect, seed: &str, radius: f32) {
    let c = seed_color(seed);
    let dark = Color32::from_rgb(c.r() / 3, c.g() / 3, c.b() / 3);
    let painter = ui.painter().with_clip_rect(rect);
    let _ = radius;
    gradient(&painter, rect, c, dark);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "♪",
        font(rect.height() * 0.42),
        Color32::from_white_alpha(200),
    );
}

/// Desenha a capa (imagem da URL) ou a capa gerada, no retângulo dado
pub fn cover_at(ui: &mut Ui, rect: Rect, url: &str, seed: &str, radius: f32) {
    placeholder(ui, rect, seed, radius);
    if !url.is_empty() {
        egui::Image::new(url).corner_radius(radius).paint_at(ui, rect);
    }
}

/// Capa de playlist no estilo Spotify: mosaico 2x2 com as capas das primeiras músicas
pub fn mosaic_at(ui: &mut Ui, rect: Rect, covers: &[String], seed: &str, radius: f32) {
    let distinct: Vec<&String> = {
        let mut seen = std::collections::HashSet::new();
        covers.iter().filter(|c| !c.is_empty() && seen.insert(*c)).take(4).collect()
    };
    if distinct.len() < 4 {
        return cover_at(ui, rect, distinct.first().map(|s| s.as_str()).unwrap_or(""), seed, radius);
    }
    let half = rect.size() / 2.0;
    for (i, url) in distinct.iter().enumerate() {
        let min = rect.min + Vec2::new(if i % 2 == 1 { half.x } else { 0.0 }, if i >= 2 { half.y } else { 0.0 });
        let r = Rect::from_min_size(min, half);
        egui::Image::new(url.as_str()).paint_at(ui, r);
    }
}

pub fn mosaic(ui: &mut Ui, covers: &[String], seed: &str, size: f32, radius: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    mosaic_at(ui, rect, covers, seed, radius);
    resp
}

/// Capa especial (Favoritos, Offline, Flow): gradiente com um símbolo
pub fn special_cover(ui: &mut Ui, size: f32, top: Color32, bottom: Color32, symbol: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    gradient(ui.painter(), rect, top, bottom);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, symbol, font(size * 0.42), TEXT);
    resp
}

// ------------------------------------------------------------ Botões

/// Ícone clicável (sem moldura): cinza, branco ao passar o mouse, verde quando ativo
pub fn icon_button(ui: &mut Ui, icon: &str, size: f32, active: bool, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 8.0), Sense::click());
    let color = if active {
        GREEN
    } else if resp.hovered() {
        TEXT
    } else {
        SUB
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, font(size), color);
    if active {
        ui.painter().circle_filled(rect.center_bottom() - Vec2::new(0.0, 1.0), 2.0, GREEN);
    }
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// Botão redondo preenchido (play)
pub fn circle_button(ui: &mut Ui, icon: &str, size: f32, fill: Color32, fg: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let r = if resp.hovered() { size * 0.52 } else { size * 0.5 };
    ui.painter().circle_filled(rect.center(), r, fill);
    // o triângulo do play parece deslocado à esquerda; compensa um pouco
    let nudge = if icon == "▶" { Vec2::new(size * 0.05, 0.0) } else { Vec2::ZERO };
    ui.painter()
        .text(rect.center() + nudge, Align2::CENTER_CENTER, icon, font(size * 0.45), fg);
    resp
}

/// Botão em pílula (ex.: "Tocar", filtros)
pub fn pill(ui: &mut Ui, text: &str, fill: Color32, fg: Color32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), font_bold(14.0), fg);
    let size = galley.size() + Vec2::new(28.0, 14.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() { fill.gamma_multiply(1.15) } else { fill };
    ui.painter().rect_filled(rect, size.y / 2.0, fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// Barra de progresso fina, clicável/arrastável. Devolve a nova fração se o usuário mexeu.
pub fn progress(ui: &mut Ui, frac: f32, width: f32) -> Option<f32> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::click_and_drag());
    let bar = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), 4.0));
    let hovered = resp.hovered() || resp.dragged();
    ui.painter().rect_filled(bar, 2.0, Color32::from_rgb(0x4D, 0x4D, 0x4D));
    let pointer = resp
        .interact_pointer_pos()
        .map(|p| ((p.x - bar.left()) / bar.width()).clamp(0.0, 1.0));
    let shown = if resp.dragged() {
        pointer.unwrap_or(frac)
    } else {
        frac.clamp(0.0, 1.0)
    };
    let filled = Rect::from_min_max(bar.min, Pos2::new(bar.left() + bar.width() * shown, bar.max.y));
    ui.painter().rect_filled(filled, 2.0, if hovered { GREEN } else { TEXT });
    if hovered {
        ui.painter().circle_filled(Pos2::new(filled.right(), bar.center().y), 6.0, TEXT);
    }
    if resp.drag_stopped() || resp.clicked() { pointer } else { None }
}

pub fn fmt_duration(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}
