use super::*;
use ldf_model::{EditOp, diff, load};

fn rev(id: i64, parent: Option<i64>) -> RevisionInfo {
    RevisionInfo {
        id,
        project_id: 1,
        parent_id: parent,
        label: format!("r{id}"),
        source_name: None,
        created_at: String::new(),
        protocol_version: None,
        language_version: None,
        speed_bps: None,
        encoding: "utf-8".into(),
    }
}

fn ids(rows: &[TreeRow]) -> Vec<(i64, usize)> {
    rows.iter().map(|r| (r.id, r.depth)).collect()
}

#[test]
fn children_follow_their_parent_depth_first() {
    //  1 ─ 2 ─ 4
    //   └─ 3        5(별도 루트)
    let revs = [
        rev(1, None),
        rev(2, Some(1)),
        rev(3, Some(1)),
        rev(4, Some(2)),
        rev(5, None),
    ];
    let rows = tree_rows(&revs);
    assert_eq!(ids(&rows), vec![(1, 0), (2, 1), (4, 2), (3, 1), (5, 0)]);
    assert_eq!(rows[0].children, 2);
    assert_eq!(rows[1].children, 1);
    assert_eq!(rows[4].children, 0);
}

#[test]
fn a_missing_parent_makes_a_root_and_cycles_do_not_lose_or_repeat_revisions() {
    // 부모가 지워진 리비전은 루트
    let rows = tree_rows(&[rev(7, Some(3)), rev(8, Some(7))]);
    assert_eq!(ids(&rows), vec![(7, 0), (8, 1)]);
    // 서로를 부모로 가리키는 손상된 데이터
    let cyc = tree_rows(&[rev(1, Some(2)), rev(2, Some(1)), rev(3, Some(3))]);
    let mut got: Vec<i64> = cyc.iter().map(|r| r.id).collect();
    got.sort_unstable();
    assert_eq!(got, vec![1, 2, 3]);
    assert!(tree_rows(&[]).is_empty());
}

#[test]
fn long_chains_do_not_overflow_the_stack() {
    let revs: Vec<RevisionInfo> = (1..=5000)
        .map(|i| rev(i, (i > 1).then_some(i - 1)))
        .collect();
    let rows = tree_rows(&revs);
    assert_eq!(rows.len(), 5000);
    assert_eq!(rows.last().map(|r| r.depth), Some(4999));
}

#[test]
fn diff_rows_put_a_header_before_each_objects_fields() {
    let a = load("LIN_speed = 19.2 kbps;\nNodes { Master: M, 1 ms, 0 ms; Slaves: S; }").model;
    let mut b = a.clone();
    EditOp::Batch(vec![EditOp::SetSpeed(10_400), EditOp::AddSlave("T".into())])
        .apply(&mut b)
        .expect("apply");
    let rows = diff_rows(&diff(&a, &b));
    let header = rows
        .iter()
        .position(|r| r.kind == "Header")
        .expect("header row");
    assert_eq!(rows[header].change, "변경");
    assert_eq!(rows[header].class, Class::Changed);
    let speed = &rows[header + 1];
    assert_eq!(
        (
            speed.field.as_str(),
            speed.before.as_str(),
            speed.after.as_str()
        ),
        ("speed_bps", "19200", "10400")
    );
    assert!(speed.kind.is_empty(), "필드 줄은 종류 칸이 비어 있다");
    let node = rows.iter().find(|r| r.kind == "Node").expect("node row");
    assert_eq!((node.name.as_str(), node.class), ("T", Class::Added));
    assert!(diff_rows(&Diff::default()).is_empty());
}
