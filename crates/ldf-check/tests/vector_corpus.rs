//! WP6 코퍼스 하니스: Vector 테스트 LDF의 `Failed Checks` 기대값과 `ldf_check::run_all` 결과를 비교한다.
//!
//! `LDF_VECTOR_TESTFILES`(폴더)가 없으면 건너뛴다. 파일은 상용 제품 동봉물이므로 저장소에 넣지 않고
//! 이 하니스도 파일 내용을 어디에도 복사하지 않는다(NFR-06).
//!
//! 비교 규칙은 `HowTo.txt`를 따른다.
//! - 각 파일의 `Failed Checks:` 뒤 문자열을 `, ( ) { } [ ]`와 공백으로 나눠 정수가 되는 조각을 검사 번호로 본다
//!   (앞의 0은 의미 없음). 검사 번호는 중복을 세는 멀티셋이다.
//! - `solely`: 실제 결과의 멀티셋이 기대와 같아야 한다. `ignore`: 파일을 건너뛴다.
//!   그 밖(normal)에는 기대 멀티셋이 실제 결과의 부분 멀티셋이면 통과한다.
//!
//! 일치율이 [`MIN_PASS_PERCENT`] 아래로 떨어지면 실패한다(회귀 방지용 래칫). 개별 불일치는 실패 사유가 아니라
//! 보고 대상이며 `docs/check-deviations.md`에 사유와 함께 기록한다.

use ldf_check::run_all;
use ldf_model::load;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// HowTo 규칙(normal = 부분 멀티셋)으로 센 전체 일치율의 하한(%). 올라가면 같이 올린다.
const MIN_PASS_PERCENT: f64 = 95.0;
/// 기대 멀티셋과 실제 멀티셋이 정확히 같은 파일 비율의 하한(%). 정밀도 회귀를 잡는다.
const MIN_EXACT_PERCENT: f64 = 50.0;

type Multiset = BTreeMap<u16, usize>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Solely,
    Ignore,
}

struct Expectation {
    mode: Mode,
    codes: Multiset,
    /// 앞에 0을 붙여 적은 검사 번호(그 파일이 일부러 노리는 결과). 나머지는 템플릿에서 따라오는 결과다.
    primary: Multiset,
}

/// 파일 바이트를 문자열로. UTF-8이 아니면 Latin-1(바이트=코드포인트)로 읽는다.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// `Failed Checks:` 줄을 찾아 기대값을 만든다. 줄이 없으면 `None`.
fn parse_expectation(text: &str) -> Option<Expectation> {
    const KEY: &str = "Failed Checks:";
    for line in text.lines() {
        // 접두부는 '/'와 공백만 허용한다
        let trimmed = line.trim_start_matches(['/', ' ', '\t']);
        let Some(spec) = trimmed.strip_prefix(KEY) else {
            continue;
        };
        let mode = if spec.contains("ignore") {
            Mode::Ignore
        } else if spec.contains("solely") {
            Mode::Solely
        } else {
            Mode::Normal
        };
        let mut codes = Multiset::new();
        let mut primary = Multiset::new();
        for slot in spec.split(|c: char| {
            matches!(c, ',' | '(' | ')' | '{' | '}' | '[' | ']') || c.is_whitespace()
        }) {
            if let Ok(n) = slot.parse::<u16>() {
                *codes.entry(n).or_insert(0) += 1;
                if slot.len() > 1 && slot.starts_with('0') {
                    *primary.entry(n).or_insert(0) += 1;
                }
            }
        }
        return Some(Expectation {
            mode,
            codes,
            primary,
        });
    }
    None
}

/// "수행하지 못함" 파일의 비교 대상.
///
/// Vector는 읽기 자체가 실패하면 모든 검사를 실패로 보고하므로 `20, 30, 101 … 408`처럼 거의 모든 번호가 나열된다
/// (Basic 폴더와 `0044b`·`0097`·`0104`·`0017`). 모든 번호를 되풀이하는 것은 의미가 없으므로 그 파일이 노리는 번호만 비교한다:
/// 앞에 0을 붙인 번호가 있으면 그것, 없으면 구조 오류(20)이거나 낮은 번호(00xx).
fn gate_target(exp: &Expectation) -> Option<Expectation> {
    if exp.codes.len() < 25 {
        return None;
    }
    let codes = if !exp.primary.is_empty() {
        exp.primary.clone()
    } else if exp.codes.contains_key(&20) {
        Multiset::from([(20, 1)])
    } else {
        let low: Multiset = exp
            .codes
            .iter()
            .filter(|(c, _)| **c < 100)
            .map(|(c, n)| (*c, *n))
            .collect();
        if low.is_empty() {
            return None;
        }
        low
    };
    Some(Expectation {
        mode: Mode::Normal,
        codes,
        primary: Multiset::new(),
    })
}
fn fmt_set(m: &Multiset) -> String {
    if m.is_empty() {
        return "(없음)".to_owned();
    }
    m.iter()
        .map(|(c, n)| {
            if *n == 1 {
                format!("{c:04}")
            } else {
                format!("{c:04}x{n}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `a`에서 `b`를 뺀 멀티셋(모자란 만큼만).
fn minus(a: &Multiset, b: &Multiset) -> Multiset {
    let mut out = Multiset::new();
    for (c, n) in a {
        let have = b.get(c).copied().unwrap_or(0);
        if *n > have {
            out.insert(*c, n - have);
        }
    }
    out
}

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

#[derive(Default)]
struct Tally {
    total: usize,
    pass: usize,
    exact: usize,
}

impl Tally {
    fn add(&mut self, pass: bool, exact: bool) {
        self.total += 1;
        self.pass += usize::from(pass);
        self.exact += usize::from(exact);
    }
    fn pct(n: usize, d: usize) -> f64 {
        if d == 0 {
            100.0
        } else {
            n as f64 * 100.0 / d as f64
        }
    }
    fn line(&self) -> String {
        format!(
            "통과 {}/{} ({:.1}%), 정확 일치 {}/{} ({:.1}%)",
            self.pass,
            self.total,
            Self::pct(self.pass, self.total),
            self.exact,
            self.total,
            Self::pct(self.exact, self.total)
        )
    }
}

#[test]
fn vector_corpus_matches_expected_checks() {
    let Ok(dir) = std::env::var("LDF_VECTOR_TESTFILES") else {
        eprintln!("LDF_VECTOR_TESTFILES가 없어 건너뜀");
        return;
    };
    let root = PathBuf::from(&dir);
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "코퍼스 폴더에 .ldf가 없다: {dir}");

    let mut by_folder: BTreeMap<String, Tally> = BTreeMap::new();
    let mut overall = Tally::default();
    let mut without_basic = Tally::default();
    let mut mismatches = Vec::new();
    let mut clean_violations = Vec::new();
    let mut skipped = Vec::new();

    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f);
        let name = rel.to_string_lossy().replace('\\', "/");
        let folder = name.split('/').next().unwrap_or("").to_owned();
        let Ok(bytes) = std::fs::read(f) else {
            mismatches.push(format!("{name}: 파일을 읽지 못했다"));
            continue;
        };
        let text = decode(&bytes);
        let Some(exp) = parse_expectation(&text) else {
            skipped.push(format!("{name}: `Failed Checks:` 줄 없음"));
            continue;
        };
        if exp.mode == Mode::Ignore {
            skipped.push(format!("{name}: ignore"));
            continue;
        }
        // 모든 검사가 실패로 나열된 파일은 노리는 번호만 본다(docs/check-deviations.md)
        let exp = gate_target(&exp).unwrap_or(exp);
        let loaded = load(&text);
        let diags = run_all(&loaded.model);
        let mut actual = Multiset::new();
        for d in &diags {
            *actual.entry(d.code).or_insert(0) += 1;
        }

        let missing = minus(&exp.codes, &actual);
        let extra = minus(&actual, &exp.codes);
        let exact = missing.is_empty() && extra.is_empty();
        let pass = match exp.mode {
            Mode::Solely => exact,
            _ => missing.is_empty(),
        };

        if folder == "Clean" && !diags.is_empty() {
            clean_violations.push(format!("{name}: {}", fmt_set(&actual)));
        }

        overall.add(pass, exact);
        if folder != "Basic" {
            without_basic.add(pass, exact);
        }
        by_folder.entry(folder).or_default().add(pass, exact);

        let status = match (pass, exact) {
            (true, true) => "정확",
            (true, false) => "통과",
            _ => "불일치",
        };
        eprintln!("[{status}] {name}");
        if !pass || !exact {
            let mut load_codes = Multiset::new();
            for d in &loaded.diagnostics {
                *load_codes.entry(d.code).or_insert(0) += 1;
            }
            eprintln!("    로더 진단: {}", fmt_set(&load_codes));
            eprintln!("    기대: {}", fmt_set(&exp.codes));
            eprintln!("    실제: {}", fmt_set(&actual));
            if !missing.is_empty() {
                eprintln!("    부족: {}", fmt_set(&missing));
            }
            if !extra.is_empty() {
                eprintln!("    초과: {}", fmt_set(&extra));
            }
        }
        if !pass {
            mismatches.push(format!(
                "{name}: 기대 [{}] 실제 [{}] 부족 [{}]",
                fmt_set(&exp.codes),
                fmt_set(&actual),
                fmt_set(&missing)
            ));
        }
    }

    eprintln!("\n===== 요약 =====");
    for (folder, t) in &by_folder {
        eprintln!("{folder:<10} {}", t.line());
    }
    eprintln!("전체       {}", overall.line());
    eprintln!("Basic 제외 {}", without_basic.line());
    for s in &skipped {
        eprintln!("건너뜀: {s}");
    }
    eprintln!("\n불일치 파일 {}개:", mismatches.len());
    for m in &mismatches {
        eprintln!("  {m}");
    }

    assert!(
        clean_violations.is_empty(),
        "Clean 파일은 진단이 하나도 없어야 한다: {clean_violations:?}"
    );
    let pass_rate = Tally::pct(overall.pass, overall.total);
    let exact_rate = Tally::pct(overall.exact, overall.total);
    assert!(
        pass_rate >= MIN_PASS_PERCENT,
        "코퍼스 통과율 {pass_rate:.1}%가 하한 {MIN_PASS_PERCENT}% 아래다"
    );
    assert!(
        exact_rate >= MIN_EXACT_PERCENT,
        "코퍼스 정확 일치율 {exact_rate:.1}%가 하한 {MIN_EXACT_PERCENT}% 아래다"
    );
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn parses_howto_line() {
        let e = parse_expectation("// \tFailed Checks:  0203, 403,403 [406] (solely) //\n");
        let Some(e) = e else {
            unreachable!("줄을 찾아야 한다");
        };
        assert_eq!(e.mode, Mode::Solely);
        assert_eq!(e.codes.get(&203), Some(&1));
        assert_eq!(e.codes.get(&403), Some(&2));
        assert_eq!(e.codes.get(&406), Some(&1));
    }

    #[test]
    fn ignore_and_empty() {
        let e = parse_expectation("//   Failed Checks: 0403 (ignore)  //");
        assert!(matches!(
            e,
            Some(Expectation {
                mode: Mode::Ignore,
                ..
            })
        ));
        let e = parse_expectation("//   Failed Checks:   //");
        assert!(matches!(e, Some(ref x) if x.codes.is_empty() && x.mode == Mode::Normal));
        assert!(parse_expectation("// nothing").is_none());
    }

    #[test]
    fn multiset_difference() {
        let mut a = Multiset::new();
        a.insert(403, 4);
        let mut b = Multiset::new();
        b.insert(403, 1);
        assert_eq!(minus(&a, &b).get(&403), Some(&3));
        assert!(minus(&b, &a).is_empty());
    }
}
