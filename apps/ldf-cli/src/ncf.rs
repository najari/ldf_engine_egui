//! `ldf-cli ncf`: 노드 기능 파일(NCF)을 읽어 내용을 보여 준다(읽기 전용).

use crate::args::Args;
use ldf_engine::{NcfDocument, read_ncf};
use ldf_i18n::{t, tf, tr, twriteln};
use ldf_model::{LineIndex, Severity};
use ldf_ncf::{
    Direction, EncodingRef, NcfEncoding, NcfNode, describe_entry, describe_init, format_ms, hex,
};
use serde_json::json;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// `라벨: 값` 한 줄(라벨은 번역한다).
fn line(
    out: &mut dyn Write,
    indent: usize,
    label: &'static str,
    value: impl std::fmt::Display,
) -> std::io::Result<()> {
    writeln!(out, "{:indent$}{}: {value}", "", t(label))
}

fn opt<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

fn write_encoding(out: &mut dyn Write, indent: usize, e: &NcfEncoding) -> std::io::Result<()> {
    writeln!(out, "{:indent$}{}", "", e.name)?;
    for entry in &e.entries {
        writeln!(out, "{:indent$}  {}", "", describe_entry(entry))?;
    }
    Ok(())
}

fn write_node(out: &mut dyn Write, n: &NcfNode) -> std::io::Result<()> {
    line(out, 0, "노드", &n.name)?;
    let g = &n.general;
    writeln!(out, "  {}", t("일반"))?;
    line(
        out,
        4,
        "LIN 프로토콜 버전",
        opt(g.protocol_version.as_deref()),
    )?;
    line(out, 4, "공급자 ID", opt(g.supplier.map(|v| hex(v, 4))))?;
    line(out, 4, "기능 ID", opt(g.function.map(|v| hex(v, 4))))?;
    line(out, 4, "변형", opt(g.variant))?;
    line(
        out,
        4,
        "비트레이트",
        opt(g.bitrate.as_ref().map(|b| b.describe())),
    )?;
    if let Some(w) = &g.sends_wake_up_signal {
        line(out, 4, "웨이크업 신호 전송", w)?;
    }
    for item in &g.extra {
        writeln!(out, "    {}: {}", item.key, item.value)?;
    }

    let d = &n.diagnostic;
    writeln!(out, "  {}", t("진단"))?;
    line(out, 4, "NAD", opt(d.nad.as_ref().map(|v| v.describe())))?;
    if let Some(v) = d.diagnostic_class {
        line(out, 4, "진단 클래스", v)?;
    }
    for (label, v) in [
        ("P2_min", d.p2_min_ms),
        ("ST_min", d.st_min_ms),
        ("N_As_timeout", d.n_as_timeout_ms),
        ("N_Cr_timeout", d.n_cr_timeout_ms),
    ] {
        if let Some(ms) = v {
            writeln!(out, "    {label}: {}", format_ms(ms))?;
        }
    }
    if let Some(v) = d.max_message_length {
        line(out, 4, "최대 메시지 길이", v)?;
    }
    if !d.support_sid.is_empty() {
        let sids: Vec<String> = d.support_sid.iter().map(|s| hex(*s, 2)).collect();
        line(out, 4, "지원 서비스 ID", sids.join(", "))?;
    }
    for item in &d.extra {
        writeln!(out, "    {}: {}", item.key, item.value)?;
    }

    twriteln!(out, "  프레임 {}개", n.frames.len())?;
    for f in &n.frames {
        let dir = match f.direction {
            Direction::Publish => t("발행"),
            Direction::Subscribe => t("구독"),
        };
        let mut parts = Vec::new();
        if let Some(v) = f.length {
            parts.push(format!("{} {v}", t("길이")));
        }
        if let Some(v) = f.message_id {
            parts.push(format!("{} {}", t("메시지 ID"), hex(v, 4)));
        }
        if let Some(e) = &f.event_triggered {
            parts.push(format!("{} {}", t("이벤트 트리거"), e.describe()));
        }
        if let (Some(lo), Some(hi)) = (f.min_period_ms, f.max_period_ms) {
            parts.push(format!("{} {}–{}", t("주기"), format_ms(lo), format_ms(hi)));
        } else if let Some(v) = f.min_period_ms.or(f.max_period_ms) {
            parts.push(format!("{} {}", t("주기"), format_ms(v)));
        }
        writeln!(out, "    {dir} {}  {}", f.name, parts.join(", "))?;
        for s in &f.signals {
            let enc = match &s.encoding {
                Some(EncodingRef::Named(n)) => format!(", {} {n}", t("인코딩")),
                Some(EncodingRef::Inline(e)) => format!(", {} {}", t("인코딩"), e.name),
                None => String::new(),
            };
            writeln!(
                out,
                "      {}  {} {}, {} {}, {} {}{enc}",
                s.name,
                t("초기값"),
                opt(s.init.as_ref().map(describe_init)),
                t("크기"),
                opt(s.size),
                t("오프셋"),
                opt(s.offset),
            )?;
        }
        for item in &f.extra {
            writeln!(out, "      {}: {}", item.key, item.value)?;
        }
    }

    if !n.encodings.is_empty() {
        twriteln!(out, "  인코딩 {}개", n.encodings.len())?;
        for e in &n.encodings {
            write_encoding(out, 4, e)?;
        }
    }
    if !n.status_management.is_empty() {
        writeln!(out, "  {}", t("상태 관리"))?;
        for item in &n.status_management {
            writeln!(out, "    {}: {}", item.key, item.value)?;
        }
    }
    if let Some(text) = &n.free_text {
        writeln!(out, "  {}", t("자유 텍스트"))?;
        for l in text.lines() {
            writeln!(out, "    {l}")?;
        }
    }
    for item in &n.extra {
        writeln!(out, "  {}: {}", item.key, item.value)?;
    }
    Ok(())
}

fn write_text(out: &mut dyn Write, path: &str, doc: &NcfDocument) -> std::io::Result<()> {
    writeln!(out, "{path}")?;
    line(
        out,
        0,
        "NCF 언어 버전",
        opt(doc.file.language_version.as_deref()),
    )?;
    for n in &doc.file.nodes {
        write_node(out, n)?;
    }
    let idx = LineIndex::new(&doc.text);
    for d in &doc.diagnostics {
        let (l, c) = d.span.map_or((0, 0), |s| idx.line_col(&doc.text, s.start));
        writeln!(
            out,
            "{path}:{l}:{c}: {} [{:04}] {}",
            severity_name(d.severity),
            d.code,
            tr(&d.message)
        )?;
    }
    Ok(())
}

fn write_json(out: &mut dyn Write, docs: &[(String, NcfDocument)]) -> std::io::Result<()> {
    let files: Vec<_> = docs
        .iter()
        .map(|(path, doc)| {
            let idx = LineIndex::new(&doc.text);
            let diags: Vec<_> = doc
                .diagnostics
                .iter()
                .map(|d| {
                    let (l, c) = d.span.map_or((0, 0), |s| idx.line_col(&doc.text, s.start));
                    json!({
                        "code": d.code,
                        "severity": severity_name(d.severity),
                        "line": l,
                        "col": c,
                        "message": tr(&d.message),
                    })
                })
                .collect();
            json!({ "file": path, "ncf": doc.file, "diagnostics": diags })
        })
        .collect();
    let s = serde_json::to_string_pretty(&files).map_err(std::io::Error::other)?;
    writeln!(out, "{s}")
}

/// 종료 코드: 오류 진단이 있으면 2, 경고가 있으면 1, 아니면 0(참고만 있으면 0).
fn exit_code(docs: &[(String, NcfDocument)]) -> u8 {
    let worst = docs
        .iter()
        .flat_map(|(_, d)| d.diagnostics.iter())
        .map(|d| d.severity)
        .max();
    match worst {
        Some(Severity::Error) => 2,
        Some(Severity::Warning) => 1,
        _ => 0,
    }
}

/// 명령을 실행하고 종료 코드를 돌려준다.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let parsed = match Args::parse(args, &["--format"], &[]) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 3;
        }
    };
    let format = match parsed.value("--format") {
        None | Some("text") => Format::Text,
        Some("json") => Format::Json,
        Some(other) => {
            let _ = writeln!(
                err,
                "{}",
                tf!("--format 값은 text 또는 json이어야 한다: {other:?}")
            );
            return 3;
        }
    };
    if parsed.positional.is_empty() {
        let _ = writeln!(err, "{}", t("읽을 NCF 파일을 하나 이상 지정해야 한다"));
        return 3;
    }

    let mut docs = Vec::new();
    let mut failed = false;
    for f in &parsed.positional {
        match std::fs::read(Path::new(f)) {
            Ok(bytes) => docs.push((f.clone(), read_ncf(&bytes))),
            Err(e) => {
                let _ = twriteln!(err, "{f}: 파일을 읽을 수 없다: {e}");
                failed = true;
            }
        }
    }

    let written = match format {
        Format::Text => docs
            .iter()
            .try_for_each(|(path, doc)| write_text(out, path, doc)),
        Format::Json => write_json(out, &docs),
    };
    if written.is_err() {
        return 3;
    }
    if failed { 3 } else { exit_code(&docs) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, content: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("ldfcli-ncf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let p = dir.join(name);
        std::fs::write(&p, content).expect("write");
        p.to_string_lossy().into_owned()
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

    pub(super) const NCF: &[u8] = b"node_capability_file;\nLIN_language_version = \"2.1\";\n\
node Motor {\n general { LIN_protocol_version = \"2.1\"; supplier = 0x1e; function = 1; variant = 0; bitrate = 19.2 kbps; }\n\
 diagnostic { NAD = 5; P2_min = 50 ms; }\n\
 frames { publish State { length = 2; event_triggered_frame = Ev; signals { Temp { init_value = 0; size = 8; offset = 0; enc; } } } publish Ev { length = 2; } }\n\
 encoding { enc { physical_value, 0, 200, 0.5, -20, \"Degree\"; } }\n\
 status_management { response_error = Temp; }\n}\n";

    #[test]
    fn prints_the_node_in_text() {
        let f = tmp("motor.ncf", NCF);
        let (code, out, _) = go(&[&f]);
        assert_eq!(code, 0, "{out}");
        for needle in [
            "NCF 언어 버전: 2.1",
            "노드: Motor",
            "공급자 ID: 0x001E",
            "비트레이트: 19.2 kbps",
            "NAD: 0x05",
            "P2_min: 50 ms",
            "프레임 2개",
            "발행 State",
            "이벤트 트리거 Ev",
            "Temp  초기값 0, 크기 8, 오프셋 0, 인코딩 enc",
            "physical_value 0..200, × 0.5, + -20 [Degree]",
            "response_error: Temp",
        ] {
            assert!(out.contains(needle), "{needle}\n{out}");
        }
    }

    #[test]
    fn json_output_is_valid_and_carries_the_structure() {
        let f = tmp("motor-json.ncf", NCF);
        let (code, out, _) = go(&[&f, "--format", "json"]);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(v[0]["ncf"]["nodes"][0]["name"], "Motor");
        assert_eq!(v[0]["ncf"]["nodes"][0]["general"]["supplier"], 0x1e);
        assert_eq!(v[0]["ncf"]["nodes"][0]["frames"][0]["name"], "State");
        assert_eq!(v[0]["diagnostics"], serde_json::json!([]));
    }

    #[test]
    fn a_file_that_is_not_an_ncf_exits_with_one() {
        let f = tmp("not.ncf", b"LIN_description_file;\n");
        let (code, out, _) = go(&[&f]);
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("[9201]"), "{out}");
    }

    #[test]
    fn bad_arguments_and_missing_files() {
        assert_eq!(go(&[]).0, 3);
        assert_eq!(go(&["x.ncf", "--format", "xml"]).0, 3);
        assert_eq!(go(&["--bogus", "x.ncf"]).0, 3);
        let (code, _, err) = go(&["Z:/no/such.ncf"]);
        assert_eq!(code, 3);
        assert!(err.contains("읽을 수 없다"));
    }
}
