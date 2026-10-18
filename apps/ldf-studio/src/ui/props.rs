//! 속성 창. 계산된 값은 회색, 해석되지 않은 참조는 빨간색으로 보여준다.
//! 고칠 수 있는 값(프레임 이름·ID·크기, 시그널 이름·길이·초기값, 노드 이름·타이밍, 슬롯 delay 등)은 더블클릭해 바로 고친다(FR-EDT-01):
//! 입력하는 동안 틀리면 빨갛게 되고 사유가 보이며 적용되지 않는다. Enter로 적용, Esc로 취소.

use crate::i18n::t;
use crate::view::Selection;
use crate::view::edit::{InlineScope, inline_edit, inline_editable};
use crate::view::props::{PropKind, PropSection};
use egui::{Grid, Key, RichText, Sense, TextEdit, Ui};
use ldf_engine::EditOp;
use ldf_model::LdfModel;

/// 지금 값을 고치는 중인 칸.
#[derive(Default)]
pub struct Inline {
    active: Option<(InlineScope, &'static str)>,
    text: String,
    focus: bool,
}

/// 편집 가능한 표를 그릴 때 필요한 것.
pub struct EditCtx<'a> {
    /// 모델.
    pub model: &'a LdfModel,
    /// 선택.
    pub selection: &'a Selection,
    /// 어느 표인지.
    pub scope: InlineScope,
    /// 입력 상태.
    pub inline: &'a mut Inline,
}

/// 속성 묶음 하나를 표로 그리고, 값을 고쳤다면 적용할 연산을 돌려준다.
pub fn section_editable(
    ui: &mut Ui,
    id: &str,
    sec: &PropSection,
    cx: EditCtx<'_>,
) -> Option<EditOp> {
    draw(ui, id, sec, Some(cx))
}

fn draw(ui: &mut Ui, id: &str, sec: &PropSection, mut cx: Option<EditCtx<'_>>) -> Option<EditOp> {
    let mut applied = None;
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
                let editable = p.kind == PropKind::Normal
                    && cx
                        .as_ref()
                        .is_some_and(|c| inline_editable(c.scope, c.selection, p.key));
                match (&mut cx, editable) {
                    (Some(c), true) => {
                        if let Some(op) = editable_cell(ui, c, p.key, &p.value, error) {
                            applied = Some(op);
                        }
                    }
                    _ => {
                        match p.kind {
                            PropKind::Normal => ui.label(&p.value),
                            PropKind::Derived => ui.colored_label(weak, &p.value),
                            PropKind::Unresolved => ui.colored_label(error, &p.value),
                        };
                    }
                }
                ui.end_row();
            }
        });
    applied
}

/// 값 한 칸: 평소에는 글자이고, 더블클릭하면 입력칸이 된다.
fn editable_cell(
    ui: &mut Ui,
    c: &mut EditCtx<'_>,
    key: &'static str,
    value: &str,
    error: egui::Color32,
) -> Option<EditOp> {
    let editing = c.inline.active == Some((c.scope, key));
    if !editing {
        let r = ui
            .add(egui::Label::new(value).sense(Sense::click()))
            .on_hover_text(t("더블클릭해 바로 고친다"));
        if r.double_clicked() {
            c.inline.active = Some((c.scope, key));
            c.inline.text = value.to_owned();
            c.inline.focus = true;
        }
        return None;
    }
    let verdict = inline_edit(c.model, c.scope, c.selection, key, &c.inline.text);
    let mut applied = None;
    ui.vertical(|ui| {
        let mut edit = TextEdit::singleline(&mut c.inline.text).desired_width(150.0);
        if verdict.is_err() {
            edit = edit.text_color(error);
        }
        let r = ui.add(edit);
        if std::mem::take(&mut c.inline.focus) {
            r.request_focus();
        }
        if let Err(msg) = &verdict {
            ui.colored_label(error, msg);
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        if escape {
            c.inline.active = None;
        } else if r.lost_focus() {
            // Enter로 벗어났고 값이 맞으면 적용한다. 틀렸거나 다른 곳을 눌렀으면 그냥 닫는다
            if enter && let Ok(op) = verdict {
                applied = Some(op);
            }
            c.inline.active = None;
        }
    });
    applied
}

/// 아무것도 고르지 않았을 때의 안내.
pub fn placeholder(ui: &mut Ui, title: &str, hint: &str) {
    ui.strong(title);
    ui.weak(hint);
}
