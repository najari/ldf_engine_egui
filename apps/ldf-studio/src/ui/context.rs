//! 오브젝트 컨텍스트 뷰: 선택한 객체의 하위 목록 표. 가상 스크롤이라 행이 수천 개여도 프레임 비용이 일정하다.

use super::UiState;
use crate::i18n::t;
use crate::view::Selection;
use crate::view::context::context_table;
use egui::{Sense, Ui};
use egui_extras::{Column, TableBuilder};
use ldf_engine::ModelSnapshot;

/// 컨텍스트 뷰를 그린다.
pub fn show(ui: &mut Ui, snap: &ModelSnapshot, st: &mut UiState, hex: bool) {
    let key = (snap.version, st.selection.clone(), hex);
    let table = st
        .ctx_cache
        .get(key, || context_table(&snap.model, &st.selection, hex));
    if table.columns.is_empty() {
        ui.strong("Object Context");
        ui.weak(t("왼쪽 트리에서 항목을 선택하세요"));
        return;
    }
    ui.strong(&table.title);

    let error = ui.visuals().error_fg_color;
    let selection = st.selection.clone();
    let mut select: Option<Selection> = None;

    // 라벨이 클릭을 가로채지 않도록 텍스트 선택을 끈다(행 어디를 눌러도 행이 선택된다)
    ui.style_mut().interaction.selectable_labels = false;
    let mut builder = TableBuilder::new(ui)
        .id_salt("context-table")
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    let last = table.columns.len() - 1;
    for i in 0..table.columns.len() {
        builder = builder.column(if i == 0 {
            Column::initial(116.0).at_least(80.0).clip(true)
        } else if i == last {
            Column::remainder().at_least(60.0).clip(true)
        } else {
            // 종류 칸은 "Unconditional"이 100~200% 배율에서 줄임표 없이 들어가는 너비로 둔다
            let width = if table.columns[i] == "Type" {
                92.0
            } else {
                80.0
            };
            Column::initial(width).at_least(50.0).clip(true)
        });
    }

    builder
        .header(20.0, |mut h| {
            for c in &table.columns {
                h.col(|ui| {
                    ui.strong(*c);
                });
            }
        })
        .body(|body| {
            body.rows(20.0, table.rows.len(), |mut row| {
                let r = &table.rows[row.index()];
                row.set_selected(r.target != Selection::None && r.target == selection);
                for cell in &r.cells {
                    row.col(|ui| {
                        if cell.unresolved {
                            ui.colored_label(error, &cell.text);
                        } else {
                            ui.label(&cell.text);
                        }
                    });
                }
                if row.response().clicked() && r.target != Selection::None {
                    select = Some(r.target.clone());
                }
            });
        });

    if let Some(sel) = select {
        super::nav::sync_tree_selection(st, &sel);
        st.selection = sel;
    }
}
