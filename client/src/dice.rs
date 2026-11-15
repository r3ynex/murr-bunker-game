use crate::appearance::{FontWeight, Theme};
use crate::motion::animated_button;
use eframe::egui::{self, Align2, Color32, Painter, Pos2, Shape, Stroke, Vec2};
use serde::Deserialize;
use std::f32::consts::{PI, TAU};
use std::io::Read;
use std::time::{Duration, Instant};

const MESHES: &str = include_str!("../../web/dice-models.json");

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

    pub fn show(&self, ctx: &egui::Context, theme: &Theme) {
        let Some((roll, start)) = &self.active else {
            return;
        };
        let now = Instant::now();
        if now < *start {
            ctx.request_repaint_after(*start - now);
            return;
        }
        let elapsed = now.duration_since(*start).as_secs_f32() * 1000.0;
        if elapsed >= roll.duration_ms as f32 {
            return;
        }
        let t = (elapsed / roll.settle_ms as f32).min(1.0);
        let fade = smooth((elapsed / 180.0).min(1.0))
            * smooth(((roll.duration_ms as f32 - elapsed) / 450.0).min(1.0));
        let result = smooth(((elapsed - roll.settle_ms as f32) / 180.0).clamp(0.0, 1.0));
        let scene = ctx.content_rect();
        let radius = (scene.height() * 0.23).min(scene.width() * 0.34).min(235.0);
        let stop = scene.center() - Vec2::new(0.0, 21.0);
        let flight = 1.0 - (1.0 - (t / 0.82).min(1.0)).powi(3);
        let center = Pos2::new(
            stop.x + (1.0 - flight) * radius * 0.52,
            egui::lerp((scene.bottom() + radius * 1.6)..=stop.y, flight)
                - (PI * flight).sin() * radius * 0.44,
        );
        let mesh = if roll.sides == 6 {
            &self.meshes.d6
        } else {
            &self.meshes.d20
        };
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dice-animation"));
        ctx.move_to_top(layer);
        let mut painter = ctx.layer_painter(layer);
        painter.set_clip_rect(scene);
        painter.rect_filled(scene, 0u8, Color32::from_black_alpha((216.0 * fade) as u8));
        paint_glow(&painter, stop, radius * 1.95, roll.sides, fade);
        paint_die(&painter, theme, mesh, roll, center, radius, t, fade);
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
    fn unit(self) -> Self {
        self.times(self.dot(self).sqrt().recip())
    }
    fn rotate(self, x: f32, y: f32, z: f32) -> Self {
        let (sx, cx) = x.sin_cos();
        let (sy, cy) = y.sin_cos();
        let (sz, cz) = z.sin_cos();
        let y1 = cx * self.1 - sx * self.2;
        let z1 = sx * self.1 + cx * self.2;
        let x2 = cy * self.0 + sy * z1;
        Self(cz * x2 - sz * y1, sz * x2 + cz * y1, cy * z1 - sy * self.0)
    }
}

#[derive(Deserialize)]
struct Meshes {
    d6: DieMesh,
    d20: DieMesh,
}
#[derive(Deserialize)]
struct DieMesh {
    vertices: Vec<V>,
    faces: Vec<Face>,
}
#[derive(Deserialize)]
struct Face {
    value: u32,
    indices: Vec<usize>,
}

impl Meshes {
    fn load() -> Self {
        const LIMIT: u64 = 65_536;
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
    fn valid(&self, sides: u32, corners: usize) -> bool {
        if self.faces.len() != sides as usize
            || self.vertices.len() > 80
            || self.vertices.iter().any(|&v| !(v.dot(v) <= 2.0))
        {
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
    let channel = |value| (f32::from(value) * light).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(color.r()),
        channel(color.g()),
        channel(color.b()),
        (opacity * 255.0) as u8,
    )
}

fn paint_glow(painter: &Painter, center: Pos2, radius: f32, sides: u32, opacity: f32) {
    let glow = if sides == 6 {
        Color32::from_rgb(38, 94, 112)
    } else {
        Color32::from_rgb(91, 57, 119)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, tint(glow, opacity * 0.63));
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
    let (_, normal, right, up) = face_basis(&mesh.vertices, face);
    let direction = if value % 2 == 0 { -1.0 } else { 1.0 };
    let spin = (1.0 - t).powf(1.75);
    move |v: V| {
        V(v.dot(right), v.dot(up), v.dot(normal)).rotate(
            0.18 + spin * PI * 7.2,
            -0.25 + direction * spin * PI * 8.4,
            0.09 + spin * PI * 3.6,
        )
    }
}

fn paint_die(
    painter: &Painter,
    theme: &Theme,
    mesh: &DieMesh,
    roll: &DiceRoll,
    center: Pos2,
    radius: f32,
    t: f32,
    opacity: f32,
) {
    let transform = orientation(mesh, roll.value, t);
    let project = |p: V| {
        let lens = radius * 3.8 / (3.8 - p.2);
        center + Vec2::new(p.0, -p.1) * lens
    };
    let vertices: Vec<_> = mesh.vertices.iter().copied().map(transform).collect();
    let mut visible: Vec<_> = mesh
        .faces
        .iter()
        .filter_map(|face| {
            let (mid, normal, right, up) = face_basis(&vertices, face);
            let towards_camera = V(0.0, 0.0, 3.8).minus(mid).dot(normal);
            (towards_camera > 0.0).then_some((face, mid, normal, right, up))
        })
        .collect();
    visible.sort_by(|a, b| a.1 .2.total_cmp(&b.1 .2));
    let light = V(-0.44, 0.62, 0.80).unit();
    for (face, mid, normal, right, up) in visible {
        let front = face.value == roll.value && t > 0.84;
        let strength = (0.32 + normal.dot(light).max(0.0) * 0.73).min(1.0);
        let base = if roll.sides == 6 {
            Color32::from_rgb(43, 120, 139)
        } else {
            Color32::from_rgb(102, 66, 142)
        };
        let highlight = if front {
            Color32::from_rgb(246, 198, 111)
        } else {
            Color32::from_rgb(173, 166, 183)
        };
        let outline = tint(highlight, opacity * if front { 0.96 } else { 0.57 });
        let corners = face.indices.iter().map(|&i| project(vertices[i])).collect();
        painter.add(Shape::convex_polygon(
            corners,
            shade(base, strength * 0.54, opacity),
            Stroke::new(1.2_f32, outline),
        ));
        let inside = face
            .indices
            .iter()
            .map(|&i| project(mid.plus(vertices[i].minus(mid).times(0.91))))
            .collect();
        painter.add(Shape::convex_polygon(
            inside,
            shade(base, strength, opacity),
            Stroke::NONE,
        ));
        let mark = tint(
            Color32::from_rgb(255, 235, 194),
            opacity * (0.57 + 0.43 * normal.2.clamp(0.0, 1.0)),
        );
        if roll.sides == 6 {
            paint_pips(painter, face.value, mid, right, up, project, mark);
        } else if normal.2 > 0.15 {
            let caption = if matches!(face.value, 6 | 9) {
                format!("{}.", face.value)
            } else {
                face.value.to_string()
            };
            painter.text(
                project(mid),
                Align2::CENTER_CENTER,
                caption,
                FontWeight::Bold.font(radius * 0.30 * (0.56 + normal.2 * 0.44)),
                mark,
            );
        }
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
    let mut pip = |x, y| {
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
