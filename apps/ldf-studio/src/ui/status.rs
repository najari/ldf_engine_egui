//! 상태 바: 왼쪽에 진단 요약, 오른쪽에 Master·Jitter·Timebase·Channel·속도·프로토콜.

use crate::i18n::t;
use crate::view::derive;
use egui::{Align, Layout, Ui};
use ldf_engine::ModelSnapshot;
use ldf_model::{NodeRole, Severity};

/// 상태 바를 그린다. 왼쪽 진단 요약을 눌렀으면 `true`(Checker 창을 연다).
pub fn show(ui: &mut Ui, snap: Option<&ModelSnapshot>, busy: bool, perf_ms: Option<f32>) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        if busy {
            ui.spinner();
            ui.label(t("읽는 중…"));
            ui.separator();
        }
        let Some(snap) = snap else {
            ui.weak(t("열린 파일 없음"));
            return;
        };
        clicked = diagnostics_summary(ui, snap);
        if let Some(ms) = perf_ms {
            ui.separator();
            ui.label(tf!("UI {ms:.1} ms/프레임"));
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let m = &snap.model;
            let master = m
                .nodes
                .iter()
                .find(|(_, n)| n.role == NodeRole::Master)
                .map(|(_, n)| n);
            let dash = || "—".to_owned();
            let segments = [
                format!(
                    "LIN Protocol: {}",
                    m.header.protocol_version.clone().unwrap_or_else(dash)
                ),
                format!(
                    "LIN Speed: {}",
                    m.header.speed_bps.map_or_else(dash, derive::speed)
                ),
                format!(
                    "Channel: {}",
                    m.header.channel_name.clone().unwrap_or_else(dash)
                ),
                format!(
                    "Timebase: {} ms",
                    master
                        .and_then(|n| n.timebase_ms)
                        .map_or_else(dash, derive::ms)
                ),
                format!(
                    "Jitter: {} ms",
                    master
                        .and_then(|n| n.jitter_ms)
                        .map_or_else(dash, derive::ms)
                ),
                format!("Master: {}", master.map_or_else(dash, |n| n.name.clone())),
            ];
            // 오른쪽부터 채우므로 역순으로 넣는다
            for (i, s) in segments.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.label(s);
            }
        });
    });
    clicked
}

fn diagnostics_summary(ui: &mut Ui, snap: &ModelSnapshot) -> bool {
    let count = |s: Severity| snap.diagnostics.iter().filter(|d| d.severity == s).count();
    let (errors, warnings, infos) = (
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
    );
    if errors + warnings + infos == 0 {
        ui.label(t("문제 없음"));
        return false;
    }
    let color = if errors > 0 {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().warn_fg_color
    };
    let text = tf!(
        "진단 {}건: 오류 {errors} · 경고 {warnings} · 참고 {infos}",
        errors + warnings + infos
    );
    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).sense(egui::Sense::click()))
        .on_hover_text(t("눌러서 Checker 창(결과 목록과 텍스트 편집)을 연다"))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
