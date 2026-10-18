//! LIN Consistency Checker 창: 왼쪽에 검사 결과 트리, 오른쪽에 텍스트 편집기(FR-CHK-03..08, 10).

use super::Cache;
use super::editor::{self, EditorState};
use crate::view::checker::{self, CheckerTree, FlatRow, OpenKey};
use egui::{Context, RichText, Sense, Ui};
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use ldf_engine::{ModelSnapshot, RuleSetting, RuleSettings, rule_infos};
use ldf_model::Severity;
use std::collections::HashSet;

/// 창이 앱에 요청하는 동작.
pub enum CheckerAction {
    /// 편집한 텍스트를 새 리비전으로 저장하고 다시 검사한다.
    RunChecks(String),
    /// 결과를 CSV 파일로 저장한다.
    SaveCsv(Vec<u8>),
    /// 규칙을 켜고 끄거나 심각도를 바꾼다(FR-CHK-06).
    SetRule {
        /// 규칙 번호.
        code: u16,
        /// 바꾼 설정.
        setting: RuleSetting,
    },
}

/// Checker 창 상태.
#[derive(Default)]
pub struct CheckerState {
    /// 창이 열려 있는지.
    pub open: bool,
    /// 편집기.
    pub editor: EditorState,
    /// 편집기 텍스트를 마지막으로 채운 스냅샷 번호.
    base_version: Option<u64>,
    /// 접힌 항목.
    collapsed: HashSet<OpenKey>,
    /// 선택한 결과(카테고리, 규칙, 순번).
    selected: Option<(usize, usize, usize)>,
    /// 결과 트리 캐시(스냅샷 번호가 키).
    tree: Cache<u64, CheckerTree>,
    /// "줄 이동" 입력.
    goto_input: String,
    /// 규칙 설정 창이 열려 있는지.
    pub show_rules: bool,
}

fn severity_color(ui: &Ui, s: Severity) -> egui::Color32 {
    match s {
        Severity::Error => ui.visuals().error_fg_color,
        Severity::Warning => ui.visuals().warn_fg_color,
        Severity::Info => ui.visuals().weak_text_color(),
    }
}

/// Checker 창을 그린다. 사용자가 누른 동작이 있으면 돌려준다.
pub fn show(
    ctx: &Context,
    snap: &ModelSnapshot,
    rules: &RuleSettings,
    st: &mut CheckerState,
) -> Option<CheckerAction> {
    if !st.open {
        return None;
    }
    if st.base_version != Some(snap.version) {
        st.editor.reset(snap.source.clone());
        st.base_version = Some(snap.version);
        st.collapsed.clear();
        st.selected = None;
    }

    let mut open = st.open;
    let mut action = None;
    egui::Window::new("LIN Consistency Checker")
        .open(&mut open)
        .default_size([1180.0, 640.0])
        .min_size([760.0, 360.0])
        .resizable(true)
        .show(ctx, |ui| {
            action = body(ui, snap, st);
        });
    st.open = open;
    action.or_else(|| rules_window(ctx, rules, st))
}

/// 규칙별 켜기·끄기와 심각도 변경 창. 바꾼 것은 프로젝트에 저장되고 다음 검사부터 적용된다.
fn rules_window(
    ctx: &Context,
    rules: &RuleSettings,
    st: &mut CheckerState,
) -> Option<CheckerAction> {
    if !st.show_rules {
        return None;
    }
    let mut open = true;
    let mut action = None;
    egui::Window::new("검사 규칙 설정")
        .open(&mut open)
        .default_size([620.0, 480.0])
        .show(ctx, |ui| {
            ui.weak("바꾼 설정은 프로젝트(.ldfdb)에 저장되고 열려 있는 파일에 바로 적용된다.");
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut last = None;
                    egui::Grid::new("rules-grid")
                        .striped(true)
                        .num_columns(4)
                        .show(ui, |ui| {
                            for info in rule_infos() {
                                if info.category != last {
                                    last = info.category;
                                    if let Some(c) = last {
                                        ui.label(RichText::new(c.label()).strong());
                                        ui.end_row();
                                    }
                                }
                                let mut setting = rules.get(info.code);
                                let before = setting;
                                ui.checkbox(&mut setting.enabled, format!("{:04}", info.code));
                                ui.label(info.title);
                                let current = setting.severity.unwrap_or(info.severity);
                                egui::ComboBox::from_id_salt(("rule-sev", info.code))
                                    .selected_text(if setting.severity.is_some() {
                                        severity_name(current).to_owned()
                                    } else {
                                        format!("{} (기본)", severity_name(current))
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut setting.severity, None, "기본");
                                        for s in
                                            [Severity::Info, Severity::Warning, Severity::Error]
                                        {
                                            ui.selectable_value(
                                                &mut setting.severity,
                                                Some(s),
                                                severity_name(s),
                                            );
                                        }
                                    });
                                if ui.small_button("기본값").clicked() {
                                    setting = RuleSetting::default();
                                }
                                if setting != before {
                                    action = Some(CheckerAction::SetRule {
                                        code: info.code,
                                        setting,
                                    });
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    st.show_rules = open;
    action
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Info => "Info",
        Severity::Warning => "Warning",
        Severity::Error => "Error",
    }
}

fn body(ui: &mut Ui, snap: &ModelSnapshot, st: &mut CheckerState) -> Option<CheckerAction> {
    let tree = st.tree.get(snap.version, || checker::build(snap));
    let mut action = None;

    ui.horizontal(|ui| {
        let run = ui.add_enabled(
            st.editor.dirty,
            egui::Button::new("Run Checks (적용 후 재검사)"),
        );
        if run
            .on_hover_text("편집한 텍스트를 새 리비전으로 저장하고 다시 검사한다")
            .clicked()
        {
            action = Some(CheckerAction::RunChecks(st.editor.text.clone()));
        }
        if ui
            .add_enabled(st.editor.dirty, egui::Button::new("되돌리기"))
            .clicked()
        {
            st.editor.reset(snap.source.clone());
        }
        if ui.button("보고서 CSV 저장").clicked() {
            action = Some(CheckerAction::SaveCsv(checker::to_csv(&tree)));
        }
        if ui.button("규칙 설정…").clicked() {
            st.show_rules = !st.show_rules;
        }
        ui.separator();
        ui.label("줄 이동");
        let resp = ui.add(egui::TextEdit::singleline(&mut st.goto_input).desired_width(60.0));
        if resp.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Ok(n) = st.goto_input.trim().parse::<usize>()
        {
            st.editor.goto = Some(n);
        }
        ui.separator();
        let total = tree.total();
        if total == 0 {
            ui.label("문제 없음");
        } else {
            ui.label(format!("결과 {total}건"));
        }
        if st.editor.dirty {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "수정됨 — 결과는 Run Checks 후 갱신된다",
            );
        }
    });
    ui.separator();

    StripBuilder::new(ui)
        .size(Size::relative(0.47).at_least(380.0))
        .size(Size::remainder().at_least(300.0))
        .horizontal(|mut strip| {
            strip.cell(|ui| results(ui, &tree, st));
            strip.cell(|ui| editor::show(ui, &mut st.editor));
        });
    action
}

fn results(ui: &mut Ui, tree: &CheckerTree, st: &mut CheckerState) {
    let rows = checker::flatten(tree, &st.collapsed);
    let mut toggle: Option<OpenKey> = None;
    let mut select: Option<(usize, usize, usize)> = None;
    let mut goto: Option<usize> = None;
    let selected = st.selected;
    let collapsed = st.collapsed.clone();
    let weak = ui.visuals().weak_text_color();

    // 라벨이 클릭을 가로채지 않도록 텍스트 선택을 끈다(행 어디를 눌러도 행이 선택된다)
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .id_salt("checker-results")
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(250.0).at_least(140.0).clip(true))
        .column(Column::initial(120.0).at_least(60.0).clip(true))
        .column(Column::initial(130.0).at_least(60.0).clip(true))
        .column(Column::remainder().at_least(40.0))
        .header(22.0, |mut h| {
            for t in ["Inconsistency", "Source", "Constraint", "Line"] {
                h.col(|ui| {
                    ui.add(egui::Label::new(RichText::new(t).strong()).truncate());
                });
            }
        })
        .body(|body| {
            body.rows(20.0, rows.len(), |mut row| match &rows[row.index()] {
                FlatRow::Group {
                    g,
                    text,
                    has_results,
                } => {
                    let key = OpenKey::Group(*g);
                    let is_closed = collapsed.contains(&key);
                    row.col(|ui| {
                        let color = if *has_results {
                            ui.visuals().text_color()
                        } else {
                            weak
                        };
                        ui.label(
                            RichText::new(format!("{} {text}", sign(is_closed)))
                                .color(color)
                                .strong(),
                        );
                    });
                    row.col(|_| {});
                    row.col(|_| {});
                    row.col(|_| {});
                    if row.response().clicked() && *has_results {
                        toggle = Some(key);
                    }
                }
                FlatRow::Rule { g, r, text, worst } => {
                    let key = OpenKey::Rule(*g, *r);
                    let is_closed = collapsed.contains(&key);
                    row.col(|ui| {
                        ui.add_space(14.0);
                        let c = severity_color(ui, *worst);
                        ui.colored_label(c, format!("{} {text}", sign(is_closed)));
                    });
                    row.col(|_| {});
                    row.col(|_| {});
                    row.col(|_| {});
                    if row.response().clicked() {
                        toggle = Some(key);
                    }
                }
                FlatRow::Item { g, r, i } => {
                    let Some(item) = tree
                        .groups
                        .get(*g)
                        .and_then(|x| x.rules.get(*r))
                        .and_then(|x| x.items.get(*i))
                    else {
                        return;
                    };
                    row.set_selected(selected == Some((*g, *r, *i)));
                    row.col(|ui| {
                        ui.add_space(30.0);
                        let c = severity_color(ui, item.severity);
                        ui.colored_label(c, &item.message);
                    });
                    row.col(|ui| {
                        ui.label(&item.source);
                    });
                    row.col(|ui| {
                        ui.label(&item.constraint);
                    });
                    row.col(|ui| {
                        ui.label(
                            item.line_col
                                .map_or_else(String::new, |(l, _)| l.to_string()),
                        );
                    });
                    if row.response().clicked() {
                        select = Some((*g, *r, *i));
                        goto = item.line_col.map(|(l, _)| l);
                    }
                }
            });
        });

    if let Some(k) = toggle
        && !st.collapsed.remove(&k)
    {
        st.collapsed.insert(k);
    }
    if select.is_some() {
        st.selected = select;
    }
    if goto.is_some() {
        st.editor.goto = goto;
    }
}

fn sign(closed: bool) -> &'static str {
    if closed { "[+]" } else { "[-]" }
}
