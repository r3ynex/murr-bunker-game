use crate::model::{category_name, LocalConfig, Room, Session};
use crate::network;
use eframe::egui;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn portable_path(filename: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join(filename)))
        .unwrap_or_else(|| PathBuf::from(filename))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay { None, Join, Create, Settings, Avatars, GameMaster, Survival }

#[derive(Clone)]
struct GmTarget {
    player_id: String,
    kind: String,
    value: String,
    revealed: bool,
}

fn accent() -> egui::Color32 { egui::Color32::from_rgb(242, 154, 65) }
fn avatar_color(i: usize) -> egui::Color32 {
    const COLORS: [[u8; 3]; 8] = [
        [132, 70, 104], [63, 112, 129], [125, 104, 56], [76, 115, 78],
        [136, 77, 69], [91, 76, 139], [73, 120, 114], [132, 99, 72],
    ];
    let c = COLORS[i % COLORS.len()];
    egui::Color32::from_rgb(c[0], c[1], c[2])
}
fn avatar_chip(ui: &mut egui::Ui, i: usize, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    ui.painter().rect_filled(rect, 2.0, avatar_color(i));
    ui.painter().text(
        rect.center(), egui::Align2::CENTER_CENTER, format!("{:02}", i + 1),
        egui::FontId::proportional((size * 0.35).min(24.0)), egui::Color32::WHITE,
    );
    response
}
fn section_heading(ui: &mut egui::Ui, jump: &mut Option<usize>, i: usize, label: &str) {
    ui.add_space(26.0);
    let response = ui.heading(label);
    if *jump == Some(i) {
        ui.scroll_to_rect(response.rect, Some(egui::Align::TOP));
        *jump = None;
    }
    ui.add_space(6.0);
    ui.separator();
}

pub struct BunkerApp {
    server_url: String,
    nickname: String,
    avatar_id: usize,
    profile_complete: bool,
    overlay: Overlay,
    jump_to: Option<usize>,
    gm_target: Option<GmTarget>,
    apply_pack_on_connect: bool,
    leave_requested: bool,
    event_probability: i32,
    join_code: String,
    status: String,
    session: Option<Session>,
    room: Option<Room>,
    pending: Option<Receiver<Result<Session, String>>>,
    sender: Option<WsSender>,
    receiver: Option<WsReceiver>,
    online: bool,
    dice_sides: u32,
    pack_json: String,
    pack_file: String,
}

impl BunkerApp {
    pub fn new() -> Self {
        let config = fs::read_to_string(portable_path("murr-config.json"))
            .ok()
            .and_then(|json| serde_json::from_str::<LocalConfig>(&json).ok())
            .unwrap_or_default();
        Self {
            server_url: config.server_url,
            nickname: config.nickname,
            avatar_id: config.avatar_id % 24,
            profile_complete: config.profile_complete,
            overlay: Overlay::None,
            jump_to: None,
            gm_target: None,
            apply_pack_on_connect: false,
            leave_requested: false,
            event_probability: 50,
            join_code: String::new(),
            status: "Запусти локальный сервер или укажи адрес Cloudflare Worker".into(),
            session: None,
            room: None,
            pending: None,
            sender: None,
            receiver: None,
            online: false,
            dice_sides: 6,
            pack_json: include_str!("../packs/demo.json").to_owned(),
            pack_file: "my-pack.json".into(),
        }
    }

    fn save_config(&mut self) {
        let config = LocalConfig {
            server_url: self.server_url.trim().trim_end_matches('/').to_owned(),
            nickname: self.nickname.trim().to_owned(),
            avatar_id: self.avatar_id,
            profile_complete: self.profile_complete,
        };
        match serde_json::to_string_pretty(&config)
            .map_err(|err| err.to_string())
            .and_then(|text| fs::write(portable_path("murr-config.json"), text).map_err(|err| err.to_string()))
        {
            Ok(()) => self.status = "Настройки сохранены рядом с приложением".into(),
            Err(err) => self.status = format!("Не удалось сохранить настройки: {err}"),
        }
    }

    fn start_request(&mut self, code: Option<String>) {
        if self.pending.is_some() {
            return;
        }
        self.sender = None;
        self.receiver = None;
        self.online = false;
        self.room = None;
        self.session = None;
        self.status = "Связываемся с сервером…".into();
        self.pending = Some(network::request_room(
            self.server_url.clone(),
            self.nickname.clone(),
            self.avatar_id,
            code,
        ));
    }

    fn connect_session(&mut self, session: Session) {
        match network::connect(&self.server_url, &session) {
            Ok((sender, receiver)) => {
                self.status = format!("Комната {} — подключаемся…", session.code);
                self.session = Some(session);
                self.sender = Some(sender);
                self.receiver = Some(receiver);
                self.online = false;
            }
            Err(err) => self.status = err,
        }
    }

    fn send(&mut self, value: Value) {
        if !self.online {
            self.status = "Соединение отсутствует. Нажми «Переподключиться»".into();
            return;
        }
        if let Some(sender) = &mut self.sender {
            sender.send(WsMessage::Text(value.to_string().into()));
        }
    }

    fn poll_network(&mut self) {
        let reply = self.pending.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(reply) = reply {
            self.pending = None;
            match reply {
                Ok(session) => self.connect_session(session),
                Err(err) => self.status = err,
            }
        }
        let events: Vec<WsEvent> = match self.receiver.as_ref() {
            Some(rx) => std::iter::from_fn(|| rx.try_recv()).take(100).collect(),
            None => Vec::new(),
        };
        for event in events {
            match event {
                WsEvent::Opened => {
                    self.online = true;
                    self.status = "Связь с комнатой установлена".into();
                }
                WsEvent::Closed => {
                    self.online = false;
                    if self.leave_requested {
                        self.leave_requested = false;
                        self.session = None;
                        self.room = None;
                        self.sender = None;
                        self.receiver = None;
                        self.status = "Ты покинул комнату".into();
                    } else {
                        self.status = "Связь с комнатой потеряна".into();
                    }
                }
                WsEvent::Error(err) => {
                    self.online = false;
                    self.status = format!("Ошибка соединения: {err}");
                }
                WsEvent::Message(WsMessage::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<Value>(&text) {
                        match message.get("type").and_then(Value::as_str) {
                            Some("state") => {
                                if let Some(data) = message.get("data") {
                                    match serde_json::from_value::<Room>(data.clone()) {
                                        Ok(room) => {
                                            let send_pack = self.apply_pack_on_connect && self.online
                                                && room.you == room.host_id && room.stage == "lobby";
                                            self.room = Some(room);
                                            if send_pack {
                                                self.apply_pack_on_connect = false;
                                                if let Ok(pack) = serde_json::from_str::<Value>(&self.pack_json) {
                                                    self.send(json!({"type":"set_pack","pack":pack}));
                                                }
                                            }
                                        },
                                        Err(err) => self.status = format!("Ошибка данных комнаты: {err}"),
                                    }
                                }
                            }
                            Some("error") => {
                                self.status = message.get("message").and_then(Value::as_str)
                                    .unwrap_or("Ошибка сервера").to_owned();
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn load_pack(&mut self) {
        let path = portable_path(&self.pack_file);
        match fs::read_to_string(&path) {
            Ok(text) => {
                self.pack_json = text;
                self.status = format!("Загружен пак: {}", path.display());
            }
            Err(err) => self.status = format!("Не удалось прочитать пак: {err}"),
        }
    }

    fn save_pack(&mut self) {
        let path = portable_path(&self.pack_file);
        match serde_json::from_str::<Value>(&self.pack_json) {
            Err(err) => self.status = format!("Ошибка JSON: {err}"),
            Ok(pack) => match serde_json::to_string_pretty(&pack)
                .map_err(|err| err.to_string())
                .and_then(|text| fs::write(&path, text).map_err(|err| err.to_string()))
            {
                Ok(()) => self.status = format!("Пак сохранён: {}", path.display()),
                Err(err) => self.status = format!("Не удалось сохранить пак: {err}"),
            }
        }
    }

    fn default_nickname() -> String {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos()).unwrap_or(8549);
        format!("Выживший #{}", 100 + (timestamp % 9900))
    }

    fn request_leave(&mut self) {
        if self.room.as_ref().is_some_and(|r| r.stage != "lobby") {
            self.status = "После старта покинуть комнату через меню пока нельзя (WIP)".into();
            return;
        }
        if self.online {
            self.send(json!({"type":"leave"}));
            self.leave_requested = true;
        } else {
            self.room = None;
            self.session = None;
            self.sender = None;
            self.receiver = None;
            self.status = "Комната закрыта на этом устройстве".into();
        }
    }

    fn profile_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.26).min(170.0));
        ui.vertical_centered(|ui| {
            ui.heading(egui::RichText::new("Добро пожаловать в Бункер!").size(34.0).color(accent()));
            ui.add_space(10.0);
            ui.label("Для начала выбери ник и аватар (можно изменить позже)");
            ui.add_space(28.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 600.0).max(0.0) * 0.5);
                if avatar_chip(ui, self.avatar_id, 58.0).clicked() {
                    self.overlay = Overlay::Avatars;
                }
                ui.add_sized([350.0, 58.0],
                    egui::TextEdit::singleline(&mut self.nickname).hint_text("Выживший #8549"));
                let next = egui::Button::new(egui::RichText::new("➜").size(24.0).color(egui::Color32::BLACK))
                    .fill(accent());
                if ui.add_sized([70.0, 58.0], next).clicked() {
                    if self.nickname.trim().is_empty() { self.nickname = Self::default_nickname(); }
                    if !(2..=24).contains(&self.nickname.chars().count()) {
                        self.status = "Ник должен содержать от 2 до 24 символов".into();
                    } else {
                        self.profile_complete = true;
                        self.save_config();
                    }
                }
            });
        });
    }

    fn main_menu_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space((ui.available_height() * 0.30).min(185.0));
        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 325.0).max(0.0) * 0.5);
                if avatar_chip(ui, self.avatar_id, 64.0).clicked() { self.overlay = Overlay::Avatars; }
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    ui.heading(&self.nickname);
                    ui.small("Нажми на аватар, чтобы изменить его");
                });
            });
            ui.add_space(75.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 650.0).max(0.0) * 0.5);
                if ui.add_sized([205.0, 52.0], egui::Button::new("Присоединиться к игре")).clicked() {
                    self.overlay = Overlay::Join;
                }
                if ui.add_sized([205.0, 52.0],
                    egui::Button::new("Создать игру").fill(accent())).clicked() {
                    self.overlay = Overlay::Create;
                }
                if ui.add_sized([205.0, 52.0], egui::Button::new("Настройки")).clicked() {
                    self.overlay = Overlay::Settings;
                }
            });
        });
    }

    fn show_overlay(&mut self, ctx: &egui::Context) {
        let overlay = self.overlay;
        if overlay == Overlay::None { return; }
        let title = match overlay {
            Overlay::Join => "Присоединиться к игре",
            Overlay::Create => "Создать игру",
            Overlay::Settings => "Настройки — WIP",
            Overlay::Avatars => "Выберите аватар",
            Overlay::GameMaster => "Редактирование характеристики — ГМ",
            Overlay::Survival => "Оценка выживаемости — WIP",
            Overlay::None => "",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(if overlay == Overlay::Avatars { 530.0 } else { 460.0 })
            .show(ctx, |ui| {
                match overlay {
                    Overlay::Join => {
                        ui.label("Введи код комнаты:");
                        ui.add_sized([360.0, 42.0],
                            egui::TextEdit::singleline(&mut self.join_code)
                                .hint_text("8 символов"));
                        if ui.button("Присоединиться к игре").clicked() {
                            let code = self.join_code.trim().to_ascii_uppercase();
                            if code.len() == 8 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
                                self.start_request(Some(code));
                                self.overlay = Overlay::None;
                            } else {
                                self.status = "Код комнаты должен состоять из 8 символов".into();
                            }
                        }
                    }
                    Overlay::Create => {
                        ui.label(egui::RichText::new("Игровой пак").strong().color(accent()));
                        ui.label("Доступен текущий JSON-пак. Автоматическая загрузка ZIP с изображениями — следующая задача.");
                        egui::CollapsingHeader::new("Выбрать или изменить JSON-пак")
                            .show(ui, |ui| self.pack_editor(ui, false));
                        ui.add_space(12.0);
                        ui.label("Вероятность события (%) — WIP, сервер пока её не использует");
                        ui.add(egui::Slider::new(&mut self.event_probability, 0..=100));
                        ui.add_space(14.0);
                        if ui.add_sized([210.0, 44.0], egui::Button::new("Создать комнату").fill(accent())).clicked() {
                            self.apply_pack_on_connect = true;
                            self.start_request(None);
                            self.overlay = Overlay::None;
                        }
                    }
                    Overlay::Settings => {
                        ui.label("Адрес Cloudflare Worker:");
                        ui.text_edit_singleline(&mut self.server_url);
                        if ui.button("Сохранить адрес сервера").clicked() { self.save_config(); }
                        ui.separator();
                        ui.label("Графика, звук, язык, управление — UNDER CONSTRUCTION");
                    }
                    Overlay::Avatars => {
                        ui.label("Встроенные цветные заглушки. Картинки придут вместе с ресурсами игры.");
                        for row in 0..3 {
                            ui.horizontal(|ui| {
                                for col in 0..8 {
                                    let index = row * 8 + col;
                                    if avatar_chip(ui, index, 52.0).clicked() {
                                        self.avatar_id = index;
                                        self.save_config();
                                        self.overlay = Overlay::None;
                                    }
                                }
                            });
                        }
                    }
                    Overlay::GameMaster => {
                        if let Some(target) = self.gm_target.as_mut() {
                            ui.label(format!("Категория: {}", category_name(&target.kind)));
                            ui.label("Если карта закрыта для тебя, её прежнее значение не раскрывается.");
                            ui.add(egui::TextEdit::multiline(&mut target.value).desired_rows(4));
                            ui.checkbox(&mut target.revealed, "Открыть характеристику всем игрокам");
                        }
                        let can_apply = self.gm_target.as_ref()
                            .is_some_and(|x| !x.value.trim().is_empty());
                        if ui.add_enabled(can_apply, egui::Button::new("Сохранить изменение").fill(accent())).clicked() {
                            if let Some(target) = self.gm_target.take() {
                                self.send(json!({
                                    "type":"gm_set_card", "playerId":target.player_id,
                                    "kind":target.kind, "value":target.value,
                                    "revealed":target.revealed
                                }));
                            }
                            self.overlay = Overlay::None;
                        }
                    }
                    Overlay::Survival => {
                        ui.label("Здесь появятся общие шансы и объяснение факторов расчёта.");
                        ui.label("Сейчас реальной формулы выживаемости нет. Не показываем выдуманный процент.");
                    }
                    Overlay::None => {}
                }
                ui.add_space(10.0);
                if ui.small_button("Закрыть").clicked() { self.overlay = Overlay::None; }
            });
    }

    fn lobby_ui(&mut self, ui: &mut egui::Ui, room: &Room) {
        let host = room.you == room.host_id;
        let others_ready = room.players.iter().all(|p| p.id == room.host_id || p.ready);
        let own_ready = room.players.iter().find(|p| p.id == room.you).is_some_and(|p| p.ready);
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.heading(format!("Лобби: [{}]", room.code));
            ui.label(format!("Игроки: {} из 12", room.players.len()));
            ui.add_space(32.0);
        });
        let columns = if ui.available_width() > 930.0 { 3 } else { 2 };
        egui::Grid::new("lobby-players").num_columns(columns).spacing([34.0, 42.0]).show(ui, |ui| {
            for (i, p) in room.players.iter().enumerate() {
                ui.horizontal(|ui| {
                    let ready = p.ready || p.id == room.host_id;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(3.0, 56.0)),
                        0.0,
                        if ready { egui::Color32::from_rgb(45, 205, 100) }
                        else { egui::Color32::from_rgb(200, 48, 60) },
                    );
                    ui.add_space(7.0);
                    avatar_chip(ui, p.avatar_id, 56.0);
                    ui.vertical(|ui| {
                        if p.id == room.host_id {
                            ui.label(egui::RichText::new("♛ Владелец").color(accent()));
                        } else {
                            ui.label(egui::RichText::new(if p.ready {"● Готов"} else {"● Не готов"})
                                .color(if p.ready { egui::Color32::LIGHT_GREEN } else { egui::Color32::LIGHT_RED }));
                        }
                        ui.label(egui::RichText::new(&p.name).size(19.0).strong());
                    });
                });
                if (i + 1) % columns == 0 { ui.end_row(); }
            }
        });
        ui.add_space(45.0);
        ui.horizontal(|ui| {
            if ui.add_sized([165.0, 46.0], egui::Button::new("Покинуть игру")).clicked() {
                self.request_leave();
            }
            if host {
                if ui.add_enabled(others_ready && room.players.len() >= 2,
                    egui::Button::new("Начать игру").fill(egui::Color32::from_rgb(0, 140, 119)))
                    .clicked() { self.send(json!({"type":"start"})); }
            } else {
                let label = if own_ready { "Не готов" } else { "Готов" };
                if ui.add_sized([190.0, 46.0],
                    egui::Button::new(label).fill(egui::Color32::from_rgb(0, 140, 119)))
                    .clicked() { self.send(json!({"type":"set_ready","ready":!own_ready})); }
            }
            if ui.add_sized([165.0, 46.0], egui::Button::new("Настройки")).clicked() {
                self.overlay = Overlay::Settings;
            }
        });
        if host {
            if !others_ready { ui.colored_label(accent(), "Начало доступно, когда все остальные игроки готовы."); }
            ui.add_space(22.0);
            ui.label(format!("Пак: {}", room.pack_name));
            egui::CollapsingHeader::new("Управление комнатой / выбор пака (WIP)")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Мест в бункере:");
                        let mut seats = room.seats as i32;
                        if ui.add(egui::DragValue::new(&mut seats).range(1..=12)).changed() {
                            self.send(json!({"type":"set_seats","seats":seats}));
                        }
                    });
                    self.pack_editor(ui, true);
                });
        }
    }

    fn dossier_ui(&mut self, ui: &mut egui::Ui, room: &Room) {
        let own = room.players.iter().find(|p| p.id == room.you);
        let Some(player) = own else { ui.label("Досье недоступно"); return; };
        ui.heading(format!("Досье игрока [{}]", player.name));
        ui.add_space(12.0);
        let midpoint = (player.cards.len() + 1) / 2;
        ui.columns(2, |cols| {
            for (i, card) in player.cards.iter().enumerate() {
                let col = if i < midpoint { 0 } else { 1 };
                cols[col].horizontal_wrapped(|ui| {
                    let name = room.category_labels.get(&card.kind)
                        .map(String::as_str).unwrap_or_else(|| category_name(&card.kind));
                    ui.label(format!("{name}:"));
                    ui.label(egui::RichText::new(&card.value).color(accent()));
                    let symbol = if card.revealed { "🔓" } else { "🔒" };
                    let color = if card.revealed { egui::Color32::LIGHT_GREEN } else { egui::Color32::LIGHT_RED };
                    if ui.add(egui::Button::new(egui::RichText::new(symbol).color(color))
                        .frame(false)).on_hover_text("Открыть характеристику").clicked()
                        && !card.revealed && player.active {
                        self.send(json!({"type":"reveal","kind":card.kind}));
                    }
                });
                cols[col].separator();
            }
        });
    }

    fn game_table(&mut self, ui: &mut egui::Ui, room: &Room, abilities: bool) {
        let host = room.you == room.host_id;
        let mut kinds: Vec<String> = room.players.iter()
            .flat_map(|p| p.cards.iter().map(|c| c.kind.clone()))
            .filter(|k| (k == "ability") == abilities).collect();
        kinds.sort();
        kinds.dedup();
        if abilities && kinds.is_empty() { kinds.push("ability".to_owned()); }
        if !abilities {
            const PREFERRED: &[&str] = &[
                "gender", "body", "personality", "profession", "health",
                "hobby", "fear", "equipment", "backpack", "fact",
            ];
            kinds.sort_by_key(|k| PREFERRED.iter().position(|n| *n == k).unwrap_or(999));
        }
        egui::ScrollArea::horizontal().id_salt(if abilities { "ability-tbl" } else { "cards-tbl" })
            .show(ui, |ui| {
                egui::Grid::new(if abilities { "grid-ability" } else { "grid-cards" })
                    .striped(true).spacing([16.0, 14.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new("Игрок").strong());
                        for kind in &kinds {
                            let label = room.category_labels.get(kind).map(String::as_str)
                                .unwrap_or_else(|| category_name(kind));
                            ui.add_sized([130.0, 38.0], egui::Label::new(label).wrap());
                        }
                        ui.end_row();
                        for player in &room.players {
                            ui.horizontal(|ui| {
                                avatar_chip(ui, player.avatar_id, 27.0);
                                ui.label(&player.name);
                                if !player.active { ui.colored_label(egui::Color32::LIGHT_RED, "×"); }
                            });
                            for kind in &kinds {
                                let card = player.cards.iter().find(|c| c.kind == *kind);
                                ui.horizontal_wrapped(|ui| {
                                    let text = card.filter(|c| c.revealed).map(|c| c.value.as_str()).unwrap_or("—");
                                    ui.add_sized([124.0, 36.0], egui::Label::new(text).wrap());
                                    if host {
                                        let edit = ui.small_button("✎")
                                            .on_hover_text("Изменить характеристику (ГМ)");
                                        if edit.clicked() {
                                            self.gm_target = Some(GmTarget {
                                                player_id: player.id.clone(),
                                                kind: kind.clone(),
                                                value: card.filter(|c| c.revealed)
                                                    .map(|c| c.value.clone()).unwrap_or_default(),
                                                revealed: card.is_some_and(|c| c.revealed),
                                            });
                                            self.overlay = Overlay::GameMaster;
                                        }
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn room_ui(&mut self, ui: &mut egui::Ui, room: &Room) {
        if room.stage == "lobby" {
            egui::ScrollArea::vertical().show(ui, |ui| self.lobby_ui(ui, room));
            return;
        }
        if room.stage == "finished" {
            ui.vertical_centered(|ui| {
                ui.heading("Партия завершена");
                ui.label("Оставшиеся участники:");
                for p in room.players.iter().filter(|p| p.active) { ui.label(&p.name); }
                if ui.button("Оценка выживаемости — WIP").clicked() {
                    self.overlay = Overlay::Survival;
                }
            });
            return;
        }
        let is_host = room.you == room.host_id;
        egui::ScrollArea::vertical().id_salt("game-screen")
            .auto_shrink([false, false]).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(22.0);
                    ui.heading(egui::RichText::new("Катаклизм").size(27.0));
                    ui.add_space(10.0);
                    ui.label(&room.catastrophe);
                    ui.add_space(14.0);
                    ui.small("Иллюстрация катаклизма — ресурс из игрового пака (WIP)");
                    ui.add_space(35.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 365.0).max(0.0) * 0.5);
                        for (i, label) in ["▲", "⌂", "▣", "▤", "♙", "✦"].iter().enumerate() {
                            if ui.add_sized([52.0, 32.0],
                                egui::Button::new(*label).fill(if self.jump_to == Some(i) { accent() }
                                    else { egui::Color32::from_gray(50) })).clicked() {
                                self.jump_to = Some(i);
                            }
                        }
                    });
                });
                section_heading(ui, &mut self.jump_to, 0, "Катаклизм");
                ui.label(&room.catastrophe);
                section_heading(ui, &mut self.jump_to, 1, "Информация о бункере");
                ui.label(&room.bunker);
                ui.colored_label(accent(), format!("Количество мест: {}", room.seats));
                section_heading(ui, &mut self.jump_to, 2, "События");
                ui.vertical_centered(|ui| {
                    ui.add_space(42.0);
                    ui.colored_label(egui::Color32::RED, "UNDER CONSTRUCTION");
                    ui.add_space(42.0);
                });
                section_heading(ui, &mut self.jump_to, 3, "Досье игрока");
                self.dossier_ui(ui, room);
                section_heading(ui, &mut self.jump_to, 4,
                    &format!("Желающие попасть в бункер: {}/{}",
                        room.players.iter().filter(|p| p.active).count(), room.players.len()));
                self.game_table(ui, room, false);
                section_heading(ui, &mut self.jump_to, 5, "Таблица спец. возможностей");
                self.game_table(ui, room, true);
                ui.add_space(32.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Раунд {}", room.round));
                    if is_host {
                        if ui.button("Следующий раунд").clicked() {
                            self.send(json!({"type":"next_round"}));
                        }
                        if room.vote_open {
                            if ui.button("Закончить голосование").clicked() {
                                self.send(json!({"type":"finish_vote"}));
                            }
                        } else if ui.button("Начать голосование").clicked() {
                            self.send(json!({"type":"start_vote"}));
                        }
                        if ui.button("Завершить игру").clicked() {
                            self.send(json!({"type":"finish"}));
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("Кубик:");
                    ui.add(egui::DragValue::new(&mut self.dice_sides).range(2..=100).prefix("d"));
                    if ui.button("Бросить").clicked() {
                        self.send(json!({"type":"roll","sides":self.dice_sides}));
                    }
                    if ui.button("Выживаемость — WIP").clicked() { self.overlay = Overlay::Survival; }
                });
                if room.vote_open {
                    ui.label(format!("Голосование открыто. Голосов: {}", room.ballots_count));
                    for p in room.players.iter().filter(|p| p.active && p.id != room.you) {
                        if ui.button(format!("Голосовать за исключение: {}", p.name)).clicked() {
                            self.send(json!({"type":"vote","target":p.id}));
                        }
                    }
                }
                for result in &room.vote_results {
                    ui.label(format!("{} — {} голос(ов)", result.name, result.votes));
                }
                if is_host {
                    egui::CollapsingHeader::new("Панель ГМа / управление игроками")
                        .show(ui, |ui| {
                            ui.label("Изменяй значения через ✎ в таблицах. Исключение только вручную.");
                            for p in &room.players {
                                ui.horizontal(|ui| {
                                    ui.label(&p.name);
                                    let label = if p.active {"Исключить"} else {"Вернуть"};
                                    if ui.small_button(label).clicked() {
                                        self.send(json!({"type":"set_active","playerId":p.id,"active":!p.active}));
                                    }
                                });
                            }
                        });
                }
                ui.add_space(26.0);
            });
    }

    fn pack_editor(&mut self, ui: &mut egui::Ui, can_apply: bool) {
        ui.label("Готовый JSON (v1/v2 или экспорт Бункер Онлайн).");
        ui.label("Для раздельных файлов редактируй packs/ и собирай через node tools/pack.mjs build.");
        ui.horizontal(|ui| {
            ui.label("Файл:");
            ui.text_edit_singleline(&mut self.pack_file);
            if ui.button("Загрузить").clicked() {
                self.load_pack();
            }
            if ui.button("Сохранить").clicked() {
                self.save_pack();
            }
        });
        ui.add(egui::TextEdit::multiline(&mut self.pack_json)
            .font(egui::TextStyle::Monospace).desired_rows(13).desired_width(f32::INFINITY));
        if can_apply && ui.button("Отправить пак в комнату").clicked() {
            match serde_json::from_str::<Value>(&self.pack_json) {
                Ok(pack) => self.send(json!({ "type": "set_pack", "pack": pack })),
                Err(err) => self.status = format!("Ошибка JSON: {err}"),
            }
        }
    }
}

impl eframe::App for BunkerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_network();
        ctx.request_repaint_after(Duration::from_millis(80));

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::BLACK;
        visuals.window_fill = egui::Color32::from_rgb(20, 20, 20);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(55, 55, 55);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(83, 83, 83);
        visuals.selection.bg_fill = accent();
        ctx.set_visuals(visuals);

        if self.room.is_some() {
            egui::TopBottomPanel::top("game-header").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MURR / БУНКЕР").strong().color(accent()));
                    if let Some(room) = self.room.as_ref() {
                        ui.separator();
                        ui.label(format!("Комната {}", room.code));
                        ui.label(format!("Пак: {}", room.pack_name));
                        ui.label(if self.online { "● Онлайн" } else { "○ Нет соединения" });
                    }
                });
            });
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.small(&self.status);
            if self.session.is_some() && !self.online && !self.leave_requested
                && ui.small_button("Переподключиться").clicked() {
                if let Some(session) = self.session.take() { self.connect_session(session); }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(room) = self.room.clone() {
                self.room_ui(ui, &room);
            } else if !self.profile_complete {
                self.profile_ui(ui);
            } else {
                self.main_menu_ui(ui);
            }
        });
        self.show_overlay(ctx);
    }
}
