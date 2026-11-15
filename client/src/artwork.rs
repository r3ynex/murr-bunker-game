use crate::appearance::portable_path;
use eframe::egui::{self, Color32, Pos2, Rect, Response, RichText, Ui, Vec2};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const UV: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));

#[derive(Clone, Copy)]
pub enum Icon {
    Login,
    Join,
    Catastrophe,
    Bunker,
    Events,
    Dossier,
    Players,
    Abilities,
    Host,
    Locked,
    Unlocked,
    Edit,
}

impl Icon {
    const ALL: [Self; 12] = [
        Self::Login,
        Self::Join,
        Self::Catastrophe,
        Self::Bunker,
        Self::Events,
        Self::Dossier,
        Self::Players,
        Self::Abilities,
        Self::Host,
        Self::Locked,
        Self::Unlocked,
        Self::Edit,
    ];
    pub const NAVIGATION: [Self; 6] = [
        Self::Catastrophe,
        Self::Bunker,
        Self::Events,
        Self::Dossier,
        Self::Players,
        Self::Abilities,
    ];

    fn filename(self) -> &'static str {
        match self {
            Self::Login => "login-continue.png",
            Self::Join => "join-submit.png",
            Self::Catastrophe => "nav-catastrophe.png",
            Self::Bunker => "nav-bunker.png",
            Self::Events => "nav-events.png",
            Self::Dossier => "nav-dossier.png",
            Self::Players => "nav-players.png",
            Self::Abilities => "nav-abilities.png",
            Self::Host => "lobby-host.png",
            Self::Locked => "card-locked.png",
            Self::Unlocked => "card-unlocked.png",
            Self::Edit => "card-edit.png",
        }
    }
}

pub struct Artwork {
    icons: [Option<egui::TextureHandle>; Icon::ALL.len()],
    pub catastrophe: Option<egui::TextureHandle>,
    pub bunker: Option<egui::TextureHandle>,
}

impl Artwork {
    pub fn load(ctx: &egui::Context) -> Self {
        Self {
            icons: Icon::ALL.map(|icon| load_icon_slot(ctx, icon)),
            catastrophe: texture(
                ctx,
                "design-catastrophe",
                include_bytes!("../assets/artwork/catastrophe.jpg"),
                2048,
            ),
            bunker: texture(
                ctx,
                "design-bunker",
                include_bytes!("../assets/artwork/bunker.png"),
                1024,
            ),
        }
    }

    pub fn reload_icons(&mut self, ctx: &egui::Context) {
        for icon in Icon::ALL {
            self.icons[icon as usize] = load_icon_slot(ctx, icon);
        }
    }

    pub fn icon(&self, slot: Icon, size: f32) -> Option<egui::Image<'static>> {
        let texture = self.icons[slot as usize].as_ref()?;
        let dimensions = texture.size_vec2();
        let size = dimensions * (size / dimensions.max_elem());
        Some(egui::Image::new((texture.id(), size)))
    }

    pub fn button(&self, slot: Icon, fallback: &str, size: f32) -> egui::Button<'static> {
        if let Some(image) = self.icon(slot, size) {
            egui::Button::image(image)
        } else {
            egui::Button::new(RichText::new(fallback).size(size))
        }
    }
}

fn load_icon_slot(ctx: &egui::Context, icon: Icon) -> Option<egui::TextureHandle> {
    let name = icon.filename();
    let relative = format!("icons/{name}");
    let portable = portable_path(&relative);
    let path = if portable.exists() {
        portable
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(relative)
    };
    load_icon(ctx, name, &path)
}

fn load_icon(ctx: &egui::Context, name: &str, path: &Path) -> Option<egui::TextureHandle> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1_048_576 {
        return None;
    }
    texture(ctx, name, &bytes, 512)
}

pub fn texture(
    ctx: &egui::Context,
    name: &str,
    data: &[u8],
    max_size: u32,
) -> Option<egui::TextureHandle> {
    let image = decode_image(data, max_size)?.into_rgba8();
    Some(ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    ))
}

pub(crate) fn decode_image(data: &[u8], max_size: u32) -> Option<image::DynamicImage> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 || width > max_size || height > max_size {
        return None;
    }
    image::load_from_memory(data).ok()
}

pub fn paint_cover(painter: &egui::Painter, texture: &egui::TextureHandle, rect: Rect) {
    let source = texture.size_vec2();
    let scale = (rect.width() / source.x).max(rect.height() / source.y);
    let visible = rect.size() / (source * scale);
    let uv = Rect::from_center_size(UV.center(), visible);
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

pub fn paint_contain(painter: &egui::Painter, texture: &egui::TextureHandle, rect: Rect) {
    let source = texture.size_vec2();
    let scale = (rect.width() / source.x).min(rect.height() / source.y);
    let fitted = Rect::from_center_size(rect.center(), source * scale);
    painter.image(texture.id(), fitted, UV, Color32::WHITE);
}

pub fn paint_gradient(rect: Rect, first: Color32, last: Color32, vertical: bool) -> egui::Shape {
    let (top_right, bottom_left) = if vertical {
        (first, last)
    } else {
        (last, first)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), first);
    mesh.colored_vertex(rect.right_top(), top_right);
    mesh.colored_vertex(rect.right_bottom(), last);
    mesh.colored_vertex(rect.left_bottom(), bottom_left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    egui::Shape::mesh(mesh)
}

pub fn flat_button(
    ui: &mut Ui,
    size: [f32; 2],
    button: egui::Button<'_>,
    colors: [Color32; 2],
    radius: u8,
) -> Response {
    let behind = ui.painter().add(egui::Shape::Noop);
    let response = ui.add_sized(size, button.frame(false).min_size(size.into()));
    let (hover, pressed) = crate::motion::button_progress(ui, &response);
    let shade = |color: Color32| color.gamma_multiply(1.0 + 0.15 * hover - 0.40 * pressed);
    let rect = response.rect;
    let shape = if colors[0] == colors[1] {
        egui::Shape::rect_filled(rect, radius, shade(colors[0]))
    } else {
        paint_gradient(rect, shade(colors[0]), shade(colors[1]), false)
    };
    ui.painter().set(behind, shape);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0_f32, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    response
}

pub fn centered_row<R>(
    ui: &mut Ui,
    width: f32,
    height: f32,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let rect = Rect::from_center_size(rect.center(), Vec2::new(width, height));
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    contents(&mut row)
}

pub fn input_row(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    height: f32,
    fill: Color32,
    font: egui::FontId,
) -> Response {
    ui.add_sized(
        [width, height],
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .hint_text_font(font.clone())
            .font(font)
            .margin(egui::Margin::symmetric(18, 0))
            .min_size(Vec2::new(width, height))
            .background_color(fill)
            .vertical_align(egui::Align::Center),
    )
}

pub const TABLE_CELL_PADDING: f32 = 4.0;

pub fn table_cell(
    ui: &mut Ui,
    size: [f32; 2],
    color: Color32,
    fill: Color32,
    contents: impl FnOnce(&mut Ui),
) {
    let (rect, _) = ui.allocate_exact_size(size.into(), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0u8, fill);
    ui.painter().rect_stroke(
        rect,
        0u8,
        egui::Stroke::new(0.5_f32, color),
        egui::StrokeKind::Inside,
    );
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(TABLE_CELL_PADDING))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    cell.set_clip_rect(rect.intersect(ui.clip_rect()));
    contents(&mut cell);
}
