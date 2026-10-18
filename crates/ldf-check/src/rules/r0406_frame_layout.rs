//! 규칙 0406: 프레임 안의 시그널 배치 문제(프레임 밖으로 넘침, 서로 겹침, 바이트 경계, 쓰지 않는 바이트,
//! 이벤트 트리거 연관 프레임의 첫 바이트 사용).

use crate::Rule;
use crate::util::{big_endian, bit_spans, bytes_touched, diag, etf_associated, unconditional};
use ldf_model::{Diagnostic, InitValue, LdfModel, Severity};

/// 규칙 0406.
pub(crate) struct R0406;
/// 등록용 인스턴스.
pub(crate) static RULE: R0406 = R0406;

fn overlap(a: &[(u64, u64)], b: &[(u64, u64)]) -> bool {
    a.iter()
        .any(|&(al, ah)| b.iter().any(|&(bl, bh)| al <= bh && bl <= ah))
}

impl Rule for R0406 {
    fn code(&self) -> u16 {
        406
    }

    fn title(&self) -> &'static str {
        "Frame layout inconsistent"
    }

    fn severity(&self) -> Severity {
        Severity::Warning
    }

    fn check(&self, m: &LdfModel, out: &mut Vec<Diagnostic>) {
        let be = big_endian(m);
        let etf = etf_associated(m);
        for f in unconditional(m) {
            let name = &f.frame.name;
            let bits = u64::from(f.size) * 8;
            // (시그널 이름, 비트 구간)
            let mut placed: Vec<(String, Vec<(u64, u64)>)> = Vec::new();
            for fs in f.signals {
                let Some(sig) = fs.signal.id().and_then(|id| m.signals.get(id)) else {
                    continue;
                };
                let spans = bit_spans(fs.offset, sig.size_bits, be);
                if spans.iter().any(|&(_, hi)| hi >= bits) {
                    out.push(diag(
                        Severity::Warning,
                        406,
                        f.frame.span,
                        format!(
                            "프레임 `{name}`({}바이트)에서 시그널 `{}`이(가) 프레임 밖으로 넘친다",
                            f.size, sig.name
                        ),
                    ));
                }
                // 바이트 배열은 바이트 경계에 놓여야 하고, 스칼라는 바이트 경계를 한 번까지만 넘을 수 있다
                let crossing = match &sig.init {
                    InitValue::Array(_) => fs.offset % 8 != 0 || sig.size_bits % 8 != 0,
                    InitValue::Scalar(_) => bytes_touched(&spans) > 2,
                };
                if crossing {
                    out.push(diag(
                        Severity::Warning,
                        406,
                        f.frame.span,
                        format!(
                            "프레임 `{name}`의 시그널 `{}`이(가) 바이트 경계를 허용되지 않는 방식으로 넘는다",
                            sig.name
                        ),
                    ));
                }
                placed.push((sig.name.clone(), spans));
            }
            for (i, (a, sa)) in placed.iter().enumerate() {
                for (b, sb) in placed.iter().skip(i + 1) {
                    if overlap(sa, sb) {
                        out.push(diag(
                            Severity::Warning,
                            406,
                            f.frame.span,
                            format!("프레임 `{name}`에서 시그널 `{a}`와 `{b}`가 겹친다"),
                        ));
                    }
                }
            }
            let all: Vec<(u64, u64)> = placed
                .iter()
                .flat_map(|(_, s)| s.iter().copied())
                .filter(|&(lo, _)| lo < bits)
                .collect();
            // 이벤트 트리거 연관 프레임의 첫 바이트는 프레임 ID 확인용이라 비어 있어도 된다
            let is_assoc = etf.contains(&f.id);
            let mut used = all.clone();
            if is_assoc {
                used.push((0, 7));
            }
            if (bytes_touched(&used) as u64) < u64::from(f.size) {
                out.push(diag(
                    Severity::Warning,
                    406,
                    f.frame.span,
                    format!("프레임 `{name}`에 어떤 시그널도 쓰지 않는 바이트가 있다"),
                ));
            }
            // 이벤트 트리거 프레임의 연관 프레임은 첫 바이트를 프레임 ID 확인에 쓴다
            if is_assoc && overlap(&all, &[(0, 7)]) {
                out.push(diag(
                    Severity::Warning,
                    406,
                    f.frame.span,
                    format!("이벤트 트리거 연관 프레임 `{name}`의 첫 바이트에 시그널이 있다"),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::load;

    fn n(frame_body: &str, size: u32) -> usize {
        let src = format!(
            "Nodes {{ Master: M, 1 ms, 0 ms; Slaves: S; }}\n\
             Signals {{ A: 8, 0, M, S; B: 8, 0, M, S; C: 16, 0, M, S; }}\n\
             Frames {{ F: 1, M, {size} {{ {frame_body} }} }}"
        );
        let mut out = Vec::new();
        RULE.check(&load(&src).model, &mut out);
        out.len()
    }

    #[test]
    fn fully_packed_frames_are_clean() {
        assert_eq!(n("A, 0; B, 8;", 2), 0);
        assert_eq!(n("C, 0;", 2), 0);
    }

    #[test]
    fn unused_bytes_overlap_and_overflow() {
        assert_eq!(n("A, 0;", 2), 1);
        assert_eq!(n("A, 0; B, 4;", 2), 1); // 겹침(8비트 두 개가 4비트 어긋남)
        assert_eq!(n("C, 8;", 2), 1); // 프레임 밖으로 넘침
    }
}
