//! 속성 창. M1에서는 읽기 전용이며, 계산된 값은 회색, 해석되지 않은 참조는 빨간색으로 보여준다.

use crate::view::props::{PropKind, PropSection};
use egui::{Grid, RichText, Ui};

/// 속성 묶음 하나를 표로 그린다.
pub fn section(ui: &mut Ui, id: &str, sec: &PropSection) {
    ui.strong(&sec.title);
    let weak = ui.visuals().weak_text_color();
    let error = ui.visuals().error_fg_color;
    Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing([14.0, 4.0])
        .show(ui, |ui| {
            for p in &sec.props {
                ui.label(RichText::new(p.key).color(weak));
                match p.kind {
                    PropKind::Normal => ui.label(&p.value),
                    PropKind::Derived => ui.colored_label(weak, &p.value),
                    PropKind::Unresolved => ui.colored_label(error, &p.value),
                };
                ui.end_row();
            }
        });
}

/// 아무것도 고르지 않았을 때의 안내.
pub fn placeholder(ui: &mut Ui, title: &str, hint: &str) {
    ui.strong(title);
    ui.weak(hint);
}
