//! 규칙 0205: 인코딩 타입 안의 값이 모순이다.
//!
//! - `bcd_value`·`ascii_value`는 다른 값과 함께 쓸 수 없고 서로도 함께 쓸 수 없다.
//! - 물리값의 최솟값이 최댓값보다 크다.
//! - 같은 논리값이 두 번 정의되었다.

use crate::Rule;
use crate::util::diag;
use ldf_model::{Diagnostic, EncodingEntry, LdfModel, Severity};
use std::collections::BTreeSet;

/// 규칙 0205.
pub(crate) struct R0205;
/// 등록용 인스턴스.
pub(crate) static RULE: R0205 = R0205;

impl Rule for R0205 {
    fn code(&self) -> u16 {
        205
    }

    fn title(&self) -> &'static str {
        "Encoding values inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        for (_, e) in m.encodings.iter() {
            let n = e.entries.len();
            let special = e
                .entries
                .iter()
                .filter(|x| matches!(x, EncodingEntry::Bcd | EncodingEntry::Ascii))
                .count();
            if special > 0 && n > 1 {
                out.push(diag(
                    Severity::Error,
                    205,
                    e.span,
                    format!(
                        "인코딩 `{}`이(가) bcd_value/ascii_value를 다른 값과 함께 정의한다",
                        e.name
                    ),
                ));
            }
            let mut seen = BTreeSet::new();
            for x in &e.entries {
                match x {
                    EncodingEntry::Physical { min, max, .. } if min > max => out.push(diag(
                        Severity::Error,
                        205,
                        e.span,
                        format!(
                            "인코딩 `{}`의 물리값 범위 {min}..{max}에서 최솟값이 더 크다",
                            e.name
                        ),
                    )),
                    EncodingEntry::Logical { value, .. } if !seen.insert(*value) => out.push(diag(
                        Severity::Error,
                        205,
                        e.span,
                        format!(
                            "인코딩 `{}`에서 논리값 {value}이(가) 두 번 정의되었다",
                            e.name
                        ),
                    )),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(body: &str) -> usize {
        let src = format!("Signal_encoding_types {{ E {{ {body} }} }}");
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn combinations_and_ranges() {
        assert_eq!(
            n("logical_value, 0, \"a\"; physical_value, 1, 5, 1, 0, \"u\";"),
            0
        );
        assert_eq!(n("bcd_value;"), 0);
        assert_eq!(n("bcd_value; ascii_value;"), 1);
        assert_eq!(n("logical_value, 0, \"a\"; bcd_value;"), 1);
        assert_eq!(n("physical_value, 5, 1, 1, 0, \"u\";"), 1);
        assert_eq!(n("logical_value, 0, \"a\"; logical_value, 0, \"b\";"), 1);
    }
}
