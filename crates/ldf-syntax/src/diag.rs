//! 진단(Diagnostic). 파서·검사기가 공유하는 어휘.

use crate::Span;
use serde::Serialize;

/// 심각도.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Severity {
    /// 참고.
    Info,
    /// 경고.
    Warning,
    /// 오류.
    Error,
}

/// 진단 코드. 00xx–04xx는 Vector 호환 검사 규칙(`ldf-check`)이 쓰고,
/// 9xxx는 파서·lowering이 쓴다.
pub mod codes {
    /// 알 수 없는 문자.
    pub const UNEXPECTED_CHAR: u16 = 9001;
    /// 닫히지 않은 문자열.
    pub const UNTERMINATED_STRING: u16 = 9002;
    /// 닫히지 않은 블록 주석.
    pub const UNTERMINATED_COMMENT: u16 = 9003;
    /// 짝이 없는 `}`.
    pub const UNEXPECTED_CLOSE_BRACE: u16 = 9010;
    /// 닫히지 않은 `{`.
    pub const UNCLOSED_BLOCK: u16 = 9011;
    /// 중첩이 너무 깊다.
    pub const NESTING_TOO_DEEP: u16 = 9012;
    /// 섹션 안의 문장 형식이 맞지 않는다.
    pub const BAD_STATEMENT: u16 = 9101;
    /// 숫자가 와야 하는 자리.
    pub const EXPECTED_NUMBER: u16 = 9102;
    /// 이름이 와야 하는 자리.
    pub const EXPECTED_NAME: u16 = 9103;
    /// 같은 섹션이 두 번 나왔다.
    pub const DUPLICATE_SECTION: u16 = 9104;
    /// 값이 범위를 벗어났다.
    pub const VALUE_OUT_OF_RANGE: u16 = 9105;
    /// 알 수 없는 섹션(원문으로 보관).
    pub const UNKNOWN_SECTION: u16 = 9106;
    /// 헤더 값이 없거나 형식이 틀렸다.
    pub const BAD_HEADER: u16 = 9107;
    /// 같은 종류의 객체에 이름이 중복된다(첫 번째가 이름 해석에 쓰인다).
    pub const DUPLICATE_NAME: u16 = 9108;
    /// `Signal_representation`이 없는 시그널을 가리켜 항목을 버렸다.
    pub const UNRESOLVED_REPRESENTATION: u16 = 9109;
    /// `LIN_speed`의 단위가 명세(`kbps`)와 다르다.
    pub const SPEED_UNIT: u16 = 9110;
}

/// 진단 하나.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// 진단 코드.
    pub code: u16,
    /// 심각도.
    pub severity: Severity,
    /// 원문 위치. 위치가 없는 진단도 있다.
    pub span: Option<Span>,
    /// 사람이 읽는 설명.
    pub message: String,
}

impl Diagnostic {
    /// 오류 진단을 만든다.
    pub fn error(code: u16, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            span: Some(span),
            message: message.into(),
        }
    }

    /// 경고 진단을 만든다.
    pub fn warning(code: u16, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            span: Some(span),
            message: message.into(),
        }
    }

    /// 참고 진단을 만든다.
    pub fn info(code: u16, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Info,
            span: Some(span),
            message: message.into(),
        }
    }
}
