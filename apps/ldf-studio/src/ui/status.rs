//! 상태 바: 왼쪽에 진단 요약, 오른쪽에 Master·Jitter·Timebase·Channel·속도·프로토콜.

use crate::view::derive;
use egui::{Align, Layout, Ui};
use ldf_engine::ModelSnapshot;
use ldf_model::{NodeRole, Severity};

/// 상태 바를 그린다.
pub fn show(ui: &mut Ui, snap: Option<&ModelSnapshot>, busy: bool, perf_ms: Option<f32>) {
    ui.horizontal(|ui| {
        if busy {
            ui.spinner();
            ui.label("읽는 중…");
            ui.separator();
        }
        let Some(snap) = snap else {
            ui.weak("열린 파일 없음");
            return;
        };
        diagnostics_summary(ui, snap);
        if let Some(ms) = perf_ms {
            ui.separator();
            ui.label(format!("UI {ms:.1} ms/프레임"));
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
}

fn diagnostics_summary(ui: &mut Ui, snap: &ModelSnapshot) {
    let count = |s: Severity| snap.diagnostics.iter().filter(|d| d.severity == s).count();
    let (errors, warnings, infos) = (
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
    );
    if errors + warnings + infos == 0 {
        ui.label("문제 없음");
        return;
    }
    let color = if errors > 0 {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().warn_fg_color
    };
    let text = format!(
        "진단 {}건: 오류 {errors} · 경고 {warnings} · 참고 {infos}",
        errors + warnings + infos
    );
    ui.colored_label(color, text)
        .on_hover_text("Checker 창(검사 결과 목록과 텍스트 편집)은 M2에서 제공된다");
}
