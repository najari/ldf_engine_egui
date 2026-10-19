//! 스케줄 뷰: 테이블과 슬롯을 트리형 표로 보여준다.

use super::UiState;
use super::icons::{self, Icon};
use crate::i18n::t;
use crate::view::edit::{drop_frame_op, move_slot_op};
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
        ui.weak(t("스케줄 테이블이 없다"));
        return;
    }

    let lines = lines(snap, &st.sched_open);
    let mut toggle: Option<ScheduleId> = None;
    let mut select: Option<Selection> = None;
    let selection = st.selection.clone();
    let open = st.sched_open.clone();
    let error = ui.visuals().error_fg_color;
    let weak = ui.visuals().weak_text_color();
    // 그려진 줄의 영역(끌어다 놓을 자리를 찾는 데 쓴다)
    let mut rects: Vec<(Line, egui::Rect)> = Vec::new();
    let mut started_drag: Option<(ScheduleId, usize)> = None;

    // 라벨이 클릭을 가로채지 않도록 텍스트 선택을 끈다(행 어디를 눌러도 행이 선택된다)
    ui.style_mut().interaction.selectable_labels = false;
    TableBuilder::new(ui)
        .id_salt("schedule-table")
        .striped(true)
        .resizable(true)
        .sense(Sense::click_and_drag())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        // 이름 칸만 고정 폭이고, 나머지는 머리글·내용에 맞춰 가장 좁게 잡는다(좁은 창에서도 머리글이 줄지 않게)
        .column(Column::initial(150.0).at_least(100.0).clip(true))
        .column(Column::auto().at_least(60.0).clip(true))
        .column(Column::auto().at_least(60.0))
        .column(Column::auto().at_least(100.0))
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
                    ui.strong(t);
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
                                icons::inline(ui, Icon::Schedule);
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
                let resp = row.response();
                rects.push((line, resp.rect));
                if resp.clicked() {
                    select = Some(target);
                }
                if let Some(i) = line.slot
                    && resp.drag_started()
                {
                    started_drag = Some((line.sched, i));
                }
            });
        });

    if started_drag.is_some() {
        st.drag_slot = started_drag;
    }
    if st.drag_slot.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // 슬롯을 놓았다: 같은 테이블의 다른 슬롯 줄 위면 그 자리로 옮긴다
    if let Some((sched, from)) = st.drag_slot
        && ui.input(|i| i.pointer.any_released())
    {
        st.drag_slot = None;
        let pos = ui.input(|i| i.pointer.interact_pos());
        let target = pos.and_then(|p| rects.iter().find(|(_, r)| r.contains(p)));
        if let Some((l, _)) = target
            && l.sched == sched
            && let Some(to) = l.slot
            && let Some(t) = m.schedules.get(sched)
        {
            st.pending_op = move_slot_op(&t.name, from, to);
        }
    }
    // 내비게이션에서 프레임을 끌어다 놓았다: 놓은 줄의 테이블에 슬롯을 만든다(슬롯 줄이면 그 뒤에)
    if let Some((frame, pos)) = st.drop_frame.take()
        && let Some((l, _)) = rects.iter().find(|(_, r)| r.contains(pos))
        && let (Some(t), Some(f)) = (m.schedules.get(l.sched), m.frames.get(frame))
    {
        st.pending_op = Some(drop_frame_op(m, &t.name, l.slot, &f.name));
    }

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
