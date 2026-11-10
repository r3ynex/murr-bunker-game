use crate::model::{category_name, LocalConfig, Room, Session};
use crate::network;
use eframe::egui;
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

fn portable_path(filename: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join(filename)))
        .unwrap_or_else(|| PathBuf::from(filename))
}

pub struct BunkerApp {
    server_url: String,
    nickname: String,
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
                    self.status = "Связь с комнатой потеряна".into();
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
                                        Ok(room) => self.room = Some(room),
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

    fn room_ui(&mut self, ui: &mut egui::Ui, room: &Room) {
        let is_host = room.you == room.host_id;
        ui.horizontal_wrapped(|ui| {
            ui.heading(format!("Комната {}", room.code));
            ui.label(if self.online { "● Онлайн" } else { "○ Нет подключения" });
            ui.label(format!("Подключений: {}", room.connected));
            if ui.button("Переподключиться").clicked() {
                if let Some(session) = self.session.take() {
                    self.connect_session(session);
                }
            }
        });
        ui.label(format!("Пак: {}  |  Раунд: {}  |  Мест: {}", room.pack_name, room.round, room.seats));
        ui.separator();

        if room.stage == "lobby" {
            ui.heading("Лобби");
            for player in &room.players {
                let host_badge = if player.id == room.host_id { " (ведущий)" } else { "" };
                ui.label(format!("• {}{}", player.name, host_badge));
            }
            ui.add_space(12.0);
            if is_host {
                ui.horizontal(|ui| {
                    ui.label("Места в бункере:");
                    let mut seats = room.seats as i32;
                    if ui.add(egui::DragValue::new(&mut seats).range(1..=15)).changed() {
                        self.send(json!({ "type": "set_seats", "seats": seats }));
                    }
                    if ui.button("Начать игру (от 2 игроков)").clicked() {
                        self.send(json!({ "type": "start" }));
                    }
                });
                egui::CollapsingHeader::new("Пак — редактор JSON (для ведущего)")
                    .default_open(false)
                    .show(ui, |ui| self.pack_editor(ui, true));
            } else {
                ui.label("Ждём ведущего. Для теста достаточно двух игроков.");
            }
            return;
        }

        if room.stage == "finished" {
            ui.heading("Партия завершена");
            ui.label("Активные участники:");
            for player in room.players.iter().filter(|p| p.active) {
                ui.label(format!("• {}", player.name));
            }
            return;
        }

        ui.heading("Сценарий");
        ui.label(format!("Катаклизм: {}", room.catastrophe));
        ui.label(format!("Бункер: {}", room.bunker));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Раунд {}", room.round));
            if is_host {
                if ui.button("Следующий раунд").clicked() {
                    self.send(json!({ "type": "next_round" }));
                }
                if ui.button("Открыть голосование").clicked() {
                    self.send(json!({ "type": "start_vote" }));
                }
                if room.vote_open && ui.button("Закрыть голосование").clicked() {
                    self.send(json!({ "type": "finish_vote" }));
                }
                if ui.button("Завершить партию").clicked() {
                    self.send(json!({ "type": "finish" }));
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Кубик:");
            ui.add(egui::DragValue::new(&mut self.dice_sides).range(2..=100).prefix("d"));
            if ui.button("Бросить").clicked() {
                self.send(json!({ "type": "roll", "sides": self.dice_sides }));
            }
        });
        if room.vote_open {
            ui.label(format!("Голосование открыто. Получено голосов: {}", room.ballots_count));
        }
        for result in &room.vote_results {
            ui.label(format!("Голосование: {} — {}", result.name, result.votes));
        }

        ui.separator();
        ui.heading("Участники");
        egui::ScrollArea::vertical().id_salt("participants").show(ui, |ui| {
            for player in &room.players {
                egui::CollapsingHeader::new(format!("{} {}",
                    if player.active { "●" } else { "○" }, player.name))
                    .id_salt(&player.id)
                    .default_open(player.id == room.you)
                    .show(ui, |ui| {
                        if is_host {
                            let label = if player.active { "Исключить" } else { "Вернуть" };
                            if ui.button(label).clicked() {
                                self.send(json!({ "type": "set_active", "playerId": player.id, "active": !player.active }));
                            }
                        }
                        if room.vote_open && player.active && player.id != room.you {
                            if ui.button("Голосовать за исключение").clicked() {
                                self.send(json!({ "type": "vote", "target": player.id }));
                            }
                        }
                        for card in &player.cards {
                            ui.horizontal_wrapped(|ui| {
                                let label = room.category_labels.get(&card.kind)
                                    .map(String::as_str)
                                    .unwrap_or_else(|| category_name(&card.kind));
                                ui.label(format!("{}: {}", label, card.value));
                                if player.id == room.you && !card.revealed && player.active {
                                    if ui.button("Открыть").clicked() {
                                        self.send(json!({ "type": "reveal", "kind": card.kind }));
                                    }
                                }
                            });
                        }
                        if player.cards.is_empty() {
                            ui.label("Характеристики ещё не раскрыты");
                        }
                    });
            }
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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("MURR / Бункер");
                ui.separator();
                ui.small("Прототип 0.1 — интерфейс временный");
            });
        });
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.label(&self.status);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let room = self.room.clone();
            if let Some(room) = room {
                self.room_ui(ui, &room);
            } else {
                ui.heading("Подключение");
                ui.label("Адрес API Cloudflare Worker или локального Wrangler:");
                ui.text_edit_singleline(&mut self.server_url);
                ui.horizontal(|ui| {
                    ui.label("Ник:");
                    ui.text_edit_singleline(&mut self.nickname);
                    if ui.button("Сохранить настройки").clicked() {
                        self.save_config();
                    }
                });
                ui.add_space(14.0);
                let busy = self.pending.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    if ui.button("Создать комнату").clicked() {
                        self.start_request(None);
                    }
                    ui.horizontal(|ui| {
                        ui.label("Код:");
                        ui.text_edit_singleline(&mut self.join_code);
                        if ui.button("Подключиться").clicked() {
                            let code = self.join_code.trim().to_ascii_uppercase();
                            if code.len() == 8 && code.chars().all(|ch| ch.is_ascii_alphanumeric()) {
                                self.start_request(Some(code));
                            } else {
                                self.status = "Укажи восьмизначный код комнаты".into();
                            }
                        }
                    });
                });
                ui.separator();
                egui::CollapsingHeader::new("Локальный редактор пака")
                    .show(ui, |ui| self.pack_editor(ui, false));
                if let Some(session) = self.session.as_ref() {
                    ui.label(format!("Последняя комната: {} (игрок {})", session.code, session.player_id));
                    if ui.button("Попробовать переподключиться").clicked() {
                        if let Some(session) = self.session.take() {
                            self.connect_session(session);
                        }
                    }
                }
            }
        });
    }
}
