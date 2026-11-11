use crate::model::{category_name, Card, LocalConfig, Room, Session};
use crate::appearance::{portable_path, with_icon, Layout, Theme};
use crate::artwork::{centered_row, flat_button, input_row, paint_cover, paint_gradient, table_cell, Artwork, Icon};
use crate::network;
use eframe::egui::{self, Color32, RichText, Stroke};
use ewebsock::{WsEvent, WsMessage, WsSender};
use serde_json::{json, Value};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_PACK_BYTES: usize = 2_000_000;

fn random_int(min: u32, max: u32) -> u32 {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos()).unwrap_or(34567);
    min + ((nanos ^ (nanos >> 23)) as u32 % (max - min + 1))
}
fn avatar_color(id: u8) -> Color32 {
    let palette = [
        (147, 112, 255), (215, 99, 129), (82, 162, 183), (238, 161, 66),
        (114, 181, 116), (198, 137, 208), (198, 179, 87), (77, 147, 213),
        (174, 107, 77), (106, 184, 172), (224, 109, 99), (157, 164, 189)
    ];
    let (r,g,b) = palette[(id.saturating_sub(1) as usize) % palette.len()];
    Color32::from_rgb(r,g,b)
}
fn avatar_symbol(id: u8) -> &'static str {
    const MARKS: [&str; 12] = ["✦", "◆", "☼", "✿", "★", "♣", "◇", "◈", "✧", "●", "☻", "♟"];
    MARKS[(id.saturating_sub(1) as usize) % MARKS.len()]
}

fn load_avatar_images(ctx: &egui::Context) -> Vec<Option<egui::TextureHandle>> {
    (1u8..=12).map(|avatar| {
        ["png", "jpg", "jpeg", "webp"].iter().find_map(|extension| {
            let file = format!("{avatar:02}.{extension}");
            // Portable release: assets next to Murr-Bunker.exe.
            // Developer builds: sources in client/assets/avatars.
            let paths = [
                portable_path(&format!("avatars/{file}")),
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join("avatars").join(&file),
            ];
            for path in paths {
                let Ok(data) = fs::read(path) else { continue };
                if data.len() > 3_000_000 { continue; }
                // Reject overly large images before allocating the decoded texture.
                let dimensions = image::ImageReader::new(Cursor::new(&data))
                    .with_guessed_format().ok()
                    .and_then(|reader| reader.into_dimensions().ok());
                let Some((width, height)) = dimensions else { continue };
                if width == 0 || height == 0 || width > 1024 || height > 1024 {
                    continue;
                }
                let Ok(img) = image::load_from_memory(&data) else { continue };
                let img = img.to_rgba8();
                let texture = ctx.load_texture(
                    format!("avatar-{avatar:02}"),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [width as usize, height as usize], img.as_raw()
                    ),
                    egui::TextureOptions::LINEAR,
                );
                return Some(texture);
            }
            None
        })
    }).collect()
}

fn heading(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    ui.vertical_centered(|ui| {
        ui.add(egui::Label::new(RichText::new(name).size(theme.fonts.heading).strong()));
    });
    ui.add_space(theme.spacing.heading_gap);
}
fn section_rule(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing.section_gap);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(line, 0u8, theme.palette.outline.color());
    ui.add_space(theme.spacing.section_gap);
}
fn card_text(text: &str) -> String {
    let trimmed = text.lines().next().unwrap_or("");
    let short: String = trimmed.chars().take(38).collect();
    if trimmed.chars().count() > 38 { format!("{short}…") } else { short }
}
fn appearance_warnings(warnings: [Option<String>; 3]) -> String {
    warnings.into_iter().flatten().collect::<Vec<_>>().join("\n")
}

struct Toast {
    text: String,
    started: Instant,
}

pub struct BunkerApp {
    theme: Theme,
    artwork: Artwork,
    layout: Layout,
    toast: Option<Toast>,
    server_url: String,
    nickname: String,
    avatar: Option<u8>,
    avatar_images: Vec<Option<egui::TextureHandle>>,
    profile_ready: bool,
    join_code: String,
    session: Option<Session>,
    room: Option<Room>,
    pending: Option<Receiver<Result<Session, String>>>,
    sender: Option<WsSender>,
    receiver: Option<Receiver<WsEvent>>,
    online: bool,
    create_on_connect: bool,
    show_avatar_picker: bool,
    show_join: bool,
    show_create: bool,
    show_settings: bool,
    show_host_tools: bool,
    show_survivability: bool,
    editor: Option<(String, String)>,
    editor_value: String,
    pending_section: Option<usize>,
    active_section: usize,
    dice_sides: u32,
    event_chance: u32,
    pack_json: String,
    pack_file: String,
    pack_zip: Option<Vec<u8>>,
    upload_pending: Option<Receiver<Result<String, String>>>,
    download_pending: Option<Receiver<Result<Vec<u8>, String>>>,
    downloading_sha: Option<String>,
    loaded_sha: Option<String>,
    hero_texture: Option<egui::TextureHandle>,
}

impl BunkerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, layout: Layout, layout_warning: Option<String>) -> Self {
        let (theme, theme_warning) = Theme::load();
        let font_warning = theme.apply(&cc.egui_ctx);
        let appearance_message = appearance_warnings([theme_warning, layout_warning, font_warning]);
        let mut config = fs::read_to_string(portable_path("murr-config.json")).ok()
            .and_then(|s| serde_json::from_str::<LocalConfig>(&s).ok()).unwrap_or_default();
        // Upgrade the original local prototype address, which used to be saved
        // in murr-config.json. Preserve all custom server addresses.
        if matches!(config.server_url.trim().trim_end_matches('/'),
            "" | "http://127.0.0.1:8787" | "http://localhost:8787") {
            config.server_url = LocalConfig::default().server_url;
        }
        let avatar_images = load_avatar_images(&cc.egui_ctx);
        let profile_ready = !config.nickname.trim().is_empty();
        Self {
            theme,
            artwork: Artwork::load(&cc.egui_ctx),
            layout,
            toast: (!appearance_message.is_empty()).then(|| Toast {
                text: appearance_message,
                started: Instant::now(),
            }),
            server_url: config.server_url,
            nickname: config.nickname,
            avatar: if profile_ready { Some(config.avatar) } else { None },
            avatar_images,
            profile_ready, join_code: String::new(),
            session: None, room: None, pending: None, sender: None, receiver: None, online: false,
            create_on_connect: false,
            show_avatar_picker: false, show_join: false, show_create: false,
            show_settings: false, show_host_tools: false, show_survivability: false,
            editor: None, editor_value: String::new(), pending_section: None, active_section: 0,
            dice_sides: 6, event_chance: 0,
            pack_json: include_str!("../packs/demo.json").to_owned(),
            pack_file: String::new(), pack_zip: None,
            upload_pending: None, download_pending: None, downloading_sha: None,
            loaded_sha: None, hero_texture: None,
        }
    }
    fn reload_appearance(&mut self, ctx: &egui::Context) {
        let (theme, theme_warning) = Theme::load();
        let (layout, layout_warning) = Layout::load();
        let font_warning = theme.apply(ctx);
        if layout.window_size != self.layout.window_size {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(layout.window_size.into()));
        }
        self.artwork.reload_icons(ctx);
        self.avatar_images = load_avatar_images(ctx);
        self.theme = theme;
        self.layout = layout;
        let warnings = appearance_warnings([theme_warning, layout_warning, font_warning]);
        if warnings.is_empty() {
            self.notify("Оформление обновлено");
        } else {
            self.notify(warnings);
        }
        ctx.request_repaint();
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), started: Instant::now() });
    }

    fn show_toast(&mut self, ctx: &egui::Context) {
        const FADE_IN: f32 = 0.22;
        const HOLD_UNTIL: f32 = 3.5;
        const FADE_OUT: f32 = 0.40;

        let Some(toast) = &self.toast else { return; };
        let elapsed = toast.started.elapsed().as_secs_f32();
        let end = HOLD_UNTIL + FADE_OUT;
        if elapsed >= end {
            self.toast = None;
            return;
        }

        let progress = (elapsed / FADE_IN).min((end - elapsed) / FADE_OUT).clamp(0.0, 1.0);
        let opacity = progress * progress * (3.0 - 2.0 * progress);
        let bottom = f32::from(self.layout.status_margin) + 8.0;
        egui::Area::new(egui::Id::new("status-toast"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -bottom + 6.0 * (1.0 - opacity)))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                egui::Frame::new().fill(self.theme.palette.status.color())
                    .corner_radius(5u8).inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.add(egui::Label::new(RichText::new(&toast.text)
                            .color(self.theme.palette.text.color()).size(self.theme.fonts.small)).wrap());
                    });
            });
        if elapsed < FADE_IN || elapsed >= HOLD_UNTIL {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs_f32(HOLD_UNTIL - elapsed));
        }
    }

    fn save_config(&mut self) {
        let config = LocalConfig {
            server_url: self.server_url.trim().trim_end_matches('/').to_owned(),
            nickname: self.nickname.trim().to_owned(),
            avatar: self.avatar.unwrap_or(1),
        };
        match serde_json::to_string_pretty(&config)
            .map_err(|e| e.to_string())
            .and_then(|text| fs::write(portable_path("murr-config.json"), text).map_err(|e| e.to_string())) {
            Ok(_) => self.notify("Настройки сохранены"),
            Err(e) => self.notify(format!("Не удалось сохранить: {e}")),
        }
    }
    fn complete_profile(&mut self) {
        if self.nickname.trim().is_empty() {
            self.nickname = format!("Выживший #{}", random_int(100, 9999));
        }
        if self.nickname.trim().chars().count() > 24 || self.nickname.trim().chars().count() < 2 {
            self.notify("Ник должен содержать от 2 до 24 символов");
            return;
        }
        if self.avatar.is_none() { self.avatar = Some(random_int(1, 12) as u8); }
        self.profile_ready = true;
        self.save_config();
    }
    fn start_request(&mut self, join: Option<String>) {
        if self.pending.is_some() { return; }
        self.sender = None;
        self.receiver = None;
        self.online = false;
        self.session = None;
        self.room = None;
        self.loaded_sha = None;
        self.downloading_sha = None;
        self.hero_texture = None;
        self.create_on_connect = join.is_none();
        self.notify("Подключаемся к комнате…");
        self.pending = Some(network::request_room(
            self.server_url.clone(), self.nickname.trim().to_owned(),
            self.avatar.unwrap_or(1), join
        ));
    }
    fn connect_session(&mut self, ctx: &egui::Context, session: Session) {
        let ctx = ctx.clone();
        match network::connect(&self.server_url, &session, move || ctx.request_repaint()) {
            Ok((sender, receiver)) => {
                self.notify(format!("Подключаемся к комнате {}…", session.code));
                self.session = Some(session);
                self.sender = Some(sender);
                self.receiver = Some(receiver);
                self.online = false;
            }
            Err(e) => self.notify(e),
        }
    }
    fn send(&mut self, command: Value) {
        if !self.online {
            self.notify("Нет соединения — переподключись");
        } else if let Some(sender) = &mut self.sender {
            sender.send(WsMessage::Text(command.to_string().into()));
        }
    }
    fn apply_pack(&mut self) {
        if let Some(bytes) = &self.pack_zip {
            if let Some(session) = self.session.clone() {
                self.upload_pending = Some(network::upload_bundle(
                    self.server_url.clone(), session, bytes.clone()
                ));
                self.notify("Передаём ZIP-пак комнате…");
            }
        } else {
            match serde_json::from_str::<Value>(&self.pack_json) {
                Ok(pack) => self.send(json!({"type":"set_pack", "pack":pack})),
                Err(e) => self.notify(format!("Ошибка пака: {e}")),
            }
        }
    }
    fn load_pack_path(&mut self, path: &Path) {
        let result = (|| -> Result<(String, Option<Vec<u8>>), String> {
            let extension = path.extension().and_then(|x| x.to_str()).unwrap_or_default().to_lowercase();
            if extension == "zip" {
                let bytes = fs::read(path).map_err(|e| e.to_string())?;
                if bytes.len() > MAX_PACK_BYTES { return Err("ZIP-пак больше 2 МБ".into()); }
                let cursor = Cursor::new(bytes.as_slice());
                let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
                let mut manifest = String::new();
                archive.by_name("pack.json").map_err(|_| "Не найден pack.json в ZIP".to_owned())?
                    .take(500_001).read_to_string(&mut manifest).map_err(|e| e.to_string())?;
                serde_json::from_str::<Value>(&manifest).map_err(|e| e.to_string())?;
                Ok((manifest, Some(bytes)))
            } else if extension == "json" {
                let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
                serde_json::from_str::<Value>(&text).map_err(|e| e.to_string())?;
                Ok((text, None))
            } else {
                Err("Выбери файл .json или .zip".into())
            }
        })();
        match result {
            Ok((json, zip)) => {
                self.pack_json = json;
                self.pack_zip = zip;
                self.pack_file = path.display().to_string();
                self.notify(format!("Выбран пак: {}", path.display()));
            }
            Err(e) => self.notify(format!("Ошибка загрузки пака: {e}")),
        }
    }
    fn pick_pack(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Паки Murr", &["json", "zip"]).pick_file() {
            self.load_pack_path(&path);
        }
    }
    fn load_texture_from_zip(&mut self, ctx: &egui::Context, bytes: &[u8], name: &str) -> Result<(), String> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
        let image_file = archive.by_name(name).map_err(|e| e.to_string())?;
        if image_file.size() > 6_000_000 { return Err("Изображение слишком большое".into()); }
        let mut buffer = Vec::new();
        image_file.take(6_000_001).read_to_end(&mut buffer).map_err(|e| e.to_string())?;
        if buffer.len() > 6_000_000 { return Err("Изображение слишком большое".into()); }
        let rgba = image::load_from_memory(&buffer).map_err(|e| e.to_string())?.to_rgba8();
        let (w,h) = rgba.dimensions();
        if w > 4096 || h > 4096 { return Err("Фон больше 4096 × 4096".into()); }
        let colors = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize], rgba.as_raw()
        );
        self.hero_texture = Some(ctx.load_texture("murr-room-background", colors, egui::TextureOptions::LINEAR));
        Ok(())
    }
    fn poll_network(&mut self, ctx: &egui::Context) {
        if let Some(reply) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.pending = None;
            match reply {
                Ok(session) => self.connect_session(ctx, session),
                Err(e) => self.notify(e),
            }
        }
        if let Some(reply) = self.upload_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.upload_pending = None;
            match reply {
                Ok(name) => self.notify(format!("ZIP-пак «{name}» передан всем участникам")),
                Err(e) => self.notify(format!("Ошибка ZIP-пака: {e}")),
            }
        }
        if let Some(reply) = self.download_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.download_pending = None;
            let hash = self.downloading_sha.take().unwrap_or_default();
            self.loaded_sha = Some(hash);
            match reply {
                Ok(bytes) => {
                    let background = self.room.as_ref().and_then(|r| r.bundle.as_ref())
                        .and_then(|b| b.background.clone());
                    if let Some(name) = background {
                        if let Err(e) = self.load_texture_from_zip(ctx, &bytes, &name) {
                            self.notify(format!("Фон пака не загрузился: {e}"));
                        }
                    }
                }
                Err(e) => self.notify(format!("Не удалось получить медиа пака: {e}")),
            }
        }
        let events: Vec<WsEvent> = self.receiver.as_ref()
            .map(|rx| std::iter::from_fn(|| rx.try_recv().ok()).take(100).collect())
            .unwrap_or_default();
        if events.len() == 100 { ctx.request_repaint(); }
        for event in events {
            match event {
                WsEvent::Opened => {
                    self.online = true;
                    self.notify("Соединение установлено");
                    if self.create_on_connect {
                        self.create_on_connect = false;
                        self.apply_pack();
                        self.send(json!({"type":"set_event_chance","chance":self.event_chance}));
                    }
                }
                WsEvent::Closed => {
                    if self.online { self.notify("Соединение закрыто"); }
                    self.online = false;
                }
                WsEvent::Error(e) => { self.online = false; self.notify(format!("Связь: {e}")); }
                WsEvent::Message(WsMessage::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        match message.get("type").and_then(Value::as_str) {
                            Some("state") => {
                                if let Some(data) = message.get("data") {
                                    match serde_json::from_value::<Room>(data.clone()) {
                                        Ok(room) => self.update_room(room),
                                        Err(e) => self.notify(format!("Данные комнаты: {e}")),
                                    }
                                }
                            }
                            Some("error") => self.notify(message["message"]
                                .as_str().unwrap_or("Ошибка сервера")),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(room) = &self.room {
            if let (Some(bundle), Some(session)) = (&room.bundle, &self.session) {
                if self.loaded_sha.as_deref() != Some(&bundle.sha256)
                    && self.downloading_sha.as_deref() != Some(&bundle.sha256)
                    && self.download_pending.is_none() {
                    self.downloading_sha = Some(bundle.sha256.clone());
                    self.download_pending = Some(network::download_bundle(
                        self.server_url.clone(), session.clone()
                    ));
                }
            } else {
                self.loaded_sha = None;
                self.hero_texture = None;
            }
        }
    }
    fn update_room(&mut self, room: Room) {
        let joined = self.room.as_ref()
            .filter(|previous| previous.code == room.code && room.stage == "lobby")
            .and_then(|previous| room.players.iter().find(|player| {
                !previous.players.iter().any(|known| known.id == player.id)
            }))
            .map(|player| format!("Новый участник: {}", player.name));
        self.room = Some(room);
        if let Some(message) = joined {
            self.notify(message);
        }
    }

    fn leave_room(&mut self) {
        if self.online { self.send(json!({"type":"leave"})); }
        self.sender = None;
        self.receiver = None;
        self.room = None;
        self.session = None;
        self.online = false;
        self.notify("Вы вышли в главное меню");
    }
    fn avatar_widget(&self, ui: &mut egui::Ui, avatar: u8, selected: bool, size: f32) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, avatar_color(avatar).gamma_multiply(0.55));
        if let Some(texture) = self.avatar_images.get(avatar.saturating_sub(1) as usize).and_then(Option::as_ref) {
            painter.image(
                texture.id(), rect.shrink(2.0),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(), egui::Align2::CENTER_CENTER, avatar_symbol(avatar),
                egui::FontId::proportional(size * 0.45), Color32::WHITE,
            );
        }
        painter.rect_stroke(
            rect, 0u8,
            Stroke::new(2.0_f32, if selected { self.theme.palette.accent.color() } else { self.theme.palette.text.color() }),
            egui::StrokeKind::Outside,
        );
        response
    }
    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height() * self.layout.welcome_top_fraction);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Добро пожаловать в Бункер!")
                .size(self.theme.fonts.title).color(self.theme.palette.accent_title.color()).strong());
            ui.label("Для старта выберите никнейм и аватар (только при первом запуске)");
            ui.add_space(24.0);

            let side = self.layout.entry_button_size;
            let gaps = 2.0 * ui.spacing().item_spacing.x;
            let field = self.layout.welcome_name_width.min((ui.available_width() - 2.0 * side - gaps).max(60.0));
            centered_row(ui, 2.0 * side + field + gaps, side, |ui| {
                if self.avatar_widget(ui, self.avatar.unwrap_or(1), self.avatar.is_some(), side)
                    .on_hover_text("Выбрать аватар").clicked()
                {
                    self.show_avatar_picker = true;
                }
                let nickname = input_row(ui, &mut self.nickname, "Выживший #8549", field, side, self.theme.palette.input.color());
                let button = self.artwork.button(Icon::Login, "→", self.theme.fonts.icon);
                let submit = flat_button(ui, [side; 2], button, self.theme.accent_gradient(), self.theme.buttons.radius)
                    .on_hover_text("Продолжить");
                if submit.clicked() || (nickname.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    self.show_avatar_picker = false;
                    self.complete_profile();
                }
            });
        });
    }

    fn ui_main_menu(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height() * self.layout.menu_top_fraction);
        let font = egui::FontId::proportional(self.theme.fonts.heading);
        let name_width = ui.painter().layout_no_wrap(self.nickname.clone(), font, Color32::WHITE).size().x;
        let side = self.layout.profile_avatar_size;
        let gap = ui.spacing().item_spacing.x;
        let row_width = (side + gap + name_width + 4.0).min(ui.available_width());
        centered_row(ui, row_width, side, |ui| {
            if self.avatar_widget(ui, self.avatar.unwrap_or(1), false, side)
                .on_hover_text("Сменить аватар").clicked()
            {
                self.show_avatar_picker = true;
            }
            ui.add(egui::Label::new(RichText::new(&self.nickname).size(self.theme.fonts.heading).strong()).truncate());
        });
        ui.add_space(self.layout.menu_profile_gap);

        let [width, height] = self.theme.buttons.menu_size;
        let width = width.min(((ui.available_width() - 2.0 * gap) / 3.0).max(20.0));
        centered_row(ui, 3.0 * width + 2.0 * gap, height, |ui| {
            let neutral = [self.theme.palette.button.color(); 2];
            let radius = self.theme.buttons.radius;
            let size = [width, height];
            let join = with_icon(&self.theme.icons.join, "Присоединиться к игре");
            let create = with_icon(&self.theme.icons.create, "Создать игру");
            let settings = with_icon(&self.theme.icons.settings, "Настройки");
            if flat_button(ui, size, egui::Button::new(join).wrap(), neutral, radius).clicked() {
                self.show_join = true;
            }
            if flat_button(ui, size, egui::Button::new(RichText::new(create).color(self.theme.palette.accent_text.color())).wrap(),
                self.theme.accent_gradient(), radius).clicked()
            {
                self.show_create = true;
            }
            if flat_button(ui, size, egui::Button::new(settings).wrap(), neutral, radius).clicked() {
                self.show_settings = true;
            }
        });
    }

    fn ui_lobby(&mut self, ui: &mut egui::Ui, room: &Room) {
        let host = room.you == room.host_id;
        let height = ui.available_height();
        ui.add_space(0.10 * height);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("Лобби: {}", room.code)).size(self.theme.fonts.heading).strong());
            ui.label(format!("Игроки: {} из 15", room.players.len())).on_hover_text(&room.pack_name);
        });
        ui.add_space(14.0);

        let [action_width, action_height] = self.theme.buttons.menu_size;
        let roster_height = (ui.available_height() - action_height - 38.0).max(75.0);
        let columns = self.layout.lobby_columns;
        let side = self.layout.lobby_avatar_size;
        let roster_width = ui.available_width() * 0.82;
        centered_row(ui, roster_width, roster_height, |ui| {
            ui.vertical(|ui| {
                ui.set_width(roster_width);
                egui::ScrollArea::vertical().id_salt("lobby-roster").max_height(roster_height).show(ui, |ui| {
                    ui.columns(columns, |cols| {
                        for (index, player) in room.players.iter().enumerate() {
                            let column = &mut cols[index % columns];
                            let player_is_host = player.id == room.host_id;
                            let ready = player.ready || player_is_host;
                            column.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_height(28.0);
                                    if player_is_host {
                                        ui.horizontal(|ui| {
                                            ui.add_space(side * 0.30);
                                            if let Some(icon) = self.artwork.icon(Icon::Host, 26.0) {
                                                ui.add(icon).on_hover_text("Ведущий");
                                            } else {
                                                ui.label(RichText::new(&self.theme.icons.host).color(self.theme.palette.accent.color())).on_hover_text("Ведущий");
                                            }
                                        });
                                    }
                                });
                            });
                            column.horizontal(|ui| {
                                let color = if ready { self.theme.palette.positive.color() } else { self.theme.palette.negative.color() };
                                let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, side), egui::Sense::hover());
                                ui.painter().rect_filled(bar, 0u8, color);
                                self.avatar_widget(ui, player.avatar, false, side);
                                ui.add(egui::Label::new(RichText::new(&player.name).size(self.theme.fonts.body).strong()).truncate());
                            });
                        }
                    });
                });
            });
        });

        let gap = ui.spacing().item_spacing.x;
        let action_width = action_width.min(((ui.available_width() - 2.0 * gap) / 2.6).max(20.0));
        let side_width = action_width * 0.8;
        centered_row(ui, action_width + 2.0 * side_width + 2.0 * gap, action_height, |ui| {
            let neutral = [self.theme.palette.button.color(); 2];
            let radius = self.theme.buttons.radius;
            let side_size = [side_width, action_height * 0.8];
            if flat_button(ui, side_size, egui::Button::new("Покинуть игру"), neutral, radius).clicked() {
                self.leave_room();
                return;
            }
            let ready = room.players.iter().any(|p| p.id == room.you && p.ready);
            let can_start = room.players.len() >= 2 && room.players.iter().all(|p| p.id == room.host_id || p.ready);
            let label = if host { "Начать игру" } else if ready { "Не готов" } else { "Готов" };
            let palette = &self.theme.palette;
            let fill = if !host && !ready { [palette.ready.color(), palette.ready_end.color()] } else { self.theme.accent_gradient() };
            let button = ui.add_enabled_ui(!host || can_start, |ui| {
                flat_button(ui, [action_width, action_height], egui::Button::new(label), fill, radius)
            }).inner;
            if host && !can_start {
                button.on_disabled_hover_text("Нужны минимум два игрока. Все гости должны нажать «Готов».");
            } else if button.clicked() {
                if host { self.send(json!({"type":"start"})); }
                else { self.send(json!({"type":"set_ready","ready":!ready})); }
            }
            if flat_button(ui, side_size, egui::Button::new("Настройки"), neutral, radius).clicked() {
                self.show_host_tools = true;
            }
        });
    }

    fn at_section(&mut self, ui: &mut egui::Ui, index: usize) {
        let viewport = ui.clip_rect();
        if ui.cursor().top() <= viewport.top() + 0.25 * viewport.height() {
            self.active_section = index;
        }
        if self.pending_section == Some(index) {
            ui.scroll_to_rect(ui.cursor(), Some(egui::Align::Min));
            self.active_section = index;
            self.pending_section = None;
        }
    }

    fn hero(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 0);
        let width = ui.available_width();
        let height = self.layout.hero_height;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, self.theme.palette.hero.color());
        if let Some(texture) = &self.hero_texture {
            paint_cover(&painter, texture, rect);
            painter.add(paint_gradient(rect, Color32::from_black_alpha(100), self.theme.palette.background.color(), true));
        } else if let Some(texture) = &self.artwork.catastrophe {
            paint_cover(&painter, texture, rect);
        }
        let inset = self.layout.hero_inset;
        let text_width = self.layout.hero_text_size[0].min((width - 2.0 * inset[0]).max(80.0));
        let text_rect = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - 0.5 * text_width, rect.top() + self.layout.hero_top_gap + inset[1]),
            egui::vec2(text_width, (height - self.layout.hero_top_gap - inset[1]).max(80.0)),
        );
        let mut text = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
        text.set_clip_rect(rect.intersect(ui.clip_rect()));
        text.vertical_centered(|ui| {
            ui.label(RichText::new("Катаклизм").size(self.theme.fonts.heading).strong());
            ui.add_space(30.0);
            let description = egui::Label::new(&room.catastrophe).wrap();
            egui::ScrollArea::vertical().id_salt("catastrophe-description").max_height(self.layout.hero_text_size[1])
                .show(ui, |ui| { ui.add(description); });
            ui.add_space(30.0);
            ui.label(format!("Раунд {} · Выживших: {}", room.round, room.players.iter().filter(|p| p.active).count()));
        });
    }

    fn game_navigation(&mut self, ctx: &egui::Context) {
        let sections = ["Катаклизм", "Бункер", "События", "Досье", "Игроки", "Спецвозможности"];
        egui::Area::new(egui::Id::new("game-section-nav"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -54.0))
            .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(8).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.horizontal(|ui| {
                        for (index, (label, icon)) in sections.into_iter().zip(Icon::NAVIGATION).enumerate() {
                            let fallback = &self.theme.icons.navigation[index];
                            let button = self.artwork.button(icon, if fallback.is_empty() { label } else { fallback }, self.theme.fonts.icon);
                            let colors = if index == self.active_section {
                                self.theme.accent_gradient()
                            } else {
                                [self.theme.palette.button.color(); 2]
                            };
                            if flat_button(ui, self.theme.buttons.navigation_size, button, colors, self.theme.buttons.radius)
                                .on_hover_text(label).clicked()
                            {
                                self.pending_section = Some(index);
                                ctx.request_repaint();
                            }
                        }
                    });
                });
            });
    }

    fn bunker_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 1);
        let width = ui.available_width();
        let height = self.layout.section_min_height;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        if let Some(texture) = &self.artwork.bunker {
            let side = (0.36 * width).min(height);
            let drawing = egui::Rect::from_min_size(egui::pos2(rect.right() - side, rect.center().y - side / 2.0), egui::vec2(side, side));
            paint_cover(&ui.painter_at(rect), texture, drawing);
        }
        let margin = (0.083 * width).max(f32::from(self.layout.panel_padding));
        let content = egui::Rect::from_min_size(
            egui::pos2(rect.left() + margin, rect.top() + 0.21 * height),
            egui::vec2((width * 0.64 - margin).max(100.0), height * 0.72),
        );
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(content));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        body.label(RichText::new("Информация о бункере").size(self.theme.fonts.heading).strong());
        body.add_space(self.theme.spacing.heading_gap);
        egui::ScrollArea::vertical().id_salt("bunker-description").max_height(height * 0.45).show(&mut body, |ui| {
            ui.add(egui::Label::new(&room.bunker).wrap());
        });
        body.add_space(16.0);
        body.label(RichText::new(format!("Количество мест: {}", room.seats)).size(self.theme.fonts.heading).strong());
    }

    fn event_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 2);
        let top = ui.cursor().top();
        ui.add_space(36.0);
        heading(ui, &self.theme, "События");
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("UNDER CONSTRUCTION").size(self.theme.fonts.heading).strong().color(self.theme.palette.negative.color()));
            ui.label(RichText::new(format!("Шанс события: {}% · Реальные события пока не запускаются",
                room.event_chance)).color(self.theme.palette.muted.color()));
        });
        ui.add_space((self.layout.section_min_height - (ui.cursor().top() - top)).max(0.0));
    }
    fn dossier_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 3);
        let Some(me) = room.players.iter().find(|p| p.id == room.you) else { return; };
        let top = ui.cursor().top();
        ui.add_space(44.0);
        heading(ui, &self.theme, &format!("Досье игрока {}", me.name));
        let (abilities, cards): (Vec<_>, Vec<_>) = me.cards.iter().partition(|card| matches!(card.kind.as_str(), "ability" | "ability2"));
        let columns = self.layout.dossier_columns;
        let per_column = cards.len().div_ceil(columns);
        let inset = ui.available_width() * 0.08;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(inset.min(127.0) as i8, 0)).show(ui, |ui| {
            ui.columns(columns, |cols| {
                for (index, card) in cards.into_iter().enumerate() {
                    self.dossier_card(&mut cols[index / per_column], room, card, me.active);
                }
            });
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                let width = (ui.available_width() * 0.64).max(320.0);
                ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(width);
                    for card in abilities {
                        self.dossier_card(ui, room, card, me.active);
                    }
                });
            });
        });
        ui.add_space((self.layout.section_min_height - (ui.cursor().top() - top)).max(0.0));
    }

    fn dossier_card(&mut self, ui: &mut egui::Ui, room: &Room, card: &Card, active: bool) {
        let [x, y] = self.layout.dossier_padding;
        let palette = &self.theme.palette;
        let text = palette.text.color();
        let accent = palette.accent_title.color();
        let line = palette.surface.color();
        let name = room.category_labels.get(&card.kind).map(String::as_str).unwrap_or_else(|| category_name(&card.kind));
        egui::Frame::new().inner_margin(egui::Margin::symmetric(x, y)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).color(text).size(self.theme.fonts.small));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (icon, marker, tooltip) = if card.revealed {
                        (Icon::Unlocked, &self.theme.icons.unlocked, "Уже открыто")
                    } else {
                        (Icon::Locked, &self.theme.icons.locked, "Открыть карту всем игрокам")
                    };
                    let marker = if marker.is_empty() { "Открыть" } else { marker.as_str() };
                    let button = self.artwork.button(icon, marker, 17.0).frame(false);
                    if ui.add_enabled(active && !card.revealed, button).on_hover_text(tooltip).clicked() {
                        self.send(json!({"type":"reveal","kind":card.kind}));
                    }
                    ui.add(egui::Label::new(RichText::new(&card.value).color(accent).size(self.theme.fonts.small)).wrap());
                });
            });
        });
        let (rule, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
        ui.painter().rect_filled(rule, 0u8, line);
    }

    fn edit_card(&mut self, player_id: &str, kind: &str, value: &str) {
        self.editor = Some((player_id.to_owned(), kind.to_owned()));
        self.editor_value = value.to_owned();
    }

    fn overview_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 4);
        ui.add_space(30.0);
        let count = room.players.iter().filter(|p| p.active).count();
        heading(ui, &self.theme, &format!("Желающие попасть в бункер: {}/{}", count, room.players.len()));
        let keys = ["gender", "body", "personality", "profession", "health", "hobby", "fear", "equipment", "backpack", "fact"];
        self.player_table(ui, room, "player-matrix", &keys, self.layout.players_cell_size);
    }

    fn abilities_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 5);
        ui.add_space(30.0);
        heading(ui, &self.theme, "Таблица спец. возможностей");
        self.player_table(ui, room, "ability-matrix", &["ability", "ability2"], self.layout.abilities_cell_size);
    }

    fn player_table(&mut self, ui: &mut egui::Ui, room: &Room, id: &str, keys: &[&str], cell: [f32; 2]) {
        let host = room.you == room.host_id;
        let player_width = 130.0;
        let width = player_width + keys.len() as f32 * cell[0];
        let offset = ((ui.available_width() - width) * 0.5).max(0.0);
        let line = self.theme.palette.button.color();
        egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(offset);
                egui::Grid::new(id).spacing([0.0, 0.0]).show(ui, |ui| {
                    ui.add_sized([player_width, 36.0], egui::Label::new("Игрок"));
                    for &kind in keys {
                        let name = room.category_labels.get(kind).map(String::as_str).unwrap_or_else(|| category_name(kind));
                        ui.add_sized([cell[0], 36.0], egui::Label::new(RichText::new(name).size(self.theme.fonts.small)).wrap());
                    }
                    ui.end_row();
                    for player in &room.players {
                        table_cell(ui, [player_width, cell[1]], line, |ui| {
                            self.avatar_widget(ui, player.avatar, false, (cell[1] - 8.0).min(35.0));
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(RichText::new(&player.name).size(self.theme.fonts.small).strong()).truncate());
                                let color = if player.active { self.theme.palette.muted.color() } else { self.theme.palette.negative.color() };
                                ui.label(RichText::new(if player.active { "Участник" } else { "Выбыл" }).color(color).size(self.theme.fonts.small));
                            });
                        });
                        for &kind in keys {
                            let card = player.cards.iter().find(|card| card.kind == kind);
                            table_cell(ui, cell, line, |ui| {
                                if host {
                                    let icon = self.artwork.button(Icon::Edit, &self.theme.icons.edit, 13.0).frame(false);
                                    if ui.add(icon).on_hover_text("Изменить карточку").clicked() {
                                        self.edit_card(&player.id, kind, card.map(|card| card.value.as_str()).unwrap_or(""));
                                    }
                                }
                                if let Some(card) = card {
                                    ui.add(egui::Label::new(RichText::new(card_text(&card.value)).size(self.theme.fonts.small)).wrap()).on_hover_text(&card.value);
                                } else if let Some(icon) = self.artwork.icon(Icon::Locked, 12.0) {
                                    ui.add(icon).on_hover_text("Закрыто");
                                } else {
                                    ui.label(RichText::new("Закрыто").size(self.theme.fonts.small));
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
            });
        });
    }

    fn ui_game(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.active_section = 0;
        egui::ScrollArea::vertical().id_salt("entire-game").auto_shrink([false,false]).show(ui, |ui| {
            self.hero(ui, room);
            section_rule(ui, &self.theme);
            self.bunker_section(ui, room);
            section_rule(ui, &self.theme);
            self.event_section(ui, room);
            section_rule(ui, &self.theme);
            self.dossier_section(ui, room);
            section_rule(ui, &self.theme);
            self.overview_section(ui, room);
            section_rule(ui, &self.theme);
            self.abilities_section(ui, room);
            section_rule(ui, &self.theme);
            ui.horizontal_wrapped(|ui| {
                if room.you == room.host_id {
                    if ui.add(self.theme.accent_button("Панель ведущего")).clicked() { self.show_host_tools = true; }
                    if ui.button("Следующий раунд").clicked() { self.send(json!({"type":"next_round"})); }
                    if ui.button(if room.vote_open {"Закрыть голосование"} else {"Начать голосование"}).clicked() {
                        self.send(json!({"type":if room.vote_open {"finish_vote"} else {"start_vote"}}));
                    }
                    if ui.button("Завершить").clicked() { self.send(json!({"type":"finish"})); }
                }
                ui.add(egui::DragValue::new(&mut self.dice_sides).range(2..=100).prefix("d"));
                if ui.button("Бросить кубик").clicked() {
                    self.send(json!({"type":"roll","sides":self.dice_sides}));
                }
            });
            if room.vote_open {
                ui.label(format!("Голосование открыто · получено голосов: {}", room.ballots_count));
                for p in room.players.iter().filter(|p| p.active && p.id != room.you) {
                    if ui.button(format!("Голосовать за {}", p.name)).clicked() {
                        self.send(json!({"type":"vote","target":p.id}));
                    }
                }
            }
            for result in &room.vote_results {
                ui.label(format!("{}: {} голосов", result.name, result.votes));
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Журнал партии").size(18.0).strong());
            for event in room.log.iter().rev().take(8) {
                ui.label(RichText::new(event).color(self.theme.palette.muted.color()));
            }
        });
    }
    fn ui_finished(&mut self, ui: &mut egui::Ui, room: &Room) {
        ui.add_space(70.0);
        heading(ui, &self.theme, "Партия завершена");
        ui.vertical_centered(|ui| {
            ui.label(format!("Осталось участников: {}", room.players.iter().filter(|p| p.active).count()));
            for p in room.players.iter().filter(|p| p.active) {
                ui.horizontal(|ui| { self.avatar_widget(ui, p.avatar, false, 29.0); ui.label(&p.name); });
            }
            if ui.add(self.theme.accent_button("Оценка выживаемости · WIP")).clicked() {
                self.show_survivability = true;
            }
            if ui.button("В главное меню").clicked() { self.leave_room(); }
        });
    }
    fn ui_overlays(&mut self, ctx: &egui::Context) {
        if self.show_avatar_picker {
            let modal = egui::Modal::new(egui::Id::new("choose-avatar"))
                .frame(egui::Frame::new().fill(self.theme.palette.panel.color()).corner_radius(5u8)
                    .stroke(Stroke::new(1.0_f32, self.theme.palette.accent.color())).inner_margin(15))
                .show(ctx, |ui| {
                    let columns = self.layout.avatar_picker_columns;
                    let gap = ui.spacing().item_spacing.x;
                    let side = self.layout.avatar_picker_size.min(
                        (ctx.content_rect().width() - gap * f32::from(columns - 1) - 60.0) / f32::from(columns)
                    );
                    let grid_width = side * f32::from(columns) + gap * f32::from(columns - 1);
                    ui.set_width(grid_width);
                    ui.vertical_centered(|ui| { ui.label("Выберите аватар:"); });
                    ui.add_space(8.0);
                    for row in (1u8..=12).collect::<Vec<_>>().chunks(usize::from(columns)) {
                        let width = side * row.len() as f32 + gap * (row.len() - 1) as f32;
                        centered_row(ui, width, side, |ui| {
                            for &id in row {
                                if self.avatar_widget(ui, id, self.avatar == Some(id), side).clicked() {
                                    self.avatar = Some(id);
                                    self.show_avatar_picker = false;
                                    if self.profile_ready { self.save_config(); }
                                }
                            }
                        });
                    }
                });
            if modal.should_close() { self.show_avatar_picker = false; }
        }
        if self.show_join {
            let modal = egui::Modal::new(egui::Id::new("join-room"))
                .frame(egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(12))
                .show(ctx, |ui| {
                    let side = self.layout.entry_button_size;
                    let field = self.layout.welcome_name_width;
                    let row_width = side + field + ui.spacing().item_spacing.x;
                    ui.set_width(row_width);
                    centered_row(ui, row_width, side, |ui| {
                        let field = input_row(ui, &mut self.join_code, "Введите код игры", field, side, self.theme.palette.input.color());
                        let button = self.artwork.button(Icon::Join, "→", self.theme.fonts.icon);
                        let submit = flat_button(ui, [side; 2], button, self.theme.accent_gradient(), self.theme.buttons.radius)
                            .on_hover_text("Присоединиться");
                        if submit.clicked() || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            let code = self.join_code.trim().to_uppercase();
                            if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
                                self.start_request(Some(code));
                                self.show_join = false;
                            } else {
                                self.notify("Нужен восьмисимвольный код");
                            }
                        }
                    });
                });
            if modal.should_close() { self.show_join = false; }
        }
        if self.show_create {
            let mut open = true;
            egui::Window::new("Создать игру").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open)
                .min_width(self.layout.dialog_width).default_width(self.layout.dialog_width).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Набор карт").strong().size(self.theme.fonts.body));
                    ui.label(if self.pack_file.is_empty() { "Murr: стартовый набор" } else { &self.pack_file });
                    if ui.button("Выбрать JSON / ZIP-пак…").clicked() { self.pick_pack(); }
                    ui.label(RichText::new("ZIP: pack.json и изображения в assets/. Раздаётся остальным автоматически.").color(self.theme.palette.muted.color()).size(self.theme.fonts.small));
                    ui.separator();
                    ui.label(RichText::new("События").strong().size(self.theme.fonts.body));
                    ui.add(egui::Slider::new(&mut self.event_chance, 0..=100).text("Шанс появления, %"));
                    ui.label(RichText::new("WIP: параметр сохраняется в комнате, сами события пока не запускаются.").color(self.theme.palette.muted.color()));
                    ui.add_space(12.0);
                    if ui.add(self.theme.accent_button("Создать комнату")).clicked() {
                        self.show_create = false;
                        self.start_request(None);
                    }
                });
            });
            self.show_create &= open;
        }
        if self.show_settings {
            let mut open = true;
            egui::Window::new("Настройки").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label("Адрес сервера:");
                    ui.add(egui::TextEdit::singleline(&mut self.server_url).desired_width(430.0));

                    if ui.button("Сохранить").clicked() { self.save_config(); }
                    ui.separator();
                    if ui.button("Перезагрузить оформление").on_hover_text("Ctrl+Shift+R").clicked() {
                        self.reload_appearance(ctx);
                    }
                    ui.label(portable_path("theme.json").parent().unwrap_or(Path::new(".")).display().to_string());
                });
            });
            self.show_settings = open;
        }
        if self.show_host_tools {
            let mut open = true;
            let snapshot = self.room.clone();
            egui::Window::new("Параметры комнаты / ведущий").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open)
                .default_width(430.0).show(ctx, |ui| {
                if let Some(room) = snapshot {
                    let host = room.you == room.host_id;
                    ui.label(format!("Пак: {}", room.pack_name));
                    ui.label("Управление игроками и характеристиками: в таблицах нажимай ⋯ у ячейки.");
                    if host {
                        let mut seats = room.seats as i32;
                        ui.horizontal(|ui| {
                            ui.label("Мест в бункере");
                            if ui.add(egui::DragValue::new(&mut seats).range(1..=15)).changed() {
                                self.send(json!({"type":"set_seats","seats":seats}));
                            }
                        });
                        if room.stage == "lobby" {
                            let mut chance = room.event_chance;
                            if ui.add(egui::Slider::new(&mut chance, 0..=100).text("Шанс событий (WIP)")).changed() {
                                self.send(json!({"type":"set_event_chance","chance":chance}));
                            }
                            if ui.button("Выбрать другой пак…").clicked() { self.pick_pack(); }
                            if ui.button("Применить выбранный пак").clicked() { self.apply_pack(); }
                        }
                        if room.stage == "playing" {
                            ui.horizontal(|ui| {
                                if ui.button("Следующий раунд").clicked() { self.send(json!({"type":"next_round"})); }
                                if ui.button("Завершить партию").clicked() { self.send(json!({"type":"finish"})); }
                            });
                            ui.separator();
                            for p in &room.players {
                                ui.horizontal(|ui| {
                                    ui.label(&p.name);
                                    if ui.button(if p.active {"Исключить"} else {"Вернуть"}).clicked() {
                                        self.send(json!({"type":"set_active","playerId":p.id,"active":!p.active}));
                                    }
                                });
                            }
                        }
                    }
                    ui.separator();
                    ui.label(RichText::new("Смена катаклизма, бункера и управление сложными событиями — WIP").color(self.theme.palette.muted.color()));
                }
            });
            self.show_host_tools = open;
        }
        if let Some((player, kind)) = self.editor.clone() {
            let mut open = true;
            egui::Window::new("Редактор характеристики ведущего").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open)
                .default_width(470.0).show(ctx, |ui| {
                ui.label(format!("Категория: {} · Редактирование видно всем после раскрытия", kind));
                ui.add(egui::TextEdit::multiline(&mut self.editor_value)
                    .desired_rows(3).desired_width(450.0));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Применить текст").clicked() {
                        self.send(json!({"type":"host_edit_card","playerId":player,"kind":kind,"value":self.editor_value}));
                        self.editor = None;
                    }
                    if ui.button("Открыть всем").clicked() {
                        self.send(json!({"type":"host_reveal_card","playerId":player,"kind":kind}));
                        self.editor = None;
                    }
                    if ui.button("Перегенерировать").clicked() {
                        self.send(json!({"type":"host_redraw_card","playerId":player,"kind":kind}));
                        self.editor = None;
                    }
                });
            });
            if !open { self.editor = None; }
        }
        if self.show_survivability {
            let mut open = true;
            egui::Window::new("Оценка выживаемости — WIP").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label("Сюжетные финалы и расчёт ресурсов будут добавлены после завершения основных правил.");
                ui.label("Пока здесь намеренно нет выдуманной оценки.");
            });
            self.show_survivability = open;
        }
    }
}

impl eframe::App for BunkerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::R)) {
            self.reload_appearance(ctx);
        }
        self.poll_network(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(self.layout.content_margin))
            .show(ctx, |ui| {
                let room = self.room.clone();
                if let Some(room) = room {
                    match room.stage.as_str() {
                        "lobby" => self.ui_lobby(ui, &room),
                        "playing" => {
                            self.ui_game(ui, &room);
                            self.game_navigation(ctx);
                            egui::Area::new(egui::Id::new("leave-game"))
                                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
                                .movable(false).show(ctx, |ui| {
                                    if ui.button("В меню").clicked() { self.leave_room(); }
                                });
                        },
                        _ => self.ui_finished(ui, &room)
                    }
                } else if !self.profile_ready {
                    self.ui_welcome(ui);
                } else {
                    self.ui_main_menu(ui);
                }
            });
        self.ui_overlays(ctx);
        self.show_toast(ctx);
        if self.pending.is_some() || self.upload_pending.is_some() || self.download_pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}
