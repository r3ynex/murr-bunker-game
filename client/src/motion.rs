use eframe::egui::{self, Color32, Response, Ui, Widget};

pub const WINDOW_SECONDS: f32 = 0.16;
pub const SCREEN_SECONDS: f64 = 0.18;

pub fn button_progress(ui: &Ui, response: &Response) -> (f32, f32) {
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("button-hover"),
        response.enabled() && (response.hovered() || response.has_focus()),
        0.12,
    );
    let pressed = ui.ctx().animate_bool_with_time(
        response.id.with("button-press"),
        response.enabled() && response.is_pointer_button_down_on(),
        0.08,
    );
    (hover, pressed)
}

pub fn button_highlight(ui: &Ui, response: &Response) {
    let (hover, pressed) = button_progress(ui, response);
    let rect = response.rect;
    let radius = ui.visuals().widgets.inactive.corner_radius;
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            radius,
            Color32::from_white_alpha((14.0 * hover) as u8),
        );
    }
    if pressed > 0.0 {
        ui.painter().rect_filled(
            rect,
            radius,
            Color32::from_black_alpha((44.0 * pressed) as u8),
        );
    }
}

pub fn animated_button(button: egui::Button<'_>) -> impl Widget + '_ {
    move |ui: &mut Ui| {
        let response = button.ui(ui);
        button_highlight(ui, &response);
        response
    }
}

pub fn modal_opacity(ctx: &egui::Context, id: egui::Id, open: bool) -> f32 {
    ctx.animate_bool_with_time(id.with("modal-opacity"), open, WINDOW_SECONDS)
}

pub fn screen_opacity(elapsed: f64) -> f32 {
    let t = (elapsed / SCREEN_SECONDS).clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_fade_reaches_full_opacity_without_overshooting() {
        assert_eq!(screen_opacity(0.0), 0.0);
        assert_eq!(screen_opacity(SCREEN_SECONDS / 2.0), 0.5);
        assert_eq!(screen_opacity(SCREEN_SECONDS * 2.0), 1.0);
    }
}
