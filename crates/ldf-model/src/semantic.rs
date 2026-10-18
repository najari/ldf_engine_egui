//! 의미 동등 비교: 원문 위치(`span`)를 무시하고 두 모델이 같은지 본다.
//!
//! 내보내기 검증(출력한 LDF를 다시 읽어 같은 모델인지, FR-EXP-02)과 왕복 테스트가 쓴다.

use crate::LdfModel;
use serde_json::Value;

fn normalized(m: &LdfModel) -> Value {
    let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
    strip_spans(&mut v);
    // notes는 원문에서 버려진 사실의 기록이지 모델의 의미가 아니다(출력 텍스트를 다시 읽으면 달라질 수 있다)
    if let Value::Object(map) = &mut v {
        map.remove("notes");
        // 최상위 문장 순서는 표현(서식)이지 의미가 아니다. 순서 보존은 별도로 검사한다
        map.remove("section_order");
    }
    v
}

fn strip_spans(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("span");
            map.values_mut().for_each(strip_spans);
        }
        Value::Array(items) => items.iter_mut().for_each(strip_spans),
        _ => {}
    }
}

fn first_difference(path: &str, a: &Value, b: &Value) -> Option<String> {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, va) in x {
                match y.get(k) {
                    Some(vb) => {
                        if let Some(d) = first_difference(&format!("{path}.{k}"), va, vb) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{path}.{k}: 오른쪽에 없다")),
                }
            }
            y.keys()
                .find(|k| !x.contains_key(*k))
                .map(|k| format!("{path}.{k}: 왼쪽에 없다"))
        }
        (Value::Array(x), Value::Array(y)) => {
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                if let Some(d) = first_difference(&format!("{path}[{i}]"), va, vb) {
                    return Some(d);
                }
            }
            (x.len() != y.len()).then(|| format!("{path}: 길이 {} != {}", x.len(), y.len()))
        }
        _ => (a != b).then(|| format!("{path}: {a} != {b}")),
    }
}

/// 두 모델이 원문 위치를 제외하고 같으면 `None`, 다르면 처음 다른 곳의 경로와 값.
pub fn semantic_diff(a: &LdfModel, b: &LdfModel) -> Option<String> {
    first_difference("$", &normalized(a), &normalized(b))
}

/// 두 모델이 원문 위치를 제외하고 같은지.
pub fn semantically_equal(a: &LdfModel, b: &LdfModel) -> bool {
    semantic_diff(a, b).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_are_ignored_but_content_is_not() {
        let a = crate::load("Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }").model;
        // 공백만 다른 같은 내용: 위치는 다르지만 의미는 같다
        let b = crate::load("\n\n  Nodes {  Master: M, 1 ms, 0 ms;\n Slaves: A;\n}").model;
        assert!(semantically_equal(&a, &b));
        assert_ne!(a, b, "스팬 때문에 구조 동등은 아니다");

        let c = crate::load("Nodes { Master: M, 2 ms, 0 ms; Slaves: A; }").model;
        let d = semantic_diff(&a, &c).expect("차이");
        assert!(d.contains("timebase_ms"), "{d}");
    }

    #[test]
    fn length_and_missing_key_differences_are_reported() {
        let a = crate::load("Nodes { Master: M, 1 ms, 0 ms; Slaves: A, B; }").model;
        let b = crate::load("Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }").model;
        assert!(semantic_diff(&a, &b).is_some());
    }
}
