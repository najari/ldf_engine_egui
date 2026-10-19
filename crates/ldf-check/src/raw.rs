//! 모델이 해석하지 않고 원문으로 보관한 섹션(`unknown_sections`)을 규칙이 읽을 수 있게 푸는 도우미.

use ldf_model::{LdfModel, RawSection};

/// 이름이 `name`(대소문자 무시)인 보관 섹션.
pub(crate) fn section<'a>(m: &'a LdfModel, names: &[&str]) -> Vec<&'a RawSection> {
    m.unknown_sections
        .iter()
        .filter(|s| names.iter().any(|n| s.name.eq_ignore_ascii_case(n)))
        .collect()
}

/// 텍스트에서 `{ ... }` 안쪽(첫 `{`와 마지막 `}` 사이). 괄호가 없으면 전체.
pub(crate) fn body(text: &str) -> &str {
    let start = text.find('{').map_or(0, |i| i + 1);
    let end = text.rfind('}').unwrap_or(text.len());
    text.get(start..end.max(start)).unwrap_or("")
}

/// 텍스트 안의 정수 토큰(10진·`0x` 16진)을 순서대로 모은다. 주석(`//`, `/* */`)은 건너뛴다.
pub(crate) fn numbers(text: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b.get(i) {
            Some(b'/') if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b.get(i) != Some(&b'\n') {
                    i += 1;
                }
            }
            Some(b'/') if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < b.len() && !(b.get(i) == Some(&b'*') && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 2;
            }
            Some(c) if c.is_ascii_digit() => {
                let start = i;
                while i < b.len()
                    && b.get(i)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                {
                    i += 1;
                }
                let tok = text.get(start..i).unwrap_or("");
                let parsed = match tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
                    Some(h) => u64::from_str_radix(h, 16).ok(),
                    None => tok.parse::<u64>().ok(),
                };
                if let Some(v) = parsed {
                    out.push(v);
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// `dynamic_frames { 1, 2 }`가 선언한 동적 프레임 ID들(LIN 2.0). 모델로 읽은 섹션과, ID 목록이 아니라 원문으로 남은 섹션을 모두 본다.
pub(crate) fn dynamic_frame_ids(m: &LdfModel) -> Vec<u64> {
    let mut ids: Vec<u64> = m
        .dynamic_frames
        .iter()
        .flat_map(|d| d.ids.iter().map(|id| u64::from(*id)))
        .collect();
    ids.extend(
        section(m, &["dynamic_frames"])
            .iter()
            .flat_map(|s| numbers(body(&s.text))),
    );
    ids
}

/// 슬롯 명령 원문 `Name { a, b, c } delay 10 ms ;`의 인자(중괄호 안 쉼표 구분 토큰, 공백 제거).
pub(crate) fn command_args(text: &str) -> Vec<String> {
    let inner = text
        .find('{')
        .and_then(|s| text.rfind('}').map(|e| (s, e)))
        .and_then(|(s, e)| text.get(s + 1..e))
        .unwrap_or("");
    inner
        .split(',')
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect()
}

/// 인자 토큰을 부호 있는 정수로 읽는다(`-1`, `0xFF` 지원). 읽을 수 없으면 `None`.
pub(crate) fn parse_int(tok: &str) -> Option<i128> {
    let (neg, rest) = match tok.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, tok.strip_prefix('+').unwrap_or(tok)),
    };
    let v = match rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        Some(h) => i128::from_str_radix(h, 16).ok()?,
        None => rest.parse::<i128>().ok()?,
    };
    Some(if neg { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_skip_comments() {
        assert_eq!(numbers("1, 0x2 // 9\n, 3 /* 7 */ , 4"), vec![1, 2, 3, 4]);
    }

    #[test]
    fn body_and_args() {
        assert_eq!(body("dynamic_frames { 1, 2 }").trim(), "1, 2");
        assert_eq!(
            command_args("FreeFormat { 256, -1 , 0x10 } delay 10 ms;"),
            vec!["256", "-1", "0x10"]
        );
        assert_eq!(parse_int("-1"), Some(-1));
        assert_eq!(parse_int("0xFF"), Some(255));
        assert_eq!(parse_int("abc"), None);
    }
}
