//! 리비전 창(FR-REV-01..05): 부모–자식 트리, 두 리비전의 비교(Diff 탭), 비교 결과 저장, 리비전 삭제.

use crate::i18n::{change_label, t, tr};
use crate::view::revisions::{Class, DiffRow, diff_rows, tree_rows};
use egui::{Color32, ComboBox, Context, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use ldf_engine::RevisionInfo;
use ldf_model::Diff;
use std::sync::Arc;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Tree,
    Diff,
}

/// 화면에 보여 주는 비교 결과.
pub struct DiffShown {
    /// 이전 리비전.
    pub before: i64,
    /// 이후 리비전.
    pub after: i64,
    /// 차이.
    pub diff: Arc<Diff>,
    rows: Arc<Vec<DiffRow>>,
}

impl DiffShown {
    /// 비교 결과를 표 행으로 펴 둔다.
    pub fn new(before: i64, after: i64, diff: Arc<Diff>) -> Self {
        let rows = Arc::new(diff_rows(&diff));
        Self {
            before,
            after,
            diff,
            rows,
        }
    }
}

/// 리비전 창 상태.
#[derive(Default)]
pub struct RevState {
    /// 창이 열려 있는지.
    pub open: bool,
    tab: Tab,
    before: Option<i64>,
    after: Option<i64>,
    /// 마지막 비교 결과.
    pub diff: Option<DiffShown>,
    confirm_delete: Option<i64>,
}

/// 창이 앱에 요청하는 동작.
pub enum RevAction {
    /// 리비전을 연다.
    Open(i64),
    /// 리비전을 지운다(확인을 마쳤다).
    Delete(i64),
    /// 두 리비전을 비교한다.
    Diff {
        /// 이전.
        before: i64,
        /// 이후.
        after: i64,
    },
    /// 비교 결과를 파일로 저장한다.
    SaveDiff {
        /// CSV면 참, HTML이면 거짓.
        csv: bool,
    },
}

fn label(r: &RevisionInfo) -> String {
    format!("#{} {} ({})", r.id, tr(&r.label), r.created_at)
}

fn pick(ui: &mut Ui, id: &str, value: &mut Option<i64>, revisions: &[RevisionInfo]) {
    let text = value
        .and_then(|v| revisions.iter().find(|r| r.id == v))
        .map_or_else(|| t("(선택)").to_owned(), label);
    ComboBox::from_id_salt(id)
        .width(300.0)
        .selected_text(text)
        .show_ui(ui, |ui| {
            for r in revisions {
                ui.selectable_value(value, Some(r.id), label(r));
            }
        });
}

/// 리비전 창을 그린다.
pub fn show(
    ctx: &Context,
    revisions: &[RevisionInfo],
    current: Option<i64>,
    st: &mut RevState,
) -> Option<RevAction> {
    if !st.open {
        return None;
    }
    // 처음 열 때(또는 목록이 바뀌어 고른 것이 사라졌을 때) 기본 비교 대상을 정한다: 열린 리비전과 그 부모
    let exists = |id: Option<i64>| id.is_some_and(|i| revisions.iter().any(|r| r.id == i));
    if !exists(st.after) {
        st.after = current.or_else(|| revisions.last().map(|r| r.id));
    }
    if !exists(st.before) {
        st.before = st
            .after
            .and_then(|a| revisions.iter().find(|r| r.id == a))
            .and_then(|r| r.parent_id)
            .or_else(|| revisions.first().map(|r| r.id));
    }

    let mut open = st.open;
    let mut action = None;
    egui::Window::new(t("리비전"))
        .open(&mut open)
        .default_size([860.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut st.tab, Tab::Tree, t("리비전"));
                ui.selectable_value(&mut st.tab, Tab::Diff, t("비교 (Diff)"));
            });
            ui.separator();
            match st.tab {
                Tab::Tree => action = tree(ui, revisions, current, st),
                Tab::Diff => action = diff_tab(ui, revisions, st),
            }
        });
    st.open = open;
    action.or_else(|| confirm_window(ctx, revisions, st))
}

fn tree(
    ui: &mut Ui,
    revisions: &[RevisionInfo],
    current: Option<i64>,
    st: &mut RevState,
) -> Option<RevAction> {
    let mut action = None;
    if revisions.is_empty() {
        ui.weak(t(
            "이 프로젝트에는 아직 리비전이 없다. LDF를 열면 리비전이 만들어진다.",
        ));
        return None;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Grid::new("rev-tree")
                .striped(true)
                .num_columns(6)
                .show(ui, |ui| {
                    for h in [
                        t("리비전"),
                        t("출처 파일"),
                        t("생성 시각"),
                        t("버전"),
                        "",
                        "",
                    ] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for row in tree_rows(revisions) {
                        let Some(r) = revisions.iter().find(|r| r.id == row.id) else {
                            continue;
                        };
                        let indent = "    ".repeat(row.depth);
                        let branch = if row.depth > 0 { "└ " } else { "" };
                        let text = format!("{indent}{branch}#{} {}", r.id, tr(&r.label));
                        let text = if Some(r.id) == current {
                            RichText::new(tf!("{text}  ◀ 열림")).strong()
                        } else {
                            RichText::new(text)
                        };
                        ui.label(text);
                        ui.label(r.source_name.as_deref().unwrap_or("-"));
                        ui.label(&r.created_at);
                        ui.label(r.protocol_version.as_deref().unwrap_or("?"));
                        if ui
                            .add_enabled(Some(r.id) != current, egui::Button::new(t("열기")))
                            .clicked()
                        {
                            action = Some(RevAction::Open(r.id));
                        }
                        if ui.button(t("삭제…")).clicked() {
                            st.confirm_delete = Some(r.id);
                        }
                        ui.end_row();
                    }
                });
        });
    action
}

fn diff_tab(ui: &mut Ui, revisions: &[RevisionInfo], st: &mut RevState) -> Option<RevAction> {
    let mut action = None;
    if revisions.len() < 2 {
        ui.weak(t(
            "비교하려면 리비전이 두 개 이상 있어야 한다(편집을 저장하거나 다른 판을 열어 보라).",
        ));
    }
    egui::Grid::new("diff-pick").num_columns(2).show(ui, |ui| {
        ui.label(t("이전"));
        pick(ui, "diff-before", &mut st.before, revisions);
        ui.end_row();
        ui.label(t("이후"));
        pick(ui, "diff-after", &mut st.after, revisions);
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let ready = st.before.is_some() && st.after.is_some();
        if ui
            .add_enabled(ready, egui::Button::new(t("비교")))
            .clicked()
            && let (Some(before), Some(after)) = (st.before, st.after)
        {
            action = Some(RevAction::Diff { before, after });
        }
        let has = st.diff.is_some();
        if ui
            .add_enabled(has, egui::Button::new(t("CSV로 저장")))
            .clicked()
        {
            action = Some(RevAction::SaveDiff { csv: true });
        }
        if ui
            .add_enabled(has, egui::Button::new(t("HTML로 저장")))
            .clicked()
        {
            action = Some(RevAction::SaveDiff { csv: false });
        }
    });
    ui.separator();
    let Some(shown) = &st.diff else {
        ui.weak(t("두 리비전을 고르고 비교를 누른다."));
        return action;
    };
    let (added, removed, changed, renamed) = shown.diff.counts();
    ui.label(tf!(
        "#{} → #{}: 추가 {added}, 삭제 {removed}, 변경 {changed}, 이름 변경 {renamed}",
        shown.before,
        shown.after
    ));
    if shown.diff.is_empty() {
        ui.weak(t("차이 없음"));
        return action;
    }
    let rows = Arc::clone(&shown.rows);
    let (green, red, yellow) = (
        Color32::from_rgb(90, 190, 110),
        Color32::from_rgb(230, 100, 100),
        Color32::from_rgb(230, 190, 80),
    );
    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .column(Column::exact(110.0))
        .column(Column::initial(190.0))
        .column(Column::exact(70.0))
        .column(Column::initial(150.0))
        .column(Column::initial(200.0))
        .column(Column::remainder())
        .header(20.0, |mut h| {
            for t in [
                t("종류"),
                t("이름"),
                t("변경"),
                t("필드"),
                t("이전"),
                t("이후"),
            ] {
                h.col(|ui| {
                    ui.strong(t);
                });
            }
        })
        .body(|body| {
            body.rows(20.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let color = match r.class {
                    Class::Added => green,
                    Class::Removed => red,
                    Class::Changed => yellow,
                };
                row.col(|ui| {
                    ui.label(r.kind);
                });
                row.col(|ui| {
                    ui.colored_label(color, &r.name);
                });
                row.col(|ui| {
                    ui.colored_label(color, change_label(r.change));
                });
                row.col(|ui| {
                    ui.label(&r.field);
                });
                row.col(|ui| {
                    ui.label(&r.before);
                });
                row.col(|ui| {
                    ui.label(&r.after);
                });
            });
        });
    action
}

/// 삭제 확인(FR-REV-05): 자식 리비전이 있으면 부모 연결이 끊긴다고 경고한다.
fn confirm_window(
    ctx: &Context,
    revisions: &[RevisionInfo],
    st: &mut RevState,
) -> Option<RevAction> {
    let id = st.confirm_delete?;
    let children = revisions.iter().filter(|r| r.parent_id == Some(id)).count();
    let mut action = None;
    egui::Window::new(t("리비전 삭제"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(tf!("리비전 #{id}을(를) 삭제할까? 되돌릴 수 없다."));
            if children > 0 {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    tf!("이 리비전에서 갈라진 자식 리비전이 {children}개 있다. 삭제하면 그 리비전들의 부모 연결이 끊긴다."),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t("삭제")).clicked() {
                    action = Some(RevAction::Delete(id));
                    st.confirm_delete = None;
                }
                if ui.button(t("취소")).clicked() {
                    st.confirm_delete = None;
                }
            });
        });
    action
}
