//! 규칙 트레이트와 레지스트리.

use ldf_model::{Diagnostic, LdfModel, Severity};

/// 검사 규칙 하나.
pub trait Rule: Sync {
    /// 규칙 번호(진단의 `code`). 예: 203.
    fn code(&self) -> u16;
    /// 사람이 읽는 제목. 예: "Frame inconsistency".
    fn title(&self) -> &'static str;
    /// 기본 심각도.
    fn severity(&self) -> Severity;
    /// 모델을 검사해 진단을 `out`에 추가한다. 패닉하지 않는다.
    fn check(&self, model: &LdfModel, out: &mut Vec<Diagnostic>);
}

/// 등록된 모든 규칙(코드 순).
pub fn all_rules() -> Vec<&'static dyn Rule> {
    let mut rules = crate::rules::registered();
    rules.sort_by_key(|r| r.code());
    rules
}

/// 모든 규칙을 실행한다. 결과는 코드 순, 같은 코드 안에서는 원문 위치 순이다.
pub fn run_all(model: &LdfModel) -> Vec<Diagnostic> {
    run_rules(model, &all_rules())
}

/// 주어진 규칙만 실행한다(프로젝트에서 끈 규칙을 뺄 때, 증분 검사에 쓴다).
pub fn run_rules(model: &LdfModel, rules: &[&dyn Rule]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for r in rules {
        let start = out.len();
        r.check(model, &mut out);
        // 규칙이 코드·심각도를 채우지 않았어도 규칙의 값을 보장한다
        for d in out.iter_mut().skip(start) {
            d.code = r.code();
        }
        out[start..].sort_by_key(|d| d.span.map_or(usize::MAX, |s| s.start));
    }
    out
}
