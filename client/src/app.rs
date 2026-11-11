use crate::model::{category_name, LocalConfig, Room, Session};
use crate::appearance::{portable_path, with_icon, Layout, Theme};
use crate::network;
use eframe::egui::{self, Color32, RichText, Stroke};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde_json::{json, Value};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    ui.separator();
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

pub struct BunkerApp {
    theme: Theme,
    layout: Layout,
    appearance_message: String,
    server_url: String,
    nickname: String,
    avatar: Option<u8>,
    avatar_images: Vec<Option<egui::TextureHandle>>,
    profile_ready: bool,
    join_code: String,
    status: String,
    session: Option<Session>,
    room: Option<Room>,
    pending: Option<Receiver<Result<Session, String>>>,
    sender: Option<WsSender>,
    receiver: Option<WsReceiver>,
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
            layout,
            appearance_message,
            server_url: config.server_url,
            nickname: config.nickname,
            avatar: if profile_ready { Some(config.avatar) } else { None },
            avatar_images,
            profile_ready, join_code: String::new(),
            status: "Готово к подключению".into(),
            session: None, room: None, pending: None, sender: None, receiver: None, online: false,
            create_on_connect: false,
            show_avatar_picker: false, show_join: false, show_create: false,
            show_settings: false, show_host_tools: false, show_survivability: false,
            editor: None, editor_value: String::new(), pending_section: None,
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
        self.theme = theme;
        self.layout = layout;
        self.appearance_message = appearance_warnings([theme_warning, layout_warning, font_warning]);
        if self.appearance_message.is_empty() {
            self.appearance_message = "Оформление обновлено".into();
        }
        ctx.request_repaint();
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
            Ok(_) => self.status = "Настройки сохранены".into(),
            Err(e) => self.status = format!("Не удалось сохранить: {e}"),
        }
    }
    fn complete_profile(&mut self) {
        if self.nickname.trim().is_empty() {
            self.nickname = format!("Выживший #{}", random_int(100, 9999));
        }
        if self.nickname.trim().chars().count() > 24 || self.nickname.trim().chars().count() < 2 {
            self.status = "Ник должен содержать от 2 до 24 символов".into();
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
        self.status = "Подключаемся к комнате…".into();
        self.pending = Some(network::request_room(
            self.server_url.clone(), self.nickname.trim().to_owned(),
            self.avatar.unwrap_or(1), join
        ));
    }
    fn connect_session(&mut self, session: Session) {
        match network::connect(&self.server_url, &session) {
            Ok((sender, receiver)) => {
                self.status = format!("Подключаемся к комнате {}…", session.code);
                self.session = Some(session);
                self.sender = Some(sender);
                self.receiver = Some(receiver);
                self.online = false;
            }
            Err(e) => self.status = e,
        }
    }
    fn send(&mut self, command: Value) {
        if !self.online {
            self.status = "Нет соединения — переподключись".into();
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
                self.status = "Передаём ZIP-пак комнате…".into();
            }
        } else {
            match serde_json::from_str::<Value>(&self.pack_json) {
                Ok(pack) => self.send(json!({"type":"set_pack", "pack":pack})),
                Err(e) => self.status = format!("Ошибка пака: {e}"),
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
                self.status = format!("Выбран пак: {}", path.display());
            }
            Err(e) => self.status = format!("Ошибка загрузки пака: {e}"),
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
                Ok(session) => self.connect_session(session),
                Err(e) => self.status = e,
            }
        }
        if let Some(reply) = self.upload_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.upload_pending = None;
            match reply {
                Ok(name) => self.status = format!("ZIP-пак «{name}» передан всем участникам"),
                Err(e) => self.status = format!("Ошибка ZIP-пака: {e}"),
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
                            self.status = format!("Фон пака не загрузился: {e}");
                        }
                    }
                }
                Err(e) => self.status = format!("Не удалось получить медиа пака: {e}"),
            }
        }
        let events: Vec<WsEvent> = self.receiver.as_ref()
            .map(|rx| std::iter::from_fn(|| rx.try_recv()).take(100).collect())
            .unwrap_or_default();
        for event in events {
            match event {
                WsEvent::Opened => {
                    self.online = true;
                    self.status = "Соединение установлено".into();
                    if self.create_on_connect {
                        self.create_on_connect = false;
                        self.apply_pack();
                        self.send(json!({"type":"set_event_chance","chance":self.event_chance}));
                    }
                }
                WsEvent::Closed => { self.online = false; self.status = "Соединение закрыто".into(); }
                WsEvent::Error(e) => { self.online = false; self.status = format!("Связь: {e}"); }
                WsEvent::Message(WsMessage::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        match message.get("type").and_then(Value::as_str) {
                            Some("state") => {
                                if let Some(data) = message.get("data") {
                                    match serde_json::from_value::<Room>(data.clone()) {
                                        Ok(room) => self.room = Some(room),
                                        Err(e) => self.status = format!("Данные комнаты: {e}"),
                                    }
                                }
                            }
                            Some("error") => self.status = message["message"]
                                .as_str().unwrap_or("Ошибка сервера").to_owned(),
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
    fn leave_room(&mut self) {
        if self.online { self.send(json!({"type":"leave"})); }
        self.sender = None;
        self.receiver = None;
        self.room = None;
        self.session = None;
        self.online = false;
        self.status = "Вы вышли в главное меню".into();
    }
    fn avatar_widget(
        ui: &mut egui::Ui, avatar: u8, selected: bool, size: f32,
        images: &[Option<egui::TextureHandle>],
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, avatar_color(avatar).gamma_multiply(0.55));
        if let Some(texture) = images.get(avatar.saturating_sub(1) as usize).and_then(Option::as_ref) {
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
            rect, 4.0,
            Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, if selected { self.theme.palette.accent.color() } else { self.theme.palette.button.color() }),
            egui::StrokeKind::Outside,
        );
        response
    }
    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height().max(300.0) * 0.16);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Добро пожаловать в Бункер!").size(self.theme.fonts.title).color(self.theme.palette.accent.color()).strong());
            ui.add_space(7.0);
            ui.label("Выбери ник и аватар — или оставь поля пустыми, чтобы получить случайные.");
            ui.add_space(35.0);
            ui.horizontal(|ui| {
                let row_width = 70.0 + self.layout.welcome_name_width
                    + self.theme.buttons.accent_size[0] + 2.0 * ui.spacing().item_spacing.x;
                ui.add_space((ui.available_width() - row_width).max(0.0) * 0.5);
                if Self::avatar_widget(ui, self.avatar.unwrap_or(1), self.avatar.is_some(), 70.0, &self.avatar_images).clicked() {
                    self.show_avatar_picker = true;
                }
                ui.add(egui::TextEdit::singleline(&mut self.nickname)
                    .hint_text("Выживший #8549").desired_width(self.layout.welcome_name_width.min((ui.available_width() - self.theme.buttons.accent_size[0] - 100.0).max(80.0))));
                if ui.add(self.theme.accent_button("Продолжить →")).clicked() {
                    self.complete_profile();
                }
            });
            ui.label(RichText::new("Нажми на аватар для выбора. Пропущенное заполнится автоматически.").color(self.theme.palette.muted.color()));
        });
    }
    fn ui_main_menu(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height().max(300.0) * self.layout.menu_top_fraction);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 260.0).max(0.0) * 0.5);
                Self::avatar_widget(ui, self.avatar.unwrap_or(1), true, 58.0, &self.avatar_images);
                ui.label(RichText::new(&self.nickname).size(self.theme.fonts.heading).strong());
            });
            ui.add_space(52.0);
            ui.horizontal(|ui| {
                let [width, height] = self.theme.buttons.menu_size;
                let gaps = 2.0 * ui.spacing().item_spacing.x;
                let width = width.min(((ui.available_width() - gaps) / 3.0).max(1.0));
                ui.add_space((ui.available_width() - 3.0 * width - gaps).max(0.0) * 0.5);
                if ui.add_sized([width, height], egui::Button::new(with_icon(&self.theme.icons.join, "Присоединиться к игре")).wrap()).clicked() {
                    self.show_join = true;
                }
                if ui.add_sized([width, height], self.theme.accent_button(with_icon(&self.theme.icons.create, "Создать игру")).min_size(egui::Vec2::ZERO).wrap()).clicked() {
                    self.show_create = true;
                }
                if ui.add_sized([width, height], egui::Button::new(with_icon(&self.theme.icons.settings, "Настройки")).wrap()).clicked() {
                    self.show_settings = true;
                }
            });
        });
    }
    fn ui_lobby(&mut self, ui: &mut egui::Ui, room: &Room) {
        let host = room.you == room.host_id;
        ui.add_space(38.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("Лобби: {}", room.code)).size(self.theme.fonts.title).strong());
            ui.label(RichText::new(format!("Игроки: {} из 15 · Пак: {}", room.players.len(), room.pack_name)).color(self.theme.palette.muted.color()));
        });
        ui.add_space(32.0);
        let count = room.players.len();
        let columns = self.layout.lobby_columns;
        ui.columns(columns, |cols| {
            for (i, player) in room.players.iter().enumerate() {
                cols[i % columns].horizontal(|ui| {
                    ui.label(RichText::new("▏").size(40.0).color(if player.ready || player.id == room.host_id { self.theme.palette.positive.color() } else { self.theme.palette.negative.color() }));
                    Self::avatar_widget(ui, player.avatar, false, self.layout.lobby_avatar_size, &self.avatar_images);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&player.name).size(self.theme.fonts.body).strong());
                        if player.id == room.host_id {
                            ui.label(RichText::new(with_icon(&self.theme.icons.host, "Ведущий")).color(self.theme.palette.accent.color()).size(self.theme.fonts.small));
                        } else {
                            ui.label(RichText::new(if player.ready { "Готов" } else { "Не готов" })
                                .color(if player.ready { self.theme.palette.positive.color() } else { self.theme.palette.negative.color() }).size(self.theme.fonts.small));
                        }
                    });
                });
                if i % columns == columns - 1 { cols[0].add_space(9.0); }
            }
        });
        if count == 0 { ui.label("Нет игроков"); }
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 520.0).max(0.0) * 0.5);
                if ui.button("Выйти из комнаты").clicked() { self.leave_room(); }
                if host {
                    let ready = room.players.len() >= 2 &&
                        room.players.iter().all(|p| p.id == room.host_id || p.ready);
                    let response = ui.add_enabled(ready, self.theme.accent_button("Начать игру"));
                    if response.clicked() { self.send(json!({"type":"start"})); }
                    if !ready {
                        response.on_hover_text("Все участники должны нажать «Готов». Минимум два игрока.");
                    }
                } else {
                    let ready = room.players.iter().find(|p| p.id == room.you).map(|p| p.ready).unwrap_or(false);
                    if ui.add(self.theme.accent_button(if ready { "Не готов" } else { "Готов" })).clicked() {
                        self.send(json!({"type":"set_ready","ready":!ready}));
                    }
                }
                if ui.button("Настройки").clicked() { self.show_host_tools = true; }
            });
            if host {
                ui.add_space(13.0);
                ui.label(RichText::new("Ведущий начинает игру, когда готовы все участники.").color(self.theme.palette.muted.color()));
            }
        });
    }
    fn at_section(&mut self, ui: &mut egui::Ui, index: usize) {
        if self.pending_section == Some(index) {
            ui.scroll_to_rect(ui.cursor(), Some(egui::Align::Min));
            self.pending_section = None;
        }
    }
    fn hero(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 0);
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, self.layout.hero_height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, self.theme.buttons.radius, self.theme.palette.hero.color());
        if let Some(texture) = &self.hero_texture {
            painter.image(texture.id(), rect, egui::Rect::from_min_max(
                egui::pos2(0.0,0.0),egui::pos2(1.0,1.0)), Color32::from_white_alpha(185));
        }
        painter.rect_filled(rect, self.theme.buttons.radius, Color32::from_black_alpha(125));
        painter.rect_stroke(rect, self.theme.buttons.radius, Stroke::new(1.0, self.theme.palette.outline.color()), egui::StrokeKind::Outside);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(self.layout.hero_inset.into())), |ui| {
            ui.add_space(self.layout.hero_top_gap);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Катаклизм").size(self.theme.fonts.title).strong().color(self.theme.palette.text.color()));
                ui.add_space(10.0);
                ui.add_sized([ui.available_width().min(self.layout.hero_text_size[0]), self.layout.hero_text_size[1]],
                    egui::Label::new(RichText::new(room.catastrophe.as_str()).size(self.theme.fonts.body).color(self.theme.palette.text.color())).wrap());
                ui.add_space(9.0);
                ui.label(RichText::new(format!("Раунд {} · Выживших: {}",
                    room.round, room.players.iter().filter(|p| p.active).count())).color(self.theme.palette.accent.color()));
            });
        });
        ui.add_space(10.0);
        let sections = ["Катаклизм", "Бункер", "События", "Досье", "Игроки", "Спецвозможности"];
        ui.horizontal_wrapped(|ui| {
            for (i, label) in sections.into_iter().enumerate() {
                let icon = &self.theme.icons.navigation[i];
                let text = if icon.is_empty() { label } else { icon.as_str() };
                let color = if i == 0 { self.theme.palette.accent.color() } else { self.theme.palette.text.color() };
                if ui.add(egui::Button::new(RichText::new(text).size(self.theme.fonts.icon).color(color))
                    .min_size(self.theme.buttons.navigation_size.into())).on_hover_text(label).clicked() {
                    self.pending_section = Some(i);
                }
            }
        });
    }
    fn bunker_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 1);
        heading(ui, &self.theme, "Информация о бункере");
        egui::Frame::new().fill(self.theme.palette.surface.color())
            .inner_margin(egui::Margin::same(self.layout.panel_padding)).show(ui, |ui| {
                ui.columns(2, |columns| {
                    columns[0].add(egui::Label::new(RichText::new(room.bunker.as_str()).size(self.theme.fonts.body)).wrap());
                    columns[0].add_space(15.0);
                    columns[0].label(RichText::new(format!("Количество мест: {}", room.seats)).size(self.theme.fonts.heading).strong().color(self.theme.palette.accent.color()));
                    columns[1].vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(RichText::new("◇").size(92.0).color(Color32::from_rgb(70,70,73)));
                        ui.label(RichText::new("Схема бункера — WIP").color(self.theme.palette.muted.color()).italics());
                    });
                });
            });
    }
    fn event_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 2);
        heading(ui, &self.theme, "События");
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("UNDER CONSTRUCTION").size(self.theme.fonts.heading).strong().color(self.theme.palette.negative.color()));
            ui.label(RichText::new(format!("Шанс события: {}% · Реальные события пока не запускаются",
                room.event_chance)).color(self.theme.palette.muted.color()));
        });
        ui.add_space(12.0);
    }
    fn dossier_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 3);
        let me = match room.players.iter().find(|p| p.id == room.you) {
            Some(p) => p, None => return
        };
        heading(ui, &self.theme, &format!("Досье игрока {}", me.name));
        let columns = self.layout.dossier_columns;
        ui.columns(columns, |cols| {
            for (i, card) in me.cards.iter().enumerate() {
                let col = &mut cols[i % columns];
                egui::Frame::new().fill(self.theme.palette.surface.color())
                    .inner_margin(egui::Margin::symmetric(self.layout.dossier_padding[0], self.layout.dossier_padding[1])).show(col, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(
                                room.category_labels.get(&card.kind).map(String::as_str)
                                    .unwrap_or_else(|| category_name(&card.kind))
                            ).color(self.theme.palette.muted.color()));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let icon = if card.revealed { &self.theme.icons.unlocked } else { &self.theme.icons.locked };
                                let marker = if icon.is_empty() {
                                    if card.revealed { "Открыто" } else { "Открыть" }
                                } else { icon.as_str() };
                                let color = if card.revealed { self.theme.palette.positive.color() } else { self.theme.palette.negative.color() };
                                if ui.add(egui::Button::new(RichText::new(marker).color(color)).frame(false))
                                    .on_hover_text(if card.revealed { "Уже открыто" } else { "Открыть карту всем игрокам" }).clicked()
                                    && !card.revealed && me.active {
                                    self.send(json!({"type":"reveal","kind":card.kind}));
                                }
                                ui.label(RichText::new(&card.value).color(self.theme.palette.accent.color()));
                            });
                        });
                    });
            }
        });
    }
    fn edit_card(&mut self, player_id: &str, kind: &str, value: &str) {
        self.editor = Some((player_id.to_owned(), kind.to_owned()));
        self.editor_value = value.to_owned();
    }
    fn overview_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 4);
        let count = room.players.iter().filter(|p| p.active).count();
        heading(ui, &self.theme, &format!("Желающие попасть в бункер: {}/{}", count, room.players.len()));
        let keys = ["gender","body","personality","profession","health","hobby","fear","equipment","backpack","fact"];
        let host = room.you == room.host_id;
        egui::ScrollArea::horizontal().id_salt("player-matrix").show(ui, |ui| {
            egui::Grid::new("players-matrix-grid").spacing(egui::vec2(8.0,9.0))
                .striped(true).show(ui, |ui| {
                    ui.add_sized([145.0,36.0], egui::Label::new("Игрок"));
                    for kind in keys {
                        ui.add_sized([self.layout.players_cell_size[0], 36.0], egui::Label::new(
                            RichText::new(room.category_labels.get(kind).map(String::as_str).unwrap_or(kind))
                                .size(self.theme.fonts.small).color(self.theme.palette.muted.color())
                        ).wrap());
                    }
                    ui.end_row();
                    for p in &room.players {
                        ui.horizontal(|ui| {
                            Self::avatar_widget(ui, p.avatar, false, 30.0, &self.avatar_images);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&p.name).strong());
                                ui.label(RichText::new(if p.active { "Участник" } else { "Выбыл" })
                                    .color(if p.active { self.theme.palette.muted.color() } else { self.theme.palette.negative.color() }).size(self.theme.fonts.small));
                            });
                        });
                        for kind in keys {
                            let cell = p.cards.iter().find(|c| c.kind == kind);
                            ui.horizontal(|ui| {
                                let label = cell.map(|c| card_text(&c.value)).unwrap_or_else(|| with_icon(&self.theme.icons.locked, "Закрыто"));
                                let response = ui.add_sized(self.layout.players_cell_size, egui::Label::new(label).wrap());
                                if let Some(card) = cell { response.on_hover_text(&card.value); }
                                if host {
                                    if ui.small_button(if self.theme.icons.edit.is_empty() { "Изменить" } else { self.theme.icons.edit.as_str() }).on_hover_text("Изменить карточку").clicked() {
                                        self.edit_card(&p.id, kind, cell.map(|c| c.value.as_str()).unwrap_or(""));
                                    }
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
        });
    }
    fn abilities_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.at_section(ui, 5);
        heading(ui, &self.theme, "Таблица специальных возможностей");
        let host = room.you == room.host_id;
        egui::ScrollArea::horizontal().id_salt("ability-matrix").show(ui, |ui| {
            egui::Grid::new("special-cards-grid").spacing(egui::vec2(18.0, 12.0))
                .striped(true).show(ui, |ui| {
                    ui.add_sized([175.0,35.0], egui::Label::new("Игрок"));
                    ui.add_sized([self.layout.abilities_cell_size[0], 35.0], egui::Label::new("Спец. возможность 1"));
                    ui.add_sized([self.layout.abilities_cell_size[0], 35.0], egui::Label::new("Спец. возможность 2"));
                    ui.end_row();
                    for p in &room.players {
                        ui.horizontal(|ui| { Self::avatar_widget(ui, p.avatar, false, 29.0, &self.avatar_images); ui.label(&p.name); });
                        for kind in ["ability", "ability2"] {
                            let card = p.cards.iter().find(|c| c.kind == kind);
                            ui.horizontal(|ui| {
                                let value = card.map(|c| card_text(&c.value)).unwrap_or_else(|| with_icon(&self.theme.icons.locked, "Закрыто"));
                                let response = ui.add_sized(self.layout.abilities_cell_size, egui::Label::new(value).wrap());
                                if let Some(card) = card { response.on_hover_text(&card.value); }
                                if host && ui.small_button(if self.theme.icons.edit.is_empty() { "Изменить" } else { self.theme.icons.edit.as_str() }).clicked() {
                                    self.edit_card(&p.id, kind, card.map(|c| c.value.as_str()).unwrap_or(""));
                                }
                            });
                        }
                        ui.end_row();
                    }
                });
        });
    }
    fn ui_game(&mut self, ui: &mut egui::Ui, room: &Room) {
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
                ui.horizontal(|ui| { Self::avatar_widget(ui, p.avatar, false, 29.0, &self.avatar_images); ui.label(&p.name); });
            }
            if ui.add(self.theme.accent_button("Оценка выживаемости · WIP")).clicked() {
                self.show_survivability = true;
            }
            if ui.button("В главное меню").clicked() { self.leave_room(); }
        });
    }
    fn ui_overlays(&mut self, ctx: &egui::Context) {
        if self.show_avatar_picker {
            let mut open = true;
            egui::Window::new("Выбор аватара").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label("Аватарки: папка avatars рядом с EXE, файлы 01.png–12.png. При отсутствии файла — цветной значок.");
                egui::Grid::new("avatar-selector-grid").num_columns(self.layout.avatar_picker_columns.into()).spacing(self.theme.spacing.items).show(ui, |ui| {
                    for id in 1..=12 {
                        if Self::avatar_widget(ui, id, self.avatar == Some(id), self.layout.avatar_picker_size, &self.avatar_images).clicked() {
                            self.avatar = Some(id);
                            self.show_avatar_picker = false;
                        }
                        if id % self.layout.avatar_picker_columns == 0 { ui.end_row(); }
                    }
                });
            });
            self.show_avatar_picker &= open;
        }
        if self.show_join {
            let mut open = true;
            egui::Window::new("Присоединиться к игре").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label("Введи код комнаты:");
                ui.add(egui::TextEdit::singleline(&mut self.join_code).desired_width(260.0));
                if ui.add(self.theme.accent_button("Присоединиться →")).clicked() {
                    let code = self.join_code.trim().to_uppercase();
                    if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
                        self.start_request(Some(code));
                        self.show_join = false;
                    } else { self.status = "Нужен восьмисимвольный код".into(); }
                }
            });
            self.show_join &= open;
        }
        if self.show_create {
            let mut open = true;
            egui::Window::new("Создать игру").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open)
                .min_width(self.layout.dialog_width).default_width(self.layout.dialog_width).show(ctx, |ui| {
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
            self.show_create &= open;
        }
        if self.show_settings {
            let mut open = true;
            egui::Window::new("Настройки").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).show(ctx, |ui| {
                ui.label("Адрес сервера:");
                ui.add(egui::TextEdit::singleline(&mut self.server_url).desired_width(430.0));

                if ui.button("Сохранить").clicked() { self.save_config(); }
                ui.separator();
                if ui.button("Перезагрузить оформление").on_hover_text("Ctrl+Shift+R").clicked() {
                    self.reload_appearance(ctx);
                }
                ui.label(portable_path("theme.json").parent().unwrap_or(Path::new(".")).display().to_string());
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
        ctx.request_repaint_after(Duration::from_millis(80));
        egui::TopBottomPanel::bottom("murr-status")
            .frame(egui::Frame::new().fill(self.theme.palette.status.color()).inner_margin(self.layout.status_margin))
            .show(ctx, |ui| {
                if !self.appearance_message.is_empty() {
                    ui.label(RichText::new(&self.appearance_message).color(self.theme.palette.accent.color()).size(self.theme.fonts.small));
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).color(self.theme.palette.muted.color()).size(self.theme.fonts.small));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(room) = self.room.as_ref().map(|r| (r.code.clone(), r.round)) {
                            if ui.small_button("В меню").clicked() { self.leave_room(); }
                            ui.label(RichText::new(format!("{} · Раунд {}", room.0, room.1)).color(self.theme.palette.muted.color()));
                        }
                    });
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(self.layout.content_margin))
            .show(ctx, |ui| {
                let room = self.room.clone();
                if let Some(room) = room {
                    match room.stage.as_str() {
                        "lobby" => self.ui_lobby(ui, &room),
                        "playing" => self.ui_game(ui, &room),
                        _ => self.ui_finished(ui, &room)
                    }
                } else if !self.profile_ready {
                    self.ui_welcome(ui);
                } else {
                    self.ui_main_menu(ui);
                }
            });
        self.ui_overlays(ctx);
    }
}
