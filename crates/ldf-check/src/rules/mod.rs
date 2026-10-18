//! 규칙 모음. 규칙 하나 = 파일 하나(`rNNNN_이름.rs`), 여기에 등록한다.

use crate::Rule;

/// 등록된 규칙 목록. 새 규칙은 모듈을 추가하고 아래 벡터에 넣는다.
pub(crate) fn registered() -> Vec<&'static dyn Rule> {
    vec![]
}
