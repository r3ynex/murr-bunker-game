use crate::appearance::{FontWeight, Theme};
use crate::motion::animated_button;
use eframe::egui::{self, Align2, Color32, Painter, Pos2, Shape, Stroke, TextureId, Vec2};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};
use std::io::Read;
use std::time::{Duration, Instant};

const MESHES: &str = include_str!("../../web/dice-models.json");
const BUNDLED_D6_GLB: &[u8] = include_bytes!("../../d6.glb");
const BUNDLED_D20_GLB: &[u8] = include_bytes!("../../d20.glb");
const GLB_MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"
const MAX_GLB_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiceRoll {
    id: String,
    player: String,
    sides: u32,
    value: u32,
    elapsed_ms: i64,
    settle_ms: u32,
    duration_ms: u32,
}

pub struct DiceOverlay {
    meshes: Meshes,
    active: Option<(DiceRoll, Instant)>,
}

impl Default for DiceOverlay {
    fn default() -> Self {
        Self {
            meshes: Meshes::load(),
            active: None,
        }
    }
}

impl DiceOverlay {
    pub fn reload_meshes(&mut self) {
        self.meshes = Meshes::load();
    }

    pub fn sync(&mut self, roll: Option<&DiceRoll>) {
        let Some(roll) = roll.filter(|roll| {
            matches!(roll.sides, 6 | 20)
                && (1..=roll.sides).contains(&roll.value)
                && (1..roll.duration_ms).contains(&roll.settle_ms)
                && roll.duration_ms <= 10_000
                && (-1000..i64::from(roll.duration_ms)).contains(&roll.elapsed_ms)
        }) else {
            self.active = None;
            return;
        };
        if self
            .active
            .as_ref()
            .is_some_and(|(active, _)| active.id == roll.id)
        {
            return;
        }
        let now = Instant::now();
        let offset = Duration::from_millis(roll.elapsed_ms.unsigned_abs());
        let start = if roll.elapsed_ms >= 0 {
            now.checked_sub(offset).unwrap_or(now)
        } else {
            now + offset
        };
        self.active = Some((roll.clone(), start));
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some((roll, start)) = self.active.clone() else {
            return;
        };
        let now = Instant::now();
        if now < start {
            ctx.request_repaint_after(start - now);
            return;
        }
        let elapsed = now.duration_since(start).as_secs_f32() * 1000.0;
        if elapsed >= roll.duration_ms as f32 {
            return;
        }
        let t = (elapsed / roll.settle_ms as f32).clamp(0.0, 1.0);
        let fade = smooth((elapsed / 180.0).min(1.0))
            * smooth(((roll.duration_ms as f32 - elapsed) / 450.0).min(1.0));
        let result = smooth(((elapsed - roll.settle_ms as f32) / 180.0).clamp(0.0, 1.0));
        let scene = ctx.content_rect();
        let radius = (scene.height() * 0.23).min(scene.width() * 0.34).min(235.0);
        let stop = scene.center() - Vec2::new(0.0, 21.0);
        let flight = 1.0 - (1.0 - (t / 0.78).min(1.0)).powi(3);
        let center = Pos2::new(
            stop.x + (1.0 - flight) * radius * 0.45,
            egui::lerp((scene.bottom() + radius * 1.6)..=stop.y, flight)
                - (PI * flight).sin() * radius * 0.38,
        );
        let (mesh, label_prefix) = if roll.sides == 6 {
            (&mut self.meshes.d6, "dice-d6")
        } else {
            (&mut self.meshes.d20, "dice-d20")
        };
        mesh.ensure_textures(ctx, label_prefix);
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dice-animation"));
        ctx.move_to_top(layer);
        let mut painter = ctx.layer_painter(layer);
        painter.set_clip_rect(scene);
        painter.rect_filled(scene, 0u8, Color32::from_black_alpha((216.0 * fade) as u8));
        paint_glow(&painter, stop, radius * 1.95, roll.sides, fade);
        paint_die(&painter, theme, mesh, &roll, center, radius, t, fade);
        let caption = format!("{}  ·  D{}", roll.player, roll.sides);
        painter.text(
            stop + Vec2::new(0.0, radius * 1.19),
            Align2::CENTER_TOP,
            caption,
            FontWeight::Medium.font(theme.fonts.body),
            tint(Color32::from_rgb(230, 223, 207), fade * flight),
        );
        if result > 0.0 {
            painter.text(
                stop + Vec2::new(0.0, radius * 1.39),
                Align2::CENTER_TOP,
                roll.value,
                FontWeight::Bold.font(39.0),
                tint(Color32::from_rgb(255, 218, 148), fade * result),
            );
        }
        ctx.request_repaint();
    }
}

pub fn roll_buttons(ui: &mut egui::Ui, enabled: bool) -> Option<u32> {
    let mut sides = None;
    for (value, label) in [(6, "Бросить D6"), (20, "Бросить D20")] {
        if ui
            .add_enabled(enabled, animated_button(egui::Button::new(label)))
            .clicked()
        {
            sides = Some(value);
        }
    }
    sides
}

#[derive(Clone, Copy, Default, Deserialize)]
struct V(f32, f32, f32);

impl V {
    fn plus(self, b: Self) -> Self {
        Self(self.0 + b.0, self.1 + b.1, self.2 + b.2)
    }
    fn minus(self, b: Self) -> Self {
        self.plus(b.times(-1.0))
    }
    fn times(self, s: f32) -> Self {
        Self(self.0 * s, self.1 * s, self.2 * s)
    }
    fn dot(self, b: Self) -> f32 {
        self.0 * b.0 + self.1 * b.1 + self.2 * b.2
    }
    fn cross(self, b: Self) -> Self {
        Self(
            self.1 * b.2 - self.2 * b.1,
            self.2 * b.0 - self.0 * b.2,
            self.0 * b.1 - self.1 * b.0,
        )
    }
    fn len(self) -> f32 {
        self.dot(self).sqrt()
    }
    fn unit(self) -> Self {
        let l = self.len();
        if l > 1e-8 {
            self.times(l.recip())
        } else {
            Self(0.0, 0.0, 1.0)
        }
    }
    // Rodrigues' single-axis rotation: strictly unidirectional spin without gimbal wobble.
    fn rotate_axis(self, axis: Self, angle: f32) -> Self {
        let (s, c) = angle.sin_cos();
        let axv = axis.cross(self);
        let adv = axis.dot(self);
        self.times(c)
            .plus(axv.times(s))
            .plus(axis.times(adv * (1.0 - c)))
    }
}

struct EmbeddedTexture {
    bytes: Vec<u8>,
    handle: Option<egui::TextureHandle>,
    failed: bool,
}

#[derive(Deserialize)]
struct Meshes {
    d6: DieMesh,
    d20: DieMesh,
}

#[derive(Deserialize)]
struct DieMesh {
    vertices: Vec<V>,
    #[serde(default)]
    uvs: Vec<[f32; 2]>,
    faces: Vec<Face>,
    #[serde(default)]
    triangles: Vec<MeshTriangle>,
    #[serde(skip)]
    textures: Vec<EmbeddedTexture>,
}

#[derive(Deserialize)]
struct Face {
    value: u32,
    #[serde(default)]
    indices: Vec<usize>,
    #[serde(default)]
    center: Option<V>,
    #[serde(default)]
    normal: Option<V>,
    #[serde(default)]
    right: Option<V>,
    #[serde(default)]
    up: Option<V>,
    #[serde(default, rename = "triCount")]
    tri_count: usize,
}

#[derive(Clone, Deserialize)]
struct MeshTriangle {
    indices: [usize; 3],
    #[serde(default)]
    face: Option<u32>,
    #[serde(default)]
    color: Option<[u8; 3]>,
    #[serde(default)]
    image: Option<usize>,
}

impl Meshes {
    fn load() -> Self {
        let mut meshes = Self::load_json();
        if let Some(d6) = load_glb_file(&["d6.glb", "assets/dice/d6.glb"], 6)
            .or_else(|| parse_glb(BUNDLED_D6_GLB, 6))
        {
            meshes.d6 = d6;
        }
        if let Some(d20) = load_glb_file(&["d20.glb", "assets/dice/d20.glb"], 20)
            .or_else(|| parse_glb(BUNDLED_D20_GLB, 20))
        {
            meshes.d20 = d20;
        }
        meshes
    }

    fn load_json() -> Self {
        const LIMIT: u64 = 2 * 1024 * 1024;
        let mut data = Vec::new();
        if std::fs::File::open(crate::appearance::portable_path("dice-models.json"))
            .and_then(|file| file.take(LIMIT + 1).read_to_end(&mut data))
            .is_ok()
            && data.len() <= LIMIT as usize
        {
            if let Ok(custom) = serde_json::from_slice::<Self>(&data) {
                if custom.d6.valid(6, 4) && custom.d20.valid(20, 3) {
                    return custom;
                }
            }
        }
        serde_json::from_str(MESHES).expect("bundled dice meshes")
    }
}

impl DieMesh {
    fn ensure_textures(&mut self, ctx: &egui::Context, prefix: &str) {
        for (idx, tex) in self.textures.iter_mut().enumerate() {
            if tex.handle.is_some() || tex.failed || tex.bytes.is_empty() {
                continue;
            }
            match image::load_from_memory(&tex.bytes) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let handle = ctx.load_texture(
                        format!("{prefix}-tex-{idx}"),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    tex.handle = Some(handle);
                }
                Err(_) => {
                    tex.failed = true;
                }
            }
        }
    }

    fn has_loaded_texture(&self) -> bool {
        self.textures.iter().any(|t| t.handle.is_some())
    }

    fn texture_id(&self, idx: Option<usize>) -> Option<TextureId> {
        idx.and_then(|i| self.textures.get(i))
            .and_then(|t| t.handle.as_ref())
            .map(|h| h.id())
    }

    fn valid(&self, sides: u32, corners: usize) -> bool {
        if self.faces.len() != sides as usize || self.vertices.is_empty() {
            return false;
        }
        if !self.triangles.is_empty() {
            let mut seen = [false; 21];
            for face in &self.faces {
                if !(1..=sides).contains(&face.value) || seen[face.value as usize] {
                    return false;
                }
                let (_, normal, right, _) = face_anchor(&self.vertices, face);
                if !(normal.dot(normal) > 0.9 && right.dot(right) > 0.9) {
                    return false;
                }
                seen[face.value as usize] = true;
            }
            return self
                .triangles
                .iter()
                .all(|t| t.indices.iter().all(|&i| i < self.vertices.len()));
        }
        if self.vertices.len() > 80 || self.vertices.iter().any(|&v| !(v.dot(v) <= 2.0)) {
            return false;
        }
        let mut seen = [false; 21];
        for face in &self.faces {
            if !(1..=sides).contains(&face.value)
                || seen[face.value as usize]
                || face.indices.len() != corners
                || face.indices.iter().any(|&i| i >= self.vertices.len())
            {
                return false;
            }
            let (center, normal, right, _) = face_basis(&self.vertices, face);
            if !(normal.dot(center) > 0.01 && right.dot(right) > 0.9)
                || self
                    .vertices
                    .iter()
                    .any(|&v| normal.dot(v.minus(center)) > 0.003)
            {
                return false;
            }
            seen[face.value as usize] = true;
        }
        true
    }
}

fn face_anchor(vertices: &[V], face: &Face) -> (V, V, V, V) {
    if let (Some(c), Some(n), Some(r), Some(u)) = (face.center, face.normal, face.right, face.up) {
        return (c, n.unit(), r.unit(), u.unit());
    }
    face_basis(vertices, face)
}

fn face_basis(vertices: &[V], face: &Face) -> (V, V, V, V) {
    let points = &face.indices;
    let a = vertices[points[0]];
    let b = vertices[points[1]];
    let normal = b.minus(a).cross(vertices[points[2]].minus(a)).unit();
    let center = points
        .iter()
        .map(|&index| vertices[index])
        .fold(V::default(), V::plus)
        .times((points.len() as f32).recip());
    let up = if points.len() == 4 {
        a.plus(b).times(0.5).minus(center)
    } else {
        a.minus(center)
    };
    let right = up.cross(normal).unit();
    (center, normal, right, normal.cross(right))
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn tint(color: Color32, opacity: f32) -> Color32 {
    color.gamma_multiply(opacity)
}

fn shade(color: Color32, light: f32, opacity: f32) -> Color32 {
    let channel = |value| (f32::from(value) * light).clamp(0.0, 255.0).round() as u8;
    Color32::from_rgb(
        channel(color.r()),
        channel(color.g()),
        channel(color.b()),
    )
    .gamma_multiply(opacity)
}

fn paint_glow(painter: &Painter, center: Pos2, radius: f32, sides: u32, opacity: f32) {
    let glow = if sides == 6 {
        Color32::from_rgb(38, 94, 112)
    } else {
        Color32::from_rgb(180, 102, 24)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, tint(glow, opacity * 0.56));
    for index in 0..48 {
        let angle = TAU * index as f32 / 48.0;
        mesh.colored_vertex(center + Vec2::angled(angle) * radius, Color32::TRANSPARENT);
        mesh.add_triangle(0, index + 1, (index + 1) % 48 + 1);
    }
    painter.add(Shape::mesh(mesh));
}

fn orientation(mesh: &DieMesh, value: u32, t: f32) -> impl Fn(V) -> V {
    let face = mesh
        .faces
        .iter()
        .find(|face| face.value == value)
        .expect("numbered dice face");
    let (_, normal, right, up) = face_anchor(&mesh.vertices, face);
    let spin_axis = V(0.32, 0.92, 0.22).unit();
    let remaining = (1.0 - t.clamp(0.0, 1.0)).powi(3);
    // Monotonic single-direction 3 full revolutions around one fixed 3D axis, landing at angle = 0.
    let angle = remaining * TAU * 3.0;
    move |v: V| {
        let aligned = V(v.dot(right), v.dot(up), v.dot(normal));
        aligned.rotate_axis(spin_axis, angle)
    }
}

fn screen_double_area(a: Pos2, b: Pos2, c: Pos2) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

fn paint_die(
    painter: &Painter,
    _theme: &Theme,
    mesh: &DieMesh,
    roll: &DiceRoll,
    center: Pos2,
    radius: f32,
    t: f32,
    opacity: f32,
) {
    let transform = orientation(mesh, roll.value, t);
    let project = |p: V| {
        let lens = radius * 3.8 / (3.8 - p.2).max(0.5);
        center + Vec2::new(p.0, -p.1) * lens
    };
    let vertices: Vec<_> = mesh.vertices.iter().copied().map(&transform).collect();
    let light = V(-0.44, 0.62, 0.80).unit();
    let default_base = if roll.sides == 6 {
        Color32::from_rgb(43, 120, 139)
    } else {
        Color32::from_rgb(188, 112, 34)
    };

    if !mesh.triangles.is_empty() {
        let has_uvs = mesh.uvs.len() == mesh.vertices.len();
        let mut visible: Vec<_> = mesh
            .triangles
            .iter()
            .filter_map(|tri| {
                let [i0, i1, i2] = tri.indices;
                let (a, b, c) = (vertices[i0], vertices[i1], vertices[i2]);
                let mid = a.plus(b).plus(c).times(1.0 / 3.0);
                let cross_n = b.minus(a).cross(c.minus(a));
                if cross_n.len() <= 1e-7 {
                    return None;
                }
                let normal = cross_n.unit();
                let towards = V(0.0, 0.0, 3.8).minus(mid).dot(normal);
                if towards <= 1e-4 {
                    return None;
                }
                let pa = project(a);
                let pb = project(b);
                let pc = project(c);
                // Screen Y is downward, so outward CCW 3D triangles have negative 2D signed area.
                // Skipping grazing / sliver triangles prevents projection artifacts.
                if screen_double_area(pa, pb, pc) > -0.25 {
                    return None;
                }
                Some((tri, [pa, pb, pc], mid.2, normal))
            })
            .collect();
        visible.sort_by(|a, b| a.2.total_cmp(&b.2));

        // Batch triangles into textured egui::Mesh shapes with ZERO strokes
        // (no wireframe mesh lines and no acute-angle miter spikes).
        let mut current_tex: Option<TextureId> = None;
        let mut batch = egui::Mesh::default();
        let flush = |painter: &Painter, batch: &mut egui::Mesh| {
            if !batch.indices.is_empty() {
                painter.add(Shape::mesh(std::mem::take(batch)));
            }
        };

        for (tri, pts, _, normal) in visible {
            let tex_id = if has_uvs {
                mesh.texture_id(tri.image)
            } else {
                None
            };
            let target_tex = tex_id.unwrap_or_default();
            if current_tex != Some(target_tex) {
                flush(painter, &mut batch);
                batch = egui::Mesh::with_texture(target_tex);
                current_tex = Some(target_tex);
            }

            let lit_color = if tex_id.is_some() {
                // Keep embedded white/orange/black textures crisp with clear 3D facet depth.
                let strength = (0.64 + normal.dot(light).max(0.0) * 0.36).min(1.0);
                let l = (strength * 255.0).round() as u8;
                Color32::from_rgb(l, l, l).gamma_multiply(opacity)
            } else {
                let strength = (0.34 + normal.dot(light).max(0.0) * 0.72).min(1.0);
                let base = tri
                    .color
                    .map(|[r, g, b]| Color32::from_rgb(r, g, b))
                    .unwrap_or(default_base);
                shade(base, strength, opacity)
            };

            let base_idx = batch.vertices.len() as u32;
            for k in 0..3 {
                let uv = if tex_id.is_some() {
                    let [u, v] = mesh.uvs[tri.indices[k]];
                    Pos2::new(u, v)
                } else {
                    egui::epaint::WHITE_UV
                };
                batch.vertices.push(egui::epaint::Vertex {
                    pos: pts[k],
                    uv,
                    color: lit_color,
                });
            }
            batch.add_triangle(base_idx, base_idx + 1, base_idx + 2);
        }
        flush(painter, &mut batch);

        // When the model uses its own embedded textures, never draw overlay numbers or pips.
        if mesh.has_loaded_texture() {
            return;
        }

        // Legacy untextured GLB fallback: draw procedural pips on D6 only (no D20 overlay).
        if roll.sides == 6 {
            for face in &mesh.faces {
                if face.tri_count > 6 {
                    continue;
                }
                let (fc_orig, fn_orig, fr_orig, fu_orig) = face_anchor(&mesh.vertices, face);
                let fc = transform(fc_orig);
                let fn_now = transform(fc_orig.plus(fn_orig.times(0.2))).minus(fc).unit();
                if fn_now.2 <= 0.18 {
                    continue;
                }
                let fr_now = transform(fc_orig.plus(fr_orig.times(0.2))).minus(fc).unit();
                let fu_now = transform(fc_orig.plus(fu_orig.times(0.2))).minus(fc).unit();
                let mark = tint(
                    Color32::from_rgb(255, 238, 200),
                    opacity * (0.60 + 0.40 * fn_now.2.clamp(0.0, 1.0)),
                );
                paint_pips(
                    painter,
                    face.value,
                    fc,
                    fr_now.times(0.85),
                    fu_now.times(0.85),
                    project,
                    mark,
                );
            }
        }
        return;
    }

    // Legacy JSON polyhedron fallback (used only if GLB files cannot be parsed).
    let mut visible: Vec<_> = mesh
        .faces
        .iter()
        .filter_map(|face| {
            let (mid, normal, right, up) = face_basis(&vertices, face);
            let towards_camera = V(0.0, 0.0, 3.8).minus(mid).dot(normal);
            (towards_camera > 1e-4).then_some((face, mid, normal, right, up))
        })
        .collect();
    visible.sort_by(|a, b| a.1 .2.total_cmp(&b.1 .2));
    for (face, mid, normal, right, up) in visible {
        let strength = (0.32 + normal.dot(light).max(0.0) * 0.73).min(1.0);
        let corners = face.indices.iter().map(|&i| project(vertices[i])).collect();
        painter.add(Shape::convex_polygon(
            corners,
            shade(default_base, strength * 0.62, opacity),
            Stroke::NONE,
        ));
        let inside = face
            .indices
            .iter()
            .map(|&i| project(mid.plus(vertices[i].minus(mid).times(0.93))))
            .collect();
        painter.add(Shape::convex_polygon(
            inside,
            shade(default_base, strength, opacity),
            Stroke::NONE,
        ));
        if roll.sides == 6 {
            let mark = tint(
                Color32::from_rgb(255, 238, 200),
                opacity * (0.57 + 0.43 * normal.2.clamp(0.0, 1.0)),
            );
            paint_pips(painter, face.value, mid, right, up, project, mark);
        }
    }
}

fn parse_face_material_name(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for i in 0..bytes.len() {
        if lower[i..].starts_with("face") {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            if !prev_ok {
                continue;
            }
            let mut j = i + 4;
            if j < bytes.len() && matches!(bytes[j], b'_' | b'-') {
                j += 1;
            }
            let start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let next_ok = j == bytes.len() || !bytes[j].is_ascii_alphanumeric();
            if j > start && next_ok {
                if let Ok(value) = lower[start..j].parse::<u32>() {
                    if (1..=100).contains(&value) {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

fn load_glb_file(candidates: &[&str], expected_sides: u32) -> Option<DieMesh> {
    for rel in candidates {
        let path = crate::appearance::portable_path(rel);
        let mut bytes = Vec::new();
        if std::fs::File::open(path)
            .and_then(|file| file.take(MAX_GLB_BYTES + 1).read_to_end(&mut bytes))
            .is_ok()
            && bytes.len() <= MAX_GLB_BYTES as usize
        {
            if let Some(mesh) = parse_glb(&bytes, expected_sides) {
                return Some(mesh);
            }
        }
    }
    None
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

fn read_f32_le(data: &[u8], offset: usize) -> Option<f32> {
    let slice: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
    Some(f32::from_le_bytes(slice))
}

fn linear_to_srgb_byte(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn mul_mat4(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}

fn node_local_mat4(node: &Value) -> [f32; 16] {
    if let Some(arr) = node.get("matrix").and_then(Value::as_array) {
        if arr.len() == 16 {
            let mut m = [0.0; 16];
            for (i, v) in arr.iter().enumerate() {
                m[i] = v.as_f64().unwrap_or(0.0) as f32;
            }
            return m;
        }
    }
    let vec3 = |key, def: [f32; 3]| -> [f32; 3] {
        node.get(key)
            .and_then(Value::as_array)
            .filter(|a| a.len() == 3)
            .map(|a| {
                [
                    a[0].as_f64().unwrap_or(def[0] as f64) as f32,
                    a[1].as_f64().unwrap_or(def[1] as f64) as f32,
                    a[2].as_f64().unwrap_or(def[2] as f64) as f32,
                ]
            })
            .unwrap_or(def)
    };
    let [tx, ty, tz] = vec3("translation", [0.0, 0.0, 0.0]);
    let [sx, sy, sz] = vec3("scale", [1.0, 1.0, 1.0]);
    let [qx, qy, qz, qw] = node
        .get("rotation")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 4)
        .map(|a| {
            [
                a[0].as_f64().unwrap_or(0.0) as f32,
                a[1].as_f64().unwrap_or(0.0) as f32,
                a[2].as_f64().unwrap_or(0.0) as f32,
                a[3].as_f64().unwrap_or(1.0) as f32,
            ]
        })
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let (x2, y2, z2) = (qx + qx, qy + qy, qz + qz);
    let (xx, xy, xz) = (qx * x2, qx * y2, qx * z2);
    let (yy, yz, zz) = (qy * y2, qy * z2, qz * z2);
    let (wx, wy, wz) = (qw * x2, qw * y2, qw * z2);
    [
        (1.0 - (yy + zz)) * sx,
        (xy + wz) * sx,
        (xz - wy) * sx,
        0.0,
        (xy - wz) * sy,
        (1.0 - (xx + zz)) * sy,
        (yz + wx) * sy,
        0.0,
        (xz + wy) * sz,
        (yz - wx) * sz,
        (1.0 - (xx + yy)) * sz,
        0.0,
        tx,
        ty,
        tz,
        1.0,
    ]
}

fn transform_point(m: &[f32; 16], p: V) -> V {
    V(
        m[0] * p.0 + m[4] * p.1 + m[8] * p.2 + m[12],
        m[1] * p.0 + m[5] * p.1 + m[9] * p.2 + m[13],
        m[2] * p.0 + m[6] * p.1 + m[10] * p.2 + m[14],
    )
}

fn subdivide_mesh(
    vertices: &mut Vec<V>,
    uvs: &mut Vec<[f32; 2]>,
    triangles: Vec<MeshTriangle>,
    levels: usize,
) -> Vec<MeshTriangle> {
    let mut tris = triangles;
    for _ in 0..levels {
        let mut next = Vec::with_capacity(tris.len() * 4);
        let mut edge_midpoints: HashMap<(usize, usize), usize> = HashMap::new();
        let mut midpoint = |a: usize, b: usize, verts: &mut Vec<V>, uvs: &mut Vec<[f32; 2]>| {
            let key = if a < b { (a, b) } else { (b, a) };
            if let Some(&idx) = edge_midpoints.get(&key) {
                return idx;
            }
            let idx = verts.len();
            verts.push(verts[a].plus(verts[b]).times(0.5));
            let [ua, va] = uvs[a];
            let [ub, vb] = uvs[b];
            uvs.push([(ua + ub) * 0.5, (va + vb) * 0.5]);
            edge_midpoints.insert(key, idx);
            idx
        };
        for tri in tris {
            let [i0, i1, i2] = tri.indices;
            let m01 = midpoint(i0, i1, vertices, uvs);
            let m12 = midpoint(i1, i2, vertices, uvs);
            let m20 = midpoint(i2, i0, vertices, uvs);
            for indices in [
                [i0, m01, m20],
                [m01, i1, m12],
                [m20, m12, i2],
                [m01, m12, m20],
            ] {
                next.push(MeshTriangle {
                    indices,
                    face: tri.face,
                    color: tri.color,
                    image: tri.image,
                });
            }
        }
        tris = next;
    }
    tris
}

fn parse_glb(bytes: &[u8], expected_sides: u32) -> Option<DieMesh> {
    if bytes.len() < 20 || read_u32_le(bytes, 0)? != GLB_MAGIC || read_u32_le(bytes, 4)? != 2 {
        return None;
    }
    let mut gltf: Option<Value> = None;
    let mut bin: Option<&[u8]> = None;
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let len = read_u32_le(bytes, offset)? as usize;
        let kind = read_u32_le(bytes, offset + 4)?;
        offset += 8;
        let chunk = bytes.get(offset..offset + len)?;
        if kind == CHUNK_JSON {
            gltf = serde_json::from_slice(chunk).ok();
        } else if kind == CHUNK_BIN {
            bin = Some(chunk);
        }
        offset += len;
    }
    let (gltf, bin) = (gltf?, bin?);
    let accessors = gltf.get("accessors")?.as_array()?;
    let views = gltf.get("bufferViews")?.as_array()?;
    let meshes = gltf.get("meshes")?.as_array()?;
    let materials = gltf.get("materials").and_then(Value::as_array);
    let gltf_textures = gltf.get("textures").and_then(Value::as_array);
    let gltf_images = gltf.get("images").and_then(Value::as_array);
    let nodes = gltf.get("nodes").and_then(Value::as_array)?;

    // Extract embedded binary images referenced via bufferView.
    let mut textures = Vec::new();
    if let Some(images) = gltf_images {
        for img in images {
            let bytes_opt = img
                .get("bufferView")
                .and_then(Value::as_u64)
                .and_then(|idx| views.get(idx as usize))
                .and_then(|bv| {
                    let start = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let len = bv.get("byteLength").and_then(Value::as_u64)? as usize;
                    bin.get(start..start + len).map(|s| s.to_vec())
                });
            textures.push(EmbeddedTexture {
                bytes: bytes_opt.unwrap_or_default(),
                handle: None,
                failed: false,
            });
        }
    }

    let read_pos = |idx: usize| -> Option<Vec<V>> {
        let acc = accessors.get(idx)?;
        if acc.get("componentType")?.as_u64()? != 5126 || acc.get("type")?.as_str()? != "VEC3" {
            return None;
        }
        let count = acc.get("count")?.as_u64()? as usize;
        let bv = views.get(acc.get("bufferView")?.as_u64()? as usize)?;
        let stride = bv.get("byteStride").and_then(Value::as_u64).unwrap_or(12) as usize;
        let base = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize
            + acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let o = base + i * stride;
            out.push(V(
                read_f32_le(bin, o)?,
                read_f32_le(bin, o + 4)?,
                read_f32_le(bin, o + 8)?,
            ));
        }
        Some(out)
    };

    let read_uv = |idx: usize| -> Option<Vec<[f32; 2]>> {
        let acc = accessors.get(idx)?;
        if acc.get("componentType")?.as_u64()? != 5126 || acc.get("type")?.as_str()? != "VEC2" {
            return None;
        }
        let count = acc.get("count")?.as_u64()? as usize;
        let bv = views.get(acc.get("bufferView")?.as_u64()? as usize)?;
        let stride = bv.get("byteStride").and_then(Value::as_u64).unwrap_or(8) as usize;
        let base = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize
            + acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let o = base + i * stride;
            out.push([read_f32_le(bin, o)?, read_f32_le(bin, o + 4)?]);
        }
        Some(out)
    };

    let read_idx = |idx: usize| -> Option<Vec<usize>> {
        let acc = accessors.get(idx)?;
        let ct = acc.get("componentType")?.as_u64()?;
        let count = acc.get("count")?.as_u64()? as usize;
        let bv = views.get(acc.get("bufferView")?.as_u64()? as usize)?;
        let base = bv.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize
            + acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let v = match ct {
                5121 => usize::from(*bin.get(base + i)?),
                5123 => usize::from(u16::from_le_bytes(
                    bin.get(base + i * 2..base + i * 2 + 2)?.try_into().ok()?,
                )),
                5125 => read_u32_le(bin, base + i * 4)? as usize,
                _ => return None,
            };
            out.push(v);
        }
        Some(out)
    };

    let mut children = HashSet::new();
    for node in nodes {
        if let Some(arr) = node.get("children").and_then(Value::as_array) {
            for c in arr {
                if let Some(i) = c.as_u64() {
                    children.insert(i as usize);
                }
            }
        }
    }
    let roots: Vec<usize> = gltf
        .get("scenes")
        .and_then(Value::as_array)
        .and_then(|s| s.get(gltf.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize))
        .and_then(|s| s.get("nodes"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_else(|| (0..nodes.len()).filter(|i| !children.contains(i)).collect());

    let mut vertices = Vec::new();
    let mut uvs = Vec::new();
    let mut raw_tris: Vec<([usize; 3], Option<u32>, Option<[u8; 3]>, Option<usize>)> = Vec::new();
    let identity: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut stack: Vec<(usize, [f32; 16])> = roots.into_iter().map(|r| (r, identity)).collect();

    while let Some((node_idx, parent)) = stack.pop() {
        let Some(node) = nodes.get(node_idx) else {
            continue;
        };
        let world = mul_mat4(&parent, &node_local_mat4(node));
        if let Some(mesh_idx) = node.get("mesh").and_then(Value::as_u64) {
            if let Some(prims) = meshes
                .get(mesh_idx as usize)
                .and_then(|m| m.get("primitives"))
                .and_then(Value::as_array)
            {
                for prim in prims {
                    if prim.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
                        continue;
                    }
                    let Some(pos_idx) = prim
                        .get("attributes")
                        .and_then(|a| a.get("POSITION"))
                        .and_then(Value::as_u64)
                    else {
                        continue;
                    };
                    let positions = read_pos(pos_idx as usize)?;
                    let prim_uvs = prim
                        .get("attributes")
                        .and_then(|a| a.get("TEXCOORD_0"))
                        .and_then(Value::as_u64)
                        .and_then(|i| read_uv(i as usize))
                        .filter(|u| u.len() == positions.len());
                    let base_v = vertices.len();
                    for (idx_p, p) in positions.iter().enumerate() {
                        vertices.push(transform_point(&world, *p));
                        uvs.push(prim_uvs.as_ref().map(|u| u[idx_p]).unwrap_or([0.0, 0.0]));
                    }
                    let indices = match prim.get("indices").and_then(Value::as_u64) {
                        Some(idx) => read_idx(idx as usize)?,
                        None => (0..positions.len()).collect(),
                    };
                    let mat = prim
                        .get("material")
                        .and_then(Value::as_u64)
                        .and_then(|i| materials.and_then(|ms| ms.get(i as usize)));
                    let face_val = mat
                        .and_then(|m| m.get("name"))
                        .and_then(Value::as_str)
                        .and_then(parse_face_material_name);
                    let pbr = mat.and_then(|m| m.get("pbrMetallicRoughness"));
                    let rgb = pbr
                        .and_then(|p| p.get("baseColorFactor"))
                        .and_then(Value::as_array)
                        .filter(|a| a.len() >= 3)
                        .map(|a| {
                            [
                                linear_to_srgb_byte(a[0].as_f64().unwrap_or(1.0) as f32),
                                linear_to_srgb_byte(a[1].as_f64().unwrap_or(1.0) as f32),
                                linear_to_srgb_byte(a[2].as_f64().unwrap_or(1.0) as f32),
                            ]
                        });
                    let image_idx = if prim_uvs.is_some() {
                        pbr.and_then(|p| p.get("baseColorTexture"))
                            .and_then(|t| t.get("index"))
                            .and_then(Value::as_u64)
                            .and_then(|ti| gltf_textures.and_then(|ts| ts.get(ti as usize)))
                            .and_then(|t| t.get("source"))
                            .and_then(Value::as_u64)
                            .map(|si| si as usize)
                            .filter(|&si| textures.get(si).is_some_and(|t| !t.bytes.is_empty()))
                    } else {
                        None
                    };
                    for tri in indices.chunks_exact(3) {
                        raw_tris.push((
                            [base_v + tri[0], base_v + tri[1], base_v + tri[2]],
                            face_val,
                            rgb,
                            image_idx,
                        ));
                    }
                }
            }
        }
        if let Some(kids) = node.get("children").and_then(Value::as_array) {
            for c in kids {
                if let Some(i) = c.as_u64() {
                    stack.push((i as usize, world));
                }
            }
        }
    }

    if vertices.is_empty() || raw_tris.is_empty() {
        return None;
    }

    let mut min = V(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = V(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &v in &vertices {
        min = V(min.0.min(v.0), min.1.min(v.1), min.2.min(v.2));
        max = V(max.0.max(v.0), max.1.max(v.1), max.2.max(v.2));
    }
    let center = min.plus(max).times(0.5);
    let mut max_d = 0.0_f32;
    for v in &mut vertices {
        *v = v.minus(center);
        max_d = max_d.max(v.len());
    }
    let scale = if max_d > 1e-6 { 0.96 / max_d } else { 1.0 };
    for v in &mut vertices {
        *v = v.times(scale);
    }

    let mut faces = Vec::with_capacity(expected_sides as usize);
    for val in 1..=expected_sides {
        let mut n_sum = V::default();
        let mut c_sum = V::default();
        let mut t_sum = V::default();
        let mut a_sum = 0.0_f32;
        let mut count = 0usize;
        for &(idx, face_val, _, _) in &raw_tris {
            if face_val != Some(val) {
                continue;
            }
            count += 1;
            let (a, b, c) = (vertices[idx[0]], vertices[idx[1]], vertices[idx[2]]);
            let n = b.minus(a).cross(c.minus(a));
            let area = n.len() * 0.5;
            if area < 1e-8 {
                continue;
            }
            let mid = a.plus(b).plus(c).times(1.0 / 3.0);
            let oriented = if n.dot(mid) >= 0.0 { n } else { n.times(-1.0) };
            n_sum = n_sum.plus(oriented);
            c_sum = c_sum.plus(mid.times(area));
            a_sum += area;

            // Derive face right axis from UV +u tangent (dP/du) so textured digits land upright.
            let uv0 = uvs[idx[0]];
            let uv1 = uvs[idx[1]];
            let uv2 = uvs[idx[2]];
            let du1 = uv1[0] - uv0[0];
            let dv1 = uv1[1] - uv0[1];
            let du2 = uv2[0] - uv0[0];
            let dv2 = uv2[1] - uv0[1];
            let det = du1 * dv2 - du2 * dv1;
            if det.abs() > 1e-8 {
                let dp1 = b.minus(a);
                let dp2 = c.minus(a);
                let tangent = dp1.times(dv2).minus(dp2.times(dv1)).times(det.recip());
                if tangent.len() > 1e-8 {
                    t_sum = t_sum.plus(tangent.unit().times(area));
                }
            }
        }
        if count == 0 || a_sum <= 1e-8 {
            return None;
        }
        let normal = n_sum.unit();
        let fc = c_sum.times(a_sum.recip());
        let t_proj = t_sum.minus(normal.times(t_sum.dot(normal)));
        let right = if t_proj.len() > 1e-5 {
            t_proj.unit()
        } else {
            let reference = if normal.1.abs() < 0.9 {
                V(0.0, 1.0, 0.0)
            } else {
                V(0.0, 0.0, 1.0)
            };
            reference.cross(normal).unit()
        };
        let up = normal.cross(right).unit();
        faces.push(Face {
            value: val,
            indices: Vec::new(),
            center: Some(fc),
            normal: Some(normal),
            right: Some(right),
            up: Some(up),
            tri_count: count,
        });
    }

    let has_any_texture = raw_tris.iter().any(|t| t.3.is_some());
    let mut triangles: Vec<MeshTriangle> = raw_tris
        .into_iter()
        .map(|(idx, face, color, image)| {
            let (a, b, c) = (vertices[idx[0]], vertices[idx[1]], vertices[idx[2]]);
            let n = b.minus(a).cross(c.minus(a));
            let mid = a.plus(b).plus(c).times(1.0 / 3.0);
            let indices = if n.dot(mid) >= 0.0 {
                idx
            } else {
                [idx[0], idx[2], idx[1]]
            };
            MeshTriangle {
                indices,
                face,
                color,
                image,
            }
        })
        .collect();

    // Subdivide coarse textured meshes so 2D affine UV interpolation is visually perspective-correct.
    if has_any_texture {
        let levels = if triangles.len() <= 16 {
            2
        } else if triangles.len() <= 40 {
            1
        } else {
            0
        };
        if levels > 0 {
            triangles = subdivide_mesh(&mut vertices, &mut uvs, triangles, levels);
        }
    }

    Some(DieMesh {
        vertices,
        uvs,
        faces,
        triangles,
        textures,
    })
}

fn paint_pips(
    painter: &Painter,
    value: u32,
    mid: V,
    right: V,
    up: V,
    project: impl Fn(V) -> Pos2,
    mark: Color32,
) {
    let pip = |x, y| {
        let center = mid.plus(right.times(x)).plus(up.times(y));
        let points = (0..12)
            .map(|i| {
                let angle = TAU * i as f32 / 12.0;
                project(
                    center
                        .plus(right.times(angle.cos() * 0.075))
                        .plus(up.times(angle.sin() * 0.075)),
                )
            })
            .collect();
        painter.add(Shape::convex_polygon(points, mark, Stroke::NONE));
    };
    if value % 2 != 0 {
        pip(0.0, 0.0);
    }
    if value >= 2 {
        pip(-0.29, 0.29);
        pip(0.29, -0.29);
    }
    if value >= 4 {
        pip(0.29, 0.29);
        pip(-0.29, -0.29);
    }
    if value == 6 {
        pip(-0.29, 0.0);
        pip(0.29, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_authoritative_result_lands_on_the_face_closest_to_the_viewer() {
        let meshes: Meshes = serde_json::from_str(MESHES).unwrap();
        for (mesh, sides, corners) in [(&meshes.d6, 6, 4), (&meshes.d20, 20, 3)] {
            assert!(mesh.valid(sides, corners));
            for value in 1..=sides {
                let settled: Vec<_> = mesh
                    .vertices
                    .iter()
                    .copied()
                    .map(orientation(mesh, value, 1.0))
                    .collect();
                let front = mesh
                    .faces
                    .iter()
                    .max_by(|a, b| {
                        face_basis(&settled, a)
                            .1
                             .2
                            .total_cmp(&face_basis(&settled, b).1 .2)
                    })
                    .unwrap();
                assert_eq!(front.value, value, "D{sides}");
            }
        }
    }

    #[test]
    fn bundled_glb_models_decode_textures_and_land_upright_on_every_face() {
        for (bytes, sides, corners) in [(BUNDLED_D6_GLB, 6, 4), (BUNDLED_D20_GLB, 20, 3)] {
            let mesh = parse_glb(bytes, sides).expect("bundled GLB must parse");
            assert!(mesh.valid(sides, corners));
            assert!(!mesh.textures.is_empty(), "D{sides} must have embedded texture");
            assert!(
                !mesh.textures[0].bytes.is_empty(),
                "D{sides} texture bytes must not be empty"
            );
            let decoded = image::load_from_memory(&mesh.textures[0].bytes)
                .expect("embedded PNG must decode");
            assert_eq!(decoded.width(), 1024);
            assert_eq!(decoded.height(), 1024);
            assert_eq!(mesh.uvs.len(), mesh.vertices.len());
            assert!(
                mesh.triangles.iter().all(|t| t.image == Some(0)),
                "D{sides} all triangles use embedded texture"
            );

            for value in 1..=sides {
                let t = orientation(&mesh, value, 1.0);
                let best = mesh
                    .faces
                    .iter()
                    .max_by(|a, b| {
                        let za = t(a.center.unwrap().plus(a.normal.unwrap().times(0.1)))
                            .minus(t(a.center.unwrap()))
                            .2;
                        let zb = t(b.center.unwrap().plus(b.normal.unwrap().times(0.1)))
                            .minus(t(b.center.unwrap()))
                            .2;
                        za.total_cmp(&zb)
                    })
                    .unwrap();
                assert_eq!(best.value, value, "Bundled GLB D{sides} landing on {value}");

                // Verify the face lands squarely flat toward the viewer (+Z) and upright (+Y).
                let fc = best.center.unwrap();
                let fn_orig = best.normal.unwrap();
                let fu = best.up.unwrap();
                let normal_after = t(fc.plus(fn_orig.times(0.1))).minus(t(fc)).unit();
                let up_after = t(fc.plus(fu.times(0.1))).minus(t(fc)).unit();
                assert!(
                    normal_after.2 > 0.999,
                    "Bundled GLB D{sides} face {value} should land flat toward viewer (nz={})",
                    normal_after.2
                );
                assert!(
                    up_after.1 > 0.999,
                    "Bundled GLB D{sides} face {value} should land upright (up_y={})",
                    up_after.1
                );
            }

            // Ensure front-facing triangles pass screen-area culling cleanly at every spin phase.
            let center = Pos2::new(400.0, 300.0);
            let radius = 160.0;
            let project = |p: V| {
                let lens = radius * 3.8 / (3.8 - p.2).max(0.5);
                center + Vec2::new(p.0, -p.1) * lens
            };
            for phase in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                let xform = orientation(&mesh, 1, phase);
                let verts: Vec<_> = mesh.vertices.iter().copied().map(&xform).collect();
                let mut visible_count = 0usize;
                for tri in &mesh.triangles {
                    let [i0, i1, i2] = tri.indices;
                    let (a, b, c) = (verts[i0], verts[i1], verts[i2]);
                    let mid = a.plus(b).plus(c).times(1.0 / 3.0);
                    let normal = b.minus(a).cross(c.minus(a)).unit();
                    if V(0.0, 0.0, 3.8).minus(mid).dot(normal) <= 1e-4 {
                        continue;
                    }
                    let (pa, pb, pc) = (project(a), project(b), project(c));
                    if screen_double_area(pa, pb, pc) > -0.25 {
                        continue;
                    }
                    for p in [pa, pb, pc] {
                        assert!(
                            p.distance(center) < radius * 2.0,
                            "D{sides} projected vertex out of bounds at t={phase}"
                        );
                    }
                    visible_count += 1;
                }
                assert!(
                    visible_count >= 16,
                    "D{sides} expected visible textured triangles at t={phase}, got {visible_count}"
                );
            }
        }
    }

    #[test]
    fn parse_glb_extracts_numbered_faces_and_lands_correctly() {
        assert_eq!(parse_face_material_name("face_01"), Some(1));
        assert_eq!(parse_face_material_name("face_20"), Some(20));
        assert_eq!(parse_face_material_name("bevel_metal"), None);

        let faces: [(&str, [[f32; 3]; 4]); 6] = [
            ("face_01", [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]]),
            ("face_02", [[-1.0, -1.0, -1.0], [-1.0, -1.0, 1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, -1.0]]),
            ("face_03", [[-1.0, 1.0, -1.0], [-1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, -1.0]]),
            ("face_04", [[-1.0, -1.0, -1.0], [1.0, -1.0, -1.0], [1.0, -1.0, 1.0], [-1.0, -1.0, 1.0]]),
            ("face_05", [[1.0, -1.0, 1.0], [1.0, -1.0, -1.0], [1.0, 1.0, -1.0], [1.0, 1.0, 1.0]]),
            ("face_06", [[1.0, -1.0, -1.0], [-1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [1.0, 1.0, -1.0]]),
        ];
        let mut bin = Vec::new();
        let mut views = Vec::new();
        let mut accessors = Vec::new();
        let mut materials = Vec::new();
        let mut prims = Vec::new();
        for (i, (name, verts)) in faces.iter().enumerate() {
            let off = bin.len();
            for v in verts {
                for c in v {
                    bin.extend_from_slice(&c.to_le_bytes());
                }
            }
            views.push(serde_json::json!({"buffer": 0, "byteOffset": off, "byteLength": 48}));
            let pos_acc = accessors.len();
            accessors.push(serde_json::json!({"bufferView": views.len() - 1, "componentType": 5126, "count": 4, "type": "VEC3"}));
            materials.push(serde_json::json!({"name": name, "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.4, 0.1, 1.0]}}));
            prims.push(serde_json::json!({"attributes": {"POSITION": pos_acc}, "material": i}));
        }
        for prim in &mut prims {
            let off = bin.len();
            for idx in [0u16, 1, 2, 0, 2, 3] {
                bin.extend_from_slice(&idx.to_le_bytes());
            }
            views.push(serde_json::json!({"buffer": 0, "byteOffset": off, "byteLength": 12}));
            let idx_acc = accessors.len();
            accessors.push(serde_json::json!({"bufferView": views.len() - 1, "componentType": 5123, "count": 6, "type": "SCALAR"}));
            prim["indices"] = serde_json::json!(idx_acc);
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let gltf = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0}],
            "meshes": [{"primitives": prims}],
            "materials": materials,
            "accessors": accessors,
            "bufferViews": views,
            "buffers": [{"byteLength": bin.len()}]
        });
        let mut json_bytes = serde_json::to_vec(&gltf).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total);
        glb.extend_from_slice(&GLB_MAGIC.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_JSON.to_le_bytes());
        glb.extend_from_slice(&json_bytes);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        glb.extend_from_slice(&bin);

        let mesh = parse_glb(&glb, 6).expect("valid cube GLB");
        assert!(mesh.valid(6, 4));
        assert_eq!(mesh.triangles.len(), 12);
        for value in 1..=6 {
            let t = orientation(&mesh, value, 1.0);
            let best = mesh
                .faces
                .iter()
                .max_by(|a, b| {
                    let za = t(a.center.unwrap().plus(a.normal.unwrap().times(0.1)))
                        .minus(t(a.center.unwrap()))
                        .2;
                    let zb = t(b.center.unwrap().plus(b.normal.unwrap().times(0.1)))
                        .minus(t(b.center.unwrap()))
                        .2;
                    za.total_cmp(&zb)
                })
                .unwrap();
            assert_eq!(best.value, value, "GLB D6 landing on face {value}");
        }
    }
}
