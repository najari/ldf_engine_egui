//! 규칙 0110: 시그널 그룹(`Signal_groups { 그룹: 크기 { 시그널, 오프셋; } }`)이 맞지 않는다.
//!
//! 그룹에 든 시그널은 `Signals`에 정의되어 있어야 하고, 그룹 안에서 시그널이 차지하는 비트가 그룹 크기를 넘지 않아야 한다.
//! (이 번호는 Vector의 검사에 없는 이 프로젝트의 것이다.)

use super::r0101_sections::section_allowed;
use crate::Rule;
use crate::util::{diag, file_proto};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0110.
pub(crate) struct R0110;
/// 등록용 인스턴스.
pub(crate) static RULE: R0110 = R0110;

impl Rule for R0110 {
    fn code(&self) -> u16 {
        110
    }

    fn title(&self) -> &'static str {
        "Signal group inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let Some(groups) = &m.signal_groups else {
            return;
        };
        // 이 버전에서 쓸 수 없는 섹션은 0101이 보고하고 여기서는 내용을 따지지 않는다
        if !section_allowed("Signal_groups", file_proto(m)) {
            return;
        }
        for g in &groups.groups {
            for s in &g.signals {
                let Some(sig) = m.find_signal(&s.signal).and_then(|id| m.signals.get(id)) else {
                    out.push(diag(
                        Severity::Error,
                        110,
                        groups.span,
                        format!(
                            "시그널 그룹 `{}`의 시그널 `{}`이(가) 정의되어 있지 않다",
                            g.name, s.signal
                        ),
                    ));
                    continue;
                };
                if s.offset.saturating_add(sig.size_bits) > g.size_bits {
                    out.push(diag(
                        Severity::Error,
                        110,
                        groups.span,
                        format!(
                            "시그널 그룹 `{}`의 시그널 `{}`이(가) 그룹 크기 {}비트를 넘는다",
                            g.name, s.signal, g.size_bits
                        ),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(version: &str, groups: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{version}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: A; }}\n\
             Signals {{ S8: 8, 0, M, A; S4: 4, 0, A, M; }}\n{groups}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn members_must_exist_and_fit() {
        assert_eq!(n("1.3", "Signal_groups { G: 16 { S8, 0; S4, 8; } }"), 0);
        assert_eq!(n("1.3", "Signal_groups { }"), 0);
        // 정의되지 않은 시그널
        assert_eq!(n("1.3", "Signal_groups { G: 16 { Ghost, 0; } }"), 1);
        // 그룹 크기를 넘는다(8 + 4 > 10)
        assert_eq!(n("1.3", "Signal_groups { G: 10 { S8, 0; S4, 8; } }"), 1);
    }

    #[test]
    fn other_versions_are_left_to_rule_0101() {
        assert_eq!(n("2.0", "Signal_groups { G: 16 { Ghost, 0; } }"), 0);
    }
}
