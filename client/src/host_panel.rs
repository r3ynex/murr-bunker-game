use crate::appearance::{FontWeight, Theme};
use crate::model::{category_name, HostControls, Room};
use crate::motion::animated_button;
use eframe::egui::{self, Ui};
use serde_json::{json, Value};

#[derive(Clone, Copy, Default, PartialEq)]
enum Tab {
    #[default]
    Round,
    Cards,
    Bunker,
    Players,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum CardAction {
    #[default]
    Redraw,
    Profession,
    Health,
    Swap,
    Take,
    Clockwise,
    Counterclockwise,
    Void,
    CureHealth,
    CureFear,
    FlipGender,
    SetGender,
    SetBody,
    Childfree,
    RemoveChildfree,
    AddFact,
    Edit,
}

impl CardAction {
    const OPTIONS: [(Self, &'static str); 17] = [
        (Self::Edit, "Редактор карточки и предметов"),
        (Self::Redraw, "Перегенерировать характеристику"),
        (Self::Profession, "Изменить стаж профессии"),
        (Self::Health, "Изменить степень здоровья"),
        (Self::Swap, "Обменять карточки или досье"),
        (Self::Take, "Забрать карточку"),
        (Self::Clockwise, "Передать по кругу вперёд"),
        (Self::Counterclockwise, "Передать по кругу назад"),
        (Self::Void, "Аннулировать характеристику"),
        (Self::CureHealth, "Сделать здоровым"),
        (Self::CureFear, "Убрать фобию"),
        (Self::FlipGender, "Изменить пол на противоположный"),
        (Self::SetGender, "Задать пол"),
        (Self::SetBody, "Задать телосложение"),
        (Self::Childfree, "Добавить пометку «Чайлдфри»"),
        (Self::RemoveChildfree, "Убрать пометку «Чайлдфри»"),
        (Self::AddFact, "Добавить дополнительное сведение"),
    ];

    fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(action, _)| *action == self)
            .unwrap()
            .1
    }

    fn effect(self) -> &'static str {
        match self {
            Self::CureHealth => "cure_health",
            Self::CureFear => "cure_fear",
            Self::FlipGender => "flip_gender",
            Self::SetGender => "set_gender",
            Self::SetBody => "set_body",
            Self::Childfree => "childfree",
            Self::RemoveChildfree => "remove_childfree",
            _ => unreachable!("card effect"),
        }
    }
}

const SECTIONS: [(&str, &str); 8] = [
    ("description", "Описание"),
    ("construction", "Как был построен"),
    ("location", "Локация"),
    ("rooms", "Комнаты"),
    ("size", "Размер"),
    ("duration", "Время нахождения"),
    ("supplies", "Еда и вода"),
    ("contents", "Что внутри"),
];

#[derive(Default)]
pub struct HostPanel {
    tab: Tab,
    card_action: CardAction,
    source: Option<String>,
    target: Option<String>,
    new_host: Option<String>,
    kind: String,
    all_cards: bool,
    value: String,
    catastrophe: Option<usize>,
    section: usize,
    bunker_text: Option<String>,
    confirm: Option<(&'static str, &'static str)>,
}

pub enum PanelAction {
    Send(Value),
    EditCard { player: String, kind: String },
}

fn action(name: &str) -> Value {
    json!({"type":"host_action","action":name})
}

fn old_action(name: &str) -> Value {
    json!({"type":name})
}

fn button(ui: &mut Ui, enabled: bool, label: &str) -> bool {
    ui.add_enabled(enabled, animated_button(egui::Button::new(label)))
        .clicked()
}

fn choose_player(
    ui: &mut Ui,
    id: &str,
    selected: &mut Option<String>,
    room: &Room,
    all: bool,
    except: Option<&str>,
) -> bool {
    let name = room
        .players
        .iter()
        .find(|p| Some(p.id.as_str()) == selected.as_deref() && Some(p.id.as_str()) != except)
        .map(|p| p.name.as_str())
        .unwrap_or(if all && selected.is_none() {
            "Все активные игроки"
        } else {
            "Выбрать игрока"
        });
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .show_ui(ui, |ui| {
            if all {
                ui.selectable_value(selected, None, "Все активные игроки");
            }
            for player in room
                .players
                .iter()
                .filter(|p| Some(p.id.as_str()) != except)
            {
                ui.selectable_value(selected, Some(player.id.clone()), &player.name);
            }
        });
    selected.as_deref().map_or(all, |id| {
        Some(id) != except && room.players.iter().any(|p| p.id == id)
    })
}

fn category_label<'a>(room: &'a Room, kind: &'a str) -> &'a str {
    room.category_labels
        .get(kind)
        .map(String::as_str)
        .unwrap_or_else(|| category_name(kind))
}

impl HostPanel {
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        room: &Room,
        open: &mut bool,
        online: bool,
        dice_sides: &mut u32,
    ) -> Option<PanelAction> {
        let mut command = None;
        egui::Window::new("Панель ведущего")
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-14.0, 48.0))
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .open(open)
            .default_width(410.0)
            .max_width(440.0)
            .fade_in(true)
            .fade_out(true)
            .show(ctx, |ui| {
                ui.set_width(410.0);
                if let Some((name, question)) = self.confirm {
                    ui.label(FontWeight::Bold.text(theme.fonts.body, question));
                    ui.horizontal_wrapped(|ui| {
                        if button(ui, online, "Подтвердить") {
                            command = Some(PanelAction::Send(action(name)));
                            self.confirm = None;
                        }
                        if button(ui, true, "Отмена") {
                            self.confirm = None;
                        }
                    });
                    return;
                }
                let controls = room.host_controls.as_ref();
                let advanced = online && controls.is_some();
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (Tab::Round, "Раунд"),
                        (Tab::Cards, "Карты"),
                        (Tab::Bunker, "Бункер"),
                        (Tab::Players, "Игроки"),
                    ] {
                        ui.selectable_value(&mut self.tab, tab, label);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 205.0).max(100.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let playing = room.stage == "playing";
                        command = match self.tab {
                            Tab::Round => self
                                .round(ui, room, online && playing, advanced && playing, dice_sides)
                                .map(PanelAction::Send),
                            Tab::Cards => self.cards(ui, room, advanced && playing),
                            Tab::Bunker => self
                                .bunker(ui, controls, advanced && playing)
                                .map(PanelAction::Send),
                            Tab::Players => self
                                .players(ui, room, online && playing, advanced)
                                .map(PanelAction::Send),
                        };
                    });
                ui.separator();
                if controls.is_none() {
                    ui.label("Для новых команд нужен обновлённый сервер");
                }
                ui.horizontal_wrapped(|ui| {
                    if button(
                        ui,
                        advanced && controls.is_some_and(|c| c.undo),
                        "Отменить действие",
                    ) {
                        command = Some(PanelAction::Send(old_action("host_undo")));
                    }
                    if button(ui, advanced, "Начать заново") {
                        self.confirm = Some(("restart", "Начать заново и раздать новые карты?"));
                    }
                    if button(ui, advanced, "Закрыть комнату") {
                        self.confirm = Some(("close", "Закрыть комнату для всех игроков?"));
                    }
                });
            });
        if !*open {
            self.confirm = None;
        }
        command
    }

    fn round(
        &mut self,
        ui: &mut Ui,
        room: &Room,
        enabled: bool,
        advanced: bool,
        sides: &mut u32,
    ) -> Option<Value> {
        let mut command = None;
        ui.label("Таймер обсуждения");
        ui.horizontal_wrapped(|ui| {
            for seconds in [15, 30, 60] {
                if button(ui, advanced, &format!("{seconds} сек.")) {
                    command =
                        Some(json!({"type":"host_action","action":"timer","seconds":seconds}));
                }
            }
            if button(ui, advanced && room.speech_ends_at > 0, "Сбросить") {
                command = Some(json!({"type":"host_action","action":"timer","seconds":0}));
            }
        });
        ui.separator();
        ui.label(format!(
            "Раунд {} · голосов: {}",
            room.round, room.ballots_count
        ));
        ui.horizontal_wrapped(|ui| {
            if button(
                ui,
                enabled,
                if room.vote_open {
                    "Подвести итоги"
                } else {
                    "Начать голосование"
                },
            ) {
                command = Some(old_action(if room.vote_open {
                    "finish_vote"
                } else {
                    "start_vote"
                }));
            }
            if button(ui, advanced, "Круг без изгнания") {
                command = Some(action("skip_vote"));
            }
            if button(ui, enabled && !room.vote_open, "Следующий раунд") {
                command = Some(old_action("next_round"));
            }
            if button(ui, enabled, "Завершить партию") {
                command = Some(old_action("finish"));
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(sides).range(2..=100).prefix("d"));
            if button(ui, enabled, "Бросить кубик") {
                command = Some(json!({"type":"roll","sides":sides}));
            }
        });
        command
    }

    fn cards(&mut self, ui: &mut Ui, room: &Room, enabled: bool) -> Option<PanelAction> {
        let previous = self.card_action;
        egui::ComboBox::from_id_salt("host-card-action")
            .selected_text(self.card_action.label())
            .show_ui(ui, |ui| {
                for (choice, label) in CardAction::OPTIONS {
                    ui.selectable_value(&mut self.card_action, choice, label);
                }
            });
        if previous != self.card_action {
            self.source = None;
            self.target = None;
            self.value.clear();
            self.all_cards = false;
        }
        let op = self.card_action;
        let pair = matches!(op, CardAction::Swap | CardAction::Take);
        let rotate = matches!(op, CardAction::Clockwise | CardAction::Counterclockwise);
        let category = pair
            || rotate
            || matches!(op, CardAction::Redraw | CardAction::Void | CardAction::Edit);
        let source_ok = if rotate {
            ui.label("По порядку активных игроков в таблице");
            room.players.iter().filter(|p| p.active).count() >= 2
        } else {
            ui.label(if pair {
                "Первый игрок"
            } else {
                "Кому применить"
            });
            choose_player(
                ui,
                "host-card-source",
                &mut self.source,
                room,
                !pair && op != CardAction::Edit,
                None,
            )
        };
        let target_ok = if pair {
            ui.label(if op == CardAction::Take {
                "Кто получит карточку"
            } else {
                "Второй игрок"
            });
            choose_player(
                ui,
                "host-card-target",
                &mut self.target,
                room,
                false,
                self.source.as_deref(),
            )
        } else {
            true
        };
        if op == CardAction::Swap {
            ui.checkbox(&mut self.all_cards, "Обменять всё досье");
        }
        if category && !self.all_cards {
            let mut kinds: Vec<_> = room
                .category_labels
                .keys()
                .map(String::as_str)
                .filter(|kind| *kind != "talent")
                .collect();
            kinds.sort_unstable_by(|a, b| category_label(room, a).cmp(category_label(room, b)));
            if !kinds.contains(&self.kind.as_str()) {
                self.kind = kinds.first().copied().unwrap_or("").into();
            }
            egui::ComboBox::from_id_salt("host-card-category")
                .selected_text(category_label(room, &self.kind))
                .show_ui(ui, |ui| {
                    for kind in kinds {
                        ui.selectable_value(
                            &mut self.kind,
                            kind.into(),
                            category_label(room, kind),
                        );
                    }
                });
        }
        if op == CardAction::Take {
            ui.label("У первого игрока ячейка опустеет. Карта второго будет заменена.");
        }
        if matches!(op, CardAction::Profession | CardAction::Health) {
            let (kind, presets) = if op == CardAction::Profession {
                (
                    "profession",
                    ["Новичок", "Опытный", "Профессионал", "Мастер-гуру"],
                )
            } else {
                (
                    "health",
                    [
                        "Лёгкая степень",
                        "Средняя степень",
                        "Тяжёлая степень",
                        "Критическая степень",
                    ],
                )
            };
            ui.horizontal_wrapped(|ui| {
                for preset in presets {
                    if button(ui, true, preset) {
                        self.value = preset.into();
                    }
                }
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.value)
                    .hint_text("Свой вариант. Пустое поле убирает значение")
                    .char_limit(160),
            );
            if button(ui, enabled && source_ok, "Случайное значение") {
                return Some(PanelAction::Send(
                    json!({"type":"host_action","action":"card_stage","playerId":self.source,"kind":kind,"random":true}),
                ));
            }
            return button(ui, enabled && source_ok, "Применить").then(|| PanelAction::Send(json!({
                "type":"host_action","action":"card_stage","playerId":self.source,"kind":kind,"value":self.value
            })));
        }
        if op == CardAction::SetGender {
            ui.horizontal(|ui| {
                for gender in ["Мужчина", "Женщина"] {
                    ui.selectable_value(&mut self.value, gender.into(), gender);
                }
            });
        } else if matches!(op, CardAction::SetBody | CardAction::AddFact) {
            ui.add(
                egui::TextEdit::multiline(&mut self.value)
                    .desired_rows(2)
                    .char_limit(2000)
                    .hint_text("Текст характеристики"),
            );
        }
        let valid_kind = !category || self.all_cards || !self.kind.is_empty();
        let valid_value = !matches!(
            op,
            CardAction::SetGender | CardAction::SetBody | CardAction::AddFact
        ) || !self.value.trim().is_empty();
        if !button(
            ui,
            enabled && source_ok && target_ok && valid_kind && valid_value,
            if op == CardAction::Edit {
                "Открыть редактор"
            } else {
                "Применить"
            },
        ) {
            return None;
        }
        if op == CardAction::Edit {
            return Some(PanelAction::EditCard {
                player: self.source.clone()?,
                kind: self.kind.clone(),
            });
        }
        let command = match op {
            CardAction::Redraw => "redraw",
            CardAction::Swap if self.all_cards => "swap_all",
            CardAction::Swap => "swap",
            CardAction::Take => "take",
            CardAction::Clockwise | CardAction::Counterclockwise => "rotate",
            CardAction::Void => "void_card",
            CardAction::AddFact => "add_fact",
            _ => "effect",
        };
        let mut data = json!({"type":"host_action","action":command,"playerId":self.source,"targetId":self.target,"kind":self.kind,"value":self.value});
        if rotate {
            data["direction"] = json!(if op == CardAction::Clockwise { 1 } else { -1 });
        }
        if command == "effect" {
            data["effect"] = json!(op.effect());
        }
        Some(PanelAction::Send(data))
    }

    fn bunker(
        &mut self,
        ui: &mut Ui,
        controls: Option<&HostControls>,
        enabled: bool,
    ) -> Option<Value> {
        let controls = controls?;
        ui.label("Катаклизм");
        let selected = self
            .catastrophe
            .and_then(|index| controls.catastrophes.get(index));
        egui::ComboBox::from_id_salt("host-catastrophe")
            .selected_text(selected.map_or("Случайный из пака", String::as_str))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.catastrophe, None, "Случайный из пака");
                for (index, title) in controls.catastrophes.iter().enumerate() {
                    ui.selectable_value(&mut self.catastrophe, Some(index), title);
                }
            });
        if button(
            ui,
            enabled && !controls.catastrophes.is_empty(),
            "Изменить катаклизм",
        ) {
            let mut command = action("catastrophe");
            if let Some(index) = self.catastrophe {
                command["index"] = json!(index);
            }
            return Some(command);
        }
        ui.separator();
        ui.label("Секция бункера");
        let previous = self.section;
        egui::ComboBox::from_id_salt("host-bunker-section")
            .selected_text(SECTIONS[self.section].1)
            .show_ui(ui, |ui| {
                for (index, (_, name)) in SECTIONS.iter().enumerate() {
                    ui.selectable_value(&mut self.section, index, *name);
                }
            });
        if previous != self.section {
            self.bunker_text = None;
        }
        let kind = SECTIONS[self.section].0;
        let original = controls
            .bunker_sections
            .get(kind)
            .map(String::as_str)
            .unwrap_or("");
        let mut draft = self.bunker_text.clone().unwrap_or_else(|| original.into());
        if ui
            .add(
                egui::TextEdit::multiline(&mut draft)
                    .desired_width(390.0)
                    .desired_rows(4)
                    .char_limit(6000),
            )
            .changed()
        {
            self.bunker_text = Some(draft);
        }
        let mut command = None;
        ui.horizontal_wrapped(|ui| {
            if button(ui, enabled && self.bunker_text.is_some(), "Сохранить текст") {
                command = Some(json!({"type":"host_action","action":"bunker","kind":kind,"value":self.bunker_text.take()}));
            }
            if button(ui, enabled, "Случайное из пака") {
                self.bunker_text = None;
                command = Some(json!({"type":"host_action","action":"bunker","kind":kind}));
            }
        });
        command
    }

    fn players(
        &mut self,
        ui: &mut Ui,
        room: &Room,
        enabled: bool,
        advanced: bool,
    ) -> Option<Value> {
        let mut command = None;
        ui.horizontal(|ui| {
            ui.label(format!("Мест в бункере: {}", room.seats));
            for (label, delta) in [("−", -1), ("+", 1)] {
                let seats = room.seats as i32 + delta;
                if button(ui, enabled && advanced && (1..=15).contains(&seats), label) {
                    command = Some(json!({"type":"host_action","action":"seats","seats":seats}));
                }
            }
        });
        ui.separator();
        for player in &room.players {
            ui.horizontal_wrapped(|ui| {
                ui.label(&player.name);
                if player.id == room.host_id {
                    ui.weak("Ведущий");
                }
                if button(
                    ui,
                    enabled,
                    if player.active {
                        "Выгнать"
                    } else {
                        "Вернуть"
                    },
                ) {
                    command = Some(
                        json!({"type":"set_active","playerId":player.id,"active":!player.active}),
                    );
                }
            });
        }
        ui.separator();
        ui.label("Передать права ведущего");
        let selected = choose_player(
            ui,
            "new-host",
            &mut self.new_host,
            room,
            false,
            Some(&room.host_id),
        );
        if button(ui, advanced && selected, "Сменить ведущего") {
            command = Some(
                json!({"type":"host_action","action":"transfer_host","targetId":self.new_host}),
            );
        }
        command
    }
}
