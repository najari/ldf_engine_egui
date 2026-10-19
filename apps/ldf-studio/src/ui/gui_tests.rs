//! egui 화면 시험(`egui_kittest`): 접근성 트리로 라벨을 찾아 클릭·입력하고, 대화상자가 돌려주는 연산을 확인한다.
//!
//! 화면 모양(픽셀)은 보지 않는다. 라벨이 보이는지, 버튼이 켜져 있는지, 누르면 어떤 `EditOp`가 나오는지를 본다.
//! 언어는 전역이라 병렬로 도는 다른 시험과 섞이지 않게 모두 한국어(기본)로 시험한다.

use super::checker::{self, CheckerAction, CheckerState};
use super::edit::{DeleteUi, Dialog, EditState, Outcome, show};
use super::export::{self, Blocked, ExportAction, ExportState};
use super::revisions::{self, DiffShown, RevAction, RevState};
use super::{UiState, status};
use crate::view::edit::{CompositionUi, FrameUi};
use crate::view::{NavTab, SelKey, Selection};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ldf_engine::{EditOp, Eol, ExportFormat, ExportOptions, ModelSnapshot, Session, Target};
use ldf_model::LdfModel;
use std::sync::Arc;

const SAMPLE: &str = "LIN_description_file;\n\
LIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A, B; }\n\
Signals { Cmd: 8, 0, M, A, B; Pos: 16, 0, A, M; }\n\
Frames { FCmd: 1, M, 1 { Cmd, 0; } FA: 2, A, 2 { Pos, 0; } }\n\
Node_attributes {\n\
  A { LIN_protocol = \"2.1\"; configured_NAD = 1; product_id = 1, 2, 3; response_error = Pos;\n\
      P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; FA; } }\n\
  B { LIN_protocol = \"2.1\"; configured_NAD = 2; product_id = 1, 2, 3; response_error = Pos;\n\
      P2_min = 50 ms; ST_min = 0 ms; configurable_frames { FCmd; } }\n\
}\n\
Schedule_tables { T { FCmd delay 10 ms; FA delay 10 ms; } }\n";

fn snapshot(src: &str) -> Arc<ModelSnapshot> {
    Session::open_in_memory()
        .expect("session")
        .import_bytes("t.ldf", src.as_bytes())
        .expect("import")
        .snapshot
}

/// 대화상자 시험용 상태: 스냅샷, 대화상자 상태, 대화상자가 돌려준 연산.
struct Dlg {
    snap: Arc<ModelSnapshot>,
    st: EditState,
    sent: Vec<EditOp>,
}

#[allow(clippy::field_reassign_with_default)] // EditState의 캐시 필드가 비공개라 구조체 갱신 문법을 쓸 수 없다
fn dialog_harness(dialog: Dialog, snap: Arc<ModelSnapshot>) -> Harness<'static, Dlg> {
    let mut st = EditState::default();
    st.dialog = Some(dialog);
    Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui_state(
            |ui, s: &mut Dlg| {
                if let Outcome::Apply { op, .. } = show(ui.ctx(), &s.snap, &mut s.st) {
                    s.sent.push(*op);
                }
            },
            Dlg {
                snap,
                st,
                sent: Vec::new(),
            },
        )
}

// ---- 상태 바 -----------------------------------------------------------------------------------

#[test]
fn status_bar_shows_clean_state_and_header_facts() {
    let snap = snapshot(SAMPLE);
    let mut h = Harness::new_ui(|ui| {
        status::show(ui, Some(&snap), false, None);
    });
    h.run();
    h.get_by_label("Master: M");
    h.get_by_label("LIN Protocol: 2.1");
    h.get_by_label("LIN Speed: 19.200 kbps");
    h.get_by_label("Timebase: 5.00 ms");
}

#[test]
fn status_bar_summarizes_diagnostics_and_opens_the_checker_when_clicked() {
    let snap = snapshot(&SAMPLE.replace("response_error = Pos;", "response_error = Ghost;"));
    assert!(!snap.diagnostics.is_empty());
    let mut opened = false;
    let mut h = Harness::new_ui(|ui| {
        opened |= status::show(ui, Some(&snap), false, None);
    });
    h.run();
    h.get_by_label_contains("진단 ").click();
    h.run();
    drop(h);
    assert!(
        opened,
        "진단 요약을 누르면 Checker를 열어 달라고 알려야 한다"
    );
}

#[test]
fn status_bar_without_a_file_and_while_busy() {
    let mut h = Harness::new_ui(|ui| {
        status::show(ui, None, true, None);
    });
    // 스피너는 계속 다시 그리므로 한 걸음만 그린다
    h.step();
    h.get_by_label("열린 파일 없음");
    h.get_by_label("읽는 중…");
}

// ---- 노드 구성 대화상자 --------------------------------------------------------------------------

#[test]
fn composition_dialog_sends_the_edited_text_as_one_operation() {
    let snap = snapshot(SAMPLE);
    let mut ui = CompositionUi::from_model(&snap.model);
    ui.text = "configuration cfg\n  Phys: A, B\n".to_owned();
    let mut h = dialog_harness(Dialog::Composition(ui), snap);
    h.run();
    h.get_by_label("노드 구성");
    h.get_by_label("OK").click();
    h.run();
    let sent = &h.state().sent;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let EditOp::SetComposition(Some(c)) = &sent[0] else {
        panic!("{:?}", sent[0]);
    };
    assert_eq!(c.configurations[0].composites[0].logical, ["A", "B"]);
    assert!(h.state().st.dialog.is_none(), "확인하면 대화상자가 닫힌다");
}

#[test]
fn composition_dialog_shows_why_it_cannot_be_confirmed() {
    let snap = snapshot(SAMPLE);
    let mut ui = CompositionUi::from_model(&snap.model);
    // 합성 노드가 없는 구성
    ui.text = "configuration cfg\n".to_owned();
    let mut h = dialog_harness(Dialog::Composition(ui), snap);
    h.run();
    h.get_by_label_contains("합성 노드가 하나는 있어야 한다");
    assert!(h.get_by_label("OK").accesskit_node().is_disabled());
    // 줄 모양이 틀리면 몇 번째 줄인지 알려 준다
    let snap = snapshot(SAMPLE);
    let mut ui = CompositionUi::from_model(&snap.model);
    ui.text = "configuration cfg\n  이게 뭐지\n".to_owned();
    let mut h = dialog_harness(Dialog::Composition(ui), snap);
    h.run();
    h.get_by_label_contains("2번째 줄");
    assert!(h.get_by_label("OK").accesskit_node().is_disabled());
}

#[test]
fn composition_dialog_previews_new_findings_but_still_allows_ok() {
    let snap = snapshot(SAMPLE);
    let mut ui = CompositionUi::from_model(&snap.model);
    ui.text = "configuration cfg\n  Phys: A, Ghost\n".to_owned();
    let mut h = dialog_harness(Dialog::Composition(ui), snap);
    h.run();
    h.get_by_label("이 변경으로 새로 생기는 검사 결과:");
    h.get_by_label_contains("[0109]");
    assert!(!h.get_by_label("OK").accesskit_node().is_disabled());
}

#[test]
fn cancelling_a_dialog_sends_nothing() {
    let snap = snapshot(SAMPLE);
    let ui = CompositionUi::from_model(&snap.model);
    let mut h = dialog_harness(Dialog::Composition(ui), snap);
    h.run();
    h.get_by_label("취소").click();
    h.run();
    assert!(h.state().sent.is_empty());
    assert!(h.state().st.dialog.is_none());
}

// ---- 프레임·삭제 대화상자 ------------------------------------------------------------------------

#[test]
fn frame_dialog_for_an_existing_frame_confirms_without_changes() {
    let snap = snapshot(SAMPLE);
    let id = snap.model.find_frame("FA").expect("FA");
    let ui = FrameUi::from_frame(&snap.model, id).expect("dialog");
    let mut h = dialog_harness(Dialog::Frame(ui), snap.clone());
    h.run();
    h.get_by_label("프레임 편집");
    h.get_by_label("OK").click();
    h.run();
    let sent = &h.state().sent;
    assert_eq!(sent.len(), 1);
    // 그대로 확인하면 모델이 바뀌지 않는다
    let mut after = snap.model.clone();
    sent[0].apply(&mut after).expect("apply");
    assert_eq!(after, snap.model);
}

#[test]
fn delete_dialog_lists_what_goes_with_the_target() {
    let snap = snapshot(SAMPLE);
    let what = "노드 `A`".to_owned();
    let d = DeleteUi::new(&snap.model, what, EditOp::DeleteNode("A".into())).expect("delete");
    let mut h = dialog_harness(Dialog::Delete(d), snap);
    h.run();
    h.get_by_label("삭제 확인");
    h.get_by_label("노드 `A`을(를) 삭제할까?");
    h.get_by_label("함께 지우거나 정리되는 항목:");
    h.get_by_label("삭제").click();
    h.run();
    assert_eq!(h.state().sent, [EditOp::DeleteNode("A".into())]);
}

// ---- 메인 화면(내비게이션 · 스케줄 뷰 · 컨텍스트 뷰 · 속성 창) --------------------------------------

/// 메인 화면 시험용 상태.
struct Main {
    snap: Arc<ModelSnapshot>,
    st: UiState,
    hex: bool,
    sent: Vec<(EditOp, Option<SelKey>)>,
}

fn main_harness(snap: Arc<ModelSnapshot>, st: UiState, hex: bool) -> Harness<'static, Main> {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        // 한 걸음이 0.25초면 두 번 누르기가 더블클릭 간격(0.3초)을 넘는다
        .with_step_dt(0.02)
        .build_ui_state(
            |ui, s: &mut Main| {
                if let Some(x) = crate::app::main_view(ui, &s.snap, &mut s.st, s.hex) {
                    s.sent.push(x);
                }
            },
            Main {
                snap,
                st,
                hex,
                sent: Vec::new(),
            },
        )
}

fn selected(snap: &ModelSnapshot, f: impl FnOnce(&LdfModel) -> Selection) -> UiState {
    UiState {
        selection: f(&snap.model),
        ..UiState::default()
    }
}

#[test]
fn main_view_lists_the_tree_and_asks_for_a_selection() {
    let snap = snapshot(SAMPLE);
    let mut h = main_harness(snap, UiState::default(), false);
    h.run();
    for label in [
        "Frames/Commands",
        "Nodes/Frames",
        "Unconditional Frames",
        "FCmd (1)",
        "FA (2)",
        "Predefined Commands",
        "AssignFrameIdRange",
        "Schedule Tables",
        "스케줄 테이블이나 슬롯을 선택하세요",
        "왼쪽 트리에서 항목을 선택하세요",
    ] {
        h.get_by_label(label);
    }
    // 스케줄 뷰: 표 머리글과 테이블 한 줄
    h.get_by_label("Max. Frame Time [ms]");
    h.get_by_label("20.00");
}

#[test]
fn selecting_a_frame_shows_properties_signals_and_the_bit_layout() {
    let snap = snapshot(SAMPLE);
    let st = selected(&snap, |m| Selection::Frame(m.find_frame("FA").expect("FA")));
    let mut h = main_harness(snap, st, false);
    h.run();
    for label in [
        "Frame Properties",
        "Unconditional",
        "0x42", // 보호된 ID
        "Enhanced",
        "Signals in Frame <FA>",
        "Pos [0..15]",
        "한 줄 = 1바이트 (왼쪽이 비트 7)",
    ] {
        h.get_by_label(label);
    }
}

#[test]
fn clicking_a_tree_item_selects_it() {
    let snap = snapshot(SAMPLE);
    let id = snap.model.find_frame("FA").expect("FA");
    let mut h = main_harness(snap, UiState::default(), false);
    h.run();
    h.get_by_label("FA (2)").click();
    h.run();
    assert_eq!(h.state().st.selection, Selection::Frame(id));
    h.get_by_label("Frame Properties");
    // 다른 프레임을 고르면 속성이 따라 바뀐다
    h.get_by_label("FCmd (1)").click();
    h.run();
    assert_ne!(h.state().st.selection, Selection::Frame(id));
    assert!(h.query_by_label("Pos [0..15]").is_none());
}

#[test]
fn the_search_box_filters_the_tree() {
    let snap = snapshot(SAMPLE);
    let mut h = main_harness(snap, UiState::default(), false);
    h.run();
    // 입력은 포커스가 있는 칸으로 간다
    h.get_by_role(Role::TextInput).click();
    h.run();
    h.get_by_role(Role::TextInput).type_text("fcm");
    h.run();
    assert_eq!(h.state().st.search, "fcm");
    h.get_by_label("FCmd (1)");
    assert!(
        h.query_by_label("FA (2)").is_none(),
        "검색에 맞지 않는 프레임은 숨는다"
    );
    assert!(h.query_by_label("AssignNAD").is_none());
    h.get_by_role(Role::TextInput).type_text("zzz");
    h.run();
    h.get_by_label("일치하는 항목이 없다");
}

#[test]
fn hex_mode_changes_every_number_that_is_shown() {
    let snap = snapshot(SAMPLE);
    let st = selected(&snap, |m| Selection::Frame(m.find_frame("FA").expect("FA")));
    let mut h = main_harness(snap, st, true);
    h.run();
    h.get_by_label("FA (0x02)");
    h.get_by_label("0x02"); // Frame ID
    assert!(h.query_by_label("FA (2)").is_none());
}

#[test]
fn the_nodes_tab_lists_what_each_node_publishes_and_subscribes() {
    let snap = snapshot(SAMPLE);
    let mut h = main_harness(snap, UiState::default(), false);
    h.run();
    h.get_by_label("Nodes/Frames").click();
    h.run();
    assert_eq!(h.state().st.nav_tab, NavTab::Nodes);
    h.get_by_label("A");
    h.get_by_label("B");
    assert!(h.query_by_label("Unconditional Frames").is_none());
}

#[test]
fn selecting_a_schedule_shows_its_slots_properties() {
    let snap = snapshot(SAMPLE);
    let st = selected(&snap, |m| {
        Selection::Schedule(m.find_schedule("T").expect("T"))
    });
    let mut h = main_harness(snap, st, false);
    h.run();
    h.get_by_label("Table Id");
    h.get_by_label("Slots");
    assert!(
        h.query_by_label("스케줄 테이블이나 슬롯을 선택하세요")
            .is_none()
    );
}

/// 라벨을 더블클릭한다(한 걸음이 짧아야 두 번 누르기가 더블클릭으로 읽힌다).
fn double_click(h: &mut Harness<'_, Main>, label: &str) {
    let pos = h.get_by_label(label).rect().center();
    h.event(egui::Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false, true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

/// 속성 창에서 고치는 중인 입력칸(검색칸이 아닌 쪽).
fn inline_input<'a>(h: &'a Harness<'_, Main>, value: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::TextInput)
        .find(|n| n.value().as_deref() == Some(value))
        .expect("입력칸")
}

fn frame_fa_harness() -> Harness<'static, Main> {
    let snap = snapshot(SAMPLE);
    let st = selected(&snap, |m| Selection::Frame(m.find_frame("FA").expect("FA")));
    let mut h = main_harness(snap, st, false);
    h.run();
    h
}

#[test]
fn double_clicking_a_property_edits_it_in_place() {
    let mut h = frame_fa_harness();
    double_click(&mut h, "FA");
    assert_eq!(
        h.get_all_by_role(Role::TextInput).count(),
        2,
        "검색칸과 고치는 칸"
    );
    inline_input(&h, "FA").type_text("X");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let sent = &h.state().sent;
    assert_eq!(sent.len(), 1, "{sent:?}");
    let (op, select) = &sent[0];
    assert!(format!("{op:?}").contains("FAX"), "{op:?}");
    assert!(select.is_some(), "이름을 바꾸면 선택이 새 이름을 따라간다");
}

#[test]
fn an_invalid_inline_value_shows_why_and_is_not_applied() {
    let mut h = frame_fa_harness();
    double_click(&mut h, "FA");
    inline_input(&h, "FA").type_text("!");
    h.run();
    // 입력하는 동안 사유가 보인다
    h.get_by_label_contains("이름으로 쓸 수 없다");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().sent.is_empty(), "{:?}", h.state().sent);
    // 닫혔으므로 다시 글자로 돌아온다
    assert_eq!(h.get_all_by_role(Role::TextInput).count(), 1);
}

#[test]
fn escape_cancels_an_inline_edit() {
    let mut h = frame_fa_harness();
    double_click(&mut h, "FA");
    inline_input(&h, "FA").type_text("Y");
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().sent.is_empty());
    assert_eq!(h.get_all_by_role(Role::TextInput).count(), 1);
    h.get_by_label("FA");
}

// ---- 리비전 창 ----------------------------------------------------------------------------------

/// 리비전 창 시험용 상태: 리비전 둘이 든 세션과 창이 돌려준 동작.
struct Revs {
    list: Vec<ldf_engine::RevisionInfo>,
    current: Option<i64>,
    st: RevState,
    actions: Vec<RevAction>,
}

#[allow(clippy::field_reassign_with_default)] // 비공개 필드가 있어 구조체 갱신 문법을 쓸 수 없다
fn open_revisions() -> RevState {
    let mut st = RevState::default();
    st.open = true;
    st
}

/// 같은 프로젝트에 두 판(둘째는 속도와 슬레이브가 다르다)을 가져와 만든 상태.
fn revisions_state() -> (Session, Revs) {
    let mut s = Session::open_in_memory().expect("session");
    s.import_bytes("one.ldf", SAMPLE.as_bytes()).expect("one");
    let second = SAMPLE
        .replace("19.2 kbps", "10.4 kbps")
        .replace("Slaves: A, B;", "Slaves: A, B, C;");
    s.import_bytes("two.ldf", second.as_bytes()).expect("two");
    let list = s.revisions().expect("list");
    assert_eq!(list.len(), 2);
    let current = list.last().map(|r| r.id);
    let st = open_revisions();
    (
        s,
        Revs {
            list,
            current,
            st,
            actions: Vec::new(),
        },
    )
}

fn revisions_harness(state: Revs) -> Harness<'static, Revs> {
    Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, s: &mut Revs| {
                if let Some(a) = revisions::show(ui.ctx(), &s.list, s.current, &mut s.st) {
                    s.actions.push(a);
                }
            },
            state,
        )
}

#[test]
fn revision_list_marks_the_open_one_and_opens_another() {
    let (_s, state) = revisions_state();
    let (first, second) = (state.list[0].id, state.list[1].id);
    let mut h = revisions_harness(state);
    h.run();
    h.get_by_label_contains("#1 one.ldf");
    h.get_by_label_contains("#2 two.ldf  ◀ 열림");
    // 열린 리비전의 "열기"는 꺼져 있고, 다른 쪽을 누르면 그 리비전을 열라고 알린다
    let buttons: Vec<_> = h.get_all_by_label("열기").collect();
    assert_eq!(buttons.len(), 2);
    let enabled: Vec<_> = buttons
        .iter()
        .filter(|b| !b.accesskit_node().is_disabled())
        .collect();
    assert_eq!(enabled.len(), 1);
    enabled[0].click();
    h.run();
    assert!(
        matches!(h.state().actions.as_slice(), [RevAction::Open(id)] if *id == first),
        "{:?} (열린 것: {second})",
        h.state().actions.len()
    );
}

#[test]
fn deleting_a_revision_asks_first() {
    let (_s, state) = revisions_state();
    let first = state.list[0].id;
    let mut h = revisions_harness(state);
    h.run();
    h.get_all_by_label("삭제…").next().expect("삭제…").click();
    h.run();
    h.get_by_label("리비전 삭제");
    h.get_by_label(&format!("리비전 #{first}을(를) 삭제할까? 되돌릴 수 없다."));
    // 취소하면 아무 동작도 없다
    h.get_by_label("취소").click();
    h.run();
    assert!(h.state().actions.is_empty());
    assert!(h.query_by_label("리비전 삭제").is_none());
    // 다시 열어 삭제하면 그 리비전을 지우라고 알린다
    h.get_all_by_label("삭제…").next().expect("삭제…").click();
    h.run();
    h.get_by_label("삭제").click();
    h.run();
    assert!(matches!(h.state().actions.as_slice(), [RevAction::Delete(id)] if *id == first));
}

#[test]
fn comparing_two_revisions_shows_the_differences() {
    let (mut s, mut state) = revisions_state();
    let (first, second) = (state.list[0].id, state.list[1].id);
    let mut h = revisions_harness({
        state.st.diff = None;
        state
    });
    h.run();
    h.get_by_label("비교 (Diff)").click();
    h.run();
    h.get_by_label("두 리비전을 고르고 비교를 누른다.");
    // 기본 선택은 열린 리비전(이후)과 그 앞 리비전(이전)이다
    h.get_by_label("비교").click();
    h.run();
    assert!(
        matches!(h.state().actions.as_slice(), [RevAction::Diff { before, after }] if (*before, *after) == (first, second)),
    );
    // 앱이 비교 결과를 채워 주면 표가 나온다
    let diff = s.diff_revisions(first, second).expect("diff");
    h.state_mut().st.diff = Some(DiffShown::new(first, second, Arc::new(diff)));
    h.run();
    h.get_by_label_contains("추가 ");
    h.get_by_label("speed_bps");
    h.get_by_label("19200");
    h.get_by_label("10400");
    // 저장 버튼은 결과가 있을 때만 켜진다
    assert!(!h.get_by_label("CSV로 저장").accesskit_node().is_disabled());
    h.get_by_label("HTML로 저장").click();
    h.run();
    assert!(matches!(
        h.state().actions.last(),
        Some(RevAction::SaveDiff { csv: false })
    ));
}

#[test]
fn comparing_needs_two_revisions() {
    let mut s = Session::open_in_memory().expect("session");
    s.import_bytes("one.ldf", SAMPLE.as_bytes()).expect("one");
    let list = s.revisions().expect("list");
    let state = Revs {
        current: list.last().map(|r| r.id),
        list,
        st: open_revisions(),
        actions: Vec::new(),
    };
    let mut h = revisions_harness(state);
    h.run();
    h.get_by_label("비교 (Diff)").click();
    h.run();
    h.get_by_label_contains("비교하려면 리비전이 두 개 이상 있어야 한다");
}

// ---- 내보내기 창 ---------------------------------------------------------------------------------

struct Exp {
    snap: Arc<ModelSnapshot>,
    st: ExportState,
    actions: Vec<ExportAction>,
}

#[allow(clippy::field_reassign_with_default)] // 비공개 필드가 있어 구조체 갱신 문법을 쓸 수 없다
fn export_harness() -> Harness<'static, Exp> {
    let mut st = ExportState::default();
    st.open = true;
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, s: &mut Exp| {
                if let Some(a) = export::show(ui.ctx(), &s.snap, &mut s.st) {
                    s.actions.push(a);
                }
            },
            Exp {
                snap: snapshot(SAMPLE),
                st,
                actions: Vec::new(),
            },
        )
}

#[test]
fn export_window_sends_the_chosen_options() {
    let mut h = export_harness();
    h.run();
    h.get_by_label("내보내기");
    h.get_by_label("편집하지 않았으므로 원문(주석·서식 포함)을 그대로 내보낸다.");
    h.get_by_label("파일 선택…").click();
    h.run();
    let [ExportAction::Choose(o)] = h.state().actions.as_slice() else {
        panic!("동작 {}개", h.state().actions.len());
    };
    assert_eq!(o.format, ExportFormat::Ldf);
    assert_eq!(
        (o.target, o.eol, o.encoding.as_deref()),
        (Target::Keep, None, None)
    );
    assert!(!o.allow_errors);
    assert!(!h.state().st.open, "파일을 고르러 가면 창이 닫힌다");
}

#[test]
fn export_window_target_version_and_line_endings() {
    let mut h = export_harness();
    h.run();
    h.get_by_label("CRLF").click();
    h.run();
    // 대상 버전 콤보상자(인코딩 콤보상자와 구분하려고 지금 값으로 찾는다)
    h.get_all_by_role(Role::ComboBox)
        .find(|n| n.value().as_deref() == Some("원본 버전 유지"))
        .expect("대상 버전 콤보상자")
        .click();
    h.run();
    h.get_by_label("LIN 2.0").click();
    h.run();
    h.get_by_label("대상 버전에서 허용되지 않는 항목은 빠지고 경고로 알린다.");
    h.get_by_label("파일 선택…").click();
    h.run();
    let [ExportAction::Choose(o)] = h.state().actions.as_slice() else {
        panic!("동작 {}개", h.state().actions.len());
    };
    assert_eq!((o.target, o.eol), (Target::V2_0, Some(Eol::CrLf)));
}

#[test]
fn model_json_export_hides_the_ldf_only_choices() {
    let mut h = export_harness();
    h.run();
    h.get_by_label("대상 버전");
    h.get_by_label("모델 JSON").click();
    h.run();
    assert!(h.query_by_label("대상 버전").is_none());
    assert!(h.query_by_label("줄바꿈").is_none());
    h.get_by_label("모델을 이름으로 서로 참조하는 JSON으로 내보낸다(UTF-8).");
    h.get_by_label("파일 선택…").click();
    h.run();
    let [ExportAction::Choose(o)] = h.state().actions.as_slice() else {
        panic!("동작 {}개", h.state().actions.len());
    };
    assert_eq!(o.format, ExportFormat::Json);
}

#[test]
fn export_with_remaining_errors_asks_for_confirmation() {
    let blocked = || Blocked {
        revision: 7,
        path: std::path::PathBuf::from("out.ldf"),
        options: ExportOptions {
            format: ExportFormat::Ldf,
            target: Target::Keep,
            eol: None,
            encoding: None,
            allow_errors: false,
        },
        errors: 3,
    };
    for (button, expect) in [("그래도 내보내기", 0), ("Checker 열기", 1), ("취소", 2)] {
        let mut h = export_harness();
        h.state_mut().st.open = false;
        h.state_mut().st.blocked = Some(blocked());
        h.run();
        h.get_by_label("오류가 남아 있다");
        h.get_by_label("이 리비전에는 오류 진단이 3개 남아 있다.");
        h.get_by_label(button).click();
        h.run();
        let acts = &h.state().actions;
        match (expect, acts.as_slice()) {
            (0, [ExportAction::Retry(b)]) => assert_eq!((b.revision, b.errors), (7, 3)),
            (1, [ExportAction::OpenChecker]) | (2, []) => {}
            _ => panic!("{button}: 동작 {}개", acts.len()),
        }
        assert!(h.state().st.blocked.is_none(), "{button}");
    }
}

// ---- Checker 창 ----------------------------------------------------------------------------------

struct Chk {
    snap: Arc<ModelSnapshot>,
    rules: ldf_engine::RuleSettings,
    st: CheckerState,
    actions: Vec<CheckerAction>,
}

#[allow(clippy::field_reassign_with_default)] // 비공개 필드가 있어 구조체 갱신 문법을 쓸 수 없다
fn checker_harness(src: &str) -> Harness<'static, Chk> {
    let mut st = CheckerState::default();
    st.open = true;
    Harness::builder()
        .with_size(egui::vec2(1300.0, 800.0))
        .build_ui_state(
            |ui, s: &mut Chk| {
                if let Some(a) = checker::show(ui.ctx(), &s.snap, &s.rules, &mut s.st) {
                    s.actions.push(a);
                }
            },
            Chk {
                snap: snapshot(src),
                rules: ldf_engine::RuleSettings::default(),
                st,
                actions: Vec::new(),
            },
        )
}

fn broken() -> String {
    SAMPLE.replace("Pos: 16, 0, A, M;", "Pos: 16, 0, Ghost, M;")
}

#[test]
fn checker_window_groups_results_by_category() {
    let mut h = checker_harness(&broken());
    h.run();
    h.get_by_label("LIN Consistency Checker");
    h.get_by_label_contains("Basic checks");
    h.get_by_label_contains("Mapping checks");
    h.get_by_label_contains("[0020]");
    // 오른쪽에는 원문이 편집기로 열려 있다
    let editor = h.get_by_role(Role::MultilineTextInput);
    assert!(
        editor
            .value()
            .is_some_and(|v| v.contains("LIN_description_file"))
    );
    assert!(h.get_all_by_label_contains(" results").count() >= 1);
}

#[test]
fn checker_window_reports_a_clean_file() {
    // 진단이 없는 최소 파일
    let clean = "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: S; }\nSignals { A: 8, 0, M, S; E: 1, 0, S, M; }\n\
Frames { F: 1, M, 1 { A, 0; } G: 2, S, 1 { E, 0; } }\n\
Node_attributes { S { LIN_protocol = \"2.1\"; configured_NAD = 1; product_id = 1, 2, 3; response_error = E;\n\
 P2_min = 50 ms; ST_min = 0 ms; configurable_frames { F; G; } } }\nSchedule_tables { T { F delay 10 ms; G delay 10 ms; } }\n";
    let snap = snapshot(clean);
    let mut h = checker_harness(clean);
    h.run();
    assert!(snap.diagnostics.is_empty(), "{:?}", snap.diagnostics);
    h.get_by_label("문제 없음");
}

#[test]
fn checker_window_saves_the_report_as_csv() {
    let mut h = checker_harness(&broken());
    h.run();
    h.get_by_label("보고서 CSV 저장").click();
    h.run();
    let [CheckerAction::SaveCsv(bytes)] = h.state().actions.as_slice() else {
        panic!("동작 {}개", h.state().actions.len());
    };
    assert!(
        bytes.starts_with(&[0xEF, 0xBB, 0xBF]),
        "Excel이 UTF-8로 읽도록 BOM이 있어야 한다"
    );
    assert!(String::from_utf8_lossy(bytes).contains("0020"));
}

#[test]
fn run_checks_is_enabled_only_after_the_text_changes() {
    let mut h = checker_harness(&broken());
    h.run();
    assert!(
        h.get_by_label("Run Checks (적용 후 재검사)")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_role(Role::MultilineTextInput).click();
    h.run();
    h.get_by_role(Role::MultilineTextInput)
        .type_text("// 메모\n");
    h.run();
    h.get_by_label_contains("수정됨");
    let run = h.get_by_label("Run Checks (적용 후 재검사)");
    assert!(!run.accesskit_node().is_disabled());
    run.click();
    h.run();
    let [CheckerAction::RunChecks(text)] = h.state().actions.as_slice() else {
        panic!("동작 {}개", h.state().actions.len());
    };
    assert!(text.contains("// 메모") && text.contains("LIN_description_file"));
}

#[test]
fn rule_settings_window_lists_rules_and_sends_changes() {
    let mut h = checker_harness(&broken());
    h.run();
    h.get_by_label("규칙 설정…").click();
    h.run();
    h.get_by_label("검사 규칙 설정");
    h.get_by_label("바꾼 설정은 프로젝트(.ldfdb)에 저장되고 열려 있는 파일에 바로 적용된다.");
    // 규칙 줄마다 켜기 칸이 있다: 첫 번째를 끄면 그 규칙 설정이 나간다
    let boxes: Vec<_> = h.get_all_by_role(Role::CheckBox).collect();
    assert!(boxes.len() >= 20, "규칙 {}개", boxes.len());
    boxes[0].click();
    h.run();
    assert!(
        matches!(h.state().actions.as_slice(), [CheckerAction::SetRule { setting, .. }] if !setting.enabled),
        "{}",
        h.state().actions.len()
    );
}
