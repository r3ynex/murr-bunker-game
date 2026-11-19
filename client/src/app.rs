use crate::model::{category_name, AvatarId, Card, LocalConfig, Room, SavedSession, Session};
use crate::appearance::{portable_path, with_icon, FontWeight, Layout, Theme};
use crate::artwork::{centered_row, flat_button, input_row, paint_contain, paint_cover, paint_gradient, table_cell, Artwork, Icon, TABLE_CELL_PADDING};
use crate::avatars::Avatars;
use crate::host_panel::{HostPanel, PanelAction};
use crate::network;
use crate::motion::{animated_button, button_highlight, modal_opacity, screen_opacity};
use eframe::egui::{self, Color32, RichText, Stroke};
use crate::network::{WsEvent, WsSender};
use crate::updater::{Updater, VERSION};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
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
fn avatar_color(id: AvatarId) -> Color32 {
    let palette = [
        (147, 112, 255), (215, 99, 129), (82, 162, 183), (238, 161, 66),
        (114, 181, 116), (198, 137, 208), (198, 179, 87), (77, 147, 213),
        (174, 107, 77), (106, 184, 172), (224, 109, 99), (157, 164, 189)
    ];
    let (r,g,b) = palette[(id.saturating_sub(1) as usize) % palette.len()];
    Color32::from_rgb(r,g,b)
}
fn avatar_symbol(id: AvatarId) -> &'static str {
    const MARKS: [&str; 12] = ["✦", "◆", "☼", "✿", "★", "♣", "◇", "◈", "✧", "●", "☻", "♟"];
    MARKS[(id.saturating_sub(1) as usize) % MARKS.len()]
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
    updater: Updater,
    server_url: String,
    nickname: String,
    avatar: Option<AvatarId>,
    avatars: Avatars,
    profile_ready: bool,
    screen_transition: Option<(Screen, f64)>,
    join_code: String,
    session: Option<Session>,
    saved_session: Option<SavedSession>,
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
    host_panel: HostPanel,
    show_survivability: bool,
    confirm_leave: bool,
    pending_vote_jump: bool,
    editor: Option<(String, String)>,
    editor_value: String,
    editor_health_degree: String,
    transfer_ids: String,
    transfer_target: Option<String>,
    pending_section: Option<usize>,
    active_section: usize,
    dice: crate::dice::DiceOverlay,
    event_chance: u32,
    pack_json: Option<String>,
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
        let saved_session = fs::read_to_string(portable_path("murr-session.json")).ok()
            .and_then(|s| serde_json::from_str::<SavedSession>(&s).ok())
            .filter(|saved| !saved.session.code.trim().is_empty() && !saved.session.token.trim().is_empty());
        let profile_ready = !config.nickname.trim().is_empty();
        Self {
            theme,
            artwork: Artwork::load(&cc.egui_ctx),
            layout,
            toast: (!appearance_message.is_empty()).then(|| Toast {
                text: appearance_message,
                started: Instant::now(),
            }),
            updater: Updater::new(),
            server_url: config.server_url,
            nickname: config.nickname,
            avatar: if profile_ready { Some(config.avatar) } else { None },
            avatars: Avatars::discover(),
            profile_ready, screen_transition: None, join_code: String::new(),
            session: None, saved_session, room: None, pending: None, sender: None, receiver: None, room_deadline: None, online: false,
            create_on_connect: false,
            show_avatar_picker: false, show_join: false, show_create: false,
            show_settings: false, show_host_tools: false, host_panel: HostPanel::default(), show_survivability: false,
            confirm_leave: false, pending_vote_jump: false,
            editor: None, editor_value: String::new(), editor_health_degree: String::new(),
            transfer_ids: String::new(), transfer_target: None,
            pending_section: None, active_section: 0,
            dice: crate::dice::DiceOverlay::default(), event_chance: 0,
            pack_json: None,
            room_packs: Vec::new(), pack_zip: None,
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
        self.dice.reload_meshes();
        self.avatars = Avatars::discover();
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
        let host_button_space = if self.room.as_ref().is_some_and(|room| room.you == room.host_id && matches!(room.stage.as_str(), "playing" | "finished")) { 42.0 } else { 0.0 };
        let bottom = f32::from(self.layout.status_margin) + 8.0 + host_button_space;
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
        if self.avatar.is_none() {
            let ids = self.avatars.ids();
            self.avatar = Some(ids[random_int(0, (ids.len() - 1) as u32) as usize]);
        }
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
    fn persist_session(&mut self, session: &Session) {
        let saved = SavedSession {
            server_url: self.server_url.trim().trim_end_matches('/').to_owned(),
            session: session.clone(),
        };
        if let Ok(text) = serde_json::to_string_pretty(&saved) {
            let _ = fs::write(portable_path("murr-session.json"), text);
        }
        self.saved_session = Some(saved);
    }
    fn clear_saved_session(&mut self) {
        self.saved_session = None;
        let _ = fs::remove_file(portable_path("murr-session.json"));
    }
    fn connect_session(&mut self, ctx: &egui::Context, session: Session) {
        let ctx = ctx.clone();
        match network::connect(&self.server_url, &session, move || ctx.request_repaint()) {
            Ok((sender, receiver)) => {
                self.notify(format!("Подключаемся к комнате {}…", session.code));
                self.persist_session(&session);
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
        } else if let Some(source) = &self.pack_json {
            match serde_json::from_str::<Value>(source) {
                Ok(pack) => self.send(json!({"type":"set_pack", "pack":pack})),
                Err(e) => self.notify(format!("Ошибка пака: {e}")),
            }
        }
    }
    fn pick_pack(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Паки Murr", &["json", "zip"]).pick_file() {
            match crate::packs::Pack::load(&path).and_then(|pack| pack.payload()) {
                Ok((json, zip)) => {
                    self.pack_json = Some(json);
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
                    if self.room.is_none() && (e.contains("HTTP 401") || e.contains("HTTP 404")) {
                        self.clear_saved_session();
                        self.session = None;
                        self.notify("Комната больше недоступна — сохранённая сессия сброшена");
                    } else {
                        self.notify(format!("Связь: {e}"));
                    }
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
        self.dice.sync(room.dice_roll.as_ref());
        if room.stage == "closed" {
            self.clear_saved_session();
        }
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
        self.clear_saved_session();
        self.room = None;
        self.dice.sync(None);
        self.show_host_tools = false;
        self.host_panel = HostPanel::default();
        self.confirm_leave = false;
        self.pending_vote_jump = false;
        self.editor = None;
        self.clear_background();
        self.session = None;
        self.online = false;
        self.notify("Вы вышли в главное меню");
    }
    fn open_avatar_picker(&mut self) {
        self.avatars = Avatars::discover();
        self.show_avatar_picker = true;
    }
    fn avatar_widget(&self, ui: &mut egui::Ui, avatar: AvatarId, selected: bool, size: f32) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, avatar_color(avatar).gamma_multiply(0.55));
        if let Some(texture) = self.avatars.texture(ui.ctx(), avatar) {
            paint_cover(&painter, &texture, rect.shrink(2.0));
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
                    self.open_avatar_picker();
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
                self.open_avatar_picker();
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
                self.room_packs = crate::packs::library();
                self.show_create = true;
            }
            if flat_button(ui, size, egui::Button::new(settings).wrap(), neutral, radius).clicked() {
                self.show_settings = true;
            }
        });
        if let Some(saved) = self.saved_session.clone() {
            ui.add_space(22.0);
            let card_w = (3.0 * width + 2.0 * gap).min(520.0);
            let mut reconnect = false;
            let mut forget = false;
            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(card_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(card_w);
                    egui::Frame::new()
                        .fill(self.theme.palette.panel.color())
                        .stroke(Stroke::new(1.0_f32, self.theme.palette.outline.color()))
                        .corner_radius(6u8)
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    FontWeight::SemiBold
                                        .text(self.theme.fonts.body, format!("Последняя комната: {}", saved.session.code))
                                        .color(self.theme.palette.accent_title.color()),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.add(animated_button(egui::Button::new("Сбросить"))).clicked() {
                                        forget = true;
                                    }
                                    if ui.add_enabled(
                                        self.pending.is_none() && self.room_deadline.is_none(),
                                        animated_button(self.theme.accent_button("Вернуться в комнату")),
                                    ).clicked() {
                                        reconnect = true;
                                    }
                                });
                            });
                        });
                });
            });
            if reconnect {
                if !saved.server_url.trim().is_empty() {
                    self.server_url = saved.server_url;
                }
                self.create_on_connect = false;
                self.connect_session(ui.ctx(), saved.session);
            } else if forget {
                self.clear_saved_session();
                self.notify("Сохранённая сессия сброшена");
            }
        }
    }

    fn ui_lobby(&mut self, ui: &mut egui::Ui, room: &Room) {
        let host = room.you == room.host_id;
        let height = ui.available_height();
        ui.add_space(0.08 * height);
        ui.vertical_centered(|ui| {
            let code_label = format!("Лобби: {}", room.code);
            let copy_text = "Копировать код";
            let heading_font = FontWeight::Bold.font(self.theme.fonts.lobby_heading);
            let title_w = ui.painter().layout_no_wrap(code_label.clone(), heading_font, Color32::WHITE).size().x;
            let copy_w = 148.0;
            let gap = 12.0;
            centered_row(ui, (title_w + copy_w + gap).min(ui.available_width()), 38.0, |ui| {
                ui.label(FontWeight::Bold.text(self.theme.fonts.lobby_heading, code_label));
                let copy_btn = egui::Button::new(
                    FontWeight::SemiBold.text(self.theme.fonts.small + 1.0, copy_text)
                        .color(self.theme.palette.accent_title.color())
                )
                .fill(self.theme.palette.panel.color())
                .stroke(Stroke::new(1.0_f32, self.theme.palette.outline.color()))
                .corner_radius(4u8);
                if ui.add_sized([copy_w, 30.0], animated_button(copy_btn))
                    .on_hover_text("Скопировать код комнаты в буфер обмена")
                    .clicked()
                {
                    ui.ctx().copy_text(room.code.clone());
                    self.notify(format!("Код комнаты скопирован: {}", room.code));
                }
            });
            ui.add_space(4.0);
            ui.label(
                FontWeight::Medium.text(
                    self.theme.fonts.lobby_count,
                    format!(
                        "Игроки: {} из 15  ·  Мест в бункере: {}  ·  В сети: {}",
                        room.players.len(),
                        room.seats,
                        room.connected
                    ),
                ),
            );
            ui.label(
                FontWeight::Regular
                    .text(self.theme.fonts.small + 1.0, format!("Пак: {}", room.pack_name))
                    .color(self.theme.palette.muted.color()),
            );
        });
        ui.add_space(16.0);

        let [action_width, action_height] = self.theme.buttons.menu_size;
        let roster_height = (ui.available_height() - action_height - 44.0).max(75.0);
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
                            let is_you = player.id == room.you;
                            let ready = player.ready || player_is_host;
                            let status_color = if ready {
                                self.theme.palette.positive.color()
                            } else {
                                self.theme.palette.negative.color()
                            };
                            let border_color = if is_you {
                                self.theme.palette.outline.color()
                            } else {
                                self.theme.palette.button.color().gamma_multiply(0.55)
                            };
                            egui::Frame::new()
                                .fill(self.theme.palette.surface.color())
                                .stroke(Stroke::new(1.0_f32, border_color))
                                .corner_radius(4u8)
                                .inner_margin(egui::Margin::symmetric(10, 8))
                                .show(column, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let (bar, _) = ui.allocate_exact_size(egui::vec2(4.0, side), egui::Sense::hover());
                                        ui.painter().rect_filled(bar, 2u8, status_color);
                                        self.avatar_widget(ui, player.avatar(), is_you, side);
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                if player_is_host {
                                                    if let Some(icon) = self.artwork.icon(Icon::Host, 18.0) {
                                                        ui.add(icon).on_hover_text("Ведущий");
                                                    } else {
                                                        ui.label(
                                                            RichText::new(&self.theme.icons.host)
                                                                .color(self.theme.palette.accent_title.color()),
                                                        )
                                                        .on_hover_text("Ведущий");
                                                    }
                                                }
                                                ui.add(
                                                    egui::Label::new(FontWeight::Bold.text(
                                                        self.theme.fonts.lobby_name,
                                                        &player.name,
                                                    ))
                                                    .truncate(),
                                                )
                                                .on_hover_text(&player.name);
                                            });
                                            let subtitle = if player_is_host {
                                                if is_you { "Ведущий · Вы" } else { "Ведущий" }
                                            } else if ready {
                                                if is_you { "Готов · Вы" } else { "Готов" }
                                            } else if is_you {
                                                "Не готов · Вы"
                                            } else {
                                                "Ожидание"
                                            };
                                            ui.label(
                                                FontWeight::Medium
                                                    .text(self.theme.fonts.small + 1.0, subtitle)
                                                    .color(if player_is_host {
                                                        self.theme.palette.accent_title.color()
                                                    } else {
                                                        status_color
                                                    }),
                                            );
                                        });
                                    });
                                });
                            column.add_space(6.0);
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
        let width_scale = (width / self.layout.window_size[0]).min(1.0);
        let base_height = (self.layout.hero_height * width_scale).min(ui.clip_rect().height());
        let scale = base_height / self.layout.hero_height;
        let font_scale = scale.max(0.8);
        let [horizontal_inset, vertical_inset] = self.layout.hero_inset;
        let text_width = self.layout.hero_text_size[0].min((width - 2.0 * horizontal_inset).max(1.0));
        let color = self.theme.palette.text.color();
        let survivors = room.players.iter().filter(|player| player.active).count();
        let text = [
            ("Катаклизм".to_owned(), FontWeight::Bold.font(self.theme.fonts.heading)),
            (room.catastrophe.clone(), FontWeight::Regular.font(self.theme.fonts.hero_body)),
            (format!("Раунд {} · Выживших: {survivors}", room.round), FontWeight::Regular.font(self.theme.fonts.hero_body)),
        ].map(|(text, mut font)| {
            font.size *= font_scale;
            let mut job = egui::text::LayoutJob::simple(text, font, color, text_width);
            job.halign = egui::Align::Center;
            ui.fonts_mut(|fonts| fonts.layout_job(job))
        });
        let gap = 30.0 * scale;
        let group_height = text.iter().map(|galley| galley.size().y).sum::<f32>() + 2.0 * gap;
        let height = base_height.max(group_height + 2.0 * vertical_inset * scale);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0u8, self.theme.palette.hero.color());
        if let Some(texture) = self.hero_texture.as_ref().or(self.artwork.catastrophe.as_ref()) {
            if scale < 1.0 {
                paint_contain(&painter, texture, rect);
            } else {
                paint_cover(&painter, texture, rect);
            }
            if self.hero_texture.is_some() {
                painter.add(paint_gradient(rect, Color32::from_black_alpha(100), self.theme.palette.background.color(), true));
            }
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
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                egui::Frame::new()
                    .fill(self.theme.palette.panel.color().gamma_multiply(0.95))
                    .stroke(Stroke::new(1.0_f32, self.theme.palette.outline.color()))
                    .corner_radius(6u8)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
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
                            ui.add_space(4.0);
                            let current = sections.get(self.active_section).copied().unwrap_or("");
                            ui.label(
                                FontWeight::SemiBold
                                    .text(self.theme.fonts.small + 1.0, current)
                                    .color(self.theme.palette.accent_title.color()),
                            );
                        });
                    });
            });
    }

    fn host_overlay(&mut self, ctx: &egui::Context, room: &Room) {
        let mut top_offset = 14.0;
        if !self.online && self.room_deadline.is_none() {
            if let Some(session) = self.session.clone() {
                egui::Area::new(egui::Id::new("reconnect-banner"))
                    .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top_offset))
                    .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(self.theme.palette.panel.color())
                            .stroke(Stroke::new(1.0_f32, self.theme.palette.negative.color()))
                            .corner_radius(6u8)
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        FontWeight::SemiBold
                                            .text(self.theme.fonts.small + 1.0, "Нет соединения с комнатой")
                                            .color(self.theme.palette.negative.color()),
                                    );
                                    if ui.add(animated_button(self.theme.accent_button("Переподключиться"))).clicked() {
                                        self.connect_session(ctx, session);
                                    }
                                });
                            });
                    });
                top_offset += 54.0;
            }
        }
        if room.speech_ends_at != 0 && room.stage == "playing" {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
            let remaining = room.speech_ends_at.saturating_sub(now);
            let seconds = remaining.div_ceil(1000);
            egui::Area::new(egui::Id::new("speech-timer"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top_offset))
                .order(egui::Order::Foreground).movable(false).interactable(false).show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(self.theme.palette.panel.color())
                        .corner_radius(6u8)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .stroke(Stroke::new(1.0_f32, if seconds <= 5 { self.theme.palette.negative.color() } else { self.theme.palette.outline.color() }))
                        .show(ui, |ui| {
                            let caption = if seconds == 0 { "Время вышло".into() } else { format!("Обсуждение · {seconds} сек.") };
                            ui.label(FontWeight::Bold.text(self.theme.fonts.button, caption));
                        });
                });
            top_offset += 46.0;
            if remaining != 0 { ctx.request_repaint_after(Duration::from_millis((remaining - 1) % 1000 + 1)); }
        }
        if room.vote_open && room.stage == "playing" {
            egui::Area::new(egui::Id::new("vote-alert-banner"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top_offset))
                .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(self.theme.palette.panel.color())
                        .stroke(Stroke::new(1.0_f32, self.theme.palette.accent.color()))
                        .corner_radius(6u8)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let status = if room.your_vote.is_some() {
                                    format!("Голосование · ваш голос учтён ({} всего)", room.ballots_count)
                                } else {
                                    format!("Идёт голосование · голосов: {}", room.ballots_count)
                                };
                                ui.label(
                                    FontWeight::SemiBold
                                        .text(self.theme.fonts.small + 2.0, status)
                                        .color(self.theme.palette.accent_title.color()),
                                );
                                if ui.add(animated_button(egui::Button::new("К голосованию"))).clicked() {
                                    self.pending_vote_jump = true;
                                    ctx.request_repaint();
                                }
                            });
                        });
                });
        }
        if room.you == room.host_id && matches!(room.stage.as_str(), "playing" | "finished") {
            egui::Area::new(egui::Id::new("host-panel-toggle"))
                .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0))
                .order(egui::Order::Foreground).movable(false).show(ctx, |ui| {
                    let title = if self.show_host_tools { "Закрыть панель" } else { "Панель ведущего" };
                    if ui.add(animated_button(self.theme.accent_button(title))).clicked() {
                        self.show_host_tools = !self.show_host_tools;
                    }
                });
        }
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
        let is_host = room.you == room.host_id;
        let has_controls = self.online && room.host_controls.is_some();
        ui.add_space(36.0);
        heading(ui, &self.theme, "События");
        let width = game_body_width(ui.available_width()).min(780.0);
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(width);
                let border = if room.current_event.is_some() {
                    self.theme.palette.accent.color()
                } else {
                    self.theme.palette.outline.color()
                };
                egui::Frame::new()
                    .fill(self.theme.palette.panel.color())
                    .stroke(Stroke::new(1.0_f32, border))
                    .corner_radius(6u8)
                    .inner_margin(egui::Margin::symmetric(18, 16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                FontWeight::SemiBold
                                    .text(
                                        self.theme.fonts.small + 1.0,
                                        format!("Шанс события при смене раунда: {}%", room.event_chance),
                                    )
                                    .color(self.theme.palette.muted.color()),
                            );
                            if is_host && room.stage == "playing" {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if room.current_event.is_some()
                                        && ui.add_enabled(has_controls, animated_button(egui::Button::new("Скрыть"))).clicked()
                                    {
                                        self.send(json!({"type":"host_action","action":"clear_event"}));
                                    }
                                    if ui.add_enabled(has_controls, animated_button(self.theme.accent_button("Запустить событие"))).clicked() {
                                        self.send(json!({"type":"host_action","action":"trigger_event"}));
                                    }
                                });
                            }
                        });
                        ui.add_space(10.0);
                        if let Some(event) = &room.current_event {
                            ui.label(
                                FontWeight::Bold
                                    .text(self.theme.fonts.button + 4.0, &event.title)
                                    .color(self.theme.palette.accent_title.color()),
                            );
                            ui.label(
                                FontWeight::Medium
                                    .text(self.theme.fonts.small + 1.0, format!("Раунд {}", event.round))
                                    .color(self.theme.palette.muted.color()),
                            );
                            if !event.description.trim().is_empty() {
                                ui.add_space(6.0);
                                ui.add(
                                    egui::Label::new(
                                        FontWeight::Regular
                                            .text(self.theme.fonts.bunker_body, &event.description)
                                            .color(self.theme.palette.text.color()),
                                    )
                                    .wrap(),
                                );
                            }
                        } else {
                            ui.label(
                                FontWeight::Regular
                                    .text(self.theme.fonts.body, "В этом раунде особых событий не произошло.")
                                    .color(self.theme.palette.muted.color()),
                            );
                        }
                        let past: Vec<_> = room.event_history.iter().rev()
                            .filter(|ev| room.current_event.as_ref().is_none_or(|cur| cur.title != ev.title || cur.round != ev.round))
                            .take(5)
                            .collect();
                        if !past.is_empty() {
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);
                            ui.label(
                                FontWeight::SemiBold
                                    .text(self.theme.fonts.small + 1.0, "Прошедшие события")
                                    .color(self.theme.palette.muted.color()),
                            );
                            ui.add_space(4.0);
                            for ev in past {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        FontWeight::SemiBold
                                            .text(self.theme.fonts.small + 1.0, format!("Раунд {} · {}", ev.round, ev.title))
                                            .color(self.theme.palette.text.color()),
                                    );
                                    if !ev.description.trim().is_empty() {
                                        ui.label(
                                            RichText::new(format!("— {}", ev.description))
                                                .size(self.theme.fonts.small + 1.0)
                                                .color(self.theme.palette.muted.color()),
                                        );
                                    }
                                });
                            }
                        }
                    });
            });
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

    fn edit_card(&mut self, player_id: &str, kind: &str, card: Option<&Card>) {
        self.editor = Some((player_id.to_owned(), kind.to_owned()));
        self.editor_value = card
            .filter(|card| card.items.is_none())
            .map(|card| card.value.clone())
            .unwrap_or_default();
        self.editor_health_degree = card.and_then(|card| card.health_degree.clone()).unwrap_or_default();
        self.transfer_ids.clear();
        self.transfer_target = None;
    }

    fn inventory_items(&mut self, ui: &mut egui::Ui, player_id: &str, kind: &str) -> bool {
        let Some(room) = &self.room else { return false; };
        let Some(items) = room.players.iter().find(|player| player.id == player_id)
            .and_then(|player| player.revealed_card(kind))
            .and_then(|card| card.items.as_deref()) else { return false; };

        ui.label(FontWeight::SemiBold.text(self.theme.fonts.body, "Предметы в ячейке"));
        if items.is_empty() {
            ui.label("Пусто");
            return true;
        }

        egui::ScrollArea::vertical().id_salt(("transfer-items", player_id, kind))
            .max_height(160.0).show(ui, |ui| {
                for item in items {
                    ui.horizontal_top(|ui| {
                        let mut ids: Vec<_> = self.transfer_ids.split(',').map(str::trim)
                            .filter(|id| !id.is_empty()).collect();
                        let mut checked = ids.contains(&item.id.as_str());
                        if ui.checkbox(&mut checked, &item.id).changed() {
                            ids.retain(|id| *id != item.id.as_str());
                            if checked { ids.push(&item.id); }
                            self.transfer_ids = ids.join(", ");
                        }
                        ui.add(egui::Label::new(&item.value).wrap());
                    });
                }
            });

        ui.add(egui::TextEdit::singleline(&mut self.transfer_ids).hint_text("ID предметов через запятую")
            .char_limit(600).desired_width(f32::INFINITY));
        true
    }

    fn edit_health_degree(&mut self, ui: &mut egui::Ui, player_id: &str) {
        if !self.room.as_ref().is_some_and(|room| room.can_set_health_degree) {
            ui.label("Для изменения степени здоровья нужно обновить сервер");
            return;
        }
        ui.label("Степень здоровья");
        ui.add(egui::TextEdit::singleline(&mut self.editor_health_degree)
            .hint_text("Например, лёгкая степень").char_limit(160).desired_width(450.0));
        if ui.add_enabled(self.online, animated_button(egui::Button::new("Сохранить степень")))
            .on_hover_text("Пустое поле убирает степень").clicked()
        {
            self.send(json!({"type":"host_set_health_degree","playerId":player_id,"degree":self.editor_health_degree}));
            self.editor = None;
        }
    }

    fn card_exchange(&mut self, ui: &mut egui::Ui, player_id: &str, kind: &str, inventory: bool) {
        let Some(room) = &self.room else { return; };
        let can_swap = room.can_swap_cards;
        let can_remove = room.host_controls.is_some();
        let can_transfer = inventory && !self.transfer_ids.trim().is_empty();
        if !can_swap && !inventory {
            ui.label("Для обмена характеристиками нужно обновить сервер");
            return;
        }
        let selected = room.players.iter().find(|player| self.transfer_target.as_deref() == Some(player.id.as_str()));
        egui::ComboBox::from_id_salt("inventory-recipient")
            .selected_text(selected.map_or("Выбрать другого игрока", |player| player.name.as_str()))
            .show_ui(ui, |ui| {
                for player in room.players.iter().filter(|player| player.id != player_id) {
                    if ui.selectable_label(self.transfer_target.as_ref() == Some(&player.id), &player.name).clicked() {
                        self.transfer_target = Some(player.id.clone());
                    }
                }
            });
        let enabled = self.online && room.players.iter().any(|player| self.transfer_target.as_ref() == Some(&player.id));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(enabled && can_swap, animated_button(egui::Button::new("Обменять карточки")))
                .on_hover_text("Поменять местами карточки этой категории целиком").clicked()
            {
                self.send(json!({"type":"host_swap_cards","playerId":player_id,"targetId":self.transfer_target,"kind":kind}));
                self.editor = None;
            }
            if inventory && ui.add_enabled(enabled && can_transfer, animated_button(self.theme.accent_button("Передать выбранное"))).clicked() {
                self.send(json!({"type":"host_transfer_items","playerId":player_id,"targetId":self.transfer_target,"kind":kind,"itemIds":self.transfer_ids}));
                self.transfer_ids.clear();
            }
            if inventory && can_remove && ui.add_enabled(self.online && can_transfer, animated_button(egui::Button::new("Удалить выбранное")))
                .on_hover_text("Можно отменить в панели ведущего").clicked()
            {
                self.send(json!({"type":"host_remove_items","playerId":player_id,"kind":kind,"itemIds":self.transfer_ids}));
                self.transfer_ids.clear();
            }
        });
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
        let mut keys = vec!["gender", "body", "personality", "profession", "health", "hobby", "fear", "equipment", "backpack", "fact"];
        let mut extra: Vec<_> = room.category_labels.keys().map(String::as_str)
            .filter(|kind| !keys.contains(kind) && !matches!(*kind, "ability" | "ability2" | "talent"))
            .collect();
        extra.sort_unstable();
        keys.extend(extra);
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
                        let text_width = (cell[0] - TABLE_CELL_PADDING * 2.0).max(1.0);
                        let cards = keys.iter().map(|&kind| {
                            let card = player.revealed_card(kind)?;
                            let galley = ui.painter().layout(
                                card.dossier_text().0.to_owned(),
                                FontWeight::Medium.font(text_size),
                                self.theme.palette.text.color(),
                                text_width,
                            );
                            Some((card, galley))
                        }).collect::<Vec<_>>();
                        let row_height = cards.iter().flatten().map(|(_, galley)| galley.size().y + TABLE_CELL_PADDING * 2.0)
                            .fold(cell[1], f32::max);
                        let fill = if player.active {
                            Color32::TRANSPARENT
                        } else {
                            self.theme.palette.surface.color()
                        };
                        table_cell(ui, [player_width, row_height], line, fill, |ui| {
                            let avatar_size = (35.0 * scale).min(ui.available_height());
                            self.avatar_widget(ui, player.avatar(), false, avatar_size);
                            let rect = ui.max_rect();
                            let left = ui.next_widget_position().x + 8.0 * scale;
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
                                    action_ui.spacing_mut().button_padding.x = 0.0;
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
                        for (&kind, entry) in keys.iter().zip(cards) {
                            table_cell(ui, [cell[0], row_height], line, fill, |ui| {
                                let rect = ui.max_rect();
                                let card = entry.map(|(card, galley)| {
                                    ui.add(egui::Label::new(galley)).on_hover_text(&card.value);
                                    card
                                });
                                if host {
                                    let edit_size = (18.0 * scale).min(rect.width() * 0.4);
                                    let edit_rect = egui::Rect::from_center_size(
                                        egui::pos2(rect.right() - edit_size / 2.0, rect.center().y),
                                        egui::Vec2::splat(edit_size),
                                    );
                                    let icon = self.artwork.button(Icon::Edit, &self.theme.icons.edit, edit_size * 0.72).frame(false);
                                    let response = ui.place(edit_rect, |ui: &mut egui::Ui| {
                                        ui.multiply_opacity(0.5);
                                        ui.add_sized(edit_rect.size(), animated_button(icon))
                                    });
                                    if response.on_hover_text("Изменить карточку").clicked() {
                                        self.edit_card(&player.id, kind, card);
                                    }
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
        let jump_to_vote = std::mem::take(&mut self.pending_vote_jump);
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
            let body_width = game_body_width(ui.available_width());
            let viewer_active = room.players.iter().any(|p| p.id == room.you && p.active);
            ui.add_space(20.0);
            let bottom_top = ui.next_widget_position().y;
            if jump_to_vote {
                scroll_target = Some(bottom_top);
            }
            ui.vertical_centered(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(body_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(body_width);
                    if room.vote_open || !room.vote_results.is_empty() {
                        let border = if room.vote_open {
                            self.theme.palette.accent.color()
                        } else {
                            self.theme.palette.outline.color()
                        };
                        egui::Frame::new()
                            .fill(self.theme.palette.panel.color())
                            .stroke(Stroke::new(1.0_f32, border))
                            .corner_radius(6u8)
                            .inner_margin(egui::Margin::symmetric(16, 14))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(
                                        FontWeight::Bold
                                            .text(
                                                self.theme.fonts.button + 2.0,
                                                if room.vote_candidates.is_some() {
                                                    "Переголосование"
                                                } else {
                                                    "Голосование"
                                                },
                                            )
                                            .color(self.theme.palette.accent_title.color()),
                                    );
                                    if room.vote_open {
                                        ui.label(
                                            FontWeight::Medium
                                                .text(
                                                    self.theme.fonts.small + 1.0,
                                                    format!("· получено голосов: {}", room.ballots_count),
                                                )
                                                .color(self.theme.palette.muted.color()),
                                        );
                                    } else if room.vote_tied {
                                        ui.label(
                                            FontWeight::Bold
                                                .text(self.theme.fonts.small + 1.0, "· Ничья по голосам!")
                                                .color(self.theme.palette.negative.color()),
                                        );
                                    }
                                });
                                ui.add_space(8.0);
                                if room.vote_open {
                                    if !viewer_active {
                                        ui.label(
                                            RichText::new("Выбывшие участники не могут голосовать.")
                                                .color(self.theme.palette.muted.color()),
                                        );
                                    } else {
                                        let candidates: Vec<_> = room.players.iter().filter(|p| {
                                            p.active
                                                && p.id != room.you
                                                && room.vote_candidates.as_ref().is_none_or(|list| list.contains(&p.id))
                                        }).collect();
                                        if candidates.is_empty() {
                                            ui.label(
                                                RichText::new("Нет доступных кандидатов для вашего голоса.")
                                                    .color(self.theme.palette.muted.color()),
                                            );
                                        } else {
                                            ui.horizontal_wrapped(|ui| {
                                                for p in candidates {
                                                    let chosen = room.your_vote.as_deref() == Some(p.id.as_str());
                                                    let label = if chosen {
                                                        format!("✓ {}", p.name)
                                                    } else {
                                                        format!("Против {}", p.name)
                                                    };
                                                    let btn = if chosen {
                                                        self.theme.accent_button(&label)
                                                    } else {
                                                        egui::Button::new(label)
                                                    };
                                                    if ui.add_enabled(self.online, animated_button(btn)).clicked() {
                                                        self.send(json!({"type":"vote","target":p.id}));
                                                    }
                                                }
                                            });
                                        }
                                    }
                                }
                                if !room.vote_results.is_empty() {
                                    if room.vote_open {
                                        ui.add_space(8.0);
                                        ui.separator();
                                        ui.add_space(6.0);
                                    }
                                    let max_votes = room.vote_results.iter().map(|r| r.votes).max().unwrap_or(1).max(1);
                                    for result in &room.vote_results {
                                        let ratio = result.votes as f32 / max_votes as f32;
                                        ui.horizontal(|ui| {
                                            ui.label(FontWeight::SemiBold.text(self.theme.fonts.body, &result.name));
                                            ui.label(
                                                RichText::new(format!("— {} голос(ов)", result.votes))
                                                    .color(self.theme.palette.accent_title.color()),
                                            );
                                        });
                                        let (bar_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(ui.available_width(), 6.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(bar_rect, 3u8, self.theme.palette.surface.color());
                                        let fill_rect = egui::Rect::from_min_size(
                                            bar_rect.min,
                                            egui::vec2(bar_rect.width() * ratio, bar_rect.height()),
                                        );
                                        ui.painter().rect_filled(fill_rect, 3u8, self.theme.palette.accent.color());
                                        ui.add_space(4.0);
                                    }
                                    if !room.vote_open && room.you == room.host_id && room.host_controls.is_some() {
                                        ui.add_space(6.0);
                                        ui.horizontal_wrapped(|ui| {
                                            if room.vote_tied {
                                                if ui.add_enabled(self.online, animated_button(self.theme.accent_button("Переголосование среди лидеров"))).clicked() {
                                                    self.send(json!({"type":"host_action","action":"runoff_vote"}));
                                                }
                                            } else if let Some(leader) = room.vote_results.first() {
                                                let leader_active = leader.player_id.as_deref().is_some_and(|id| {
                                                    room.players.iter().any(|p| p.id == id && p.active)
                                                });
                                                if leader_active
                                                    && ui.add_enabled(
                                                        self.online,
                                                        animated_button(self.theme.accent_button(&format!("Исключить лидера: {}", leader.name))),
                                                    ).clicked()
                                                {
                                                    self.send(json!({"type":"host_action","action":"exclude_vote_leader"}));
                                                }
                                            }
                                        });
                                    }
                                }
                            });
                        ui.add_space(14.0);
                    }

                    egui::Frame::new()
                        .fill(self.theme.palette.panel.color())
                        .stroke(Stroke::new(1.0_f32, self.theme.palette.button.color()))
                        .corner_radius(6u8)
                        .inner_margin(egui::Margin::symmetric(16, 14))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    FontWeight::Bold
                                        .text(self.theme.fonts.button + 1.0, "Кубики и журнал партии")
                                        .color(self.theme.palette.text.color()),
                                );
                                ui.add_space(8.0);
                                if let Some(sides) = crate::dice::roll_buttons(ui, self.online) {
                                    self.send(json!({"type":"roll","sides":sides}));
                                }
                            });
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(6.0);
                            if room.log.is_empty() {
                                ui.label(RichText::new("Событий пока нет.").color(self.theme.palette.muted.color()));
                            } else {
                                for event in room.log.iter().rev().take(12) {
                                    ui.label(RichText::new(format!("• {event}")).color(self.theme.palette.muted.color()));
                                }
                            }
                        });
                });
            });
            ui.add_space(88.0);
            // Allow the final section to reach the same top alignment as earlier sections.
            let remaining = last_section_top + viewport.height() - ui.next_widget_position().y;
            if remaining > 0.0 { ui.add_space(remaining); }
            if let Some(top) = scroll_target {
                scroll_to_section(ui, top);
            }
        });
    }
    fn ui_finished(&mut self, ui: &mut egui::Ui, room: &Room) {
        let is_host = room.you == room.host_id;
        let survivors: Vec<_> = room.players.iter().filter(|p| p.active).collect();
        let eliminated: Vec<_> = room.players.iter().filter(|p| !p.active).collect();
        ui.add_space(56.0);
        heading(ui, &self.theme, if room.stage == "closed" { "Комната закрыта ведущим" } else { "Партия завершена" });
        let card_width = (ui.available_width() * 0.62).clamp(320.0, 560.0);
        ui.vertical_centered(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(card_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(card_width);
                egui::Frame::new()
                    .fill(self.theme.palette.panel.color())
                    .stroke(Stroke::new(1.0_f32, self.theme.palette.outline.color()))
                    .corner_radius(6u8)
                    .inner_margin(egui::Margin::symmetric(18, 16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            FontWeight::Bold
                                .text(self.theme.fonts.button + 1.0, format!("Попали в бункер ({} из {})", survivors.len(), room.seats))
                                .color(self.theme.palette.positive.color()),
                        );
                        ui.add_space(8.0);
                        if survivors.is_empty() {
                            ui.label(RichText::new("Никто не остался в бункере.").color(self.theme.palette.muted.color()));
                        } else {
                            for p in &survivors {
                                ui.horizontal(|ui| {
                                    self.avatar_widget(ui, p.avatar(), p.id == room.you, 30.0);
                                    ui.label(FontWeight::Bold.text(self.theme.fonts.body, &p.name));
                                    if let Some(prof) = p.revealed_card("profession") {
                                        ui.label(
                                            RichText::new(format!("· {}", prof.dossier_text().0))
                                                .color(self.theme.palette.muted.color()),
                                        );
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        }
                        if !eliminated.is_empty() {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(6.0);
                            ui.label(
                                FontWeight::SemiBold
                                    .text(self.theme.fonts.small + 1.0, format!("Выбыли ({})", eliminated.len()))
                                    .color(self.theme.palette.muted.color()),
                            );
                            ui.horizontal_wrapped(|ui| {
                                for p in &eliminated {
                                    ui.label(
                                        RichText::new(format!("○ {}", p.name))
                                            .color(self.theme.palette.muted.color()),
                                    );
                                }
                            });
                        }
                    });
                ui.add_space(16.0);
                ui.horizontal_wrapped(|ui| {
                    if is_host && room.stage == "finished" {
                        if ui.add_enabled(self.online, animated_button(self.theme.accent_button("Начать заново"))).clicked() {
                            self.send(json!({"type":"host_action","action":"restart"}));
                        }
                    }
                    if ui.add(animated_button(egui::Button::new("Оценка выживаемости · WIP"))).clicked() {
                        self.show_survivability = true;
                    }
                    if ui.add(animated_button(egui::Button::new("В главное меню"))).clicked() {
                        self.leave_room();
                    }
                });
            });
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
                            if self.room_packs.is_empty() {
                                ui.label("Добавь JSON или ZIP в папку packs")
                                    .on_hover_text(crate::packs::directory().display().to_string());
                            }
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
            right.label(RichText::new("Вероятность случайного события при смене раунда (из пака или базового набора).")
                .color(self.theme.palette.muted.color()).size(self.theme.fonts.small));
            let remaining = (list_rect.bottom() - right.next_widget_position().y).max(0.0);
            right.add_space(remaining);
            right.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let available = self.room_packs.iter().any(|entry| entry.active() && entry.error().is_none())
                    && self.pending.is_none();
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
            self.room_packs = crate::packs::library();
            let errors = self.room_packs.iter().filter(|entry| entry.error().is_some()).count();
            self.notify(if errors == 0 { "Паки обновлены".into() } else { format!("Паки обновлены. Ошибок: {errors}") });
        }
        if create {
            match crate::packs::selected_payload(&self.room_packs) {
                Ok((json, zip)) => {
                    self.pack_json = Some(json);
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
                        let columns = usize::from(columns);
                        let ids = self.avatars.ids();
                        let mut picked = None;
                        egui::ScrollArea::vertical()
                            .id_salt("avatar-grid")
                            .max_height((ctx.content_rect().height() * 0.70).min(420.0))
                            .auto_shrink([false, true])
                            .show_rows(ui, side, ids.len().div_ceil(columns), |ui, rows| {
                                for row in rows {
                                    let avatars = &ids[row * columns..((row + 1) * columns).min(ids.len())];
                                    let width = side * avatars.len() as f32 + gap * (avatars.len() - 1) as f32;
                                    centered_row(ui, width, side, |ui| {
                                        for &id in avatars {
                                            if self.avatar_widget(ui, id, self.avatar == Some(id), side)
                                                .on_hover_text(format!("Аватар {id}")).clicked()
                                            {
                                                picked = Some(id);
                                            }
                                        }
                                    });
                                }
                            });
                        if let Some(id) = picked {
                            self.avatar = Some(id);
                            self.show_avatar_picker = false;
                            if self.profile_ready { self.save_config(); }
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
                                let valid = code.len() == 8 && code.bytes().all(|b| matches!(b, b'A'..=b'Z' | b'2'..=b'9'));
                                if valid {
                                    self.join_code = code.clone();
                                    self.start_request(Some(code));
                                    self.show_join = false;
                                } else {
                                    self.notify("Нужен 8-символьный код (латинские буквы и цифры 2–9)");
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
                    ui.label("Никнейм:");
                    ui.add(egui::TextEdit::singleline(&mut self.nickname).char_limit(24).desired_width(430.0));
                    ui.add_space(6.0);
                    ui.label("Адрес сервера:");
                    ui.add(egui::TextEdit::singleline(&mut self.server_url).desired_width(430.0));

                    ui.add_space(6.0);
                    if ui.add(animated_button(self.theme.accent_button("Сохранить"))).clicked() {
                        let name = self.nickname.trim();
                        let len = name.chars().count();
                        if !(2..=24).contains(&len) {
                            self.notify("Ник должен содержать от 2 до 24 символов");
                        } else {
                            self.nickname = name.to_owned();
                            self.save_config();
                        }
                    }
                    ui.separator();
                    if ui.add(animated_button(egui::Button::new("Перезагрузить оформление"))).on_hover_text("Ctrl+Shift+R").clicked() {
                        self.reload_appearance(ctx);
                    }
                    ui.label(portable_path("theme.json").parent().unwrap_or(Path::new(".")).display().to_string());
                    ui.separator();
                    ui.label(format!("Версия {VERSION}"));
                    if ui.add_enabled(self.updater.can_check(), animated_button(egui::Button::new("Проверить обновления"))).clicked() {
                        self.updater.check(true);
                        self.notify("Проверяем обновления…");
                    }
                });
            });
            self.show_settings = open;
        }
        {
            let mut open = self.show_host_tools;
            if let Some(room) = self.room.as_ref() {
                if room.stage == "lobby" {
                    let host = room.you == room.host_id;
                    let mut seats = room.seats;
                    let mut chance = room.event_chance;
                    let title = format!("Пак: {}", room.pack_name);
                    egui::Window::new("Параметры комнаты").anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                        .movable(false).resizable(false).collapsible(false).open(&mut open)
                        .default_width(430.0).show(ctx, |ui| {
                        ui.label(title);
                        if !host { return; }
                        ui.horizontal(|ui| {
                            ui.label("Мест в бункере");
                            if ui.add(egui::DragValue::new(&mut seats).range(1..=15)).changed() {
                                self.send(json!({"type":"set_seats","seats":seats}));
                            }
                        });
                        if ui.add(egui::Slider::new(&mut chance, 0..=100).text("Шанс событий (%)")).changed() {
                            self.send(json!({"type":"set_event_chance","chance":chance}));
                        }
                        if ui.add(animated_button(egui::Button::new("Выбрать другой пак…"))).clicked() { self.pick_pack(); }
                        if ui.add_enabled(self.pack_json.is_some(), animated_button(egui::Button::new("Применить выбранный пак"))).clicked() { self.apply_pack(); }
                    });
                } else if room.you == room.host_id && matches!(room.stage.as_str(), "playing" | "finished") {
                    match self.host_panel.show(ctx, &self.theme, room, &mut open, self.online) {
                        Some(PanelAction::Send(command)) => self.send(command),
                        Some(PanelAction::EditCard { player, kind }) => {
                            let card = room.players.iter().find(|p| p.id == player)
                                .and_then(|p| p.revealed_card(&kind)).cloned();
                            self.edit_card(&player, &kind, card.as_ref());
                        }
                        None => {}
                    }
                } else {
                    open = false;
                }
            } else {
                open = false;
            }
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
                .default_width(470.0).max_width(520.0).show(ctx, |ui| {
                if !active { ui.disable(); }
                let name = self.room.as_ref().and_then(|room| room.category_labels.get(&kind))
                    .map(String::as_str).unwrap_or_else(|| category_name(&kind));
                let player_name = self.room.as_ref().and_then(|room| room.players.iter().find(|p| p.id == player))
                    .map_or("Игрок", |p| p.name.as_str());
                ui.label(FontWeight::SemiBold.text(self.theme.fonts.body, format!("{player_name} · {name}")));
                let inventory = self.inventory_items(ui, &player, &kind);
                self.card_exchange(ui, &player, &kind, inventory);
                ui.separator();
                if kind == "health" {
                    self.edit_health_degree(ui, &player);
                    ui.separator();
                }
                ui.add(egui::TextEdit::multiline(&mut self.editor_value)
                    .hint_text(if inventory { "Название или описание предмета" } else { "Текст характеристики" })
                    .desired_rows(3).desired_width(450.0));
                ui.horizontal_wrapped(|ui| {
                    let has_text = self.online && !self.editor_value.trim().is_empty();
                    if inventory && ui.add_enabled(has_text, animated_button(egui::Button::new("Добавить предмет"))).clicked() {
                        self.send(json!({"type":"host_add_item","playerId":player,"kind":kind,"value":self.editor_value}));
                        self.editor_value.clear();
                    }
                    let apply = if inventory { "Заменить содержимое" } else { "Применить текст" };
                    if ui.add_enabled(has_text, animated_button(egui::Button::new(apply))).clicked() {
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
                    self.host_overlay(ctx, &room);
                    match room.stage.as_str() {
                        "lobby" => self.ui_lobby(ui, &room),
                        "playing" => {
                            self.ui_game(ui, &room);
                            self.game_navigation(ctx);
                            egui::Area::new(egui::Id::new("leave-game"))
                                .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
                                .order(egui::Order::Foreground)
                                .movable(false).show(ctx, |ui| {
                                    if self.confirm_leave {
                                        egui::Frame::new()
                                            .fill(self.theme.palette.panel.color())
                                            .stroke(Stroke::new(1.0_f32, self.theme.palette.negative.color()))
                                            .corner_radius(6u8)
                                            .inner_margin(egui::Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        FontWeight::SemiBold
                                                            .text(self.theme.fonts.small + 1.0, "Выйти из партии?")
                                                            .color(self.theme.palette.text.color()),
                                                    );
                                                    let yes = egui::Button::new(
                                                        FontWeight::SemiBold
                                                            .text(self.theme.fonts.small + 1.0, "Да")
                                                            .color(self.theme.palette.negative.color()),
                                                    );
                                                    if ui.add(animated_button(yes)).clicked() {
                                                        self.leave_room();
                                                    }
                                                    if ui.add(animated_button(egui::Button::new("Нет"))).clicked() {
                                                        self.confirm_leave = false;
                                                    }
                                                });
                                            });
                                    } else if ui.add(animated_button(egui::Button::new("В меню"))).clicked() {
                                        self.confirm_leave = true;
                                    }
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
        let can_install = self.room.is_none() && self.pending.is_none() && self.room_deadline.is_none()
            && !self.show_create && !self.show_join && !self.show_settings && !self.show_avatar_picker;
        if let Some(message) = self.updater.poll(ctx, can_install) {
            self.notify(message);
        }
        self.show_toast(ctx);
        self.dice.show(ctx, &self.theme);
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
