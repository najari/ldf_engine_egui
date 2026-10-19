//! egui 화면 시험(`egui_kittest`): 접근성 트리로 라벨을 찾아 클릭·입력하고, 대화상자가 돌려주는 연산을 확인한다.
//!
//! 화면 모양(픽셀)은 보지 않는다. 라벨이 보이는지, 버튼이 켜져 있는지, 누르면 어떤 `EditOp`가 나오는지를 본다.
//! 언어는 전역이라 병렬로 도는 다른 시험과 섞이지 않게 모두 한국어(기본)로 시험한다.

use super::edit::{DeleteUi, Dialog, EditState, Outcome, show};
use super::status;
use crate::view::edit::{CompositionUi, FrameUi};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use ldf_engine::{EditOp, ModelSnapshot, Session};
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
