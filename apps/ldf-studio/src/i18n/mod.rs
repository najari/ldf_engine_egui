//! 화면 언어(한국어/영어).
//!
//! 코드의 문구는 한국어로 적혀 있고, 영어 화면은 **표시하는 자리에서** 번역한다:
//! - [`t`]: 정해진 문구(`&'static str`)를 번역한다. 영어가 아니면 그대로 돌려주므로 비용이 거의 없다.
//! - [`tr`]: 데이터가 섞여 만들어진 문장(진단 메시지, 알림 등)을 번역한다. 번역 표의 **틀**(`{}`가 값 자리)과 맞추어 보고
//!   값은 그대로 옮긴다.
//!
//! 번역 표는 `table.rs`에 있고, 표에 빠진 문구가 없는지는 소스를 읽어 확인하는 시험(`coverage`)이 지킨다.

mod table;

/// `format!`과 같지만 결과 문장을 화면 언어로 번역한다(`String`).
macro_rules! tf {
    ($($arg:tt)*) => {
        $crate::i18n::tr(&format!($($arg)*)).into_owned()
    };
}

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// 화면 언어.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    /// 한국어.
    #[default]
    Ko,
    /// 영어.
    En,
}

impl Lang {
    /// 메뉴에 보일 이름(언제나 그 언어의 이름으로).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Ko => "한국어",
            Lang::En => "English",
        }
    }
}

/// 화면 언어를 바꾼다.
pub fn set_lang(lang: Lang) {
    ENGLISH.store(lang == Lang::En, Ordering::Relaxed);
}

fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

fn has_hangul(s: &str) -> bool {
    s.chars()
        .any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c) || ('\u{3131}'..='\u{318E}').contains(&c))
}

fn exact() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        table::ENTRIES
            .iter()
            .filter(|(ko, _)| !ko.contains("{}"))
            .copied()
            .collect()
    })
}

fn templates() -> &'static Vec<(Vec<&'static str>, &'static str)> {
    static LIST: OnceLock<Vec<(Vec<&'static str>, &'static str)>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut v: Vec<(Vec<&'static str>, &'static str)> = table::ENTRIES
            .iter()
            .filter(|(ko, _)| ko.contains("{}"))
            .map(|(ko, en)| (ko.split("{}").collect(), *en))
            .collect();
        // 글자가 많은(구체적인) 틀을 먼저 맞춘다: `{}`뿐인 느슨한 틀이 가로채지 못하게
        v.sort_by_key(|(parts, _)| {
            std::cmp::Reverse(parts.iter().map(|p| p.chars().count()).sum::<usize>())
        });
        v
    })
}

/// 정해진 문구를 번역한다. 영어가 아니거나 표에 없으면 그대로 돌려준다.
pub fn t(ko: &'static str) -> &'static str {
    if !is_english() {
        return ko;
    }
    exact().get(ko).copied().unwrap_or(ko)
}

/// 비교 결과의 변경 종류 이름(`ldf_model::Change::label`)을 번역한다.
/// "삭제"·"변경"은 버튼·열 머리글과 뜻이 달라 일반 번역 표와 따로 둔다.
pub fn change_label(ko: &'static str) -> &'static str {
    if is_english() {
        change_label_en(ko)
    } else {
        ko
    }
}

fn change_label_en(ko: &'static str) -> &'static str {
    match ko {
        "추가" => "Added",
        "삭제" => "Removed",
        "변경" => "Changed",
        "이름 변경" => "Renamed",
        other => other,
    }
}

/// `parts`(틀을 `{}`로 자른 조각)와 `s`를 맞춰 보고, 맞으면 `{}` 자리의 값들을 돌려준다.
///
/// 값 사이의 구분 글자가 값 안에도 있을 수 있다(`인코딩 타입 2개 삭제: A`의 공백). 그래서 앞 값은 되도록 길게 잡되(마지막 구분 글자부터),
/// 나머지가 맞지 않으면 짧은 쪽으로 물러서며 다시 맞춘다.
fn capture<'a>(parts: &[&str], s: &'a str) -> Option<Vec<&'a str>> {
    let (first, rest) = parts.split_first()?;
    let body = s.strip_prefix(first)?;
    capture_rest(rest, body)
}

fn capture_rest<'a>(parts: &[&str], s: &'a str) -> Option<Vec<&'a str>> {
    match parts {
        [] => None,
        [last] => Some(vec![s.strip_suffix(last)?]),
        [sep, more @ ..] => s.rmatch_indices(sep).find_map(|(at, _)| {
            let mut tail = capture_rest(more, &s[at + sep.len()..])?;
            tail.insert(0, &s[..at]);
            Some(tail)
        }),
    }
}
/// 첫 글자만 대문자인 낱말(`Node`, `Frame ID`)의 첫 글자를 소문자로 바꾼다. `ID`·`NAD`처럼 앞머리 글자가 모두 대문자면 그대로 둔다.
fn decapitalize(word: &str) -> Cow<'_, str> {
    let mut chars = word.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_ascii_uppercase() && b.is_ascii_lowercase() => {
            Cow::Owned(format!("{}{}", a.to_ascii_lowercase(), &word[1..]))
        }
        _ => Cow::Borrowed(word),
    }
}

/// 영어 틀에 값을 채운다. `{}`는 차례대로, `{1}`처럼 번호를 쓰면 그 번째 값(1부터)이다.
fn fill(en: &str, caps: &[&str]) -> String {
    let mut out = String::with_capacity(en.len() + 16);
    let mut next = 0usize;
    let mut rest = en;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let spec = &after[..close];
        let idx = if spec.is_empty() {
            let i = next;
            next += 1;
            Some(i)
        } else {
            spec.parse::<usize>().ok().and_then(|n| n.checked_sub(1))
        };
        match idx.and_then(|i| caps.get(i)) {
            Some(v) => match exact().get(v) {
                // 문장 가운데에 들어가는 `Node`·`Frame ID` 같은 낱말은 소문자로 시작한다(`Node A`가 아니라 `node A`)
                Some(en) if !out.is_empty() => out.push_str(&decapitalize(en)),
                _ => out.push_str(&to_english(v)),
            },
            None => out.push_str(&rest[open..open + close + 2]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// 값이 섞여 만들어진 문장을 번역한다. 영어가 아니거나 한글이 없으면 그대로, 표에 없으면 그대로 돌려준다.
pub fn tr(s: &str) -> Cow<'_, str> {
    if !is_english() {
        return Cow::Borrowed(s);
    }
    to_english(s)
}

/// 화면 언어와 상관없이 영어로 번역한다(시험은 전역 상태를 건드리지 않으려고 이것을 쓴다).
fn to_english(s: &str) -> Cow<'_, str> {
    if !has_hangul(s) {
        return Cow::Borrowed(s);
    }
    if let Some(en) = exact().get(s) {
        return Cow::Borrowed(en);
    }
    for (parts, en) in templates() {
        if let Some(caps) = capture(parts, s) {
            let mut text = fill(en, &caps);
            // 값으로 시작하는 틀(`{} ... `)은 문장 첫머리가 되므로 대문자로 맞춘다
            if en.starts_with('{')
                && let Some(first) = text.chars().next()
            {
                text.replace_range(..first.len_utf8(), &first.to_uppercase().to_string());
            }
            return Cow::Owned(text);
        }
    }
    // 값이 맞붙어 틀로 떼어 내지 못한 고정 문구
    let mut text = Cow::Borrowed(s);
    for (ko, en) in table::INLINE {
        if text.contains(ko) {
            text = Cow::Owned(text.replace(ko, en));
        }
    }
    text
}

#[cfg(test)]
mod tests;
