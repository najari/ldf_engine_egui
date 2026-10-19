//! NCF(노드 기능 파일) 창: 노드 하나의 일반 정보·진단 능력·프레임·인코딩을 읽기 전용으로 보여 준다.
//!
//! LDF 세션과 따로 열린다(열려 있는 LDF와 편집 상태에 영향이 없다).

use super::checker::{severity_color, severity_icon};
use super::icons;
use crate::i18n::{t, tr};
use crate::view::ncf::{
    Label, Row, all_encodings, diagnostic_rows, encoding_lines, extra_rows, frame_rows,
    general_rows, signal_rows, status_rows,
};
use egui::{CollapsingHeader, Context, Grid, RichText, ScrollArea, Ui};
use ldf_engine::NcfDocument;
use ldf_model::LineIndex;
use std::path::{Path, PathBuf};

/// NCF 창 상태.
#[derive(Default)]
pub struct NcfState {
    /// 창이 열려 있는지.
    pub open: bool,
    path: Option<PathBuf>,
    doc: Option<NcfDocument>,
    /// 보고 있는 노드(파일에 `node` 블록이 여럿일 때).
    node: usize,
    /// 시그널을 펼쳐 보는 프레임.
    frame: Option<usize>,
}

impl NcfState {
    /// 읽은 NCF를 창에 띄운다(이전 것은 닫는다).
    pub fn show_file(&mut self, path: PathBuf, doc: NcfDocument) {
        *self = Self {
            open: true,
            path: Some(path),
            doc: Some(doc),
            node: 0,
            frame: None,
        };
    }
}

fn label_text(l: &Label) -> String {
    match l {
        Label::Text(s) => t(s).to_owned(),
        Label::Raw(s) => s.clone(),
    }
}

/// `이름 | 값` 표 한 구획. 줄이 없으면 그리지 않는다.
fn section(ui: &mut Ui, id: &str, title: &'static str, rows: &[Row]) {
    if rows.is_empty() {
        return;
    }
    CollapsingHeader::new(RichText::new(t(title)).strong())
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            Grid::new(id).num_columns(2).striped(true).show(ui, |ui| {
                for r in rows {
                    ui.weak(label_text(&r.label));
                    ui.label(&r.value);
                    ui.end_row();
                }
            });
        });
}

/// NCF 창을 그린다. `can_import`는 가져올 LDF가 열려 있는지이고, 가져오기 버튼을 눌렀으면 `true`를 돌려준다.
pub fn show(ctx: &Context, st: &mut NcfState, can_import: bool) -> bool {
    if !st.open {
        return false;
    }
    let Some(doc) = st.doc.take() else {
        st.open = false;
        return false;
    };
    let name = st
        .path
        .as_deref()
        .and_then(Path::file_name)
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut open = true;
    let mut import = false;
    egui::Window::new(format!("NCF — {name}"))
        .id(egui::Id::new("ncf-window"))
        .open(&mut open)
        .default_size([900.0, 620.0])
        .min_size([520.0, 320.0])
        .resizable(true)
        .show(ctx, |ui| import = body(ui, &doc, st, can_import));
    st.doc = Some(doc);
    st.open = open;
    import
}

impl NcfState {
    /// 지금 보고 있는 노드(가져오기 대상).
    pub fn current_node(&self) -> Option<&ldf_ncf::NcfNode> {
        self.doc.as_ref()?.file.nodes.get(self.node)
    }
}

fn body(ui: &mut Ui, doc: &NcfDocument, st: &mut NcfState, can_import: bool) -> bool {
    let mut import = false;
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        Grid::new("ncf-file").num_columns(2).show(ui, |ui| {
            ui.weak(t("파일"));
            ui.label(
                st.path
                    .as_deref()
                    .map_or_else(String::new, |p| p.display().to_string()),
            );
            ui.end_row();
            ui.weak(t("NCF 언어 버전"));
            ui.label(doc.file.language_version.as_deref().unwrap_or("-"));
            ui.end_row();
            ui.weak(t("문자 인코딩"));
            ui.label(doc.encoding);
            ui.end_row();
        });
        ui.separator();

        if doc.file.nodes.is_empty() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t("이 파일에서 노드를 찾지 못했다"),
            );
        } else {
            st.node = st.node.min(doc.file.nodes.len() - 1);
            if doc.file.nodes.len() > 1 {
                egui::ComboBox::from_label(t("노드"))
                    .selected_text(&doc.file.nodes[st.node].name)
                    .show_ui(ui, |ui| {
                        for (i, n) in doc.file.nodes.iter().enumerate() {
                            if ui.selectable_value(&mut st.node, i, &n.name).clicked() {
                                st.frame = None;
                            }
                        }
                    });
            }
            ui.add_space(4.0);
            import = ui
                .add_enabled(can_import, egui::Button::new(t("열린 LDF에 가져오기…")))
                .on_hover_text(t("이 노드를 열려 있는 LDF의 슬레이브로 더한다(프레임·시그널·인코딩·노드 속성). Undo로 되돌릴 수 있다."))
                .on_disabled_hover_text(t("가져오려면 LDF를 먼저 열어야 한다"))
                .clicked();
            node(ui, doc, st);
        }
        diagnostics(ui, doc);
    });
    import
}

fn node(ui: &mut Ui, doc: &NcfDocument, st: &mut NcfState) {
    let Some(n) = doc.file.nodes.get(st.node) else {
        return;
    };
    ui.heading(&n.name);
    section(ui, "ncf-general", "일반", &general_rows(n));
    section(ui, "ncf-diagnostic", "진단", &diagnostic_rows(n));

    // 프레임
    let frames = frame_rows(n);
    if !frames.is_empty() {
        CollapsingHeader::new(
            RichText::new(format!("{} ({})", t("프레임"), frames.len())).strong(),
        )
        .id_salt("ncf-frames-h")
        .default_open(true)
        .show(ui, |ui| {
            Grid::new("ncf-frames")
                .num_columns(7)
                .striped(true)
                .show(ui, |ui| {
                    for h in [
                        "방향",
                        "이름",
                        "길이",
                        "메시지 ID",
                        "이벤트 트리거",
                        "주기",
                        "시그널",
                    ] {
                        ui.strong(t(h));
                    }
                    ui.end_row();
                    for (i, f) in frames.iter().enumerate() {
                        ui.label(t(f.direction));
                        if ui.selectable_label(st.frame == Some(i), &f.name).clicked() {
                            st.frame = Some(i);
                        }
                        ui.label(&f.length);
                        ui.label(&f.message_id);
                        ui.label(&f.event);
                        ui.label(&f.period);
                        ui.label(f.signals.to_string());
                        ui.end_row();
                    }
                });
            if let Some(frame) = st.frame.and_then(|i| n.frames.get(i)) {
                ui.add_space(6.0);
                ui.strong(format!("{} — {}", t("시그널"), frame.name));
                let sigs = signal_rows(frame);
                if sigs.is_empty() {
                    ui.weak(t("시그널이 없다"));
                } else {
                    Grid::new("ncf-signals")
                        .num_columns(5)
                        .striped(true)
                        .show(ui, |ui| {
                            for h in ["이름", "초기값", "크기(비트)", "오프셋(비트)", "인코딩"]
                            {
                                ui.strong(t(h));
                            }
                            ui.end_row();
                            for s in &sigs {
                                ui.label(&s.name);
                                ui.label(&s.init);
                                ui.label(&s.size);
                                ui.label(&s.offset);
                                ui.label(&s.encoding);
                                ui.end_row();
                            }
                        });
                }
            }
        });
    }

    // 인코딩
    let encodings = all_encodings(n);
    if !encodings.is_empty() {
        CollapsingHeader::new(
            RichText::new(format!("{} ({})", t("인코딩"), encodings.len())).strong(),
        )
        .id_salt("ncf-encodings-h")
        .default_open(false)
        .show(ui, |ui| {
            for (i, e) in encodings.iter().enumerate() {
                CollapsingHeader::new(&e.name)
                    .id_salt(("ncf-enc", i))
                    .show(ui, |ui| {
                        for line in encoding_lines(e) {
                            ui.monospace(line);
                        }
                    });
            }
        });
    }

    section(ui, "ncf-status", "상태 관리", &status_rows(n));
    if let Some(text) = &n.free_text {
        CollapsingHeader::new(RichText::new(t("자유 텍스트")).strong())
            .id_salt("ncf-free-text")
            .show(ui, |ui| {
                ui.label(text);
            });
    }
    section(ui, "ncf-extra", "해석하지 않은 항목", &extra_rows(n));
}

fn diagnostics(ui: &mut Ui, doc: &NcfDocument) {
    if doc.diagnostics.is_empty() {
        return;
    }
    ui.separator();
    let idx = LineIndex::new(&doc.text);
    CollapsingHeader::new(
        RichText::new(format!("{} ({})", t("읽기 진단"), doc.diagnostics.len())).strong(),
    )
    .id_salt("ncf-diagnostics")
    .default_open(true)
    .show(ui, |ui| {
        for d in &doc.diagnostics {
            let (line, col) = d.span.map_or((0, 0), |s| idx.line_col(&doc.text, s.start));
            ui.horizontal(|ui| {
                icons::inline(ui, severity_icon(d.severity));
                ui.colored_label(
                    severity_color(ui, d.severity),
                    format!("[{:04}] {line}:{col}  {}", d.code, tr(&d.message)),
                );
            });
        }
    });
}
