//! 내보내기(FR-EXP-01/02/04): 리비전의 모델을 LDF 파일로 쓰고, 쓴 결과를 다시 읽어 검증한다.

use crate::decode;
use crate::error::{EngineError, Result};
use crate::session::{ModelSnapshot, Session};
use ldf_model::{Severity, load, semantically_equal};
use ldf_writer::{Eol, Target, WriteOptions, write, write_file_atomic};
use std::path::{Path, PathBuf};

/// 내보내는 파일 형식.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    /// LDF 텍스트.
    #[default]
    Ldf,
    /// 모델 JSON(`docs/model.schema.json`).
    Json,
}

/// 내보내기 옵션.
#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    /// 파일 형식.
    pub format: ExportFormat,
    /// 대상 LIN 버전.
    pub target: Target,
    /// 줄바꿈. `None`이면 원본과 같게(원본이 CRLF면 CRLF).
    pub eol: Option<Eol>,
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
    /// 저장된 리비전을 `path`에 LDF로 쓴다. 쓰기는 원자적이다(FR-EXP-05).
    ///
    /// 오류 진단이 남은 리비전은 `allow_errors`가 아니면 [`EngineError::ExportBlocked`]로 거부한다.
    pub fn export(
        &mut self,
        revision: i64,
        path: &Path,
        opts: &ExportOptions,
    ) -> Result<ExportReport> {
        let snap = self.read_revision(revision)?;
        self.export_snapshot(&snap, path, opts)
    }

    /// 이미 열린 스냅샷(저장 안 한 편집 포함)을 `path`에 쓴다.
    pub fn export_snapshot(
        &self,
        snap: &ModelSnapshot,
        path: &Path,
        opts: &ExportOptions,
    ) -> Result<ExportReport> {
        // JSON은 다른 도구가 읽는 모델 덤프라 검사 결과와 무관하게 내보낸다
        if opts.format == ExportFormat::Json {
            let text = ldf_model::to_json_string(&snap.model);
            write_file_atomic(path, text.as_bytes()).map_err(|source| EngineError::Io {
                path: path.to_owned(),
                source,
            })?;
            return Ok(ExportReport {
                path: path.to_owned(),
                bytes: text.len(),
                encoding: decode::UTF8.to_owned(),
                warnings: Vec::new(),
                verified: true,
            });
        }
        let errors = snap
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        if errors > 0 && !opts.allow_errors {
            return Err(EngineError::ExportBlocked { errors });
        }

        // 버전을 바꾸지 않으면 스냅샷의 텍스트를 그대로 쓴다. 편집하지 않았으면 원문이고, 편집했으면 주석을 옮겨 다시 쓴 텍스트다(FR-EXP-03).
        // 줄바꿈만 다르게 고르면 그 부분만 바꾼다.
        let source_eol = if snap.source.contains("\r\n") {
            Eol::CrLf
        } else {
            Eol::Lf
        };
        let (text, warnings) = if opts.target == Target::Keep {
            let text = match opts.eol {
                Some(eol) if eol != source_eol => {
                    let lf = snap.source.replace("\r\n", "\n");
                    match eol {
                        Eol::Lf => lf,
                        Eol::CrLf => lf.replace('\n', "\r\n"),
                    }
                }
                _ => snap.source.clone(),
            };
            (text, Vec::new())
        } else {
            let written = write(
                &snap.model,
                &WriteOptions {
                    target: opts.target,
                    eol: opts.eol.unwrap_or(source_eol),
                },
            );
            (written.text, written.warnings)
        };
        let encoding = opts
            .encoding
            .clone()
            .unwrap_or_else(|| snap.revision.encoding.clone());
        let bytes = decode::encode(&text, &encoding);

        let mut warnings = warnings;
        // 인코딩으로 표현할 수 없는 글자는 `?`가 되므로 미리 알린다
        if decode::decode(&bytes, &encoding) != text {
            warnings.push(format!("{encoding}으로 표현할 수 없는 문자를 ?로 바꿨다"));
        }

        // 쓴 바이트를 다시 읽어 의미가 같은지 본다. 버전을 바꾸면 항목이 빠지므로 일치하지 않을 수 있다.
        let again = load(&decode::decode(&bytes, &encoding));
        let verified = opts.target == Target::Keep && semantically_equal(&snap.model, &again.model);

        write_file_atomic(path, &bytes).map_err(|source| EngineError::Io {
            path: path.to_owned(),
            source,
        })?;
        tracing::info!(
            bytes = bytes.len(),
            warnings = warnings.len(),
            verified,
            "내보냈다"
        );
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

    /// 검사기가 지적할 만한 최소 예제라 오류 확인은 건너뛴다(차단 동작은 별도 시험이 본다).
    fn lenient() -> ExportOptions {
        ExportOptions {
            allow_errors: true,
            ..ExportOptions::default()
        }
    }

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
        let r = s.export(id, &path, &lenient()).expect("export");
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
            eol: Some(Eol::CrLf),
            encoding: Some(decode::CP1252.to_owned()),
            allow_errors: true,
            ..ExportOptions::default()
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
            ..lenient()
        };
        let r = s.export(id, &path, &opts).expect("export");
        assert!(
            r.warnings.iter().any(|w| w.contains("표현할 수 없는")),
            "{:?}",
            r.warnings
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unedited_export_keeps_comments_and_formatting_byte_for_byte() {
        let src = "// 머리말\r\n// 두 번째 줄\r\nLIN_description_file;\r\n/* 본문 주석 */\r\nLIN_protocol_version = \"2.1\";\r\nNodes   {  Master:M,1 ms,0 ms; Slaves:A;  }\r\nSignals { S: 8, 0, A, M; }  // 끝\r\nFrames { F: 1, A, 1 { S, 0; } }\r\n";
        let mut s = Session::open_in_memory().expect("session");
        let snap = s
            .import_bytes("a.ldf", src.as_bytes())
            .expect("import")
            .snapshot;
        let path = temp("verbatim.ldf");
        s.export_snapshot(&snap, &path, &lenient()).expect("export");
        assert_eq!(std::fs::read(&path).expect("read"), src.as_bytes());

        // 줄바꿈만 고르면 그 부분만 바뀌고 주석은 남는다
        let opts = ExportOptions {
            eol: Some(Eol::Lf),
            ..lenient()
        };
        s.export_snapshot(&snap, &path, &opts).expect("export");
        let lf = std::fs::read_to_string(&path).expect("read");
        assert_eq!(lf, src.replace("\r\n", "\n"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn edited_export_keeps_the_banner_comment() {
        let src = "// 머리말\n// 두 번째 줄\nLIN_description_file;\nLIN_protocol_version = \"2.1\";\n// 사라지는 주석\nNodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n";
        let mut s = Session::open_in_memory().expect("session");
        let snap = s
            .import_bytes("a.ldf", src.as_bytes())
            .expect("import")
            .snapshot;
        let edited = s
            .edit(&snap, &ldf_model::EditOp::AddSlave("B".into()))
            .expect("edit");
        let path = temp("banner.ldf");
        s.export_snapshot(&edited, &path, &lenient())
            .expect("export");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            text.starts_with("// 머리말\n// 두 번째 줄\n\nLIN_description_file;"),
            "{text}"
        );
        assert!(text.contains("Slaves: A, B;"));
        assert!(
            text.contains("사라지는 주석"),
            "본문 주석은 같은 문장 옆에 남는다"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn json_export_writes_the_model_by_name_even_with_errors() {
        let mut s = Session::open_in_memory().expect("session");
        let snap = s
            .import_bytes(
                "bad.ldf",
                b"Nodes { Master: M, 1 ms, 0 ms; Slaves: A; } Signals { S: 8, 0, Ghost, M; }",
            )
            .expect("import")
            .snapshot;
        let path = temp("model.json");
        let opts = ExportOptions {
            format: ExportFormat::Json,
            ..ExportOptions::default()
        };
        let r = s
            .export_snapshot(&snap, &path, &opts)
            .expect("오류가 있어도 JSON은 내보낸다");
        assert_eq!(r.encoding, "utf-8");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
        assert_eq!(v["format"], "ldf-model");
        assert_eq!(v["signals"][0]["publisher"], "Ghost");
        let _ = std::fs::remove_file(&path);
    }
}
