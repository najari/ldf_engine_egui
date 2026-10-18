//! 주석 옮기기(FR-EXP-03): 모델에서 다시 쓴 텍스트에 원문의 주석을 되살린다.
//!
//! 주석은 모델에 없다. 대신 원문과 새 텍스트를 각각 구문 트리로 읽어 문장마다 **이름표**(섹션 이름, 그 안의 첫 토큰, 같은 이름이
//! 여럿이면 몇 번째인지)를 붙이고, 원문의 주석을 "바로 뒤에 오는 문장의 앞", "같은 줄 문장의 뒤", "블록 끝(`}`) 앞/뒤"에 걸어 두었다가
//! 같은 이름표를 가진 새 문장 옆에 놓는다. 지워진 문장의 주석은 함께 사라진다.
//! 첫 문장 앞의 주석은 파일 머리말(`Header::leading_comment`)이 따로 맡는다.

use ldf_syntax::{Elem, Stmt, parse};
use std::collections::HashMap;

/// 주석이 걸리는 자리.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Slot {
    /// 문장 앞(앞 줄).
    Before(String),
    /// 문장과 같은 줄 끝.
    After(String),
    /// 블록의 닫는 `}` 앞 줄.
    BeforeClose(String),
    /// 블록의 닫는 `}`와 같은 줄 끝.
    AfterClose(String),
    /// 파일 끝.
    Eof,
}

/// 문장 하나의 위치 정보.
struct Node {
    key: String,
    start: usize,
    /// 이 문장이 가진 블록의 닫는 `}` 위치(없으면 `None`).
    close: Option<usize>,
}

const MAX_DEPTH: usize = 4;

fn collect(src: &str, stmts: &[Stmt], parent: &str, depth: usize, out: &mut Vec<Node>) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for s in stmts {
        let label = s.elems.iter().find_map(|e| match e {
            Elem::Token(t) => Some(t.span.text(src).to_owned()),
            Elem::Block(_) => None,
        });
        let label = label.unwrap_or_default();
        let n = seen.entry(label.clone()).or_insert(0);
        let key = format!("{parent}/{label}#{n}");
        *n += 1;
        let block = s.elems.iter().find_map(|e| match e {
            Elem::Block(b) => Some(b),
            Elem::Token(_) => None,
        });
        let close = block.and_then(|b| {
            let end = b.span.end;
            (end > b.span.start && src.as_bytes().get(end - 1) == Some(&b'}')).then(|| end - 1)
        });
        out.push(Node {
            key: key.clone(),
            start: s.span.start,
            close,
        });
        if depth < MAX_DEPTH
            && let Some(b) = block
        {
            collect(src, &b.stmts, &key, depth + 1, out);
        }
    }
}

fn nodes(src: &str) -> Vec<Node> {
    let parsed = parse(src);
    let mut out = Vec::new();
    collect(src, &parsed.file.stmts, "", 0, &mut out);
    out
}

fn line_start(src: &str, pos: usize) -> usize {
    src.get(..pos)
        .and_then(|s| s.rfind('\n'))
        .map_or(0, |i| i + 1)
}

fn line_end(src: &str, pos: usize) -> usize {
    src.get(pos..)
        .and_then(|s| s.find('\n'))
        .map_or(src.len(), |i| pos + i)
}

/// 원문의 주석을 걸 자리별로 모은다. 같은 자리에 여러 개면 원문 순서다.
fn collect_comments(old: &str) -> (HashMap<Slot, Vec<String>>, Vec<Slot>) {
    let parsed = parse(old);
    let ns = nodes(old);
    let first_stmt = parsed
        .file
        .stmts
        .first()
        .map_or(usize::MAX, |s| s.span.start);
    let mut map: HashMap<Slot, Vec<String>> = HashMap::new();
    let mut order: Vec<Slot> = Vec::new();
    let mut put = |slot: Slot, text: String| {
        if !map.contains_key(&slot) {
            order.push(slot.clone());
        }
        map.entry(slot).or_default().push(text);
    };
    for c in &parsed.file.comments {
        if c.span.start < first_stmt {
            continue;
        }
        let text = c.span.text(old).replace("\r\n", "\n").trim_end().to_owned();
        let ls = line_start(old, c.span.start);
        let before_on_line = old.get(ls..c.span.start).unwrap_or("").trim();
        if !before_on_line.is_empty() {
            // 같은 줄의 코드 뒤: `}`로 끝나면 그 블록 뒤, 아니면 그 줄에서 시작한 문장 뒤
            if before_on_line.ends_with('}')
                && let Some(n) = ns
                    .iter()
                    .filter(|n| n.close.is_some_and(|p| p >= ls && p < c.span.start))
                    .max_by_key(|n| n.start)
            {
                put(Slot::AfterClose(n.key.clone()), text);
            } else if let Some(n) = ns
                .iter()
                .filter(|n| n.start >= ls && n.start < c.span.start)
                .max_by_key(|n| n.start)
            {
                put(Slot::After(n.key.clone()), text);
            }
            continue;
        }
        // 독립한 줄: 뒤따르는 문장 앞, 또는 그 전에 오는 블록 끝 앞
        let next_stmt = ns
            .iter()
            .filter(|n| n.start >= c.span.end)
            .min_by_key(|n| n.start);
        let next_close = ns
            .iter()
            .filter_map(|n| n.close.map(|p| (p, n)))
            .filter(|(p, _)| *p >= c.span.end)
            .min_by_key(|(p, _)| *p);
        match (next_stmt, next_close) {
            (Some(s), Some((p, n))) if p < s.start => put(Slot::BeforeClose(n.key.clone()), text),
            (Some(s), _) => put(Slot::Before(s.key.clone()), text),
            (None, Some((_, n))) => put(Slot::BeforeClose(n.key.clone()), text),
            (None, None) => put(Slot::Eof, text),
        }
    }
    (map, order)
}

/// 원문(`old`)의 주석을 새로 쓴 텍스트(`new`)의 같은 문장 옆에 놓아 돌려준다.
/// 줄바꿈은 `new`가 쓰는 것을 따른다. 걸 자리가 없어진 주석은 버린다.
pub fn carry_comments(old: &str, new: &str) -> String {
    let crlf = new.contains("\r\n");
    let new_lf = new.replace("\r\n", "\n");
    let (map, order) = collect_comments(old);
    if map.is_empty() {
        return new.to_owned();
    }
    let ns = nodes(&new_lf);
    // (삽입 위치, 순서 번호, 넣을 글)
    let mut inserts: Vec<(usize, usize, String)> = Vec::new();
    for (i, slot) in order.iter().enumerate() {
        let Some(comments) = map.get(slot) else {
            continue;
        };
        match slot {
            Slot::Before(k) | Slot::BeforeClose(k) => {
                let Some(n) = ns.iter().find(|n| &n.key == k) else {
                    continue;
                };
                let anchor = match slot {
                    Slot::Before(_) => n.start,
                    _ => match n.close {
                        Some(p) => p,
                        None => continue,
                    },
                };
                let ls = line_start(&new_lf, anchor);
                let indent: String = new_lf[ls..]
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .collect();
                let mut text = String::new();
                for c in comments {
                    for (j, l) in c.split('\n').enumerate() {
                        // 여러 줄 주석은 첫 줄만 들여 쓰고 나머지는 원문 그대로 둔다
                        if j == 0 {
                            text.push_str(&indent);
                        }
                        text.push_str(l);
                        text.push('\n');
                    }
                }
                inserts.push((ls, i, text));
            }
            Slot::After(k) | Slot::AfterClose(k) => {
                let Some(n) = ns.iter().find(|n| &n.key == k) else {
                    continue;
                };
                let anchor = match slot {
                    Slot::After(_) => n.start,
                    _ => match n.close {
                        Some(p) => p,
                        None => continue,
                    },
                };
                let le = line_end(&new_lf, anchor);
                let joined = comments.join(" ").replace('\n', " ");
                inserts.push((le, i, format!("  {joined}")));
            }
            Slot::Eof => {
                let mut text = String::new();
                if !new_lf.ends_with('\n') {
                    text.push('\n');
                }
                text.push('\n');
                for c in comments {
                    text.push_str(c);
                    text.push('\n');
                }
                inserts.push((new_lf.len(), i, text));
            }
        }
    }
    // 뒤에서부터 넣어야 앞쪽 위치가 밀리지 않는다. 같은 위치면 원문 순서(번호)를 지킨다.
    inserts.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = new_lf;
    for (pos, _, text) in inserts {
        out.insert_str(pos, &text);
    }
    if crlf { out.replace('\n', "\r\n") } else { out }
}

#[cfg(test)]
mod tests;
