//! 규칙 0010: `LIN_protocol_version`·`LIN_language_version`이 유효하고 서로 맞는지 본다.

use crate::Rule;
use crate::util::{Proto, diag};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 명세가 허용하는 버전 번호인지: 숫자이고 "0.01"–"99.99" 범위(LIN 2.2A §9.2.1.1·§9.2.1.2).
fn in_spec_range(s: &str) -> bool {
    s.parse::<f64>()
        .is_ok_and(|v| s.contains('.') && (0.01..=99.99).contains(&v))
}

/// 알려지지 않은 버전 문자열을 설명하는 문장. 명세상 유효한 번호(예: 2.3)와 형식·범위가 틀린 값을 구분한다.
fn unsupported(what: &str, s: &str) -> String {
    if in_spec_range(s) {
        format!("{what} \"{s}\"은(는) 명세상 유효한 번호지만 지원하는 버전이 아니다")
    } else {
        format!("{what} \"{s}\"은(는) 명세의 범위(\"0.01\"–\"99.99\")를 벗어났거나 형식이 틀렸다")
    }
}

/// 규칙 0010.
pub(crate) struct R0010;
/// 등록용 인스턴스.
pub(crate) static RULE: R0010 = R0010;

impl Rule for R0010 {
    fn code(&self) -> u16 {
        10
    }

    fn title(&self) -> &'static str {
        "Invalid LIN protocol or language version"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let h = &m.header;
        let proto = match h.protocol_version.as_deref() {
            None => {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    "LIN_protocol_version이 선언되지 않았다",
                ));
                None
            }
            Some(s) => {
                let p = Proto::parse_protocol(s);
                if p == Proto::Unknown {
                    out.push(diag(
                        Severity::Error,
                        10,
                        None,
                        unsupported("LIN_protocol_version", s),
                    ));
                }
                Some((s, p))
            }
        };
        let language = match h.language_version.as_deref() {
            None => {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    "LIN_language_version이 선언되지 않았다",
                ));
                None
            }
            Some(s) => {
                let p = Proto::parse_language(s);
                if p == Proto::Unknown {
                    out.push(diag(
                        Severity::Error,
                        10,
                        None,
                        unsupported("LIN_language_version", s),
                    ));
                }
                Some((s, p))
            }
        };
        if let (Some((ps, pp)), Some((ls, lp))) = (proto, language) {
            // OEM 변형은 어떤 언어 버전과도 쓸 수 있다
            let compatible = pp == Proto::Oem || (pp != Proto::Unknown && pp == lp);
            if !compatible {
                out.push(diag(
                    Severity::Error,
                    10,
                    None,
                    format!(
                        "LIN_protocol_version \"{ps}\"와 LIN_language_version \"{ls}\"가 서로 맞지 않는다"
                    ),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn count(src: &str) -> usize {
        let mut out = Vec::new();
        RULE.check(&load(src).model, &mut out);
        out.len()
    }

    #[test]
    fn valid_pairs_pass() {
        for (p, l) in [
            ("1.3", "1.3"),
            ("2.0", "2.0"),
            ("2.1", "2.1"),
            ("2.2", "2.2"),
            ("J2602_1_1.0", "J2602_3_1.0"),
            ("ISO17987:2015", "ISO17987:2015"),
            ("OEM variant", "1.3"),
        ] {
            let src = format!(
                "LIN_description_file;\nLIN_protocol_version = \"{p}\";\nLIN_language_version = \"{l}\";\n"
            );
            assert_eq!(count(&src), 0, "{p} / {l}");
        }
    }

    #[test]
    fn invalid_version_and_mismatch_are_reported() {
        let both = "LIN_protocol_version = \"2.3\";\nLIN_language_version = \"2.2\";\n";
        assert_eq!(count(both), 2); // 잘못된 값 + 불일치
        let diff = "LIN_protocol_version = \"2.2\";\nLIN_language_version = \"2.1\";\n";
        assert_eq!(count(diff), 1);
        assert_eq!(count(""), 2); // 둘 다 없음
    }

    #[test]
    fn unknown_versions_are_told_apart_by_the_spec_range() {
        assert!(in_spec_range("2.3"));
        assert!(in_spec_range("0.01"));
        assert!(in_spec_range("99.99"));
        assert!(!in_spec_range("100.00"));
        assert!(!in_spec_range("0.00"));
        assert!(!in_spec_range("abc"));
        assert!(!in_spec_range("2"));
        assert!(unsupported("X", "2.3").contains("명세상 유효한 번호"));
        assert!(unsupported("X", "abc").contains("범위"));
    }
}
