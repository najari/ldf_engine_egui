//! 리비전 비교와 삭제(FR-REV-03, FR-REV-05).

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{EditOp, Session};
use ldf_model::{Change, ObjectKind};

const LDF: &str = "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\nSignals { S: 8, 0, A, M; }\nFrames { F: 1, A, 1 { S, 0; } }\nSchedule_tables { T { F delay 10 ms; } }\n";

#[test]
fn two_revisions_are_compared_by_content() {
    let mut s = Session::open_in_memory().expect("session");
    let base = s
        .import_bytes("a.ldf", LDF.as_bytes())
        .expect("import")
        .snapshot;
    let edited = s
        .edit(
            &base,
            &EditOp::Batch(vec![EditOp::AddSlave("B".into()), EditOp::SetSpeed(10_400)]),
        )
        .expect("edit");
    let saved = s.save_edits(&edited, None).expect("save").snapshot;

    let d = s
        .diff_revisions(base.revision.id, saved.revision.id)
        .expect("diff");
    assert_eq!(d.counts().0, 1, "{d:?}"); // 노드 B가 추가
    assert!(
        d.entries
            .iter()
            .any(|e| e.kind == ObjectKind::Node && e.name == "B" && e.change == Change::Added)
    );
    assert!(
        d.entries.iter().any(
            |e| e.kind == ObjectKind::Header && e.fields.iter().any(|f| f.field == "speed_bps")
        )
    );
    // 같은 리비전끼리는 차이가 없다
    assert!(
        s.diff_revisions(saved.revision.id, saved.revision.id)
            .expect("diff")
            .is_empty()
    );
    // 거꾸로 비교하면 추가가 삭제가 된다
    let back = s
        .diff_revisions(saved.revision.id, base.revision.id)
        .expect("diff");
    assert!(
        back.entries
            .iter()
            .any(|e| e.name == "B" && e.change == Change::Removed)
    );
}

#[test]
fn deleting_a_revision_keeps_the_rest_consistent() {
    let dir = std::env::temp_dir().join(format!("ldf-revdel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::open(&dir.join("p.ldfdb")).expect("open");
    let base = s
        .import_bytes("a.ldf", LDF.as_bytes())
        .expect("import")
        .snapshot;
    let edited = s.edit(&base, &EditOp::AddSlave("B".into())).expect("edit");
    let child = s.save_edits(&edited, None).expect("save").snapshot;
    assert_eq!(s.revisions().expect("list").len(), 2);

    // 부모를 지우면 자식은 남고 부모 연결만 끊긴다
    s.delete_revision(base.revision.id).expect("delete");
    let left = s.revisions().expect("list");
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, child.revision.id);
    assert_eq!(left[0].parent_id, None);
    s.check_integrity().expect("integrity");
    assert!(
        s.delete_revision(base.revision.id).is_err(),
        "없는 리비전은 오류"
    );
    // 남은 리비전은 여전히 열린다
    assert!(
        s.open_revision(child.revision.id)
            .expect("open")
            .model
            .find_node("B")
            .is_some()
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
