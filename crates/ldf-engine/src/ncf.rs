//! NCF(노드 기능 파일) 읽기: 바이트의 인코딩을 판별해 디코딩하고 `ldf-ncf`로 푼다. Studio와 `ldf-cli ncf`가 함께 쓴다.
//!
//! NCF는 LDF 세션(리비전·편집·검사)과 따로 다룬다. 읽기 전용이라 프로젝트에 저장하지 않는다.

use crate::decode::decode_auto;
use ldf_model::Diagnostic;
use ldf_ncf::{NcfFile, parse};

/// 읽은 NCF 한 개.
#[derive(Clone, Debug)]
pub struct NcfDocument {
    /// 디코딩한 원문.
    pub text: String,
    /// 판별한 인코딩 이름.
    pub encoding: &'static str,
    /// 읽은 내용.
    pub file: NcfFile,
    /// 렉서·파서와 NCF 진단.
    pub diagnostics: Vec<Diagnostic>,
}

/// 바이트를 NCF로 읽는다. 절대 패닉하지 않고, NCF가 아니어도 진단과 함께 빈 내용으로 돌려준다.
pub fn read_ncf(bytes: &[u8]) -> NcfDocument {
    let (text, encoding) = decode_auto(bytes);
    let parsed = parse(&text);
    NcfDocument {
        text,
        encoding,
        file: parsed.file,
        diagnostics: parsed.diagnostics,
    }
}

/// 경로의 확장자가 `.ncf`(대소문자 무시)인지.
pub fn is_ncf_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|x| x.eq_ignore_ascii_case("ncf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn reads_bytes_in_any_supported_encoding() {
        let src = "node_capability_file;\nnode N { free_text { \"Türschloß\" } }\n";
        // UTF-8과 CP1252 모두 같은 글이 된다
        let doc = read_ncf(src.as_bytes());
        assert_eq!(doc.file.nodes[0].free_text.as_deref(), Some("Türschloß"));
        let cp1252: Vec<u8> = crate::decode::encode(src, crate::decode::CP1252);
        let doc = read_ncf(&cp1252);
        assert_eq!(doc.encoding, crate::decode::CP1252);
        assert_eq!(doc.file.nodes[0].free_text.as_deref(), Some("Türschloß"));
    }

    #[test]
    fn a_file_that_is_not_an_ncf_still_comes_back_with_diagnostics() {
        let doc = read_ncf(b"LIN_description_file;\n");
        assert!(doc.file.nodes.is_empty());
        assert!(!doc.diagnostics.is_empty());
        assert!(read_ncf(b"").file.nodes.is_empty());
    }

    #[test]
    fn extension_check_ignores_case() {
        assert!(is_ncf_path(Path::new("a/b.NCF")));
        assert!(is_ncf_path(Path::new("b.ncf")));
        assert!(!is_ncf_path(Path::new("b.ldf")));
        assert!(!is_ncf_path(Path::new("ncf")));
    }
}
