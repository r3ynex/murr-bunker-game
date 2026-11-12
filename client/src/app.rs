use crate::model::{category_name, Card, LocalConfig, Room, Session};
use crate::appearance::{portable_path, with_icon, FontWeight, Layout, Theme};
use crate::artwork::{centered_row, flat_button, input_row, paint_cover, paint_gradient, table_cell, Artwork, Icon};
use crate::network;
use crate::motion::{animated_button, button_highlight, modal_opacity, screen_opacity};
use eframe::egui::{self, Color32, RichText, Stroke};
use crate::network::{WsEvent, WsSender};
use serde_json::{json, Value};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};


const DOSSIER_LOCK_SIZE: f32 = 24.0;
const DOSSIER_INFO_SIZE: f32 = 16.0;
const DOSSIER_GAP: f32 = 8.0;
const TABLE_BOTTOM_GAP: f32 = 56.0;

type DossierLabels = (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>);

fn game_body_width(available: f32) -> f32 {
    available * 0.84
}

fn dossier_height((label, value): &DossierLabels) -> f32 {
    label.size().y.max(value.size().y).max(DOSSIER_LOCK_SIZE)
}

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
                    egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)),
                );
                return Some(texture);
            }
            None
        })
    }).collect()
}

fn heading(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    ui.vertical_centered(|ui| {
        ui.add(egui::Label::new(FontWeight::Bold.text(theme.fonts.heading, name)));
    });
    ui.add_space(theme.spacing.heading_gap);
}
fn section_rule(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing.section_gap);
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(line, 0u8, theme.palette.outline.color());
    ui.add_space(theme.spacing.section_gap);
}
fn scroll_to_section(ui: &egui::Ui, top: f32) {
    // Use a finite marker at the top of the section instead of the remaining cursor rectangle.
    let marker = egui::Rect::from_min_size(egui::pos2(ui.min_rect().left(), top), egui::vec2(1.0, 1.0));
    ui.scroll_to_rect_animation(
        marker,
        Some(egui::Align::Min),
        egui::style::ScrollAnimation::new(2800.0, egui::Rangef::new(0.25, 0.65)),
    );
}

fn bunker_description(text: &str, theme: &Theme, width: f32) -> Vec<(egui::text::LayoutJob, f32)> {
    let regular = egui::TextFormat::simple(FontWeight::Regular.font(theme.fonts.bunker_body), theme.palette.text.color());
    let accent = egui::TextFormat::simple(FontWeight::SemiBold.font(theme.fonts.bunker_body), theme.palette.accent_title.color());
    let mut stats_started = false;
    text.lines().map(|line| {
        let mut gap = 0.0;
        let mut job = egui::text::LayoutJob::default();
        let (label, value) = line.split_once(':').unwrap_or(("", line));
        let display = match label {
            "Внутри" | "В бункере присутствует" => Some(("В бункере присутствует", "\n")),
            "Размер" | "Размер бункера" => Some(("Размер бункера", " ")),
            "Время проживания" | "Время нахождения" => Some(("Время нахождения", " ")),
            "Еда и вода" | "Количество еды и воды" => Some(("Количество еды и воды", " ")),
            _ => None,
        };
        if let Some((title, separator)) = display {
            if !stats_started {
                gap = theme.spacing.heading_gap;
                stats_started = true;
            }
            job.append(title, 0.0, accent.clone());
            job.append(":", 0.0, accent.clone());
            job.append(separator, 0.0, regular.clone());
            job.append(value.trim_start(), 0.0, regular.clone());
        } else {
            job.append(if label == "Описание" { value.trim_start() } else { line }, 0.0, regular.clone());
        }
        job.wrap.max_width = width;
        (job, gap)
    }).collect()
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen { Welcome, Menu, Lobby, Game, Finished }

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
    screen_transition: Option<(Screen, f64)>,
    join_code: String,
    session: Option<Session>,
    room: Option<Room>,
    pending: Option<Receiver<Result<Session, String>>>,
    sender: Option<WsSender>,
    receiver: Option<Receiver<WsEvent>>,
    room_deadline: Option<Instant>,
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
    room_packs: Vec<crate::packs::Entry>,
    pack_zip: Option<Vec<u8>>,
    upload_pending: Option<Receiver<Result<String, String>>>,
    pack_upload_failed: bool,
    download_pending: Option<Receiver<Result<Vec<u8>, String>>>,
    background_key: Option<String>,
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
            profile_ready, screen_transition: None, join_code: String::new(),
            session: None, room: None, pending: None, sender: None, receiver: None, room_deadline: None, online: false,
            create_on_connect: false,
            show_avatar_picker: false, show_join: false, show_create: false,
            show_settings: false, show_host_tools: false, show_survivability: false,
            editor: None, editor_value: String::new(), pending_section: None, active_section: 0,
            dice_sides: 6, event_chance: 0,
            pack_json: crate::packs::BASE.to_owned(),
            room_packs: crate::packs::library(), pack_zip: None,
            upload_pending: None, pack_upload_failed: false, download_pending: None, background_key: None,
            hero_texture: None,
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
        if self.pending.is_some() || self.room_deadline.is_some() { return; }
        self.sender = None;
        self.receiver = None;
        self.online = false;
        self.session = None;
        self.room = None;
        self.clear_background();
        self.upload_pending = None;
        self.pack_upload_failed = false;
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
                self.room_deadline = Some(Instant::now() + Duration::from_secs(18));
                self.online = false;
            }
            Err(e) => self.notify(e),
        }
    }
    fn send(&mut self, command: Value) {
        if !self.online {
            self.notify("Нет соединения — переподключись");
        } else if let Some(sender) = &mut self.sender {
            let _ = sender.send(command.to_string());
        }
    }
    fn apply_pack(&mut self) {
        if self.upload_pending.is_some() { return; }
        self.pack_upload_failed = false;
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
    fn pick_pack(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Паки Murr", &["json", "zip"]).pick_file() {
            match crate::packs::Pack::load(&path).and_then(|pack| pack.payload()) {
                Ok((json, zip)) => {
                    self.pack_json = json;
                    self.pack_zip = zip;
                    self.notify(format!("Выбран пак: {}", path.display()));
                }
                Err(error) => self.notify(format!("Ошибка загрузки пака: {error}")),
            }
        }
    }

    fn clear_background(&mut self) {
        self.background_key = None;
        self.download_pending = None;
        self.hero_texture = None;
    }

    fn sync_background(&mut self, ctx: &egui::Context) {
        let desired = self.room.as_ref()
            .and_then(|room| room.bundle.as_ref())
            .filter(|bundle| bundle.background.is_some())
            .zip(self.session.as_ref());

        let Some((bundle, session)) = desired else {
            self.clear_background();
            return;
        };
        let key = format!("{}:{}", bundle.sha256, bundle.background.as_deref().unwrap_or_default());
        if self.background_key.as_deref() != Some(&key) {
            self.background_key = Some(key);
            self.hero_texture = None;
            self.download_pending = Some(network::download_background(
                self.server_url.clone(), session.clone(),
            ));
        }

        if let Some(reply) = self.download_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.download_pending = None;
            match reply {
                Ok(bytes) => {
                    self.hero_texture = crate::artwork::texture(ctx, "murr-room-background", &bytes, 4096);
                    if self.hero_texture.is_none() {
                        self.notify("Фон пака повреждён или больше 4096 × 4096");
                    }
                }
                Err(e) => self.notify(format!("Фон пака не загрузился: {e}")),
            }
        }
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
                Err(e) => {
                    self.pack_upload_failed = true;
                    self.notify(format!("Ошибка ZIP-пака: {e}"));
                }
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

                    if self.create_on_connect {
                        self.create_on_connect = false;
                        self.apply_pack();
                        self.send(json!({"type":"set_event_chance","chance":self.event_chance}));
                    }
                }
                WsEvent::Closed => {
                    self.stop_websocket();
                    self.notify("Соединение закрыто");
                }
                WsEvent::Error(e) => {
                    self.stop_websocket();
                    self.notify(format!("Связь: {e}"));
                }
                WsEvent::Message(text) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        match message.get("type").and_then(Value::as_str) {
                            Some("state") => {
                                if let Some(data) = message.get("data") {
                                    match serde_json::from_value::<Room>(data.clone()) {
                                        Ok(room) => {
                                            if self.room_deadline.take().is_some() {
                                                self.notify("Соединение установлено");
                                            }
                                            self.update_room(room);
                                        },
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
            }
        }
        if self.room_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            self.stop_websocket();
            self.notify("Сервер не прислал состояние комнаты. Попробуй подключиться снова.");
        }
        self.sync_background(ctx);
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

    fn stop_websocket(&mut self) {
        self.online = false;
        self.room_deadline = None;
        self.sender = None;
        self.receiver = None;
    }

    fn leave_room(&mut self) {
        if self.online { self.send(json!({"type":"leave"})); }
        self.stop_websocket();
        self.room = None;
        self.clear_background();
        self.session = None;
        self.online = false;
        self.notify("Вы вышли в главное меню");
    }
    fn avatar_widget(&self, ui: &mut egui::Ui, avatar: u8, selected: bool, size: f32) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, avatar_color(avatar).gamma_multiply(0.55));
        if let Some(texture) = self.avatar_images.get(avatar.saturating_sub(1) as usize).and_then(Option::as_ref) {
            paint_cover(&painter, texture, rect.shrink(2.0));
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
        button_highlight(ui, &response);
        response
    }
    fn ui_welcome(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height() * self.layout.welcome_top_fraction);
        ui.vertical_centered(|ui| {
            ui.label(FontWeight::Bold.text(self.theme.fonts.title, "Добро пожаловать в Бункер!")
                .color(self.theme.palette.accent_title.color()));
            ui.label(FontWeight::Regular.text(self.theme.fonts.welcome_caption, "Для старта выберите никнейм и аватар (только при первом запуске)"));
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
                let nickname = input_row(ui, &mut self.nickname, "Выживший #8549", field, side, self.theme.palette.input.color(), FontWeight::SemiBold.font(self.theme.fonts.input));
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
        let font = FontWeight::Bold.font(self.theme.fonts.profile_name);
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
            ui.add(egui::Label::new(FontWeight::Bold.text(self.theme.fonts.profile_name, &self.nickname)).truncate());
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
            ui.label(FontWeight::Bold.text(self.theme.fonts.lobby_heading, format!("Лобби: {}", room.code)));
            ui.label(FontWeight::Medium.text(self.theme.fonts.lobby_count, format!("Игроки: {} из 15", room.players.len()))).on_hover_text(&room.pack_name);
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
                                ui.add(egui::Label::new(FontWeight::Bold.text(self.theme.fonts.lobby_name, &player.name)).truncate());
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
            if flat_button(ui, side_size, egui::Button::new(FontWeight::SemiBold.text(self.theme.fonts.lobby_side_button, "Покинуть игру")), neutral, radius).clicked() {
                self.leave_room();
                return;
            }
            let ready = room.players.iter().any(|p| p.id == room.you && p.ready);
            let can_start = self.upload_pending.is_none() && !self.pack_upload_failed && room.players.len() >= 2
                && room.players.iter().all(|p| p.id == room.host_id || p.ready);
            let label = if host { "Начать игру" } else if ready { "Не готов" } else { "Готов" };
            let palette = &self.theme.palette;
            let fill = if !host && !ready { [palette.ready.color(), palette.ready_end.color()] } else { self.theme.accent_gradient() };
            let button = ui.add_enabled_ui(!host || can_start, |ui| {
                flat_button(ui, [action_width, action_height], egui::Button::new(label), fill, radius)
            }).inner;
            if host && !can_start {
                button.on_disabled_hover_text(if self.upload_pending.is_some() { "Пак ещё загружается." }
                    else if self.pack_upload_failed { "Примени пак повторно в настройках комнаты." }
                    else { "Нужны минимум два игрока. Все гости должны нажать «Готов»." });
            } else if button.clicked() {
                if host { self.send(json!({"type":"start"})); }
                else { self.send(json!({"type":"set_ready","ready":!ready})); }
            }
            if flat_button(ui, side_size, egui::Button::new(FontWeight::SemiBold.text(self.theme.fonts.lobby_side_button, "Настройки")), neutral, radius).clicked() {
                self.show_host_tools = true;
            }
        });
    }

    fn hero(&mut self, ui: &mut egui::Ui, room: &Room) {
        let width = ui.available_width();
        let [horizontal_inset, vertical_inset] = self.layout.hero_inset;
        let text_width = self.layout.hero_text_size[0].min((width - 2.0 * horizontal_inset).max(1.0));
        let color = self.theme.palette.text.color();
        let survivors = room.players.iter().filter(|player| player.active).count();
        let text = [
            ("Катаклизм".to_owned(), FontWeight::Bold.font(self.theme.fonts.heading)),
            (room.catastrophe.clone(), FontWeight::Regular.font(self.theme.fonts.hero_body)),
            (format!("Раунд {} · Выживших: {survivors}", room.round), FontWeight::Regular.font(self.theme.fonts.hero_body)),
        ].map(|(text, font)| {
            let mut job = egui::text::LayoutJob::simple(text, font, color, text_width);
            job.halign = egui::Align::Center;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        });
        let gap = 30.0;
        let group_height = text.iter().map(|galley| galley.size().y).sum::<f32>() + 2.0 * gap;
        let height = self.layout.hero_height.max(group_height + 2.0 * vertical_inset);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, self.theme.palette.hero.color());
        if let Some(texture) = &self.hero_texture {
            paint_cover(&painter, texture, rect);
            painter.add(paint_gradient(rect, Color32::from_black_alpha(100), self.theme.palette.background.color(), true));
        } else if let Some(texture) = &self.artwork.catastrophe {
            paint_cover(&painter, texture, rect);
        }

        let center = rect.center();
        let mut y = center.y - group_height / 2.0;
        for galley in text {
            let size = galley.size();
            let text_rect = egui::Rect::from_min_size(egui::pos2(center.x - size.x / 2.0, y), size);
            ui.place(text_rect, egui::Label::new(galley));
            y += size.y + gap;
        }
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
        let width = ui.available_width();
        let height = self.layout.section_min_height;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        if let Some(texture) = &self.artwork.bunker {
            let side = (0.36 * width).min(height);
            let drawing = egui::Rect::from_min_size(egui::pos2(rect.right() - side, rect.center().y - side / 2.0), egui::vec2(side, side));
            paint_cover(&ui.painter_at(rect), texture, drawing);
        }
        let margin = (0.083 * width).max(f32::from(self.layout.panel_padding));
        let content_width = (width * 0.68 - margin).max(100.0);
        let color = self.theme.palette.text.color();
        let heading = ui.painter().layout(
            "Информация о бункере".into(), FontWeight::Bold.font(self.theme.fonts.heading), color, content_width,
        );
        let seats = ui.painter().layout(
            format!("Количество мест: {}", room.seats), FontWeight::Bold.font(self.theme.fonts.bunker_seats),
            color, content_width,
        );
        let spacing = ui.spacing().item_spacing.y;
        let mut paragraphs = bunker_description(&room.bunker, &self.theme, content_width);
        let paragraphs = ui.fonts_mut(|fonts| {
            paragraphs.drain(..).map(|(job, gap)| (fonts.layout_job(job), gap)).collect::<Vec<_>>()
        });
        let natural_height = paragraphs.iter().map(|(text, gap)| text.size().y + gap + spacing).sum::<f32>();
        let natural_height = (natural_height - spacing).max(0.0);
        let fixed = heading.size().y + self.theme.spacing.heading_gap + 16.0 + seats.size().y;
        let max_description = (height - fixed - 2.0 * f32::from(self.layout.panel_padding)).max(1.0);
        let description_height = natural_height.min(max_description);
        let content_height = fixed + description_height;
        let content = egui::Rect::from_min_size(
            egui::pos2(rect.left() + margin, rect.center().y - content_height / 2.0),
            egui::vec2(content_width, content_height),
        );
        let mut body = ui.new_child(egui::UiBuilder::new().max_rect(content)
            .layout(egui::Layout::top_down(egui::Align::Min)));
        body.set_clip_rect(rect.intersect(ui.clip_rect()));
        body.spacing_mut().item_spacing.y = 0.0;
        body.add(egui::Label::new(heading));
        body.add_space(self.theme.spacing.heading_gap);
        if !paragraphs.is_empty() {
            egui::ScrollArea::vertical().id_salt("bunker-description").max_height(description_height)
                .show(&mut body, |ui| {
                    let mut first = true;
                    for (text, gap) in paragraphs {
                        if !first { ui.add_space(spacing); }
                        first = false;
                        ui.add_space(gap);
                        ui.add(egui::Label::new(text));
                    }
                });
        }
        body.add_space(16.0);
        body.add(egui::Label::new(seats));
    }

    fn event_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        let top = ui.cursor().top();
        ui.add_space(36.0);
        heading(ui, &self.theme, "События");
        ui.vertical_centered(|ui| {
            ui.label(FontWeight::Bold.text(self.theme.fonts.heading, "UNDER CONSTRUCTION").color(self.theme.palette.negative.color()));
            ui.label(RichText::new(format!("Шанс события: {}% · Реальные события пока не запускаются",
                room.event_chance)).color(self.theme.palette.muted.color()));
        });
        ui.add_space((self.layout.section_min_height - (ui.cursor().top() - top)).max(0.0));
    }
    fn dossier_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        let Some(me) = room.players.iter().find(|p| p.id == room.you) else { return; };
        let top = ui.next_widget_position().y;
        ui.add_space(44.0);
        heading(ui, &self.theme, &format!("Досье игрока {}", me.name));

        const KINDS: [&str; 10] = [
            "gender", "body", "personality", "profession", "health",
            "hobby", "fear", "equipment", "backpack", "fact",
        ];
        const ABILITIES: [&str; 2] = ["ability", "ability2"];
        let columns = self.layout.dossier_columns;
        let rows = KINDS.len().div_ceil(columns);
        let width = game_body_width(ui.available_width());
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                ui.visuals_mut().widgets.noninteractive.bg_stroke.color = self.theme.palette.surface.color();
                let column_width = (width - (columns - 1) as f32 * ui.spacing().item_spacing.x) / columns as f32;
                for row in 0..rows {
                    let cells: Vec<_> = (0..columns).map(|column| {
                        let kind = KINDS.get(column * rows + row)?;
                        let card = me.cards.iter().find(|card| card.kind == *kind)?;
                        Some((card, self.dossier_labels(ui, room, card, column_width)))
                    }).collect();
                    let height = cells.iter().flatten().map(|(_, labels)| dossier_height(labels))
                        .fold(DOSSIER_LOCK_SIZE, f32::max);
                    ui.columns(columns, |cols| {
                        for (ui, cell) in cols.iter_mut().zip(cells) {
                            if let Some((card, labels)) = cell {
                                self.dossier_card(ui, card, me.active, labels, height);
                            }
                        }
                    });
                    ui.separator();
                }

                let abilities = ABILITIES.iter().filter_map(|&kind| me.cards.iter().find(|card| card.kind == kind));
                let extra = me.cards.iter().filter(|card| {
                    let kind = card.kind.as_str();
                    kind != "talent" && !KINDS.contains(&kind) && !ABILITIES.contains(&kind)
                });
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    let width = width * 0.64;
                    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(width);
                        for card in abilities.chain(extra) {
                            let labels = self.dossier_labels(ui, room, card, width);
                            let height = dossier_height(&labels);
                            self.dossier_card(ui, card, me.active, labels, height);
                            ui.separator();
                        }
                    });
                });
            });
        });
        let remaining = top + self.layout.section_min_height - ui.next_widget_position().y;
        if remaining > 0.0 { ui.add_space(remaining); }
    }

    fn dossier_labels(&self, ui: &mut egui::Ui, room: &Room, card: &Card, width: f32) -> DossierLabels {
        let size = self.theme.fonts.dossier;
        let (value, description) = card.dossier_text();
        let width = width - 2.0 * f32::from(self.layout.dossier_padding[0]);
        let label_width = (width * 0.42).max(1.0);
        let info_width = if description.is_some() { DOSSIER_INFO_SIZE + DOSSIER_GAP } else { 0.0 };
        let value_width = (width - label_width - DOSSIER_LOCK_SIZE - 2.0 * DOSSIER_GAP - info_width).max(1.0);
        let name = room.category_labels.get(&card.kind).map(String::as_str).unwrap_or_else(|| category_name(&card.kind));
        let label = egui::text::LayoutJob::simple(
            name.to_owned(), FontWeight::Medium.font(size), self.theme.palette.text.color(), label_width,
        );
        let mut value = egui::text::LayoutJob::simple(
            value.to_owned(), FontWeight::SemiBold.font(size), self.theme.palette.accent_title.color(), value_width,
        );
        value.halign = egui::Align::RIGHT;
        ui.fonts_mut(|fonts| (fonts.layout_job(label), fonts.layout_job(value)))
    }

    fn dossier_card(&mut self, ui: &mut egui::Ui, card: &Card, active: bool, (label, value): DossierLabels, height: f32) {
        let [x, y] = self.layout.dossier_padding;
        let (_, description) = card.dossier_text();
        let (icon, marker, tooltip) = if card.revealed {
            (Icon::Unlocked, &self.theme.icons.unlocked, "Уже открыто")
        } else {
            (Icon::Locked, &self.theme.icons.locked, "Открыть карту всем игрокам")
        };
        let marker = if marker.is_empty() { if card.revealed { "✓" } else { "+" } } else { marker.as_str() };
        let button = self.artwork.button(icon, marker, 17.0).frame(false).truncate();
        egui::Frame::new().inner_margin(egui::Margin::symmetric(x, y)).show(ui, |ui| {
            let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
            let middle = row.center().y;
            let lock_rect = egui::Rect::from_center_size(
                egui::pos2(row.right() - DOSSIER_LOCK_SIZE / 2.0, middle), egui::Vec2::splat(DOSSIER_LOCK_SIZE),
            );
            let mut value_right = lock_rect.left() - DOSSIER_GAP;
            if let Some(description) = description {
                let center = egui::pos2(value_right - DOSSIER_INFO_SIZE / 2.0, middle);
                let rect = egui::Rect::from_center_size(center, egui::Vec2::splat(DOSSIER_INFO_SIZE));
                let color = self.theme.palette.accent_title.color();
                ui.painter().circle_stroke(center, DOSSIER_INFO_SIZE / 2.0 - 1.0, Stroke::new(1.0_f32, color));
                let info = egui::Button::new(FontWeight::Bold.text(11.0, "!").color(color))
                    .frame(false).min_size(rect.size());
                let response = ui.place(rect, animated_button(info));
                egui::Popup::menu(&response).show(|ui| { ui.label(description); });
                response.on_hover_text(description);
                value_right = rect.left() - DOSSIER_GAP;
            }
            let name_rect = egui::Rect::from_min_size(
                egui::pos2(row.left(), middle - label.size().y / 2.0), label.size(),
            );
            let value_rect = egui::Rect::from_min_size(
                egui::pos2(value_right - value.size().x, middle - value.size().y / 2.0), value.size(),
            );
            ui.place(name_rect, egui::Label::new(label));
            ui.place(value_rect, egui::Label::new(value));
            let button = ui.place(lock_rect, |ui: &mut egui::Ui| ui.add_enabled(active && !card.revealed, animated_button(button)));
            if button.on_hover_text(tooltip).on_disabled_hover_text(tooltip).clicked() {
                self.send(json!({"type":"reveal","kind":card.kind}));
            }
        });
    }

    fn edit_card(&mut self, player_id: &str, kind: &str, value: &str) {
        self.editor = Some((player_id.to_owned(), kind.to_owned()));
        self.editor_value = value.to_owned();
    }

    fn overview_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        ui.add_space(30.0);
        let count = room.players.iter().filter(|p| p.active).count();
        let mut heading = egui::text::LayoutJob::default();
        let font = FontWeight::Bold.font(self.theme.fonts.heading);
        heading.append("Желающие попасть в бункер: ", 0.0, egui::TextFormat::simple(
            font.clone(), self.theme.palette.text.color(),
        ));
        heading.append(&format!("{count}/{}", room.players.len()), 0.0, egui::TextFormat::simple(
            font, self.theme.palette.accent_title.color(),
        ));
        ui.vertical_centered(|ui| { ui.add(egui::Label::new(heading)); });
        ui.add_space(self.theme.spacing.heading_gap);
        let keys = ["gender", "body", "personality", "profession", "health", "hobby", "fear", "equipment", "backpack", "fact"];
        self.player_table(ui, room, "player-matrix", &keys, self.layout.players_cell_size);
        ui.add_space(TABLE_BOTTOM_GAP);
    }

    fn abilities_section(&mut self, ui: &mut egui::Ui, room: &Room) {
        ui.add_space(30.0);
        heading(ui, &self.theme, "Таблица спец. возможностей");
        self.player_table(ui, room, "ability-matrix", &["ability", "ability2"], self.layout.abilities_cell_size);
        ui.add_space(TABLE_BOTTOM_GAP);
    }

    fn player_table(&mut self, ui: &mut egui::Ui, room: &Room, id: &str, keys: &[&str], cell: [f32; 2]) {
        let host = room.you == room.host_id;
        let body_width = game_body_width(ui.available_width());
        let scale = (body_width / (130.0 + keys.len() as f32 * cell[0])).max(0.82);
        let player_width = 130.0 * scale;
        let cell = [cell[0] * scale, cell[1] * scale];
        let header_height = 36.0 * scale;
        let header_size = self.theme.fonts.table_heading * scale;
        let text_size = self.theme.fonts.table_body * scale;
        let line = self.theme.palette.button.color();
        let offset = (ui.available_width() - body_width) / 2.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(offset);
            egui::ScrollArea::horizontal().id_salt(id).max_width(body_width).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new(id).spacing([0.0, 0.0]).show(ui, |ui| {
                    ui.add_sized([player_width, header_height], egui::Label::new(FontWeight::SemiBold.text(header_size, "Игрок")));
                    for &kind in keys {
                        let name = room.category_labels.get(kind).map(String::as_str).unwrap_or_else(|| category_name(kind));
                        let name = if kind == "fact" && name == "Дополнительное сведение" {
                            "Дополнительное\nсведение"
                        } else {
                            name
                        };
                        ui.add_sized([cell[0], header_height], egui::Label::new(FontWeight::SemiBold.text(header_size, name)).wrap());
                    }
                    for player in &room.players {
                        ui.end_row();
                        let fill = if player.active {
                            Color32::TRANSPARENT
                        } else {
                            self.theme.palette.surface.color()
                        };
                        table_cell(ui, [player_width, cell[1]], line, fill, |ui| {
                            let avatar_size = (35.0 * scale).min(ui.available_height());
                            self.avatar_widget(ui, player.avatar, false, avatar_size);
                            let rect = ui.max_rect();
                            let left = ui.next_widget_position().x;
                            let width = (rect.right() - left).max(1.0);
                            let label_height = text_size * 1.45;
                            let has_action = host || !player.active;
                            let group_height = if has_action { 2.0 * label_height } else { label_height };
                            let name_rect = egui::Rect::from_min_size(
                                egui::pos2(left, rect.center().y - group_height / 2.0),
                                egui::vec2(width, label_height),
                            );
                            let mut name_ui = ui.new_child(egui::UiBuilder::new().max_rect(name_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)));
                            name_ui.add(egui::Label::new(FontWeight::Medium.text(text_size, &player.name)).truncate())
                                .on_hover_text(&player.name);
                            if has_action {
                                let action_rect = name_rect.translate(egui::vec2(0.0, label_height));
                                let mut action_ui = ui.new_child(egui::UiBuilder::new().max_rect(action_rect)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)));
                                if host {
                                    let (label, color) = if player.active {
                                        ("Выгнать", self.theme.palette.negative.color())
                                    } else {
                                        ("Вернуть", self.theme.palette.positive.color())
                                    };
                                    let action = egui::Button::new(FontWeight::Medium.text(text_size, label).color(color)).frame(false);
                                    if action_ui.add(animated_button(action)).clicked() {
                                        self.send(json!({"type":"set_active","playerId":player.id,"active":!player.active}));
                                    }
                                } else {
                                    action_ui.label(FontWeight::Medium.text(text_size, "Выбыл").color(self.theme.palette.negative.color()));
                                }
                            }
                        });
                        for &kind in keys {
                            let card = player.revealed_card(kind);
                            table_cell(ui, cell, line, fill, |ui| {
                                let width = ui.available_width();
                                let edit_size = (18.0 * scale).min(width * 0.4);
                                if host {
                                    let rect = ui.max_rect();
                                    let edit_rect = egui::Rect::from_center_size(
                                        egui::pos2(rect.right() - edit_size / 2.0, rect.center().y),
                                        egui::Vec2::splat(edit_size),
                                    );
                                    let icon = self.artwork.button(Icon::Edit, &self.theme.icons.edit, edit_size * 0.72).frame(false);
                                    let response = ui.place(edit_rect, |ui: &mut egui::Ui| {
                                        ui.multiply_opacity(0.5);
                                        ui.add(animated_button(icon))
                                    });
                                    if response.on_hover_text("Изменить карточку").clicked() {
                                        self.edit_card(&player.id, kind, card.map(|card| card.value.as_str()).unwrap_or(""));
                                    }
                                }
                                if let Some(card) = card {
                                    let text_width = if host { (width - edit_size - 4.0).max(1.0) } else { width };
                                    let rect = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(text_width, ui.max_rect().height()));
                                    let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect)
                                        .layout(egui::Layout::left_to_right(egui::Align::Center)));
                                    text_ui.add(egui::Label::new(FontWeight::Medium.text(text_size, card_text(&card.value))).wrap())
                                        .on_hover_text(&card.value);
                                }
                            });
                        }
                    }
                });
            });
        });
    }

    fn ui_game(&mut self, ui: &mut egui::Ui, room: &Room) {
        self.active_section = 0;
        let requested_section = self.pending_section.take();
        egui::ScrollArea::vertical().id_salt(("entire-game", &room.code)).auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
            let sections = [
                Self::hero,
                Self::bunker_section,
                Self::event_section,
                Self::dossier_section,
                Self::overview_section,
                Self::abilities_section,
            ];
            let active_line = ui.clip_rect().top() + 0.25 * viewport.height();
            let mut scroll_target = None;
            let mut last_section_top = 0.0;
            for (index, section) in sections.into_iter().enumerate() {
                let top = ui.next_widget_position().y;
                if top <= active_line {
                    self.active_section = index;
                }
                if requested_section == Some(index) {
                    scroll_target = Some(top);
                }
                last_section_top = top;
                section(self, ui, room);
                section_rule(ui, &self.theme);
            }
            ui.horizontal_wrapped(|ui| {
                if room.you == room.host_id {
                    if ui.add(animated_button(self.theme.accent_button("Панель ведущего"))).clicked() { self.show_host_tools = true; }
                    if ui.add(animated_button(egui::Button::new("Следующий раунд"))).clicked() { self.send(json!({"type":"next_round"})); }
                    if ui.add(animated_button(egui::Button::new(if room.vote_open {"Закрыть голосование"} else {"Начать голосование"}))).clicked() {
                        self.send(json!({"type":if room.vote_open {"finish_vote"} else {"start_vote"}}));
                    }
                    if ui.add(animated_button(egui::Button::new("Завершить"))).clicked() { self.send(json!({"type":"finish"})); }
                }
                ui.add(egui::DragValue::new(&mut self.dice_sides).range(2..=100).prefix("d"));
                if ui.add(animated_button(egui::Button::new("Бросить кубик"))).clicked() {
                    self.send(json!({"type":"roll","sides":self.dice_sides}));
                }
            });
            if room.vote_open {
                ui.label(format!("Голосование открыто · получено голосов: {}", room.ballots_count));
                for p in room.players.iter().filter(|p| p.active && p.id != room.you) {
                    if ui.add(animated_button(egui::Button::new(format!("Голосовать за {}", p.name)))).clicked() {
                        self.send(json!({"type":"vote","target":p.id}));
                    }
                }
            }
            for result in &room.vote_results {
                ui.label(format!("{}: {} голосов", result.name, result.votes));
            }
            ui.add_space(14.0);
            ui.label(FontWeight::Bold.text(self.theme.fonts.button, "Журнал партии"));
            for event in room.log.iter().rev().take(8) {
                ui.label(RichText::new(event).color(self.theme.palette.muted.color()));
            }
            // Allow the final section to reach the same top alignment as earlier sections.
            let remaining = last_section_top + viewport.height() - ui.next_widget_position().y;
            if remaining > 0.0 { ui.add_space(remaining); }
            if let Some(top) = scroll_target {
                scroll_to_section(ui, top);
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
            if ui.add(animated_button(self.theme.accent_button("Оценка выживаемости · WIP"))).clicked() {
                self.show_survivability = true;
            }
            if ui.add(animated_button(egui::Button::new("В главное меню"))).clicked() { self.leave_room(); }
        });
    }
    fn room_creation(&mut self, ui: &mut egui::Ui, width: f32, list_height: f32, active: bool) {
        ui.set_width(width);
        let gap = ui.spacing().item_spacing.x;
        let column_width = (width - gap) / 2.0;
        let mut changed = false;
        let mut picked = None;
        let mut reload = false;
        let mut create = false;
        let mut list_rect = egui::Rect::NOTHING;
        ui.columns(2, |cols| {
            let left = &mut cols[0];
            left.heading("Паки");
            let frame = egui::Frame::new()
                .stroke(Stroke::new(1.0_f32, self.theme.palette.button.color()))
                .inner_margin(10)
                .show(left, |ui| {
                    let dragging = ui.input(|input| !input.raw.hovered_files.is_empty());
                    if dragging {
                        ui.label("Отпусти JSON или ZIP здесь");
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("create-packs")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .auto_shrink([false, false])
                        .max_height(if dragging { (list_height - 28.0).max(60.0) } else { list_height })
                        .show(ui, |ui| {
                            for entry in &mut self.room_packs {
                                let status = if entry.error().is_some() { self.theme.palette.negative.color() }
                                    else if entry.active() { self.theme.palette.positive.color() }
                                    else { self.theme.palette.button.color() };
                                let title = FontWeight::Medium.text(self.theme.fonts.body, entry.name());
                                let width = ui.available_width();
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::hover());
                                let strip = egui::Rect::from_min_size(rect.min, egui::vec2(5.0, rect.height()));
                                ui.painter().rect_filled(strip, 0u8, status);
                                let button_rect = egui::Rect::from_min_max(rect.min + egui::vec2(8.0, 0.0), rect.max);
                                let response = ui.place(button_rect, animated_button(egui::Button::new(title).truncate()
                                    .fill(self.theme.palette.input.color()).corner_radius(0u8).min_size(button_rect.size())));
                                if response.on_hover_text(entry.error().unwrap_or(entry.name())).clicked() {
                                    entry.toggle();
                                    changed = true;
                                }
                            }
                        });
                });
            list_rect = frame.response.rect;
            left.horizontal(|ui| {
                if ui.add_sized([(column_width - gap - 40.0).max(80.0), 38.0], animated_button(egui::Button::new("Добавить пак"))).clicked() {
                    picked = rfd::FileDialog::new().add_filter("Паки Murr", &["json", "zip"]).pick_files();
                }
                if ui.add_sized([40.0, 38.0], animated_button(egui::Button::new("↻"))).on_hover_text("Обновить паки с диска").clicked() {
                    reload = true;
                }
            });

            let right = &mut cols[1];
            right.heading("События");
            right.label("Шанс события");
            right.add(egui::Slider::new(&mut self.event_chance, 0..=100).suffix("%"));
            right.label(RichText::new("Шанс сохраняется в комнате. Игровые события пока не запускаются.")
                .color(self.theme.palette.muted.color()).size(self.theme.fonts.small));
            let remaining = (list_rect.bottom() - right.next_widget_position().y).max(0.0);
            right.add_space(remaining);
            right.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let available = self.room_packs.iter().any(crate::packs::Entry::active) && self.pending.is_none();
                create = ui.add_enabled(available, animated_button(self.theme.accent_button("Создать комнату"))).clicked();
            });
        });

        if !active { return; }
        let mut files = picked.unwrap_or_default();
        // Native Windows file-drop events don't carry coordinates.
        ui.input_mut(|input| {
            files.extend(std::mem::take(&mut input.raw.dropped_files).into_iter().filter_map(|file| file.path));
        });
        for path in files {
            match crate::packs::add(&mut self.room_packs, &path) {
                Ok(()) => changed = true,
                Err(error) => self.notify(format!("{}: {error}", path.display())),
            }
        }
        if changed {
            if let Err(error) = crate::packs::save_library(&self.room_packs) {
                self.notify(format!("Не удалось сохранить список паков: {error}"));
            }
        }
        if reload {
            for entry in &mut self.room_packs { entry.reload(); }
            let errors = self.room_packs.iter().filter(|entry| entry.error().is_some()).count();
            self.notify(if errors == 0 { "Паки обновлены".into() } else { format!("Паки обновлены. Ошибок: {errors}") });
        }
        if create {
            match crate::packs::selected_payload(&self.room_packs) {
                Ok((json, zip)) => {
                    self.pack_json = json;
                    self.pack_zip = zip;
                    self.show_create = false;
                    self.start_request(None);
                }
                Err(error) => self.notify(error),
            }
        }
    }

    fn ui_overlays(&mut self, ctx: &egui::Context) {
        {
            let id = egui::Id::new("choose-avatar");
            let active = self.show_avatar_picker;
            let opacity = modal_opacity(ctx, id, active);
            if opacity > 0.0 {
                let modal = egui::Modal::new(id)
                    .area(egui::Modal::default_area(id).fade_in(false).interactable(active))
                    .backdrop_color(Color32::from_black_alpha((100.0 * opacity) as u8))
                    .frame(egui::Frame::new().fill(self.theme.palette.panel.color()).corner_radius(5u8)
                        .stroke(Stroke::new(1.0_f32, self.theme.palette.accent.color())).inner_margin(15).multiply_with_opacity(opacity))
                    .show(ctx, |ui| {
                        ui.multiply_opacity(opacity);
                        if !active { ui.disable(); }
                        let columns = self.layout.avatar_picker_columns;
                        let gap = ui.spacing().item_spacing.x;
                        let side = self.layout.avatar_picker_size.min(
                            (ctx.content_rect().width() - gap * f32::from(columns - 1) - 60.0) / f32::from(columns)
                        );
                        let grid_width = side * f32::from(columns) + gap * f32::from(columns - 1);
                        ui.set_width(grid_width);
                        ui.vertical_centered(|ui| { ui.label(FontWeight::Regular.text(self.theme.fonts.avatar_caption, "Выберите аватар:")); });
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
                if active && modal.should_close() { self.show_avatar_picker = false; }
            }
        }
        {
            let id = egui::Id::new("join-room");
            let active = self.show_join;
            let opacity = modal_opacity(ctx, id, active);
            if opacity > 0.0 {
                let modal = egui::Modal::new(id)
                    .area(egui::Modal::default_area(id).fade_in(false).interactable(active))
                    .backdrop_color(Color32::from_black_alpha((100.0 * opacity) as u8))
                    .frame(egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(12).multiply_with_opacity(opacity))
                    .show(ctx, |ui| {
                        ui.multiply_opacity(opacity);
                        if !active { ui.disable(); }
                        let side = self.layout.entry_button_size;
                        let field = self.layout.welcome_name_width;
                        let row_width = side + field + ui.spacing().item_spacing.x;
                        ui.set_width(row_width);
                        centered_row(ui, row_width, side, |ui| {
                            let field = input_row(ui, &mut self.join_code, "Введите код игры", field, side, self.theme.palette.input.color(), FontWeight::SemiBold.font(self.theme.fonts.input));
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
                if active && modal.should_close() { self.show_join = false; }
            }
        }
        {
            let active = self.show_create;
            let screen = ctx.content_rect().size();
            let width = (self.layout.dialog_width * 1.85).min(screen.x - 64.0);
            let list_height = (screen.y * 0.60).clamp(180.0, 460.0);
            let mut open = active;
            egui::Window::new("Создать игру").anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .movable(false).resizable(false).collapsible(false).open(&mut open).fade_in(true).fade_out(true).interactable(active)
                .default_width(width).min_width(width).max_width(width)
                .frame(egui::Frame::new().fill(self.theme.palette.background.color())
                    .stroke(Stroke::new(1.0_f32, self.theme.palette.button.color())).inner_margin(18))
                .show(ctx, |ui| {
                    if !active { ui.disable(); }
                    self.room_creation(ui, width, list_height, active);
                });
            self.show_create &= open;
        }
        {
            let active = self.show_settings;
            let mut open = active;
            egui::Window::new("Настройки").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).fade_in(true).fade_out(true).interactable(active).show(ctx, |ui| {
                if !active { ui.disable(); }
                ui.vertical_centered(|ui| {
                    ui.label("Адрес сервера:");
                    ui.add(egui::TextEdit::singleline(&mut self.server_url).desired_width(430.0));

                    if ui.add(animated_button(egui::Button::new("Сохранить"))).clicked() { self.save_config(); }
                    ui.separator();
                    if ui.add(animated_button(egui::Button::new("Перезагрузить оформление"))).on_hover_text("Ctrl+Shift+R").clicked() {
                        self.reload_appearance(ctx);
                    }
                    ui.label(portable_path("theme.json").parent().unwrap_or(Path::new(".")).display().to_string());
                });
            });
            self.show_settings = open;
        }
        {
            let active = self.show_host_tools;
            let mut open = active;
            let snapshot = self.room.clone();
            egui::Window::new("Параметры комнаты / ведущий").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).fade_in(true).fade_out(true).interactable(active)
                .default_width(430.0).show(ctx, |ui| {
                if !active { ui.disable(); }
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
                            if ui.add(animated_button(egui::Button::new("Выбрать другой пак…"))).clicked() { self.pick_pack(); }
                            if ui.add(animated_button(egui::Button::new("Применить выбранный пак"))).clicked() { self.apply_pack(); }
                        }
                        if room.stage == "playing" {
                            ui.horizontal(|ui| {
                                if ui.add(animated_button(egui::Button::new("Следующий раунд"))).clicked() { self.send(json!({"type":"next_round"})); }
                                if ui.add(animated_button(egui::Button::new("Завершить партию"))).clicked() { self.send(json!({"type":"finish"})); }
                            });
                            ui.separator();
                            for p in &room.players {
                                ui.horizontal(|ui| {
                                    ui.label(&p.name);
                                    if ui.add(animated_button(egui::Button::new(if p.active {"Исключить"} else {"Вернуть"}))).clicked() {
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
        let editor_id = egui::Id::new("editor-fade-content");
        let active = self.editor.is_some();
        if let Some(editor) = self.editor.clone() {
            ctx.data_mut(|data| data.insert_temp(editor_id, editor));
        }
        if let Some((player, kind)) = ctx.data(|data| data.get_temp::<(String, String)>(editor_id)) {
            let mut open = active;
            egui::Window::new("Редактор характеристики ведущего").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).fade_in(true).fade_out(true).interactable(active)
                .default_width(470.0).show(ctx, |ui| {
                if !active { ui.disable(); }
                ui.label(format!("Категория: {} · Редактирование видно всем после раскрытия", kind));
                ui.add(egui::TextEdit::multiline(&mut self.editor_value)
                    .desired_rows(3).desired_width(450.0));
                ui.horizontal_wrapped(|ui| {
                    if ui.add(animated_button(egui::Button::new("Применить текст"))).clicked() {
                        self.send(json!({"type":"host_edit_card","playerId":player,"kind":kind,"value":self.editor_value}));
                        self.editor = None;
                    }
                    if ui.add(animated_button(egui::Button::new("Открыть всем"))).clicked() {
                        self.send(json!({"type":"host_reveal_card","playerId":player,"kind":kind}));
                        self.editor = None;
                    }
                    if ui.add(animated_button(egui::Button::new("Перегенерировать"))).clicked() {
                        self.send(json!({"type":"host_redraw_card","playerId":player,"kind":kind}));
                        self.editor = None;
                    }
                });
            });
            if !open { self.editor = None; }
        }
        {
            let active = self.show_survivability;
            let mut open = active;
            egui::Window::new("Оценка выживаемости — WIP").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).movable(false).resizable(false).collapsible(false).open(&mut open).fade_in(true).fade_out(true).interactable(active).show(ctx, |ui| {
                if !active { ui.disable(); }
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
        let screen = match self.room.as_ref().map(|room| room.stage.as_str()) {
            Some("lobby") => Screen::Lobby,
            Some("playing") => Screen::Game,
            Some(_) => Screen::Finished,
            None if self.profile_ready => Screen::Menu,
            None => Screen::Welcome,
        };
        let now = ctx.input(|input| input.time);
        let (previous, started) = self.screen_transition.get_or_insert((screen, now));
        if *previous != screen {
            *previous = screen;
            *started = now;
        }
        let opacity = screen_opacity(now - *started);
        if opacity < 1.0 { ctx.request_repaint(); }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.theme.palette.background.color()).inner_margin(self.layout.content_margin))
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
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
                                    if ui.add(animated_button(egui::Button::new("В меню"))).clicked() { self.leave_room(); }
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
        if self.pending.is_some() || self.room_deadline.is_some() || self.upload_pending.is_some() || self.download_pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_animates_to_the_section_and_back() {
        let ctx = egui::Context::default();
        let frame = |time, target| {
            let mut offset = 0.0;
            let input = egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(850.0, 600.0))),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    offset = egui::ScrollArea::vertical().id_salt("game-test").show(ui, |ui| {
                        let start = ui.next_widget_position().y;
                        ui.add_space(1000.0);
                        let second = ui.next_widget_position().y;
                        egui::ScrollArea::horizontal().show(ui, |ui| { ui.label("Table"); });
                        ui.add_space(1000.0);
                        if let Some(index) = target {
                            scroll_to_section(ui, [start, second][index]);
                        }
                    }).state.offset.y;
                });
            });
            offset
        };
        assert_eq!(frame(0.0, None), 0.0);
        assert_eq!(frame(0.1, Some(1)), 0.0);
        let moving = frame(0.26, None);
        assert!(moving > 0.0 && moving < 950.0, "intermediate offset: {moving}");
        let arrived = frame(0.9, None);
        assert!((950.0..=1050.0).contains(&arrived), "section offset: {arrived}");
        frame(1.0, Some(0));
        assert_eq!(frame(1.8, None), 0.0);
    }
}
