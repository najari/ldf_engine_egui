//! 하위 명령이 함께 쓰는 아주 작은 인자 해석기: 위치 인자, `--옵션 값`, `-o 값`, `--플래그`.

use ldf_i18n::tf;
use std::collections::{HashMap, HashSet};

/// 해석된 인자.
#[derive(Default)]
pub struct Args {
    /// 옵션이 아닌 인자(순서대로).
    pub positional: Vec<String>,
    values: HashMap<String, String>,
    flags: HashSet<String>,
}

impl Args {
    /// `valued`는 값을 받는 옵션, `flags`는 값 없는 옵션이다(모두 앞의 `-`를 포함해 적는다).
    /// 모르는 옵션이나 값이 빠진 옵션은 오류 문장으로 돌려준다.
    pub fn parse(args: &[String], valued: &[&str], flags: &[&str]) -> Result<Args, String> {
        let mut parsed = Args::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if valued.contains(&a.as_str()) {
                let v = it.next().ok_or_else(|| tf!("{a} 옵션에 값이 필요하다"))?;
                parsed.values.insert(a.clone(), v.clone());
            } else if flags.contains(&a.as_str()) {
                parsed.flags.insert(a.clone());
            } else if a.starts_with('-') && a.len() > 1 {
                return Err(tf!("알 수 없는 옵션 `{a}`"));
            } else {
                parsed.positional.push(a.clone());
            }
        }
        Ok(parsed)
    }

    /// 옵션 값.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// 플래그가 있었는지.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn positional_values_and_flags() {
        let a = Args::parse(
            &s(&["a", "-o", "out", "b", "--force"]),
            &["-o"],
            &["--force"],
        )
        .expect("parse");
        assert_eq!(a.positional, vec!["a", "b"]);
        assert_eq!(a.value("-o"), Some("out"));
        assert!(a.flag("--force") && !a.flag("--other"));
    }

    #[test]
    fn unknown_options_and_missing_values_are_errors() {
        assert!(Args::parse(&s(&["--nope"]), &[], &[]).is_err());
        assert!(Args::parse(&s(&["-o"]), &["-o"], &[]).is_err());
        // 단독 `-`는 위치 인자다
        assert_eq!(
            Args::parse(&s(&["-"]), &[], &[]).expect("parse").positional,
            vec!["-"]
        );
    }
}
