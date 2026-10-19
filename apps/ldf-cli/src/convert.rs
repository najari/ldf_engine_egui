//! `ldf-cli convert <입력.ldf> --to <버전> -o <출력>`: 다른 LIN 버전·줄바꿈·인코딩으로 다시 쓴다(FR-CLI-02, FR-EXP-01과 같은 경고).

use crate::args::Args;
use ldf_engine::{Eol, ExportFormat, ExportOptions, Session, Target, decode};
use ldf_i18n::twriteln;
use std::io::Write;
use std::path::Path;

fn target(s: &str) -> Option<Target> {
    Some(match s {
        "keep" => Target::Keep,
        "2.0" => Target::V2_0,
        "2.1" => Target::V2_1,
        "2.2" => Target::V2_2,
        _ => return None,
    })
}

fn encoding(s: &str) -> Option<&'static str> {
    Some(match s {
        "utf-8" => decode::UTF8,
        "utf-8-bom" => decode::UTF8_BOM,
        "windows-1252" => decode::CP1252,
        _ => return None,
    })
}

/// 종료 코드: 0 = 성공, 2 = 오류 진단이 남아 쓰지 않음(`--allow-errors`로 무시), 3 = 실행 실패.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match Args::parse(
        args,
        &["--to", "-o", "--eol", "--encoding", "--format"],
        &["--allow-errors"],
    ) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let [input] = parsed.positional.as_slice() else {
        let _ = twriteln!(err, "convert는 입력 파일 하나가 필요하다");
        return 3;
    };
    let Some(output) = parsed.value("-o") else {
        let _ = twriteln!(err, "-o <출력 파일>이 필요하다");
        return 3;
    };
    let mut opts = ExportOptions {
        allow_errors: parsed.flag("--allow-errors"),
        ..ExportOptions::default()
    };
    if let Some(t) = parsed.value("--to") {
        let Some(t) = target(t) else {
            let _ = twriteln!(err, "--to 값은 keep, 2.0, 2.1, 2.2 중 하나여야 한다: {t}");
            return 3;
        };
        opts.target = t;
    }
    match parsed.value("--eol") {
        None => {}
        Some("lf") => opts.eol = Some(Eol::Lf),
        Some("crlf") => opts.eol = Some(Eol::CrLf),
        Some(other) => {
            let _ = twriteln!(err, "--eol 값은 lf 또는 crlf여야 한다: {other}");
            return 3;
        }
    }
    if let Some(e) = parsed.value("--encoding") {
        let Some(e) = encoding(e) else {
            let _ = twriteln!(
                err,
                "--encoding 값은 utf-8, utf-8-bom, windows-1252 중 하나여야 한다: {e}"
            );
            return 3;
        };
        opts.encoding = Some(e.to_owned());
    }
    match parsed.value("--format") {
        None | Some("ldf") => {}
        Some("json") => opts.format = ExportFormat::Json,
        Some(other) => {
            let _ = twriteln!(err, "--format 값은 ldf 또는 json이어야 한다: {other}");
            return 3;
        }
    }

    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            let _ = twriteln!(err, "{input}: 파일을 읽을 수 없다: {e}");
            return 3;
        }
    };
    let name = Path::new(input)
        .file_name()
        .map_or_else(|| input.clone(), |n| n.to_string_lossy().into_owned());
    let result = Session::open_in_memory().and_then(|mut s| {
        let snap = s.import_bytes(&name, &bytes)?.snapshot;
        s.export_snapshot(&snap, Path::new(output), &opts)
    });
    match result {
        Ok(report) => {
            for w in &report.warnings {
                let _ = twriteln!(err, "경고: {w}");
            }
            let _ = twriteln!(
                out,
                "{output}: {} 바이트, {}",
                report.bytes,
                report.encoding
            );
            0
        }
        Err(ldf_engine::EngineError::ExportBlocked { errors }) => {
            let _ = twriteln!(
                err,
                "오류 진단이 {errors}개 남아 있어 쓰지 않았다(그래도 쓰려면 --allow-errors). 자세한 내용은 `ldf-cli check {input}`"
            );
            2
        }
        Err(e) => {
            let _ = writeln!(err, "{e}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ldfcli-conv-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    fn go(args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    /// 2.2 파일(오류 진단은 없고 Channel_name·initial_NAD 같은 2.1+ 항목이 있다).
    const V22: &str = "LIN_description_file;\nLIN_protocol_version = \"2.2\";\nLIN_language_version = \"2.2\";\nLIN_speed = 19.2 kbps;\nChannel_name = \"Body\";\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\nSignals { S: 8, 0, A, M; }\nFrames { F: 1, A, 1 { S, 0; } }\n\
Node_attributes { A { LIN_protocol = \"2.2\"; configured_NAD = 1; initial_NAD = 1; product_id = 1, 2, 3; response_error = S; P2_min = 50 ms; ST_min = 0 ms; N_As_timeout = 1000 ms; N_Cr_timeout = 1000 ms; configurable_frames { F; } } }\n\
Schedule_tables { T { F delay 10 ms; } }\n";

    #[test]
    fn converting_down_to_2_0_prints_the_dropped_items() {
        let d = dir();
        let input = d.join("in.ldf");
        std::fs::write(&input, V22).expect("write");
        let output = d.join("out.ldf");
        let (code, out, err) = go(&[
            input.to_str().expect("p"),
            "--to",
            "2.0",
            "-o",
            output.to_str().expect("p"),
            "--allow-errors",
        ]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("out.ldf"));
        assert!(
            err.contains("Channel_name") && err.contains("initial_NAD"),
            "{err}"
        );
        let text = std::fs::read_to_string(&output).expect("read");
        assert!(
            text.contains("LIN_protocol_version = \"2.0\";") && !text.contains("Channel_name"),
            "{text}"
        );
    }

    #[test]
    fn json_and_encoding_options() {
        let d = dir();
        let input = d.join("in2.ldf");
        std::fs::write(&input, V22).expect("write");
        let json = d.join("m.json");
        let (code, _, err) = go(&[
            input.to_str().expect("p"),
            "--format",
            "json",
            "-o",
            json.to_str().expect("p"),
        ]);
        assert_eq!(code, 0, "{err}");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&json).expect("read")).expect("json");
        assert_eq!(v["header"]["protocol_version"], "2.2");
        let crlf = d.join("crlf.ldf");
        let (code, _, err) = go(&[
            input.to_str().expect("p"),
            "--eol",
            "crlf",
            "--encoding",
            "utf-8-bom",
            "-o",
            crlf.to_str().expect("p"),
            "--allow-errors",
        ]);
        assert_eq!(code, 0, "{err}");
        let bytes = std::fs::read(&crlf).expect("read");
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]) && bytes.windows(2).any(|w| w == b"\r\n"));
    }

    #[test]
    fn errors_block_the_write_unless_allowed() {
        let d = dir();
        let bad = d.join("bad.ldf");
        std::fs::write(&bad, "Nodes {").expect("write");
        let out = d.join("bad-out.ldf");
        let (code, _, err) = go(&[bad.to_str().expect("p"), "-o", out.to_str().expect("p")]);
        assert_eq!(code, 2);
        assert!(err.contains("--allow-errors"), "{err}");
        assert!(!out.exists());
        assert_eq!(
            go(&[
                bad.to_str().expect("p"),
                "-o",
                out.to_str().expect("p"),
                "--allow-errors"
            ])
            .0,
            0
        );
    }

    #[test]
    fn bad_arguments() {
        assert_eq!(go(&[]).0, 3);
        assert_eq!(go(&["x.ldf"]).0, 3);
        assert_eq!(go(&["x.ldf", "-o", "y", "--to", "3.0"]).0, 3);
        assert_eq!(go(&["Z:/no.ldf", "-o", "y"]).0, 3);
        assert_eq!(go(&["x.ldf", "-o", "y", "--eol", "cr"]).0, 3);
    }
}
