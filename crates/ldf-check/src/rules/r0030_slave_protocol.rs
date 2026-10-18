//! 규칙 0030: 슬레이브 노드의 `LIN_protocol`이 유효하고 LDF 파일의 버전과 맞는지 본다.

use crate::Rule;
use crate::util::{Proto, diag, file_proto, node_label};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0030.
pub(crate) struct R0030;
/// 등록용 인스턴스.
pub(crate) static RULE: R0030 = R0030;

/// 파일 버전 `file`에서 슬레이브가 `slave` 계열 버전을 써도 되는지.
fn allowed(file: Proto, slave: Proto) -> bool {
    match file {
        Proto::V1_3 => slave == Proto::V1_3,
        Proto::J2602 => slave == Proto::J2602,
        Proto::V2_0 | Proto::V2_1 | Proto::V2_2 | Proto::Iso => {
            let one_three = slave == Proto::V1_3;
            let rank_ok = match (file.lin2_rank(), slave.lin2_rank()) {
                (Some(f), Some(s)) => s <= f,
                _ => false,
            };
            one_three || rank_ok
        }
        // OEM 변형이거나 파일 버전을 모르면 판단하지 않는다
        Proto::Oem | Proto::Unknown => true,
    }
}

impl Rule for R0030 {
    fn code(&self) -> u16 {
        30
    }

    fn title(&self) -> &'static str {
        "Slave LIN protocol version inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let mut file = file_proto(m);
        // 버전 문자열이 틀려도(J2602_1_1.1) J2602 계열이라는 것은 알 수 있다
        if file == Proto::Unknown
            && m.header
                .protocol_version
                .as_deref()
                .is_some_and(|v| v.starts_with("J2602"))
        {
            file = Proto::J2602;
        }
        for a in &m.node_attrs {
            let Some(s) = a.lin_protocol.as_deref() else {
                continue;
            };
            let name = node_label(m, &a.node);
            let mut p = Proto::parse_protocol(s);
            if matches!(p, Proto::Unknown | Proto::Oem) {
                out.push(diag(
                    Severity::Error,
                    30,
                    a.span,
                    format!(
                        "슬레이브 `{name}`의 LIN_protocol \"{s}\"은(는) 지원하는 버전이 아니다"
                    ),
                ));
                // 계열은 알 수 있으면 계속 따진다(J2602_1_1.1은 J2602 계열이다)
                p = if s.starts_with("J2602") {
                    Proto::J2602
                } else if s.starts_with("ISO") {
                    Proto::Iso
                } else {
                    continue;
                };
            }
            if !allowed(file, p) {
                out.push(diag(
                    Severity::Error,
                    30,
                    a.span,
                    format!(
                        "슬레이브 `{name}`의 LIN_protocol \"{s}\"은(는) 파일의 버전({})과 맞지 않는다",
                        m.header.protocol_version.as_deref().unwrap_or("?")
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

    fn n(file: &str, slave: &str) -> usize {
        let src = format!(
            "LIN_protocol_version = \"{file}\";\nNodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nNode_attributes {{ S {{ LIN_protocol = \"{slave}\"; }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn slave_versions() {
        assert_eq!(n("2.2", "2.2"), 0);
        assert_eq!(n("2.2", "1.3"), 0);
        assert_eq!(n("2.0", "2.1"), 1);
        assert_eq!(n("2.0", "J2602_1_1.0"), 1);
        assert_eq!(n("2.2", "2.3"), 1);
        assert_eq!(n("2.2", "J2602_1_1.1"), 2);
        assert_eq!(n("ISO17987:2015", "2.1"), 0);
        assert_eq!(n("J2602_1_1.0", "J2602_1_1.0"), 0);
    }
}
