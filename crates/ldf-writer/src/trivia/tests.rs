use super::*;
use crate::{WriteOptions, write};
use ldf_model::{EditOp, load};

const OLD: &str = "// 머리말\nLIN_description_file;\n// 버전 설명\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps; // 속도\n\n\
/* 노드 설명\n   두 줄 */\nNodes {\n  // 마스터\n  Master: M, 5 ms, 0.1 ms;\n  Slaves: A; // 슬레이브들\n  // 노드 끝 직전\n}\n\
Signals {\n  // 명령\n  Cmd: 8, 0, M, A;\n  // 위치 (지워질 시그널)\n  Pos: 16, 0, A, M;\n} // 시그널 끝\n\
Frames { F: 1, M, 1 { Cmd, 0; } }\n// 파일 끝 주석\n";

fn rewrite(old: &str) -> String {
    write(&load(old).model, &WriteOptions::default()).text
}

#[test]
fn rewriting_without_edits_keeps_every_comment_with_its_statement() {
    let out = carry_comments(OLD, &rewrite(OLD));
    for c in [
        "// 머리말",
        "// 버전 설명",
        "// 속도",
        "/* 노드 설명\n   두 줄 */",
        "// 마스터",
        "// 슬레이브들",
        "// 노드 끝 직전",
        "// 명령",
        "// 시그널 끝",
        "// 파일 끝 주석",
    ] {
        assert!(out.contains(c), "`{c}`이(가) 사라졌다:\n{out}");
    }
    // 위치도 맞다
    let pos = |s: &str| out.find(s).unwrap_or_else(|| panic!("{s}\n{out}"));
    assert!(pos("// 버전 설명") < pos("LIN_protocol_version"));
    assert!(pos("LIN_protocol_version") < pos("LIN_speed"));
    assert!(out.contains("LIN_speed = 19.2 kbps;  // 속도"), "{out}");
    assert!(pos("// 마스터") < pos("Master: M") && pos("Master: M") < pos("Slaves: A"));
    assert!(out.contains("Slaves: A;  // 슬레이브들"), "{out}");
    assert!(pos("// 노드 끝 직전") > pos("Slaves: A") && pos("// 노드 끝 직전") < pos("Signals {"));
    assert!(pos("// 명령") < pos("Cmd: 8"));
    assert!(out.ends_with("// 파일 끝 주석\n"), "{out}");
}

#[test]
fn the_result_reads_back_to_the_same_model_and_carrying_again_changes_nothing() {
    let model = load(OLD).model;
    let out = carry_comments(OLD, &rewrite(OLD));
    let again = load(&out);
    assert!(
        again.diagnostics.iter().all(|d| d.code < 9000),
        "{:?}",
        again.diagnostics
    );
    assert!(ldf_model::semantically_equal(&model, &again.model));
    // 고정점: 결과를 다시 쓰고 주석을 옮겨도 같다
    assert_eq!(carry_comments(&out, &rewrite(&out)), out);
}

#[test]
fn comments_of_deleted_statements_go_away_and_the_rest_stay() {
    let mut m = load(OLD).model;
    EditOp::DeleteSignal("Pos".into())
        .apply(&mut m)
        .expect("delete");
    let out = carry_comments(OLD, &write(&m, &WriteOptions::default()).text);
    assert!(!out.contains("지워질 시그널"), "{out}");
    assert!(
        out.contains("// 명령") && out.contains("// 시그널 끝") && out.contains("// 속도"),
        "{out}"
    );
}

#[test]
fn new_statements_have_no_comments_and_edits_keep_neighbours() {
    let mut m = load(OLD).model;
    EditOp::AddSlave("B".into()).apply(&mut m).expect("add");
    let out = carry_comments(OLD, &write(&m, &WriteOptions::default()).text);
    assert!(out.contains("Slaves: A, B;"), "{out}");
    assert!(out.contains("// 슬레이브들"), "{out}");
}

#[test]
fn crlf_is_kept_and_unrelated_texts_are_untouched() {
    let crlf = OLD.replace('\n', "\r\n");
    let new = write(
        &load(&crlf).model,
        &WriteOptions {
            eol: crate::Eol::CrLf,
            ..WriteOptions::default()
        },
    )
    .text;
    let out = carry_comments(&crlf, &new);
    assert!(
        out.contains("// 속도") && !out.replace("\r\n", "").contains('\n'),
        "{out:?}"
    );
    // 주석이 없으면 새 텍스트 그대로
    assert_eq!(
        carry_comments("Nodes { Master: M, 1 ms, 0 ms; }", "x\n"),
        "x\n"
    );
}

#[test]
fn duplicate_names_are_matched_by_occurrence_and_garbage_does_not_panic() {
    let old = "Signals {\n  // 첫째\n  A: 8, 0, M, S;\n  // 둘째\n  A: 8, 0, M, S;\n}\n";
    let new = "Signals {\n  A: 8, 0, M, S;\n  A: 8, 0, M, S;\n}\n";
    let out = carry_comments(old, new);
    let (a, b) = (
        out.find("// 첫째").expect("첫째"),
        out.find("// 둘째").expect("둘째"),
    );
    assert!(a < b);
    for junk in ["", "{", "}}}", "/* x", "// only\n", "A { // c\n"] {
        let _ = carry_comments(junk, "Nodes { Master: M, 1 ms, 0 ms; }\n");
        let _ = carry_comments("Nodes { // c\n Master: M, 1 ms, 0 ms; }\n", junk);
    }
}
