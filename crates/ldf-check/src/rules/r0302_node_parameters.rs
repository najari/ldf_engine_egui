//! 규칙 0302: 노드 속성의 값이 범위를 벗어났다.
//!
//! NAD 1–125, 공급자 ID ≤ 0x7FFF, 기능 ID ≤ 0xFFFF, 변형 ID ≤ 255, `P2_min` 50–500 ms, 메시지 ID 0–0xFFFE
//! (0은 LIN 2.2A §9.4의 예제가 이벤트 트리거 프레임에 쓴다. Vector `Parameter/0048`이 범위 밖 값 3개(0x10000, 0xFFFF, 0x0) 중
//! 2개만 0302로 기대하므로 0xFFFF와 그 이상만 벗어난 것으로 본다. J2602는 `P2_min`과 메시지 ID 범위를 따지지 않는다).

use crate::Rule;
use crate::util::{Proto, attr_proto, diag, node_label};
use ldf_model::{Diagnostic, LdfModel, Severity};

/// 규칙 0302.
pub(crate) struct R0302;
/// 등록용 인스턴스.
pub(crate) static RULE: R0302 = R0302;

impl Rule for R0302 {
    fn code(&self) -> u16 {
        302
    }

    fn title(&self) -> &'static str {
        "Node attribute value out of range"
    }

    fn severity(&self) -> Severity {
        Severity::Error
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        if super::attribute_format_cascade(m).is_some() {
            out.push(super::cascade_diag(302, Severity::Error));
        }
        for a in &m.node_attrs {
            let name = node_label(m, &a.node);
            let mut found: Vec<String> = Vec::new();
            let mut range = |what: String| found.push(format!("{what}이(가) 범위를 벗어났다"));
            for (label, nad) in [
                ("configured_NAD", a.configured_nad),
                ("initial_NAD", a.initial_nad),
            ] {
                if let Some(v) = nad
                    && !(1..=125).contains(&v)
                {
                    range(format!("{label} {v:#04X}"));
                }
            }
            if let Some(p) = &a.product_id {
                if p.supplier > 0x7FFF {
                    range(format!("공급자 ID {:#X}", p.supplier));
                }
                if p.function > 0xFFFF {
                    range(format!("기능 ID {:#X}", p.function));
                }
                if let Some(v) = p.variant
                    && v > 255
                {
                    range(format!("변형 ID {v}"));
                }
            }
            let j2602 = attr_proto(m, a.lin_protocol.as_deref()) == Proto::J2602;
            if let Some(p2) = a.p2_min_ms
                && !j2602
                && !(50.0..=500.0).contains(&p2)
            {
                range(format!("P2_min {p2} ms"));
            }
            for c in &a.configurable_frames {
                if let Some(id) = c.message_id
                    && !j2602
                    && id > 0xFFFE
                {
                    range(format!("메시지 ID {id:#X}"));
                }
            }
            for msg in found {
                out.push(diag(
                    Severity::Error,
                    302,
                    a.span,
                    format!("노드 `{name}`의 {msg}"),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(attrs: &str) -> usize {
        let src = format!(
            "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\nNode_attributes {{ S {{ {attrs} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn ranges() {
        assert_eq!(
            n(
                "configured_NAD = 0x7D; initial_NAD = 1; product_id = 0x7FFF, 0xFFFF, 255; P2_min = 50 ms;"
            ),
            0
        );
        assert_eq!(n("configured_NAD = 0x7F;"), 1);
        assert_eq!(n("configured_NAD = 0; initial_NAD = 0x7E;"), 2);
        assert_eq!(n("product_id = 0x8000, 0x10000, 256;"), 3);
        assert_eq!(n("P2_min = 501 ms;"), 1);
        assert_eq!(n("P2_min = 49 ms;"), 1);
        // 메시지 ID: 0(이벤트 트리거 프레임)은 허용, 0xFFFF 이상은 벗어남
        let cf = |ids: &str| {
            n(&format!(
                "LIN_protocol = \"2.0\"; configurable_frames {{ {ids} }}"
            ))
        };
        assert_eq!(cf("F1 = 0; F2 = 0xFFFE;"), 0);
        assert_eq!(cf("F1 = 0xFFFF; F2 = 0x10000;"), 2);
    }
}
