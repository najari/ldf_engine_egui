//! 리비전 창이 보여 줄 값(egui 무관, 단위 테스트 대상): 부모–자식 트리 순서와 비교 결과의 표 행.

use ldf_engine::RevisionInfo;
use ldf_model::{Change, Diff};
use std::collections::HashMap;

/// 리비전 트리의 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    /// 리비전 ID.
    pub id: i64,
    /// 들여쓰기 깊이(루트가 0).
    pub depth: usize,
    /// 직접 자식 수.
    pub children: usize,
}

/// 부모–자식 관계를 깊이 우선 순서로 편다. 부모가 목록에 없는 리비전은 루트로 본다.
/// 같은 부모의 자식은 ID 순이다. 순환이 있어도(손상된 DB) 모든 리비전이 정확히 한 번 나온다.
pub fn tree_rows(revisions: &[RevisionInfo]) -> Vec<TreeRow> {
    let ids: Vec<i64> = revisions.iter().map(|r| r.id).collect();
    let mut children: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut roots = Vec::new();
    for r in revisions {
        match r.parent_id.filter(|p| ids.contains(p) && *p != r.id) {
            Some(p) => children.entry(p).or_default().push(r.id),
            None => roots.push(r.id),
        }
    }
    for list in children.values_mut() {
        list.sort_unstable();
    }
    roots.sort_unstable();

    let mut out = Vec::with_capacity(revisions.len());
    let mut seen = std::collections::HashSet::new();
    // 재귀 대신 명시적 스택(깊은 사슬에서도 안전하다)
    let mut stack: Vec<(i64, usize)> = roots.into_iter().rev().map(|r| (r, 0)).collect();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let kids = children.get(&id).cloned().unwrap_or_default();
        out.push(TreeRow {
            id,
            depth,
            children: kids.len(),
        });
        stack.extend(kids.into_iter().rev().map(|k| (k, depth + 1)));
    }
    // 순환에 갇혀 닿지 못한 리비전도 빠뜨리지 않는다
    for r in revisions {
        if seen.insert(r.id) {
            out.push(TreeRow {
                id: r.id,
                depth: 0,
                children: children.get(&r.id).map_or(0, Vec::len),
            });
        }
    }
    out
}

/// 비교 결과 표의 한 줄(객체 한 줄 + 그 필드들).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRow {
    /// 객체 종류.
    pub kind: &'static str,
    /// 이름(이름 변경이면 `옛 → 새`).
    pub name: String,
    /// 변경 종류 이름.
    pub change: &'static str,
    /// 색을 고르는 분류.
    pub class: Class,
    /// 필드(객체 머리 줄이면 비어 있다).
    pub field: String,
    /// 이전 값.
    pub before: String,
    /// 이후 값.
    pub after: String,
}

/// 비교 행의 색 분류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 추가.
    Added,
    /// 삭제.
    Removed,
    /// 변경·이름 변경.
    Changed,
}

/// 비교 결과를 표 행으로 편다. 객체마다 머리 줄 하나 뒤에 필드 줄이 이어진다.
pub fn diff_rows(d: &Diff) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for e in &d.entries {
        let class = match e.change {
            Change::Added => Class::Added,
            Change::Removed => Class::Removed,
            Change::Changed | Change::Renamed { .. } => Class::Changed,
        };
        let name = match &e.change {
            Change::Renamed { from } => format!("{from} → {}", e.name),
            _ => e.name.clone(),
        };
        rows.push(DiffRow {
            kind: e.kind.label(),
            name: name.clone(),
            change: e.change.label(),
            class,
            field: String::new(),
            before: String::new(),
            after: String::new(),
        });
        for f in &e.fields {
            rows.push(DiffRow {
                kind: "",
                name: String::new(),
                change: "",
                class,
                field: f.field.clone(),
                before: f.before.clone(),
                after: f.after.clone(),
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests;
