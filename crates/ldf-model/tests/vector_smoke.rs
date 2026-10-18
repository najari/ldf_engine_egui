//! 로컬 Vector `TestFiles`를 전부 읽어도 패닉하지 않는지 확인하는 스모크 테스트(NFR-02).
//!
//! `LDF_VECTOR_TESTFILES`에 폴더를 지정했을 때만 실행하고, 없으면 건너뛴다.
//! 상용 제품 동봉물이므로 저장소에는 파일을 넣지 않는다(NFR-06).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ldf")) {
            out.push(p);
        }
    }
}

#[test]
fn every_vector_test_file_loads_without_panicking() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();
    assert!(!files.is_empty(), "{dir} 아래에 .ldf가 없다");

    let mut by_code: BTreeMap<u16, usize> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples: BTreeMap<u16, Vec<String>> = BTreeMap::new();
    let mut empty_models = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("read");
        let text = String::from_utf8_lossy(&bytes);
        let l = ldf_model::load(&text);
        let idx = ldf_model::LineIndex::new(&text);
        for d in &l.diagnostics {
            *by_code.entry(d.code).or_default() += 1;
            let ex = examples.entry(d.code).or_default();
            if ex.len() < 4 {
                let (line, col) = d.span.map_or((0, 0), |s| idx.line_col(&text, s.start));
                let name = f
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ex.push(format!("{name}:{line}:{col} {}", d.message));
            }
        }
        for s in &l.model.unknown_sections {
            *unknown.entry(s.name.clone()).or_default() += 1;
        }
        if l.model.frames.is_empty() && l.model.signals.is_empty() {
            empty_models.push(f.display().to_string());
        }
    }
    eprintln!("파일 {}개, 진단 코드별 개수: {by_code:?}", files.len());
    eprintln!("보관한 알 수 없는 섹션: {unknown:?}");
    for (code, ex) in &examples {
        eprintln!("  [{code}] {ex:#?}");
    }
    eprintln!("프레임·시그널이 없는 모델: {empty_models:?}");
}
