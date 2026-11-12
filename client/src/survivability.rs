use crate::appearance::{FontWeight, Theme};
use crate::artwork::{flat_button, paint_cover};
use crate::motion::{animated_button, modal_opacity};
use eframe::egui::{self, Color32, Stroke, Ui};

const TITLE: &str = "Оценка выживаемости бункера";
const YES_NO: &[&str] = &["Да", "Нет"];

struct Gate {
    question: &'static str,
    choices: &'static [&'static str],
    failure: Outcome,
}

const GATES: [Gate; 3] = [
    Gate {
        question: "Хватит ли еды на весь срок пребывания в бункере с учётом того, что удалось добыть по игре?",
        choices: YES_NO,
        failure: Outcome::Hunger,
    },
    Gate {
        question: "Изолированы ли все опасные или заразные жители, либо есть ли человек или предмет, который может защитить остальных?",
        choices: YES_NO,
        failure: Outcome::Outbreak,
    },
    Gate {
        question: "Если рядом есть вражеский бункер, есть ли у вашей группы возможность отбиться?",
        choices: &[
            "Вражеского бункера нет",
            "Вражеский бункер есть, но у нас есть оружие или боеспособный участник",
            "Вражеский бункер есть, и нам нечем отбиться",
        ],
        failure: Outcome::Overrun,
    },
];

const CONDITIONS: [&str; 11] = [
    "Может ли хотя бы половина жителей выполнять основные задачи с учётом ограничений здоровья и доступной помощи?",
    "Позволяет ли физическая форма хотя бы половине жителей справляться с нагрузками в бункере?",
    "Хотя бы половина жителей бункера — люди непожилого возраста?",
    "Есть ли хотя бы у половины жителей характер без отрицательной черты, мешающей группе?",
    "Есть ли хотя бы два жителя с полезной профессией?",
    "Есть ли хотя бы два жителя с полезным хобби?",
    "Есть ли хотя бы два жителя с полезным инвентарём?",
    "Есть ли хотя бы два жителя с полезными дополнительными сведениями?",
    "Есть ли у группы хотя бы одна пара для продолжения рода или дети?",
    "Есть ли дружественный бункер, с которым можно обмениваться ресурсами и помогать друг другу?",
    "Решены ли проблемы внутри бункера, включая конфликт с посторонним, если он возник по игре?",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Hunger,
    Outbreak,
    Overrun,
    Lost,
    WithLosses,
    Survived,
}

impl Outcome {
    fn from_conditions(satisfied: usize) -> Self {
        match satisfied {
            9.. => Self::Survived,
            5..=8 => Self::WithLosses,
            _ => Self::Lost,
        }
    }

    fn story(self) -> (&'static str, &'static str) {
        match self {
            Self::Hunger => (
                "Запасы закончились",
                "Еда закончилась до того, как стало безопасно выйти наружу. Жители бункера не пережили голод.",
            ),
            Self::Outbreak => (
                "Угроза осталась внутри",
                "Группа не смогла изолировать угрозу и защитить остальных. Бункер перестал быть безопасным убежищем.",
            ),
            Self::Overrun => (
                "Бункер захвачен",
                "Вражеская группа добралась до убежища. Жители не удержали бункер и лишились запасов и укрытия.",
            ),
            Self::Lost => (
                "Убежище не продержалось",
                "Нехватка навыков, снаряжения и поддержки сорвала работу бункера. Группа была вынуждена покинуть его раньше срока.",
            ),
            Self::WithLosses => (
                "Часть группы выжила",
                "Жители сохранили убежище до выхода на поверхность, но справились с трудностями не все. Выжившие начинают всё заново.",
            ),
            Self::Survived => (
                "Все жители выжили",
                "Группа пережила весь срок в бункере и готова выйти на поверхность. Возможно, именно это убежище станет началом новой цивилизации.",
            ),
        }
    }
}

#[derive(Default)]
pub struct Assessment {
    open: bool,
    step: usize,
    gates: [Option<usize>; GATES.len()],
    conditions: [Option<usize>; CONDITIONS.len()],
    outcome: Option<Outcome>,
    reset_scroll: bool,
}

impl Assessment {
    pub fn open(&mut self) {
        self.open = true;
    }

    fn can_advance(&self) -> bool {
        match self.gates.get(self.step) {
            Some(answer) => answer.is_some(),
            None => self.conditions.iter().all(Option::is_some),
        }
    }

    fn advance(&mut self) {
        if !self.can_advance() {
            return;
        }
        if let Some(gate) = GATES.get(self.step) {
            if self.gates[self.step] == Some(gate.choices.len() - 1) {
                self.outcome = Some(gate.failure);
            } else {
                self.step += 1;
                self.reset_scroll = self.step == GATES.len();
            }
        } else {
            let satisfied = self
                .conditions
                .iter()
                .filter(|&&answer| answer == Some(0))
                .count();
            self.outcome = Some(Outcome::from_conditions(satisfied));
        }
    }

    fn back(&mut self) {
        if self.outcome.take().is_none() {
            self.step = self.step.saturating_sub(1);
        }
    }

    fn restart(&mut self) {
        *self = Self {
            open: true,
            ..Self::default()
        };
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        backdrop: Option<&egui::TextureHandle>,
    ) {
        let id = egui::Id::new("bunker-survivability");
        let opacity = modal_opacity(ctx, id, self.open);
        if opacity <= 0.0 {
            return;
        }

        let viewport = ctx.content_rect().size();
        let modal = egui::Modal::new(id)
            .area(
                egui::Modal::default_area(id)
                    .fade_in(false)
                    .interactable(self.open),
            )
            .backdrop_color(Color32::from_black_alpha((180.0 * opacity) as u8))
            .frame(
                egui::Frame::new()
                    .fill(theme.palette.background.color())
                    .stroke(Stroke::new(1.0_f32, theme.palette.outline.color()))
                    .inner_margin(18)
                    .multiply_with_opacity(opacity),
            )
            .show(ctx, |ui| {
                ui.multiply_opacity(opacity);
                if !self.open {
                    ui.disable();
                }
                let width = (viewport.x - 80.0).min(800.0);
                ui.set_width(width);
                self.header(ui, theme, backdrop, width);
                if let Some(outcome) = self.outcome {
                    self.result(ui, theme, width, outcome);
                } else {
                    self.questions(ui, theme, width, (viewport.y - 220.0).max(96.0));
                }
            });
        if self.open && modal.should_close() {
            self.open = false;
        }
    }

    fn header(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        backdrop: Option<&egui::TextureHandle>,
        width: f32,
    ) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 70.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        if let Some(backdrop) = backdrop {
            paint_cover(&painter, backdrop, rect);
            painter.rect_filled(rect, 0u8, Color32::from_black_alpha(210));
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            TITLE,
            FontWeight::Bold.font(theme.fonts.heading),
            theme.palette.accent_title.color(),
        );
        let close = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 18.0, rect.top() + 18.0),
            egui::vec2(28.0, 28.0),
        );
        if ui
            .place(close, animated_button(egui::Button::new("×")))
            .on_hover_text("Закрыть оценку")
            .clicked()
        {
            self.open = false;
        }
    }

    fn questions(&mut self, ui: &mut Ui, theme: &Theme, width: f32, max_height: f32) {
        if let Some(gate) = GATES.get(self.step) {
            question_card(
                ui,
                theme,
                gate.question,
                gate.choices,
                &mut self.gates[self.step],
            );
        } else {
            let scroll = egui::ScrollArea::vertical()
                .id_salt("survivability-conditions")
                .auto_shrink([false, true])
                .max_height(max_height);
            let scroll = if std::mem::take(&mut self.reset_scroll) {
                scroll.vertical_scroll_offset(0.0)
            } else {
                scroll
            };
            scroll.show(ui, |ui| {
                for (question, answer) in CONDITIONS.iter().zip(&mut self.conditions) {
                    question_card(ui, theme, question, YES_NO, answer);
                }
            });
        }
        let final_step = self.step == GATES.len();
        let label = if final_step {
            "Узнать результат"
        } else {
            "Далее"
        };
        let primary = if final_step {
            [theme.palette.ready_end.color(); 2]
        } else {
            theme.accent_gradient()
        };
        let ready = self.can_advance();
        self.footer(ui, theme, width, label, primary, ready);
    }

    fn result(&mut self, ui: &mut Ui, theme: &Theme, width: f32, outcome: Outcome) {
        let (heading, story) = outcome.story();
        card_frame(theme, theme.palette.outline.color()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(FontWeight::Bold.text(theme.fonts.button, heading));
                ui.label(FontWeight::Regular.text(theme.fonts.body, story));
            });
        });
        self.footer(
            ui,
            theme,
            width,
            "Пройти заново",
            theme.accent_gradient(),
            true,
        );
    }

    fn footer(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        width: f32,
        label: &str,
        colors: [Color32; 2],
        enabled: bool,
    ) {
        let height = theme.buttons.accent_size[1];
        let has_back = self.step > 0 || self.outcome.is_some();
        let button_width = if has_back {
            (width - ui.spacing().item_spacing.x) / 2.0
        } else {
            width
        };
        ui.horizontal(|ui| {
            if has_back {
                let button =
                    egui::Button::new(FontWeight::SemiBold.text(theme.fonts.button, "Назад"))
                        .fill(theme.palette.background.color())
                        .stroke(Stroke::new(1.0_f32, theme.palette.outline.color()));
                if ui
                    .add_sized([button_width, height], animated_button(button))
                    .clicked()
                {
                    self.back();
                }
            }
            let button = egui::Button::new(
                FontWeight::SemiBold
                    .text(theme.fonts.button, label)
                    .color(theme.palette.accent_text.color()),
            );
            if enabled {
                if flat_button(
                    ui,
                    [button_width, height],
                    button,
                    colors,
                    theme.buttons.radius,
                )
                .clicked()
                {
                    if self.outcome.is_some() {
                        self.restart();
                    } else {
                        self.advance();
                    }
                }
            } else {
                ui.add_enabled_ui(false, |ui| ui.add_sized([button_width, height], button));
            }
        });
    }
}

fn card_frame(theme: &Theme, border: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.palette.surface.color())
        .stroke(Stroke::new(1.0_f32, border))
        .inner_margin(12)
}

fn question_card(
    ui: &mut Ui,
    theme: &Theme,
    question: &str,
    choices: &[&str],
    answer: &mut Option<usize>,
) {
    card_frame(theme, theme.palette.button.color()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add(egui::Label::new(FontWeight::Bold.text(theme.fonts.body, question)).wrap());
        for (index, choice) in choices.iter().enumerate() {
            ui.radio_value(
                answer,
                Some(index),
                FontWeight::Regular.text(theme.fonts.body, *choice),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_answers_end_the_assessment_and_can_be_revised() {
        let mut assessment = Assessment::default();
        assessment.advance();
        assert_eq!(assessment.step, 0);
        for (step, (answer, outcome)) in [
            (1, Outcome::Hunger),
            (1, Outcome::Outbreak),
            (2, Outcome::Overrun),
        ]
        .into_iter()
        .enumerate()
        {
            assessment.gates[step] = Some(answer);
            assessment.advance();
            assert_eq!(assessment.outcome, Some(outcome));
            assessment.back();
            assert_eq!(assessment.step, step);
            assert_eq!(assessment.gates[step], Some(answer));
            assessment.gates[step] = Some(if step == 2 { 1 } else { 0 });
            assessment.advance();
        }
        assert_eq!(assessment.step, GATES.len());
        assert!(!assessment.can_advance());
    }

    #[test]
    fn complete_checklists_produce_different_endings_and_restart_clears_answers() {
        for (satisfied, expected) in [
            (0, Outcome::Lost),
            (4, Outcome::Lost),
            (5, Outcome::WithLosses),
            (8, Outcome::WithLosses),
            (9, Outcome::Survived),
            (11, Outcome::Survived),
        ] {
            let mut assessment = Assessment {
                step: GATES.len(),
                gates: [Some(0); GATES.len()],
                conditions: std::array::from_fn(|index| Some(usize::from(index >= satisfied))),
                ..Assessment::default()
            };
            assessment.advance();
            assert_eq!(assessment.outcome, Some(expected));
            assessment.restart();
            assert_eq!(assessment.step, 0);
            assert_eq!(assessment.outcome, None);
            assert!(assessment.conditions.iter().all(Option::is_none));
        }
    }
}
