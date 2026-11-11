use eframe::egui::{self, Color32};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::Value;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

const THEME: &str = include_str!("../theme.json");
const LAYOUT: &str = include_str!("../layout.json");

#[derive(Clone, Copy, Deserialize)]
pub struct Rgb([u8; 3]);

impl Rgb {
    pub fn color(self) -> Color32 {
        let [r, g, b] = self.0;
        Color32::from_rgb(r, g, b)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    Manrope,
    Builtin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub palette: Palette,
    pub fonts: Fonts,
    pub buttons: Buttons,
    pub spacing: Spacing,
    pub icons: Icons,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub background: Rgb,
    pub panel: Rgb,
    pub surface: Rgb,
    pub status: Rgb,
    pub button: Rgb,
    pub hover: Rgb,
    pub pressed: Rgb,
    pub selection: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub positive: Rgb,
    pub negative: Rgb,
    pub hero: Rgb,
    pub outline: Rgb,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fonts {
    pub family: FontFamily,
    pub file: Option<String>,
    pub body: f32,
    pub button: f32,
    pub small: f32,
    pub heading: f32,
    pub title: f32,
    pub icon: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Buttons {
    pub accent_size: [f32; 2],
    pub menu_size: [f32; 2],
    pub navigation_size: [f32; 2],
    pub padding: [f32; 2],
    pub radius: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spacing {
    pub items: [f32; 2],
    pub heading_gap: f32,
    pub section_gap: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Icons {
    pub join: String,
    pub create: String,
    pub settings: String,
    pub host: String,
    pub locked: String,
    pub unlocked: String,
    pub edit: String,
    pub navigation: [String; 6],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub window_size: [f32; 2],
    pub content_margin: i8,
    pub status_margin: i8,
    pub panel_padding: i8,
    pub menu_top_fraction: f32,
    pub welcome_name_width: f32,
    pub lobby_columns: usize,
    pub lobby_avatar_size: f32,
    pub avatar_picker_columns: u8,
    pub avatar_picker_size: f32,
    pub hero_height: f32,
    pub hero_inset: [f32; 2],
    pub hero_top_gap: f32,
    pub hero_text_size: [f32; 2],
    pub dossier_columns: usize,
    pub dossier_padding: [i8; 2],
    pub players_cell_size: [f32; 2],
    pub abilities_cell_size: [f32; 2],
    pub dialog_width: f32,
}

impl Layout {
    pub fn load() -> (Self, Option<String>) {
        load("layout.json", LAYOUT, Self::validate)
    }

    fn validate(&self) -> Result<(), String> {
        let dimensions = [
            (self.hero_inset, 0.0, 80.0),
            (self.hero_text_size, 25.0, 1600.0),
            (self.players_cell_size, 24.0, 500.0),
            (self.abilities_cell_size, 24.0, 700.0),
        ];
        for (size, min, max) in dimensions {
            for value in size {
                range(value, min, max)?;
            }
        }
        range(self.window_size[0], 820.0, 2560.0)?;
        range(self.window_size[1], 560.0, 2560.0)?;
        range(self.menu_top_fraction, 0.0, 0.5)?;
        range(self.hero_height, 240.0, 1000.0)?;
        range(self.hero_top_gap, 0.0, 120.0)?;
        range(self.welcome_name_width, 80.0, 480.0)?;
        range(self.dialog_width, 240.0, 700.0)?;
        range(self.lobby_avatar_size, 24.0, 88.0)?;
        range(self.avatar_picker_size, 32.0, 88.0)?;
        for margin in [self.content_margin, self.status_margin, self.panel_padding]
            .into_iter()
            .chain(self.dossier_padding)
        {
            range(margin.into(), 0.0, 40.0)?;
        }
        range(self.lobby_columns as f32, 1.0, 4.0)?;
        range(self.dossier_columns as f32, 1.0, 3.0)?;
        range(self.avatar_picker_columns.into(), 2.0, 6.0)?;
        Ok(())
    }
}

impl Theme {
    pub fn load() -> (Self, Option<String>) {
        load("theme.json", THEME, Self::validate)
    }

    fn validate(&self) -> Result<(), String> {
        let f = &self.fonts;
        for size in [f.body, f.button, f.small, f.heading, f.title, f.icon] {
            range(size, 8.0, 48.0)?;
        }
        for size in [
            self.buttons.accent_size,
            self.buttons.menu_size,
            self.buttons.navigation_size,
        ] {
            range(size[0], 20.0, 320.0)?;
            range(size[1], 20.0, 100.0)?;
        }
        for amount in self
            .buttons
            .padding
            .into_iter()
            .chain(self.spacing.items)
            .chain([self.spacing.heading_gap, self.spacing.section_gap])
        {
            range(amount, 0.0, 60.0)?;
        }
        range(self.buttons.radius.into(), 0.0, 32.0)?;
        let icons = &self.icons;
        for icon in [
            &icons.join,
            &icons.create,
            &icons.settings,
            &icons.host,
            &icons.locked,
            &icons.unlocked,
            &icons.edit,
        ]
        .into_iter()
        .chain(&icons.navigation)
        {
            if icon.chars().count() > 8 || icon.chars().any(char::is_control) {
                return Err("значок должен содержать до 8 символов без переносов".into());
            }
        }
        if let Some(file) = &f.file {
            if file.is_empty()
                || !file
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b" ._-".contains(&c))
                || !file.to_ascii_lowercase().ends_with(".ttf")
            {
                return Err("fonts.file должен содержать имя файла .ttf из папки fonts".into());
            }
        }
        Ok(())
    }

    pub fn apply(&self, ctx: &egui::Context) -> Option<String> {
        let (font, warning) = self.fonts.load_font();
        let mut fonts = egui::FontDefinitions::default();
        if let Some(data) = font {
            let name = "Murr custom font".to_owned();
            fonts
                .font_data
                .insert(name.clone(), egui::FontData::from_owned(data).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, name);
        }
        ctx.set_fonts(fonts);

        let mut style = (*ctx.style()).clone();
        let p = &self.palette;
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(p.text.color());
        style.visuals.panel_fill = p.background.color();
        style.visuals.window_fill = p.panel.color();
        let widgets = &mut style.visuals.widgets;
        widgets.noninteractive.bg_fill = p.panel.color();
        for (widget, color) in [
            (&mut widgets.inactive, p.button),
            (&mut widgets.hovered, p.hover),
            (&mut widgets.active, p.pressed),
            (&mut widgets.open, p.pressed),
        ] {
            widget.bg_fill = color.color();
            widget.weak_bg_fill = color.color();
            widget.corner_radius = self.buttons.radius.into();
        }
        style.visuals.selection.bg_fill = p.selection.color();
        style.spacing.item_spacing = self.spacing.items.into();
        style.spacing.button_padding = self.buttons.padding.into();
        for (kind, size) in [
            (egui::TextStyle::Body, self.fonts.body),
            (egui::TextStyle::Button, self.fonts.button),
            (egui::TextStyle::Small, self.fonts.small),
            (egui::TextStyle::Heading, self.fonts.heading),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        ctx.set_style(style);
        warning
    }

    pub fn accent_button(&self, label: impl Into<String>) -> egui::Button<'static> {
        egui::Button::new(
            egui::RichText::new(label)
                .strong()
                .color(self.palette.accent_text.color()),
        )
        .fill(self.palette.accent.color())
        .stroke(egui::Stroke::NONE)
        .corner_radius(self.buttons.radius)
        .min_size(self.buttons.accent_size.into())
    }
}

impl Fonts {
    fn load_font(&self) -> (Option<Vec<u8>>, Option<String>) {
        let warning = if let Some(filename) = &self.file {
            let data = read_bounded(&portable_path("fonts").join(filename), 8_000_000)
                .ok()
                .and_then(valid_font);
            if data.is_some() {
                return (data, None);
            }
            Some(format!("Шрифт {filename} недоступен или повреждён"))
        } else {
            None
        };

        if matches!(self.family, FontFamily::Builtin) {
            return (None, warning);
        }

        let from_zip = || {
            let data = read_bounded(&portable_path("Manrope.zip"), 16_000_000).ok()?;
            let mut zip = zip::ZipArchive::new(Cursor::new(data)).ok()?;
            let entry = zip.by_name("static/Manrope-Regular.ttf").ok()?;
            valid_font(read_limited(entry, 8_000_000).ok()?)
        };
        let data = from_zip().or_else(|| {
            let mut dirs = Vec::new();
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
            }
            if let Some(windows) = std::env::var_os("WINDIR") {
                dirs.push(PathBuf::from(windows).join("Fonts"));
            }
            dirs.into_iter().find_map(|dir| {
                ["Manrope-Regular.ttf", "Manrope-VariableFont_wght.ttf"]
                    .into_iter()
                    .find_map(|name| {
                        read_bounded(&dir.join(name), 8_000_000)
                            .ok()
                            .and_then(valid_font)
                    })
            })
        });
        (data, warning)
    }
}

fn valid_font(data: Vec<u8>) -> Option<Vec<u8>> {
    ab_glyph::FontRef::try_from_slice(&data).ok()?;
    Some(data)
}

pub fn with_icon(icon: &str, label: &str) -> String {
    if icon.is_empty() {
        label.to_owned()
    } else {
        format!("{icon} {label}")
    }
}

pub fn portable_path(filename: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|folder| folder.join(filename)))
        .unwrap_or_else(|| PathBuf::from(filename))
}

fn read_bounded(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    read_limited(File::open(path)?, max)
}

fn read_limited(reader: impl Read, max: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "файл слишком большой",
        ));
    }
    Ok(bytes)
}

fn range(value: f32, min: f32, max: f32) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("значение {value} вне диапазона {min}–{max}"))
    }
}

fn load<T: DeserializeOwned>(
    filename: &str,
    defaults: &str,
    validate: impl Fn(&T) -> Result<(), String>,
) -> (T, Option<String>) {
    let default = || serde_json::from_str(defaults).expect("valid embedded UI defaults");
    match read_bounded(&portable_path(filename), 65_536) {
        Ok(bytes) => match parse_override(defaults, &bytes, validate) {
            Ok(settings) => (settings, None),
            Err(error) => (
                default(),
                Some(format!(
                    "{filename}: {error}. Используются стандартные настройки."
                )),
            ),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => (default(), None),
        Err(error) => (
            default(),
            Some(format!(
                "{filename}: {error}. Используются стандартные настройки."
            )),
        ),
    }
}

fn parse_override<T: DeserializeOwned>(
    defaults: &str,
    bytes: &[u8],
    validate: impl FnOnce(&T) -> Result<(), String>,
) -> Result<T, String> {
    let mut merged: Value = serde_json::from_str(defaults).expect("valid embedded JSON");
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let patch = serde_json::from_slice(bytes).map_err(|err| err.to_string())?;
    merge(&mut merged, patch);
    let result = serde_json::from_value(merged).map_err(|err| err.to_string())?;
    validate(&result)?;
    Ok(result)
}

fn merge(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                merge(target.entry(key).or_insert(Value::Null), value);
            }
        }
        (target, patch) => *target = patch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_defaults_are_valid() {
        assert!(parse_override(THEME, b"{}", Theme::validate).is_ok());
        assert!(parse_override(LAYOUT, b"{}", Layout::validate).is_ok());
    }

    #[test]
    fn partial_overrides_keep_other_defaults() {
        let theme = parse_override(
            THEME,
            br#"{"palette":{"accent":[10,20,30]},"fonts":{"family":"builtin"}}"#,
            Theme::validate,
        )
        .unwrap();
        assert_eq!(theme.palette.accent.0, [10, 20, 30]);
        assert_eq!(theme.buttons.accent_size, [155.0, 38.0]);
    }

    #[test]
    fn rejects_invalid_values_and_unknown_keys() {
        assert!(parse_override(LAYOUT, br#"{"lobby_columns":0}"#, Layout::validate).is_err());
        assert!(parse_override(LAYOUT, br#"{"lobby_colums":2}"#, Layout::validate).is_err());
        assert!(parse_override(
            THEME,
            br#"{"fonts":{"file":"../bad.ttf"}}"#,
            Theme::validate
        )
        .is_err());
        assert!(parse_override(
            THEME,
            br#"{"palette":{"accent":[300,0,0]}}"#,
            Theme::validate
        )
        .is_err());
        assert!(valid_font(vec![0; 100]).is_none());
    }
}
