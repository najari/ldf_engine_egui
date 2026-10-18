//! 내보내기 창(FR-EXP-01/04): 대상 버전·줄바꿈·인코딩을 고르고, 오류가 남았으면 확인을 받는다.

use crate::i18n::t;
use egui::Context;
use ldf_engine::{Eol, ExportFormat, ExportOptions, ModelSnapshot, Target, decode};
use std::path::PathBuf;

/// 인코딩 선택.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Enc {
    /// 리비전 원본의 인코딩.
    #[default]
    Keep,
    Utf8,
    Utf8Bom,
    Cp1252,
}

impl Enc {
    fn label(self) -> &'static str {
        match self {
            Enc::Keep => t("원본과 같게"),
            Enc::Utf8 => "UTF-8",
            Enc::Utf8Bom => "UTF-8 (BOM)",
            Enc::Cp1252 => "Windows-1252",
        }
    }

    fn name(self) -> Option<String> {
        match self {
            Enc::Keep => None,
            Enc::Utf8 => Some(decode::UTF8.to_owned()),
            Enc::Utf8Bom => Some(decode::UTF8_BOM.to_owned()),
            Enc::Cp1252 => Some(decode::CP1252.to_owned()),
        }
    }
}

/// 오류가 남아 멈춘 내보내기. 사용자가 확인하면 그대로 다시 보낸다.
#[derive(Clone, Debug)]
pub struct Blocked {
    /// 리비전.
    pub revision: i64,
    /// 대상 파일.
    pub path: PathBuf,
    /// 옵션.
    pub options: ExportOptions,
    /// 남은 오류 개수.
    pub errors: usize,
}

/// 창이 앱에 요청하는 동작.
pub enum ExportAction {
    /// 옵션이 정해졌으니 저장할 파일을 고른다.
    Choose(ExportOptions),
    /// 오류를 알고도 내보내기로 했다.
    Retry(Blocked),
    /// 오류를 먼저 보려고 Checker 창을 연다.
    OpenChecker,
}

/// 내보내기 창 상태.
#[derive(Default)]
pub struct ExportState {
    /// 창을 열었는지.
    pub open: bool,
    format: ExportFormat,
    target: Option<Target>,
    eol: Option<Eol>,
    enc: Enc,
    /// 확인 대기 중인 내보내기.
    pub blocked: Option<Blocked>,
}

fn target_label(t: Target) -> &'static str {
    match t {
        Target::Keep => crate::i18n::t("원본 버전 유지"),
        Target::V2_0 => "LIN 2.0",
        Target::V2_1 => "LIN 2.1",
        Target::V2_2 => "LIN 2.2",
    }
}

/// 창을 그리고 눌린 동작을 돌려준다.
pub fn show(ctx: &Context, snap: &ModelSnapshot, st: &mut ExportState) -> Option<ExportAction> {
    let mut action = None;

    if st.open {
        let mut open = true;
        egui::Window::new(t("내보내기"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let version = snap.model.header.protocol_version.as_deref().unwrap_or("?");
                ui.label(tf!(
                    "리비전 #{} — 원본 LIN {version}, {}",
                    snap.revision.id,
                    snap.revision.encoding
                ));
                ui.add_space(6.0);
                let target = st.target.unwrap_or_default();
                egui::Grid::new("export-grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(t("형식"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut st.format, ExportFormat::Ldf, "LDF");
                            ui.radio_value(&mut st.format, ExportFormat::Json, t("모델 JSON"))
                                .on_hover_text(t("다른 도구 연동용(docs/model.schema.json)"));
                        });
                        ui.end_row();
                        if st.format == ExportFormat::Json {
                            return;
                        }
                        ui.label(t("대상 버전"));
                        egui::ComboBox::from_id_salt("export-target")
                            .selected_text(target_label(target))
                            .show_ui(ui, |ui| {
                                for t in [Target::Keep, Target::V2_0, Target::V2_1, Target::V2_2] {
                                    ui.selectable_value(&mut st.target, Some(t), target_label(t));
                                }
                            });
                        ui.end_row();
                        ui.label(t("줄바꿈"));
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut st.eol, None, t("원본과 같게"));
                            ui.radio_value(&mut st.eol, Some(Eol::CrLf), "CRLF");
                            ui.radio_value(&mut st.eol, Some(Eol::Lf), "LF");
                        });
                        ui.end_row();
                        ui.label(t("인코딩"));
                        egui::ComboBox::from_id_salt("export-enc")
                            .selected_text(st.enc.label())
                            .show_ui(ui, |ui| {
                                for e in [Enc::Keep, Enc::Utf8, Enc::Utf8Bom, Enc::Cp1252] {
                                    ui.selectable_value(&mut st.enc, e, e.label());
                                }
                            });
                        ui.end_row();
                    });
                if st.format == ExportFormat::Json {
                    ui.weak(t("모델을 이름으로 서로 참조하는 JSON으로 내보낸다(UTF-8)."));
                } else if target != Target::Keep {
                    ui.weak(t(
                        "대상 버전에서 허용되지 않는 항목은 빠지고 경고로 알린다.",
                    ));
                } else if !snap.dirty {
                    ui.weak(t(
                        "편집하지 않았으므로 원문(주석·서식 포함)을 그대로 내보낸다.",
                    ));
                } else {
                    ui.weak(t(
                        "편집한 내용은 정규 서식으로 다시 쓰고, 주석은 같은 항목 옆에 남는다.",
                    ));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t("파일 선택…")).clicked() {
                        action = Some(ExportAction::Choose(ExportOptions {
                            format: st.format,
                            target,
                            eol: st.eol,
                            encoding: st.enc.name(),
                            allow_errors: false,
                        }));
                    }
                });
            });
        if !open || action.is_some() {
            st.open = false;
        }
    }

    if let Some(b) = st.blocked.clone() {
        egui::Window::new(t("오류가 남아 있다"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    tf!("이 리비전에는 오류 진단이 {}개 남아 있다.", b.errors),
                );
                ui.label(t("그대로 내보내면 다른 도구에서 읽히지 않을 수 있다."));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t("그래도 내보내기")).clicked() {
                        action = Some(ExportAction::Retry(b.clone()));
                        st.blocked = None;
                    }
                    if ui.button(t("Checker 열기")).clicked() {
                        action = Some(ExportAction::OpenChecker);
                        st.blocked = None;
                    }
                    if ui.button(t("취소")).clicked() {
                        st.blocked = None;
                    }
                });
            });
    }
    action
}
