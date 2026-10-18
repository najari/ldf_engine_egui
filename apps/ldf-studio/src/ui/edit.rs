//! 편집 대화상자(FR-EDT-02/03/04/05/06): 프레임, 시그널, 노드, 스케줄 테이블, 인코딩 타입, 삭제 확인.
//!
//! 대화상자는 초안(`view::edit`)을 들고 있다가 OK에서 `EditOp::Batch` 하나로 바꿔 돌려준다.
//! 입력하는 동안 초안을 모델 복사본에 미리 적용해 보고, 실패 사유와 이 변경으로 새로 생기는 검사 결과를 보여 준다.

use crate::i18n::{t, tr};
use crate::ui::bits;
use crate::view::SelKey;
use crate::view::edit::{
    CompositionUi, EncodingUi, EntryUi, FrameUi, KindUi, NewSignalUi, NodeUi, Preview, ScheduleUi,
    SignalUi, SlotKind, SlotUi, deletion_effects, frame_names, node_names, preview,
};
use egui::{ComboBox, Context, DragValue, TextEdit, Ui};
use ldf_engine::{EditOp, ModelSnapshot};
use ldf_model::{LdfModel, SignalKind};
use std::hash::Hash;

/// 지금 열려 있는 대화상자.
#[derive(Clone, Debug)]
pub enum Dialog {
    /// 프레임 편집·생성.
    Frame(FrameUi),
    /// 시그널 편집·생성.
    Signal(SignalUi),
    /// 노드 편집·생성.
    Node(NodeUi),
    /// 스케줄 테이블 편집·생성.
    Schedule(ScheduleUi),
    /// 노드 구성(composite) 편집.
    Composition(CompositionUi),
    /// 삭제 확인.
    Delete(DeleteUi),
}

/// 삭제 확인 내용.
#[derive(Clone, Debug)]
pub struct DeleteUi {
    /// `프레임 `FA``처럼 대상을 가리키는 문구.
    pub what: String,
    /// 보낼 연산.
    pub op: EditOp,
    /// 함께 지우거나 정리되는 항목.
    pub effects: Vec<String>,
}

impl DeleteUi {
    /// 삭제 연산을 모델 복사본에 적용해 영향을 미리 계산한다. 적용할 수 없으면 `None`.
    pub fn new(model: &LdfModel, what: String, op: EditOp) -> Option<Self> {
        let mut after = model.clone();
        op.apply(&mut after).ok()?;
        Some(Self {
            effects: deletion_effects(model, &after),
            what,
            op,
        })
    }
}

/// 편집 대화상자 상태.
#[derive(Default)]
pub struct EditState {
    /// 주 대화상자.
    pub dialog: Option<Dialog>,
    /// 시그널 대화상자에서 연 인코딩 대화상자(OK하면 따로 적용된다).
    pub encoding: Option<EncodingUi>,
    cache: Option<Cached>,
}

struct Cached {
    key: String,
    op: Result<EditOp, String>,
    preview: Option<Preview>,
}

/// `show`가 앱에 요청하는 동작.
pub enum Outcome {
    /// 아무것도 하지 않는다.
    None,
    /// 연산을 보낸다. `select`는 편집 뒤 선택할 객체.
    Apply {
        /// 보낼 연산.
        op: Box<EditOp>,
        /// 편집 뒤 선택할 객체.
        select: Option<SelKey>,
    },
}

enum Done {
    Open,
    Ok,
    Cancel,
}

fn pick(ui: &mut Ui, id: impl Hash + std::fmt::Debug, value: &mut String, options: &[String]) {
    ComboBox::from_id_salt(id)
        .selected_text(if value.is_empty() {
            t("(선택)")
        } else {
            value.as_str()
        })
        .show_ui(ui, |ui| {
            for o in options {
                ui.selectable_value(value, o.clone(), o);
            }
        });
}

fn pick_opt(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    value: &mut Option<String>,
    options: &[String],
) {
    ComboBox::from_id_salt(id)
        .selected_text(value.as_deref().unwrap_or(t("(없음)")))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, t("(없음)"));
            for o in options {
                ui.selectable_value(value, Some(o.clone()), o);
            }
        });
}

fn check_list(ui: &mut Ui, options: &[String], selected: &mut Vec<String>) {
    ui.horizontal_wrapped(|ui| {
        for o in options {
            let mut on = selected.contains(o);
            if ui.checkbox(&mut on, o).changed() {
                if on {
                    selected.push(o.clone());
                } else {
                    selected.retain(|x| x != o);
                }
            }
        }
    });
}

fn text(ui: &mut Ui, value: &mut String, width: f32) {
    ui.add(TextEdit::singleline(value).desired_width(width));
}

fn footer(
    ui: &mut Ui,
    ok: &str,
    ok_enabled: bool,
    problems: &[String],
    error: Option<&str>,
    findings: &[ldf_model::Diagnostic],
) -> Done {
    ui.add_space(6.0);
    let red = ui.visuals().error_fg_color;
    for p in problems {
        ui.colored_label(red, p);
    }
    if let Some(e) = error {
        ui.colored_label(red, tr(e));
    }
    if !findings.is_empty() {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            t("이 변경으로 새로 생기는 검사 결과:"),
        );
        for d in findings.iter().take(6) {
            ui.weak(format!("  [{:04}] {}", d.code, tr(&d.message)));
        }
        if findings.len() > 6 {
            ui.weak(tf!("  … 외 {}건", findings.len() - 6));
        }
    }
    ui.add_space(6.0);
    let mut done = Done::Open;
    ui.horizontal(|ui| {
        if ui.add_enabled(ok_enabled, egui::Button::new(ok)).clicked() {
            done = Done::Ok;
        }
        if ui.button(t("취소")).clicked() {
            done = Done::Cancel;
        }
    });
    done
}

fn window(ctx: &Context, title: &str, body: impl FnOnce(&mut Ui) -> Done) -> Done {
    let mut open = true;
    let mut done = Done::Open;
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| done = body(ui));
    if open { done } else { Done::Cancel }
}

/// 초안으로 연산과 미리 보기를 만든다. 초안이 바뀔 때만 다시 계산한다.
fn analyze<'a>(
    cache: &'a mut Option<Cached>,
    key: String,
    model: &LdfModel,
    op: impl FnOnce() -> Result<EditOp, String>,
) -> &'a Cached {
    if cache.as_ref().is_none_or(|c| c.key != key) {
        let op = op();
        let preview = op.as_ref().ok().map(|o| preview(model, o));
        *cache = Some(Cached { key, op, preview });
    }
    cache.get_or_insert_with(|| Cached {
        key: String::new(),
        op: Err(String::new()),
        preview: None,
    })
}

/// 열려 있는 대화상자를 그린다.
pub fn show(ctx: &Context, snap: &ModelSnapshot, st: &mut EditState) -> Outcome {
    let model = &snap.model;
    let mut outcome = Outcome::None;

    if let Some(mut dlg) = st.dialog.take() {
        let (done, select) = match &mut dlg {
            Dialog::Frame(d) => (
                frame(ctx, model, d, st, &mut outcome),
                SelKey::Frame(d.name.clone()),
            ),
            Dialog::Signal(d) => (
                signal(ctx, model, d, st, &mut outcome),
                SelKey::Signal(d.name.clone()),
            ),
            Dialog::Node(d) => (
                node(ctx, model, d, st, &mut outcome),
                SelKey::Node(d.name.clone()),
            ),
            Dialog::Schedule(d) => (
                schedule(ctx, model, d, st, &mut outcome),
                SelKey::Schedule(d.name.clone()),
            ),
            Dialog::Composition(d) => (
                composition(ctx, model, d, st, &mut outcome),
                SelKey::Cluster,
            ),
            Dialog::Delete(d) => (delete(ctx, d, &mut outcome), SelKey::Cluster),
        };
        match done {
            Done::Open => st.dialog = Some(dlg),
            Done::Cancel => st.cache = None,
            Done::Ok => {
                // 대화상자가 만든 연산에 선택할 객체를 붙인다
                if let Outcome::Apply { select: s, .. } = &mut outcome {
                    *s = (!matches!(dlg, Dialog::Delete(_))).then_some(select);
                }
                st.cache = None;
            }
        }
    }

    if let Some(mut enc) = st.encoding.take() {
        let done = window(ctx, t("인코딩 타입"), |ui| encoding(ui, &mut enc));
        match done {
            Done::Open => st.encoding = Some(enc),
            Done::Cancel => {}
            Done::Ok => {
                if let Ok(op) = enc.to_op() {
                    outcome = Outcome::Apply {
                        op: Box::new(op),
                        select: None,
                    };
                }
            }
        }
    }
    outcome
}

// ---- 프레임 -------------------------------------------------------------------------------------

fn frame(
    ctx: &Context,
    model: &LdfModel,
    d: &mut FrameUi,
    st: &mut EditState,
    out: &mut Outcome,
) -> Done {
    let title = if d.orig.is_some() {
        t("프레임 편집")
    } else {
        t("프레임 만들기")
    };
    let key = format!("{d:?}");
    let c = analyze(&mut st.cache, key, model, || d.to_op(model));
    let (op, pv) = (c.op.clone(), c.preview.as_ref());
    let signals: Vec<String> = model
        .signals
        .iter()
        .filter(|(_, s)| s.kind == SignalKind::Normal)
        .map(|(_, s)| s.name.clone())
        .collect();
    let nodes = node_names(model, false);
    let frames = frame_names(model, true);
    let tables: Vec<String> = model
        .schedules
        .iter()
        .map(|(_, t)| t.name.clone())
        .collect();

    let done = window(ctx, title, |ui| {
        egui::Grid::new("frame-grid").num_columns(2).show(ui, |ui| {
            ui.label(t("이름"));
            text(ui, &mut d.name, 200.0);
            ui.end_row();
            match &mut d.kind {
                KindUi::Unconditional {
                    id,
                    publisher,
                    size,
                } => {
                    ui.label("ID (0–59)");
                    text(ui, id, 80.0);
                    ui.end_row();
                    ui.label(t("발행 노드"));
                    pick(ui, "frame-pub", publisher, &nodes);
                    ui.end_row();
                    ui.label(t("크기 (byte)"));
                    ui.add(DragValue::new(size).range(1..=8));
                    ui.end_row();
                }
                KindUi::EventTriggered { id, collision, .. } => {
                    ui.label("ID (0–59)");
                    text(ui, id, 80.0);
                    ui.end_row();
                    ui.label(t("충돌 해결 테이블"));
                    pick_opt(ui, "frame-coll", collision, &tables);
                    ui.end_row();
                }
                KindUi::Sporadic { .. } => {}
            }
        });
        match &mut d.kind {
            KindUi::Sporadic { associated } | KindUi::EventTriggered { associated, .. } => {
                ui.separator();
                ui.strong(t("연관 프레임"));
                check_list(ui, &frames, associated);
            }
            KindUi::Unconditional { .. } => placements(ui, model, d, &signals),
        }
        if let Some(pm) = pv.and_then(|p| p.model.as_ref())
            && let Some(fid) = pm.find_frame(&d.name)
        {
            bits::show(ui, pm, fid);
        }
        let mut problems = d.problems();
        problems.extend(op.as_ref().err().cloned());
        footer(
            ui,
            "OK",
            problems.is_empty() && pv.is_some_and(|p| p.error.is_none()),
            &problems,
            pv.and_then(|p| p.error.as_deref()),
            pv.map_or(&[][..], |p| p.new_findings.as_slice()),
        )
    });
    if matches!(done, Done::Ok)
        && let Ok(op) = op
    {
        *out = Outcome::Apply {
            op: Box::new(op),
            select: None,
        };
    }
    done
}

fn placements(ui: &mut Ui, model: &LdfModel, d: &mut FrameUi, signals: &[String]) {
    ui.separator();
    ui.strong(t("시그널"));
    let mut remove = None;
    egui::Grid::new("frame-signals")
        .num_columns(4)
        .show(ui, |ui| {
            for (i, (name, off)) in d.placements.iter_mut().enumerate() {
                pick(ui, ("placed", i), name, signals);
                ui.add(DragValue::new(off).range(0..=63).prefix(t("비트 ")));
                let len = model
                    .find_signal(name)
                    .and_then(|s| model.signals.get(s))
                    .map_or(0, |s| s.size_bits);
                ui.weak(tf!("{len}비트"));
                if ui
                    .small_button(t("삭제"))
                    .on_hover_text(t("프레임에서 뺀다"))
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.end_row();
            }
            for (i, n) in d.added.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(t("새"));
                    text(ui, &mut n.name, 90.0);
                });
                ui.add(
                    DragValue::new(&mut n.offset)
                        .range(0..=63)
                        .prefix(t("비트 ")),
                );
                ui.add(DragValue::new(&mut n.size).range(1..=16).suffix(t("비트")));
                if ui.small_button(t("삭제")).clicked() {
                    remove = Some(d.placements.len() + i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        if i < d.placements.len() {
            d.placements.remove(i);
        } else {
            d.added.remove(i - d.placements.len());
        }
    }
    ui.horizontal(|ui| {
        let free: Vec<String> = signals
            .iter()
            .filter(|s| !d.placements.iter().any(|(n, _)| n == *s))
            .cloned()
            .collect();
        let mut chosen = String::new();
        ComboBox::from_id_salt("add-placement")
            .selected_text(t("시그널 추가…"))
            .show_ui(ui, |ui| {
                for s in &free {
                    ui.selectable_value(&mut chosen, s.clone(), s);
                }
            });
        if !chosen.is_empty() {
            let at = next_free_offset(model, d);
            d.placements.push((chosen, at));
        }
        if ui.button(t("새 시그널")).clicked() {
            let at = next_free_offset(model, d);
            let name = unique_new_name(model, d);
            d.added.push(NewSignalUi {
                name,
                size: 8,
                offset: at,
            });
        }
    });
}

fn next_free_offset(model: &LdfModel, d: &FrameUi) -> u32 {
    let placed = d.placements.iter().map(|(n, off)| {
        let len = model
            .find_signal(n)
            .and_then(|s| model.signals.get(s))
            .map_or(0, |s| s.size_bits);
        off + len
    });
    let added = d.added.iter().map(|n| n.offset + n.size);
    placed.chain(added).max().unwrap_or(0)
}

fn unique_new_name(model: &LdfModel, d: &FrameUi) -> String {
    (1u32..)
        .map(|i| format!("Signal_{i}"))
        .find(|n| model.find_signal(n).is_none() && !d.added.iter().any(|a| a.name == *n))
        .unwrap_or_default()
}

// ---- 시그널 -------------------------------------------------------------------------------------

fn signal(
    ctx: &Context,
    model: &LdfModel,
    d: &mut SignalUi,
    st: &mut EditState,
    out: &mut Outcome,
) -> Done {
    let title = if d.orig.is_some() {
        t("시그널 편집")
    } else {
        t("시그널 만들기")
    };
    let key = format!("{d:?}");
    let c = analyze(&mut st.cache, key, model, || d.to_op());
    let (op, pv) = (c.op.clone(), c.preview.as_ref());
    let nodes = node_names(model, false);
    let encodings: Vec<String> = model
        .encodings
        .iter()
        .map(|(_, e)| e.name.clone())
        .collect();
    let mut open_encoding: Option<EncodingUi> = None;

    let done = window(ctx, title, |ui| {
        egui::Grid::new("signal-grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(t("이름"));
                text(ui, &mut d.name, 200.0);
                ui.end_row();
                ui.label(t("종류"));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut d.array, false, t("스칼라"));
                    ui.selectable_value(&mut d.array, true, t("바이트 배열"));
                });
                ui.end_row();
                ui.label(t("길이 (bit)"));
                ui.add(DragValue::new(&mut d.size).range(1..=64));
                ui.end_row();
                ui.label(if d.array {
                    t("초기값 (쉼표로 구분)")
                } else {
                    t("초기값")
                });
                text(ui, &mut d.init, 200.0);
                ui.end_row();
                ui.label(t("발행 노드"));
                pick_opt(ui, "sig-pub", &mut d.publisher, &nodes);
                ui.end_row();
                ui.label(t("구독 노드"));
                check_list(ui, &nodes, &mut d.subscribers);
                ui.end_row();
                ui.label(t("인코딩"));
                ui.horizontal(|ui| {
                    pick_opt(ui, "sig-enc", &mut d.encoding, &encodings);
                    if let Some(name) = &d.encoding
                        && ui.small_button(t("편집…")).clicked()
                    {
                        open_encoding = EncodingUi::from_name(model, name);
                    }
                    if ui.small_button(t("새로…")).clicked() {
                        open_encoding = Some(EncodingUi::new(model));
                    }
                });
                ui.end_row();
            });
        let mut problems = d.problems();
        problems.extend(op.as_ref().err().cloned());
        footer(
            ui,
            "OK",
            problems.is_empty() && pv.is_some_and(|p| p.error.is_none()),
            &problems,
            pv.and_then(|p| p.error.as_deref()),
            pv.map_or(&[][..], |p| p.new_findings.as_slice()),
        )
    });
    if open_encoding.is_some() {
        st.encoding = open_encoding;
    }
    if matches!(done, Done::Ok)
        && let Ok(op) = op
    {
        *out = Outcome::Apply {
            op: Box::new(op),
            select: None,
        };
    }
    done
}

fn encoding(ui: &mut Ui, d: &mut EncodingUi) -> Done {
    egui::Grid::new("enc-name").num_columns(2).show(ui, |ui| {
        ui.label(t("이름"));
        text(ui, &mut d.name, 200.0);
        ui.end_row();
    });
    ui.separator();
    let mut remove = None;
    egui::Grid::new("enc-entries")
        .num_columns(7)
        .show(ui, |ui| {
            for (i, e) in d.entries.iter_mut().enumerate() {
                match e {
                    EntryUi::Logical(v, t) => {
                        ui.label(crate::i18n::t("논리값"));
                        text(ui, v, 60.0);
                        text(ui, t, 140.0);
                        ui.label("");
                        ui.label("");
                        ui.label("");
                    }
                    EntryUi::Physical(min, max, scale, offset, unit) => {
                        ui.label(t("물리값"));
                        text(ui, min, 50.0);
                        text(ui, max, 50.0);
                        text(ui, scale, 50.0);
                        text(ui, offset, 50.0);
                        text(ui, unit, 60.0);
                    }
                    EntryUi::Bcd => {
                        ui.label("BCD");
                    }
                    EntryUi::Ascii => {
                        ui.label("ASCII");
                    }
                }
                if matches!(e, EntryUi::Bcd | EntryUi::Ascii) {
                    for _ in 0..5 {
                        ui.label("");
                    }
                }
                if ui.small_button(t("삭제")).clicked() {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        d.entries.remove(i);
    }
    ui.horizontal(|ui| {
        if ui.button(t("+ 논리값")).clicked() {
            d.entries.push(EntryUi::Logical("0".into(), String::new()));
        }
        if ui
            .button(t("+ 물리값 (최소, 최대, 스케일, 오프셋, 단위)"))
            .clicked()
        {
            d.entries.push(EntryUi::Physical(
                "0".into(),
                "255".into(),
                "1".into(),
                "0".into(),
                String::new(),
            ));
        }
        if ui.button("+ BCD").clicked() {
            d.entries.push(EntryUi::Bcd);
        }
        if ui.button("+ ASCII").clicked() {
            d.entries.push(EntryUi::Ascii);
        }
    });
    let err = d.to_op().err();
    footer(ui, t("적용"), err.is_none(), &[], err.as_deref(), &[])
}

// ---- 노드 ---------------------------------------------------------------------------------------

const PROTOCOLS: [&str; 6] = ["1.3", "2.0", "2.1", "2.2", "J2602_1_1.0", "ISO17987:2015"];

fn node(
    ctx: &Context,
    model: &LdfModel,
    d: &mut NodeUi,
    st: &mut EditState,
    out: &mut Outcome,
) -> Done {
    let title = match (&d.orig, d.master) {
        (None, _) => t("슬레이브 노드 만들기"),
        (Some(_), true) => t("마스터 노드 편집"),
        (Some(_), false) => t("슬레이브 노드 편집"),
    };
    let key = format!("{d:?}");
    let c = analyze(&mut st.cache, key, model, || d.to_op());
    let (op, pv) = (c.op.clone(), c.preview.as_ref());
    let published: Vec<String> = model
        .signals
        .iter()
        .filter(|(_, s)| {
            s.publisher
                .as_ref()
                .and_then(ldf_model::Ref::id)
                .is_some_and(|p| {
                    model
                        .nodes
                        .get(p)
                        .is_some_and(|n| Some(&n.name) == d.orig.as_ref() || n.name == d.name)
                })
        })
        .map(|(_, s)| s.name.clone())
        .collect();
    let frames = frame_names(model, false);

    let done = window(ctx, title, |ui| {
        egui::Grid::new("node-grid").num_columns(2).show(ui, |ui| {
            ui.label(t("이름"));
            text(ui, &mut d.name, 200.0);
            ui.end_row();
            if d.master {
                ui.label(t("타임베이스 (ms)"));
                text(ui, &mut d.timebase, 80.0);
                ui.end_row();
                ui.label(t("지터 (ms)"));
                text(ui, &mut d.jitter, 80.0);
                ui.end_row();
            }
        });
        if !d.master {
            let mut has = d.attrs.is_some();
            if ui
                .checkbox(&mut has, t("노드 속성 (Node_attributes)"))
                .changed()
            {
                d.attrs = has.then(|| crate::view::edit::AttrsUi::blank(model));
            }
            if let Some(a) = &mut d.attrs {
                egui::Grid::new("attrs-grid").num_columns(2).show(ui, |ui| {
                    ui.label("LIN_protocol");
                    ComboBox::from_id_salt("attrs-proto")
                        .selected_text(a.lin_protocol.as_str())
                        .show_ui(ui, |ui| {
                            for p in PROTOCOLS {
                                ui.selectable_value(&mut a.lin_protocol, p.to_owned(), p);
                            }
                        });
                    ui.end_row();
                    ui.label("configured_NAD");
                    text(ui, &mut a.configured_nad, 80.0);
                    ui.end_row();
                    ui.label("initial_NAD");
                    text(ui, &mut a.initial_nad, 80.0);
                    ui.end_row();
                    ui.label(t("product_id (공급자, 기능, 변형)"));
                    ui.horizontal(|ui| {
                        text(ui, &mut a.supplier, 60.0);
                        text(ui, &mut a.function, 60.0);
                        text(ui, &mut a.variant, 50.0);
                    });
                    ui.end_row();
                    ui.label("response_error");
                    pick_opt(ui, "attrs-re", &mut a.response_error, &published);
                    ui.end_row();
                    ui.label("fault_state_signals");
                    check_list(ui, &published, &mut a.fault_state);
                    ui.end_row();
                    ui.label("P2_min / ST_min (ms)");
                    ui.horizontal(|ui| {
                        text(ui, &mut a.p2_min, 60.0);
                        text(ui, &mut a.st_min, 60.0);
                    });
                    ui.end_row();
                    ui.label("N_As / N_Cr timeout (ms)");
                    ui.horizontal(|ui| {
                        text(ui, &mut a.n_as, 60.0);
                        text(ui, &mut a.n_cr, 60.0);
                    });
                    ui.end_row();
                });
                let with_ids = matches!(a.lin_protocol.as_str(), "2.0" | "J2602_1_1.0");
                ui.strong(if with_ids {
                    t("configurable_frames (메시지 ID 필요)")
                } else {
                    "configurable_frames"
                });
                let mut remove = None;
                for (i, (f, id)) in a.frames.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        pick(ui, ("cf", i), f, &frames);
                        if with_ids {
                            text(ui, id, 70.0);
                        }
                        if ui.small_button(t("삭제")).clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    a.frames.remove(i);
                }
                if ui.button(t("+ 프레임")).clicked() {
                    let first = frames.first().cloned().unwrap_or_default();
                    a.frames.push((first, String::new()));
                }
            }
        }
        let problems: Vec<String> = op.as_ref().err().cloned().into_iter().collect();
        footer(
            ui,
            "OK",
            problems.is_empty() && pv.is_some_and(|p| p.error.is_none()),
            &problems,
            pv.and_then(|p| p.error.as_deref()),
            pv.map_or(&[][..], |p| p.new_findings.as_slice()),
        )
    });
    if matches!(done, Done::Ok)
        && let Ok(op) = op
    {
        *out = Outcome::Apply {
            op: Box::new(op),
            select: None,
        };
    }
    done
}

// ---- 노드 구성 ----------------------------------------------------------------------------------

fn composition(
    ctx: &Context,
    model: &LdfModel,
    d: &mut CompositionUi,
    st: &mut EditState,
    out: &mut Outcome,
) -> Done {
    let key = format!("{d:?}");
    let c = analyze(&mut st.cache, key, model, || d.to_op());
    let (op, pv) = (c.op.clone(), c.preview.as_ref());
    let slaves = node_names(model, true);

    let done = window(ctx, t("노드 구성"), |ui| {
        ui.label(t(
            "구성마다 `configuration 이름` 줄을 쓰고 그 아래에 `합성노드: 논리노드, 논리노드` 줄을 쓴다. 비우면 노드 구성을 지운다.",
        ));
        if !slaves.is_empty() {
            ui.weak(tf!("사용할 수 있는 슬레이브: {}", slaves.join(", ")));
        }
        ui.add(
            TextEdit::multiline(&mut d.text)
                .font(egui::TextStyle::Monospace)
                .desired_rows(10)
                .desired_width(f32::INFINITY),
        );
        let problems: Vec<String> = op
            .as_ref()
            .err()
            .map(|e| tr(e).into_owned())
            .into_iter()
            .collect();
        footer(
            ui,
            "OK",
            problems.is_empty() && pv.is_some_and(|p| p.error.is_none()),
            &problems,
            pv.and_then(|p| p.error.as_deref()),
            pv.map_or(&[][..], |p| p.new_findings.as_slice()),
        )
    });
    if matches!(done, Done::Ok)
        && let Ok(op) = op
    {
        *out = Outcome::Apply {
            op: Box::new(op),
            select: None,
        };
    }
    done
}

// ---- 스케줄 -------------------------------------------------------------------------------------

fn schedule(
    ctx: &Context,
    model: &LdfModel,
    d: &mut ScheduleUi,
    st: &mut EditState,
    out: &mut Outcome,
) -> Done {
    let title = if d.orig.is_some() {
        t("스케줄 테이블 편집")
    } else {
        t("스케줄 테이블 만들기")
    };
    let key = format!("{d:?}");
    let c = analyze(&mut st.cache, key, model, || d.to_op(model));
    let (op, pv) = (c.op.clone(), c.preview.as_ref());
    let slaves = node_names(model, true);
    let frames = frame_names(model, false);
    let warnings = d.slot_warnings(model);

    let done = window(ctx, title, |ui| {
        egui::Grid::new("sched-name").num_columns(2).show(ui, |ui| {
            ui.label(t("이름"));
            text(ui, &mut d.name, 200.0);
            ui.end_row();
        });
        ui.separator();
        let mut action: Option<(usize, i32)> = None;
        egui::Grid::new("sched-slots")
            .num_columns(6)
            .show(ui, |ui| {
                for (i, s) in d.slots.iter_mut().enumerate() {
                    ui.label(format!("{}", i + 1));
                    ComboBox::from_id_salt(("slot-kind", i))
                        .selected_text(s.kind.label())
                        .show_ui(ui, |ui| {
                            for k in SlotKind::ALL {
                                ui.selectable_value(&mut s.kind, k, k.label());
                            }
                        });
                    ui.horizontal(|ui| {
                        if s.kind.takes_node() {
                            pick(ui, ("slot-node", i), &mut s.node, &slaves);
                        }
                        if s.kind.takes_frame() {
                            pick(ui, ("slot-frame", i), &mut s.frame, &frames);
                        }
                        if s.kind.takes_args() {
                            text(ui, &mut s.args, 150.0);
                        }
                    });
                    ui.horizontal(|ui| {
                        text(ui, &mut s.delay, 50.0);
                        ui.label("ms");
                    });
                    ui.horizontal(|ui| {
                        if ui.small_button("↑").clicked() {
                            action = Some((i, -1));
                        }
                        if ui.small_button("↓").clicked() {
                            action = Some((i, 1));
                        }
                        if ui.small_button(t("삭제")).clicked() {
                            action = Some((i, 0));
                        }
                    });
                    match warnings.get(i).and_then(Option::as_ref) {
                        Some(w) => {
                            ui.colored_label(ui.visuals().warn_fg_color, w);
                        }
                        None => {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        if let Some((i, dir)) = action {
            match dir {
                0 => {
                    d.slots.remove(i);
                }
                -1 if i > 0 => d.slots.swap(i, i - 1),
                1 if i + 1 < d.slots.len() => d.slots.swap(i, i + 1),
                _ => {}
            }
        }
        ui.horizontal(|ui| {
            if ui.button(t("+ 프레임 슬롯")).clicked() {
                let first = frames.first().cloned().unwrap_or_default();
                d.slots.push(SlotUi::frame(&first));
            }
            if ui.button(t("+ 진단 명령")).clicked() {
                let mut s = SlotUi::frame("");
                s.kind = SlotKind::AssignNad;
                s.node = slaves.first().cloned().unwrap_or_default();
                d.slots.push(s);
            }
        });
        let problems: Vec<String> = op.as_ref().err().cloned().into_iter().collect();
        footer(
            ui,
            "OK",
            problems.is_empty() && pv.is_some_and(|p| p.error.is_none()),
            &problems,
            pv.and_then(|p| p.error.as_deref()),
            pv.map_or(&[][..], |p| p.new_findings.as_slice()),
        )
    });
    if matches!(done, Done::Ok)
        && let Ok(op) = op
    {
        *out = Outcome::Apply {
            op: Box::new(op),
            select: None,
        };
    }
    done
}

// ---- 삭제 ---------------------------------------------------------------------------------------

fn delete(ctx: &Context, d: &DeleteUi, out: &mut Outcome) -> Done {
    let done = window(ctx, t("삭제 확인"), |ui| {
        ui.label(tf!("{}을(를) 삭제할까?", d.what));
        if d.effects.is_empty() {
            ui.weak(t("다른 항목에 미치는 영향은 없다."));
        } else {
            ui.add_space(4.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                t("함께 지우거나 정리되는 항목:"),
            );
            for e in &d.effects {
                ui.label(format!("  • {}", tr(e)));
            }
        }
        footer(ui, t("삭제"), true, &[], None, &[])
    });
    if matches!(done, Done::Ok) {
        *out = Outcome::Apply {
            op: Box::new(d.op.clone()),
            select: None,
        };
    }
    done
}
