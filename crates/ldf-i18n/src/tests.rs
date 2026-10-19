use super::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn en(s: &str) -> String {
    to_english(s).into_owned()
}

#[test]
fn exact_phrase() {
    assert_eq!(en("저장"), "Save");
    assert_eq!(en("파일"), "File");
}

#[test]
fn template_keeps_values() {
    assert_eq!(
        en("프레임 `F1`의 ID 70이(가) 0–59를 벗어났다"),
        "ID 70 of frame `F1` is outside 0–59"
    );
    assert_eq!(
        en("내보냈다: C:\\a.ldf (120 바이트, UTF-8)"),
        "Exported: C:\\a.ldf (120 bytes, UTF-8)"
    );
}

#[test]
fn template_translates_values_and_capitalizes() {
    // `{}` 자리의 종류 이름도 번역하고, 값으로 시작하는 틀은 대문자로 시작한다
    assert_eq!(en("노드 이름 `A`이 중복된다"), "Duplicate node name `A`");
    assert_eq!(en("노드 `A`이(가) 이미 있다"), "Node `A` already exists");
    assert_eq!(
        en("`Foo` 섹션은(는) LIN 2.0 파일에서 허용되지 않는다"),
        "The `Foo` section is not allowed in a LIN 2.0 file"
    );
    assert_eq!(
        en("`Foo` 섹션은 이 LIN 버전에서 무시된다"),
        "The `Foo` section is ignored in this LIN version"
    );
}

#[test]
fn longer_template_wins_over_looser_one() {
    // "스케줄 `{}`: {}"보다 구체적인 틀이 먼저 맞아야 한다
    assert_eq!(
        en("스케줄 `S`의 PID 0x1F2이(가) 한 바이트를 넘는다"),
        "PID 0x1F2 in schedule `S` exceeds one byte"
    );
    assert_eq!(
        en("스케줄 `S`: AssignNAD의 NAD가 1–125를 벗어났다"),
        "Schedule `S`: the NAD of AssignNAD is outside 1–125"
    );
}

#[test]
fn values_may_contain_the_separator() {
    // 종류 이름에 공백이 있어도 개수·이름과 섞이지 않는다
    assert_eq!(
        en("인코딩 타입 2개 삭제: A, B"),
        "Deletes 2 encoding type(s): A, B"
    );
    assert_eq!(
        en("프레임에 놓인 시그널 2개 정리"),
        "Signals placed in frames cleaned up: 2"
    );
}

#[test]
fn unknown_and_ascii_text_is_unchanged() {
    assert_eq!(en("표에 없는 문장"), "표에 없는 문장");
    assert_eq!(en("Already English"), "Already English");
    assert_eq!(en(""), "");
}

#[test]
fn korean_mode_is_identity() {
    // 전역 상태를 바꾸지 않고 한국어 쪽 경로만 본다
    assert!(!is_english());
    assert_eq!(t("저장"), "저장");
    assert_eq!(tr("노드 `A`이(가) 이미 있다"), "노드 `A`이(가) 이미 있다");
}

#[test]
fn change_labels_differ_from_button_text() {
    // 같은 한국어 "삭제"가 버튼과 비교 결과에서 다르게 번역된다
    assert_eq!(en("삭제"), "Delete");
    assert_eq!(change_label_en("삭제"), "Removed");
    assert_eq!(change_label_en("이름 변경"), "Renamed");
}

#[test]
fn table_is_well_formed() {
    let mut seen = HashSet::new();
    for (ko, eng) in table::ENTRIES {
        assert!(seen.insert(*ko), "한국어 문구가 두 번 있다: {ko}");
        assert!(!has_hangul(eng), "영어 칸에 한글이 있다: {eng}");
        assert!(!eng.is_empty(), "영어가 비었다: {ko}");
        let slots = ko.matches("{}").count();
        let mut next = 0;
        let mut rest = *eng;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            let spec = &rest[open + 1..open + close];
            let used = if spec.is_empty() {
                next += 1;
                next
            } else {
                spec.parse::<usize>().unwrap_or_default()
            };
            assert!(
                used <= slots,
                "영어의 값 자리가 한국어보다 많다: {ko} / {eng}"
            );
            rest = &rest[open + close + 1..];
        }
        if slots > 0 {
            assert!(
                ko.replace("{}", "").chars().any(|c| c != ' '),
                "{ko}: 고정 글자가 있어야 틀로 맞출 수 있다"
            );
        }
    }
}

// ───────── 번역 빠짐 검사 ─────────

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 한 줄에서 한글이 든 문자열 리터럴을 꺼낸다(주석 뒷부분은 버린다).
fn hangul_literals(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => break,
            '\'' => {
                // 문자 리터럴('"', '\'' 등)은 건너뛴다. 수명 표기('a)는 그냥 지나간다
                if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else if chars.get(i + 1) == Some(&'\\') && chars.get(i + 3) == Some(&'\'') {
                    i += 4;
                } else {
                    i += 1;
                }
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    match chars[i] {
                        '\\' => {
                            if let Some(&n) = chars.get(i + 1) {
                                s.push(match n {
                                    'n' => '\n',
                                    't' => '\t',
                                    other => other,
                                });
                            }
                            i += 2;
                        }
                        '"' => {
                            closed = true;
                            i += 1;
                            break;
                        }
                        c => {
                            s.push(c);
                            i += 1;
                        }
                    }
                }
                if closed && has_hangul(&s) {
                    out.push(s);
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// `{name}`·`{:04}`·`{}` 같은 값 자리를 모두 `{}`로 바꾼다.
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find(['{', '}']) {
            Some(end) if after.as_bytes()[end] == b'}' => {
                out.push_str("{}");
                rest = &after[end + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    // `{name}{form}`처럼 맞붙은 값 자리는 틀에서 하나로 본다
    while out.contains("{}{}") {
        out = out.replace("{}{}", "{}");
    }
    out
}

/// 화면에 나오지 않는 문구: 로그 메시지와 DB에 저장되는 기본 이름.
const NOT_SHOWN: &[&str] = &[
    "내보냈다",
    "리비전 저장",
    "LDF 프로젝트",
    "명령 처리",
    "명령 실패",
    "작업자가 패닉했다",
    "알 수 없는 패닉",
];

fn is_log_or_internal(line: &str) -> bool {
    [
        "tracing::",
        "span!(",
        "info!(",
        "warn!(",
        "error!(",
        "debug!(",
        "panic!(",
        ".expect(",
    ]
    .iter()
    .any(|m| line.contains(m))
}

fn missing_in(files: &[PathBuf]) -> Vec<String> {
    let known: HashSet<&str> = table::ENTRIES
        .iter()
        .chain(table::INLINE)
        .map(|(ko, _)| *ko)
        .collect();
    let mut missing = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if line.trim_start().starts_with("//") || is_log_or_internal(line) {
                continue;
            }
            for lit in hangul_literals(line) {
                let key = normalize(&lit);
                if !known.contains(key.as_str()) && !NOT_SHOWN.contains(&key.as_str()) {
                    let rel = f.strip_prefix(repo_root()).unwrap_or(f);
                    missing.push(format!("{}:{}  {key}", rel.display(), n + 1));
                }
            }
        }
    }
    missing
}

fn collect(dirs: &[&str], files: &[&str], skip_names: &[&str]) -> Vec<PathBuf> {
    let root = repo_root();
    let mut all = Vec::new();
    for d in dirs {
        rust_files(&root.join(d), &mut all);
    }
    for f in files {
        all.push(root.join(f));
    }
    all.retain(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !skip_names.contains(&name) && !p.components().any(|c| c.as_os_str() == "i18n")
    });
    all
}

#[test]
fn every_studio_phrase_is_translated() {
    let files = collect(
        &["apps/ldf-studio/src"],
        &[],
        &[
            "tests.rs",
            "gui_tests.rs",
            "main.rs",
            "logging.rs",
            "fonts.rs",
        ],
    );
    let missing = missing_in(&files);
    assert!(
        missing.is_empty(),
        "번역 표에 없는 화면 문구:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_cli_message_is_translated() {
    let files = collect(&["apps/ldf-cli/src"], &[], &["tests.rs"]);
    let missing = missing_in(&files);
    assert!(
        missing.is_empty(),
        "번역 표에 없는 CLI 메시지:\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_library_message_is_translated() {
    let files = collect(
        &["crates/ldf-check/src/rules"],
        &[
            "crates/ldf-model/src/edit.rs",
            "crates/ldf-model/src/lower.rs",
            "crates/ldf-model/src/semantic.rs",
            "crates/ldf-syntax/src/lexer.rs",
            "crates/ldf-syntax/src/parser.rs",
            "crates/ldf-writer/src/lib.rs",
            "crates/ldf-store/src/error.rs",
            "crates/ldf-store/src/lib.rs",
            "crates/ldf-store/src/load.rs",
            "crates/ldf-engine/src/error.rs",
            "crates/ldf-engine/src/export.rs",
            "crates/ldf-engine/src/worker.rs",
            "crates/ldf-engine/src/session.rs",
            "crates/ldf-engine/src/diff_report.rs",
        ],
        &["tests.rs"],
    );
    let missing = missing_in(&files);
    assert!(
        missing.is_empty(),
        "번역 표에 없는 메시지:\n{}",
        missing.join("\n")
    );
}

// ───────── 실제 진단 메시지 ─────────

fn untranslated(texts: &[String]) -> Vec<String> {
    let mut left: Vec<String> = texts
        .iter()
        .map(|m| to_english(m).into_owned())
        .filter(|m| has_hangul(m))
        .collect();
    left.sort();
    left.dedup();
    left
}

fn diagnostic_messages(src: &str) -> Vec<String> {
    let loaded = ldf_model::load(src);
    let mut out: Vec<String> = loaded
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    out.extend(
        ldf_check::run_all(&loaded.model)
            .into_iter()
            .map(|d| d.message),
    );
    out
}

#[test]
fn broken_sample_diagnostics_are_translated() {
    const BROKEN: &str = r#"
LIN_description_file;
LIN_protocol_version = "9.9";
LIN_language_version = "2.1";
LIN_speed = 99 kbps;
Nodes { Master: M, 1 ms, 0 ms; Slaves: A, A; }
Signals { S1: 8, 0, Ghost, A; S2: 70, 0, M, A; S2: 8, 0, M, A; B: 16, {1,2}, M, A; }
Frames {
  F1: 70, Nobody, 9 { S1, 0; S2, 4; Missing, 8; }
  F1: 1, M, 8 { S2, 0; }
}
Sporadic_frames { SP: NoSuch; }
Event_triggered_frames { EV: Nope, 3, F1; }
Diagnostic_frames { MasterReq: 0x3c { MasterReqB0, 0; } }
Node_attributes { A { LIN_protocol = "1.0"; configured_NAD = 0x1; product_id = 0x1, 0x2; response_error = Nope; } }
Schedule_tables { T { F1 delay 0.1 ms; Ghost delay 10 ms; AssignNAD { Nobody } delay 10 ms; } Empty { } }
Signal_encoding_types { E { logical_value, 0, "a"; logical_value, 0, "b"; physical_value, 5, 1, 1, 0, "u"; } }
Signal_representation { E: S1, Missing; }
Unknown_section { }
"#;
    let msgs = diagnostic_messages(BROKEN);
    assert!(
        msgs.len() > 10,
        "시료가 진단을 충분히 내야 한다: {}",
        msgs.len()
    );
    let left = untranslated(&msgs);
    assert!(left.is_empty(), "번역되지 않은 진단:\n{}", left.join("\n"));
}

/// Vector 테스트 파일이 있으면(`LDF_VECTOR_TESTFILES`) 모든 파일의 진단이 번역되는지 본다.
#[test]
fn vector_corpus_diagnostics_are_translated() {
    let Some(dir) = std::env::var_os("LDF_VECTOR_TESTFILES") else {
        return;
    };
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ldf")) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(Path::new(&dir), &mut files);
    let mut msgs = Vec::new();
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
        };
        msgs.extend(diagnostic_messages(&text));
    }
    let left = untranslated(&msgs);
    assert!(left.is_empty(), "번역되지 않은 진단:\n{}", left.join("\n"));
}
