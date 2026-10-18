//! 원자적 파일 쓰기(FR-EXP-05): 같은 폴더의 임시 파일에 쓰고 rename한다.
//! 쓰는 도중 종료되어도 기존 파일은 손상되지 않는다.

use std::io::Write;
use std::path::Path;

/// `bytes`를 `path`에 원자적으로 쓴다. 실패하면 임시 파일을 지우고 기존 파일은 그대로 둔다.
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "out".to_owned(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));

    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ldfwriter-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dir");
        d
    }

    #[test]
    fn writes_and_replaces_without_leaving_temp_files() {
        let d = dir("ok");
        let p = d.join("a.ldf");
        write_file_atomic(&p, b"first").expect("write");
        write_file_atomic(&p, b"second").expect("overwrite");
        assert_eq!(std::fs::read(&p).expect("read"), b"second");
        let leftovers: Vec<_> = std::fs::read_dir(&d)
            .expect("dir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_keeps_the_existing_file() {
        let d = dir("fail");
        let p = d.join("keep.ldf");
        std::fs::write(&p, b"original").expect("seed");
        // 대상 위치에 같은 이름의 폴더가 있으면 rename이 실패한다
        let blocked = d.join("blocked.ldf");
        std::fs::create_dir_all(blocked.join("inner")).expect("blocker");
        assert!(write_file_atomic(&blocked, b"x").is_err());
        assert_eq!(std::fs::read(&p).expect("read"), b"original");
        let tmp_left = std::fs::read_dir(&d)
            .expect("dir")
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".tmp"));
        assert!(!tmp_left, "임시 파일이 남았다");
        let _ = std::fs::remove_dir_all(&d);
    }
}
