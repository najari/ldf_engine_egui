//! M1 게이트 G1: Vector `TestFiles/Clean` 10개가 모두 임포트되고, 헤더·모델이 파일 내용과 맞는다.
//!
//! `LDF_VECTOR_TESTFILES`가 없으면 건너뛴다. 상용 제품 동봉물이므로 저장소에 파일을 넣지 않는다(NFR-06).

use ldf_engine::Session;
use ldf_model::Severity;
use std::path::PathBuf;

#[test]
fn all_clean_files_import_with_a_consistent_header() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let clean = PathBuf::from(dir).join("Clean");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&clean)
        .expect("Clean 폴더")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ldf")))
        .collect();
    files.sort();
    assert_eq!(files.len(), 10, "Clean 파일은 10개여야 한다");

    let mut session = Session::open_in_memory().expect("session");
    let mut problems = Vec::new();
    for f in &files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&std::fs::read(f).expect("read")).into_owned();
        let out = match session.import_file(f) {
            Ok(o) => o,
            Err(e) => {
                problems.push(format!("{name}: 임포트 실패 {e}"));
                continue;
            }
        };
        let snap = &out.snapshot;
        let m = &snap.model;

        // 구문 오류 없음(Clean 파일이므로)
        let errors: Vec<_> = snap
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        if !errors.is_empty() {
            problems.push(format!("{name}: 오류 진단 {errors:?}"));
        }
        // 헤더가 원문에 적힌 값과 같다
        let declared = text
            .lines()
            .find(|l| l.trim_start().starts_with("LIN_protocol_version"))
            .and_then(|l| l.split('"').nth(1))
            .map(str::to_owned);
        if declared != m.header.protocol_version {
            problems.push(format!(
                "{name}: 프로토콜 {declared:?} != {:?}",
                m.header.protocol_version
            ));
        }
        if m.header.speed_bps.is_none() {
            problems.push(format!("{name}: 속도를 읽지 못함"));
        }
        // 참조가 모두 해석되었다(Clean 파일은 깨진 참조가 없다)
        let broken_sig = m.signals.iter().any(|(_, s)| {
            s.publisher.as_ref().is_some_and(|p| p.id().is_none())
                || s.subscribers.iter().any(|r| r.id().is_none())
        });
        let broken_frame = m
            .frames
            .iter()
            .any(|(_, f)| f.signals().iter().any(|fs| fs.signal.id().is_none()));
        if broken_sig || broken_frame {
            problems.push(format!("{name}: 해석되지 않은 참조가 있다"));
        }
        if m.nodes.is_empty() || m.frames.is_empty() || m.signals.is_empty() {
            problems.push(format!("{name}: 모델이 비었다"));
        }
    }
    session.check_integrity().expect("integrity");
    eprintln!("Clean {}개 임포트, 문제 {}건", files.len(), problems.len());
    assert!(problems.is_empty(), "{problems:#?}");
}
