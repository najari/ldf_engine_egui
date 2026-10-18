//! 분석: 바이트 -> 디코딩 -> 파싱 -> 검사. 세션(임포트)과 CLI가 같은 경로를 쓴다.

use crate::decode;
use ldf_check::RuleSettings;
use ldf_model::{Diagnostic, LdfModel};

/// 분석 결과.
#[derive(Debug)]
pub struct Analysis {
    /// 모델. `header.encoding`은 판별한 원본 인코딩이다.
    pub model: LdfModel,
    /// 파서·lowering 진단 뒤에 검사 규칙 결과가 이어진다.
    pub diagnostics: Vec<Diagnostic>,
    /// 디코딩한 원문(BOM 제외). 진단의 `Span`이 가리키는 텍스트다.
    pub text: String,
    /// 판별한 인코딩 이름.
    pub encoding: &'static str,
}

/// 바이트를 분석한다. 어떤 입력에도 패닉하지 않는다.
pub fn analyze(bytes: &[u8]) -> Analysis {
    analyze_with(bytes, &RuleSettings::default())
}

/// 규칙 설정(끄기·심각도)을 적용해 바이트를 분석한다.
pub fn analyze_with(bytes: &[u8], rules: &RuleSettings) -> Analysis {
    let (text, encoding) = decode::decode_auto(bytes);
    analyze_text_with(text, encoding, rules)
}

/// 이미 디코딩한 텍스트를 분석한다(편집한 텍스트 재검사용).
pub fn analyze_text(text: String, encoding: &'static str) -> Analysis {
    analyze_text_with(text, encoding, &RuleSettings::default())
}

/// 규칙 설정을 적용해 디코딩한 텍스트를 분석한다.
pub fn analyze_text_with(text: String, encoding: &'static str, rules: &RuleSettings) -> Analysis {
    let mut loaded = ldf_model::load(&text);
    loaded.model.header.encoding = encoding.to_owned();
    let mut diagnostics = loaded.diagnostics;
    diagnostics.extend(ldf_check::run_all_with(&loaded.model, rules));
    Analysis {
        model: loaded.model,
        diagnostics,
        text,
        encoding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_reports_encoding_and_keeps_parser_diagnostics_first() {
        let a = analyze(b"LIN_description_file;\n@@ Nodes { Master: M, 1 ms, 0 ms; }");
        assert_eq!(a.encoding, "utf-8");
        assert_eq!(a.model.header.encoding, "utf-8");
        assert!(a.diagnostics.iter().any(|d| d.code == 9001));
        // 파서 진단(9xxx)은 규칙 결과보다 앞에 온다
        let first_rule = a.diagnostics.iter().position(|d| d.code < 9000);
        let last_parse = a.diagnostics.iter().rposition(|d| d.code >= 9000);
        if let (Some(r), Some(p)) = (first_rule, last_parse) {
            assert!(p < r);
        }
    }

    #[test]
    fn empty_and_garbage_inputs_do_not_panic() {
        for b in [&b""[..], b"\xff\xfe\x00", b"{{{{", &[0x80; 64]] {
            let _ = analyze(b);
        }
    }
}
