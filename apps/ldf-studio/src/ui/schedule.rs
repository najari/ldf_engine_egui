//! 스케줄 뷰: 테이블과 슬롯을 트리형 표로 보여준다.

use super::UiState;
use crate::view::{Selection, derive};
use egui::{Sense, Ui};
use egui_extras::{Column, TableBuilder};
use ldf_engine::ModelSnapshot;
use ldf_model::ScheduleId;

#[derive(Clone, Copy)]
struct Line {
    sched: ScheduleId,
    slot: Option<usize>,
}

fn lines(snap: &ModelSnapshot, open: &std::collections::HashSet<ScheduleId>) -> Vec<Line> {
    let mut out = Vec::new();
    for (sid, t) in snap.model.schedules.iter() {
        out.push(Line {
            sched: sid,
            slot: None,
        });
        if open.contains(&sid) {
            out.extend((0..t.slots.len()).map(|i| Line {
                sched: sid,
                slot: Some(i),
            }));
        }
    }
    out
}

/// 스케줄 뷰를 그린다.
pub fn show(ui: &mut Ui, snap: &ModelSnapshot, st: &mut UiState) {
    let m = &snap.model;
    ui.strong("Schedule Tables");
    if m.schedules.is_empty() {
        ui.weak("스케줄 테이블이 없다");
        return;
    }

    let lines = lines(snap, &st.sched_open);
    let mut toggle: Option<ScheduleId> = None;
    let mut select: Option<Selection> = None;
    let selection = st.selection.clone();
    let open = st.sched_open.clone();
    let error = ui.visuals().error_fg_color;
    let weak = ui.visuals().weak_text_color();

    TableBuilder::new(ui)
        .id_salt("schedule-table")
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::initial(200.0).at_least(120.0).clip(true))
        .column(Column::initial(90.0).at_least(60.0).clip(true))
        .column(Column::initial(80.0).at_least(60.0))
        .column(Column::initial(150.0).at_least(100.0))
        .column(Column::remainder().at_least(130.0))
        .header(22.0, |mut h| {
            for t in [
                "Table / Slot",
                "Type",
                "Delay [ms]",
                "Max. Frame Time [ms]",
                "Min. Frame Time [ms]",
            ] {
                h.col(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(t).strong()).truncate());
                });
            }
        })
        .body(|body| {
            body.rows(20.0, lines.len(), |mut row| {
                let line = lines[row.index()];
                let is_open = open.contains(&line.sched);
                let target = match line.slot {
                    None => Selection::Schedule(line.sched),
                    Some(i) => Selection::Slot(line.sched, i),
                };
                row.set_selected(selection == target);

                match line.slot {
                    None => {
                        let name = m
                            .schedules
                            .get(line.sched)
                            .map_or("?", |t| t.name.as_str())
                            .to_owned();
                        let stats = derive::schedule_stats(m, line.sched);
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button(if is_open { "-" } else { "+" }).clicked() {
                                    toggle = Some(line.sched);
                                }
                                ui.label(name);
                            });
                        });
                        row.col(|ui| {
                            ui.colored_label(weak, "Schedule");
                        });
                        row.col(|ui| {
                            ui.colored_label(weak, derive::ms(stats.total_cycle_ms));
                        });
                        row.col(|_| {});
                        row.col(|_| {});
                    }
                    Some(i) => {
                        let Some(info) = derive::slot_info(m, line.sched, i) else {
                            return;
                        };
                        row.col(|ui| {
                            ui.add_space(24.0);
                            let color = if info.name.unresolved {
                                error
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.colored_label(color, &info.name.text);
                        });
                        row.col(|ui| {
                            ui.label(&info.kind);
                        });
                        row.col(|ui| {
                            ui.label(derive::ms(info.delay_ms));
                        });
                        row.col(|ui| {
                            ui.label(info.max_ms.map_or_else(|| "—".to_owned(), derive::ms));
                        });
                        row.col(|ui| {
                            ui.label(info.min_ms.map_or_else(|| "—".to_owned(), derive::ms));
                        });
                    }
                }
                if row.response().clicked() {
                    select = Some(target);
                }
            });
        });

    if let Some(s) = toggle
        && !st.sched_open.remove(&s)
    {
        st.sched_open.insert(s);
    }
    if let Some(sel) = select {
        super::nav::sync_tree_selection(st, &sel);
        st.selection = sel;
    }
}
