//! 프레임 비트 레이아웃 미니맵(FR-UI-08): 8바이트 x 8비트, 시그널별 색, 겹침은 빨간 테두리.

use crate::i18n::t;
use crate::view::derive::{self, BitMap};
use egui::{Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use ldf_model::{FrameId, LdfModel, SignalId};

const CELL: f32 = 24.0;
const GAP: f32 = 2.0;
const LABEL_W: f32 = 26.0;

fn signal_color(index: usize, dark: bool) -> Color32 {
    // 황금비로 색상을 흩뿌려 인접한 시그널이 구분되게 한다
    let hue = (index as f32 * 0.618_034) % 1.0;
    let (s, v) = if dark { (0.55, 0.70) } else { (0.45, 0.95) };
    egui::ecolor::Hsva::new(hue, s, v, 1.0).into()
}

fn name_of(m: &LdfModel, id: SignalId) -> String {
    m.signals
        .get(id)
        .map_or_else(|| "?".to_owned(), |s| s.name.clone())
}

/// 비트 레이아웃을 그린다. 프레임 크기가 없는 프레임(스포라딕 등)은 아무것도 그리지 않는다.
pub fn show(ui: &mut Ui, m: &LdfModel, frame: FrameId) {
    let Some(map) = derive::bit_map(m, frame) else {
        return;
    };
    if map.bytes == 0 {
        return;
    }
    ui.add_space(8.0);
    ui.strong("Bit Layout");
    ui.weak(t("한 줄 = 1바이트 (왼쪽이 비트 7)"));

    let dark = ui.visuals().dark_mode;
    let order: Vec<SignalId> = m.frames.get(frame).map_or_else(Vec::new, |f| {
        f.signals().iter().filter_map(|s| s.signal.id()).collect()
    });
    let color_of =
        |id: SignalId| signal_color(order.iter().position(|s| *s == id).unwrap_or(0), dark);

    let rows = map.bytes as usize;
    let size = vec2(LABEL_W + 8.0 * (CELL + GAP), rows as f32 * (CELL + GAP));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    let text_color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let empty_stroke = Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);

    for byte in 0..rows {
        let y = rect.top() + byte as f32 * (CELL + GAP);
        painter.text(
            egui::pos2(rect.left() + 2.0, y + CELL / 2.0),
            Align2::LEFT_CENTER,
            format!("B{byte}"),
            FontId::monospace(11.0),
            weak,
        );
        for col in 0..8usize {
            let bit_in_byte = 7 - col;
            let bit = byte * 8 + bit_in_byte;
            let cell = egui::Rect::from_min_size(
                egui::pos2(rect.left() + LABEL_W + col as f32 * (CELL + GAP), y),
                vec2(CELL, CELL),
            );
            let owners: &[SignalId] = map.cells.get(bit).map_or(&[], Vec::as_slice);
            match owners {
                [] => {
                    painter.rect_stroke(cell, 3.0, empty_stroke, StrokeKind::Inside);
                }
                [one] => {
                    painter.rect_filled(cell, 3.0, color_of(*one));
                }
                many => {
                    painter.rect_filled(cell, 3.0, color_of(many[0]));
                    painter.rect_stroke(
                        cell,
                        3.0,
                        Stroke::new(2.0, Color32::RED),
                        StrokeKind::Inside,
                    );
                }
            }
            painter.text(
                cell.center(),
                Align2::CENTER_CENTER,
                bit_in_byte.to_string(),
                FontId::monospace(10.0),
                if owners.is_empty() {
                    weak
                } else {
                    Color32::BLACK.gamma_multiply(0.8)
                },
            );
            let tip = tooltip(m, bit, owners);
            ui.interact(cell, ui.id().with(("bit", bit)), Sense::hover())
                .on_hover_text(tip);
        }
    }

    // 범례
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for fs in m.frames.get(frame).map_or(&[][..], |f| f.signals()) {
            let Some(id) = fs.signal.id() else { continue };
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, color_of(id));
            let len = m.signals.get(id).map_or(0, |s| s.size_bits);
            let last_bit = fs.offset.saturating_add(len).saturating_sub(1);
            ui.label(
                RichText::new(format!("{} [{}..{}]", name_of(m, id), fs.offset, last_bit))
                    .color(text_color),
            );
        }
    });
    problems(ui, m, &map);
}

fn tooltip(m: &LdfModel, bit: usize, owners: &[SignalId]) -> String {
    let at = tf!("바이트 {}, 비트 {}", bit / 8, bit % 8);
    match owners {
        [] => tf!("{at}: 사용하지 않음"),
        [one] => format!("{at}: {}", name_of(m, *one)),
        many => {
            let names: Vec<String> = many.iter().map(|s| name_of(m, *s)).collect();
            tf!("{at}: 겹침! {}", names.join(" + "))
        }
    }
}

fn problems(ui: &mut Ui, m: &LdfModel, map: &BitMap) {
    let error = ui.visuals().error_fg_color;
    if map.has_overlap() {
        ui.colored_label(error, t("시그널이 겹친다 (빨간 테두리)"));
    }
    if !map.overflow.is_empty() {
        let names: Vec<String> = map.overflow.iter().map(|s| name_of(m, *s)).collect();
        ui.colored_label(
            error,
            tf!("프레임 크기를 넘는 시그널: {}", names.join(", ")),
        );
    }
}
