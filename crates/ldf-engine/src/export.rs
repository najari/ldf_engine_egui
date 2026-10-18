//! 내보내기(FR-EXP-01/02/04): 리비전의 모델을 LDF 파일로 쓰고, 쓴 결과를 다시 읽어 검증한다.

use crate::decode;
use crate::error::{EngineError, Result};
use crate::session::Session;
use ldf_model::{Severity, load, semantically_equal};
use ldf_writer::{Eol, Target, WriteOptions, write, write_file_atomic};
use std::path::{Path, PathBuf};

/// 내보내기 옵션.
#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    /// 대상 LIN 버전.
    pub target: Target,
    /// 줄바꿈.
    pub eol: Eol,
    /// 인코딩 이름(`decode::UTF8` 등). `None`이면 리비전 원본의 인코딩.
    pub encoding: Option<String>,
    /// 오류 진단이 남아 있어도 내보낸다(FR-EXP-04의 확인을 받은 뒤).
    pub allow_errors: bool,
}

/// 내보내기 결과.
#[derive(Clone, Debug)]
pub struct ExportReport {
    /// 쓴 파일.
    pub path: PathBuf,
    /// 쓴 바이트 수.
    pub bytes: usize,
    /// 사용한 인코딩.
    pub encoding: String,
    /// 대상 버전 변환 등으로 항목이 빠졌다는 경고.
    pub warnings: Vec<String>,
    /// 다시 읽은 모델이 원본 모델과 의미가 같은지. 버전을 바꿨거나 원본이 오류를 가졌으면 `false`일 수 있다.
    pub verified: bool,
}

impl Session {
    /// 리비전을 `path`에 LDF로 쓴다. 쓰기는 원자적이다(FR-EXP-05).
    ///
    /// 오류 진단이 남은 리비전은 `allow_errors`가 아니면 [`EngineError::ExportBlocked`]로 거부한다.
    pub fn export(
        &mut self,
        revision: i64,
        path: &Path,
        opts: &ExportOptions,
    ) -> Result<ExportReport> {
        let snap = self.open_revision(revision)?;
        let errors = snap
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        if errors > 0 && !opts.allow_errors {
            return Err(EngineError::ExportBlocked { errors });
        }

        let written = write(
            &snap.model,
            &WriteOptions {
                target: opts.target,
                eol: opts.eol,
            },
        );
        let encoding = opts
            .encoding
            .clone()
            .unwrap_or_else(|| snap.revision.encoding.clone());
        let bytes = decode::encode(&written.text, &encoding);

        let mut warnings = written.warnings;
        // 인코딩으로 표현할 수 없는 글자는 `?`가 되므로 미리 알린다
        if decode::decode(&bytes, &encoding) != written.text {
            warnings.push(format!("{encoding}으로 표현할 수 없는 문자를 ?로 바꿨다"));
        }

        // 쓴 바이트를 다시 읽어 의미가 같은지 본다. 버전을 바꾸면 항목이 빠지므로 일치하지 않을 수 있다.
        let again = load(&decode::decode(&bytes, &encoding));
        let verified = opts.target == Target::Keep && semantically_equal(&snap.model, &again.model);

        write_file_atomic(path, &bytes).map_err(|source| EngineError::Io {
            path: path.to_owned(),
            source,
        })?;
        Ok(ExportReport {
            path: path.to_owned(),
            bytes: bytes.len(),
            encoding,
            warnings,
            verified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LDF: &str = "LIN_description_file;\n\
        LIN_protocol_version = \"2.1\";\n\
        LIN_language_version = \"2.1\";\n\
        LIN_speed = 19.2 kbps;\n\
        Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A; }\n\
        Signals { S: 8, 0, A, M; }\n\
        Frames { F: 1, A, 1 { S, 0; } }\n";

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ldf-engine-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        dir.join(name)
    }

    #[test]
    fn exports_a_verified_file_in_the_original_encoding() {
        let mut s = Session::open_in_memory().expect("session");
        let id = s
            .import_bytes("a.ldf", LDF.as_bytes())
            .expect("import")
            .snapshot
            .revision
            .id;
        let path = temp("a.ldf");
        let r = s
            .export(id, &path, &ExportOptions::default())
            .expect("export");
        assert!(r.verified, "{:?}", r.warnings);
        assert!(r.warnings.is_empty());
        let back = std::fs::read_to_string(&path).expect("read");
        assert!(back.contains("Signals {"));
        assert_eq!(r.bytes, back.len());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn errors_block_the_export_until_confirmed() {
        let mut s = Session::open_in_memory().expect("session");
        let id = s
            .import_bytes("bad.ldf", b"Nodes {")
            .expect("import")
            .snapshot
            .revision
            .id;
        let path = temp("bad.ldf");
        match s.export(id, &path, &ExportOptions::default()) {
            Err(EngineError::ExportBlocked { errors }) => assert!(errors > 0),
            other => panic!("막혀야 한다: {other:?}"),
        }
        assert!(!path.exists());
        let opts = ExportOptions {
            allow_errors: true,
            ..ExportOptions::default()
        };
        s.export(id, &path, &opts).expect("확인 후에는 쓴다");
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn target_version_and_encoding_are_applied() {
        let mut s = Session::open_in_memory().expect("session");
        let src = LDF.replace("19.2 kbps;", "19.2 kbps;\nChannel_name = \"Böd\";");
        let id = s
            .import_bytes("a.ldf", src.as_bytes())
            .expect("import")
            .snapshot
            .revision
            .id;
        let path = temp("v20.ldf");
        let opts = ExportOptions {
            target: Target::V2_0,
            eol: Eol::CrLf,
            encoding: Some(decode::CP1252.to_owned()),
            allow_errors: false,
        };
        let r = s.export(id, &path, &opts).expect("export");
        assert!(
            r.warnings.iter().any(|w| w.contains("Channel_name")),
            "{:?}",
            r.warnings
        );
        assert!(!r.verified);
        let bytes = std::fs::read(&path).expect("read");
        assert!(bytes.windows(2).any(|w| w == b"\r\n"));
        assert!(
            std::str::from_utf8(&bytes).is_ok(),
            "ASCII뿐이라 UTF-8로도 읽힌다"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn characters_outside_the_encoding_are_reported() {
        let mut s = Session::open_in_memory().expect("session");
        let src = LDF.replace("19.2 kbps;", "19.2 kbps;\nChannel_name = \"한글\";");
        let id = s
            .import_bytes("k.ldf", src.as_bytes())
            .expect("import")
            .snapshot
            .revision
            .id;
        let path = temp("k.ldf");
        let opts = ExportOptions {
            encoding: Some(decode::CP1252.to_owned()),
            ..ExportOptions::default()
        };
        let r = s.export(id, &path, &opts).expect("export");
        assert!(
            r.warnings.iter().any(|w| w.contains("표현할 수 없는")),
            "{:?}",
            r.warnings
        );
        let _ = std::fs::remove_file(&path);
    }
}
