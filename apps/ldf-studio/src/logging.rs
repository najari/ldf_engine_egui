//! 로그(NFR-08): `%LOCALAPPDATA%\LDFStudio\logs`에 하루 단위 파일로 남기고 7일치만 보관한다.
//!
//! 고객 LDF는 기밀이므로(NFR-05) 로그에는 **내용을 남기지 않는다**: 시그널·프레임·노드 이름, 파일 이름, 경로 대신
//! 개수·크기·시간·오류의 종류만 적는다. 엔진이 `tracing`으로 남기는 줄도 같은 규칙을 따른다.

use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

/// 보관할 로그 파일 수(하루 하나).
pub const KEEP_DAYS: usize = 7;

/// 기본 로그 폴더. 환경 변수 `LDF_STUDIO_LOG_DIR`로 바꿀 수 있다(시험·시연용).
pub fn default_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("LDF_STUDIO_LOG_DIR") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join("LDFStudio").join("logs")
}

/// `dir`에 로그를 쓰기 시작한다. 돌려준 가드를 앱이 끝날 때까지 들고 있어야 마지막 줄까지 기록된다.
/// 폴더를 만들 수 없는 등 실패하면 로그 없이 계속한다(로그 때문에 앱이 못 뜨면 안 된다).
pub fn init_in(dir: &Path) -> Option<WorkerGuard> {
    std::fs::create_dir_all(dir).ok()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("ldfstudio")
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).ok()?;
    Some(guard)
}

/// 기본 폴더에 로그를 시작한다.
pub fn init() -> Option<WorkerGuard> {
    init_in(&default_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ldflog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn old_log_files_beyond_seven_days_are_removed_when_logging_starts() {
        let d = dir("prune");
        std::fs::create_dir_all(&d).expect("dir");
        // 이미 쌓여 있던 날짜별 로그 10개
        for day in 1..=10 {
            std::fs::write(d.join(format!("ldfstudio.2020-01-{day:02}.log")), "x").expect("write");
        }
        let appender = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix("ldfstudio")
            .filename_suffix("log")
            .max_log_files(KEEP_DAYS)
            .build(&d)
            .expect("appender");
        drop(appender);
        let left = std::fs::read_dir(&d).expect("read").count();
        assert!(left <= KEEP_DAYS, "{left}개가 남았다");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unwritable_folder_does_not_stop_the_app() {
        // 폴더 자리에 같은 이름의 파일이 있으면 만들 수 없다
        let f = std::env::temp_dir().join(format!("ldflog-file-{}", std::process::id()));
        std::fs::write(&f, "x").expect("write");
        assert!(init_in(&f.join("sub")).is_none());
        let _ = std::fs::remove_file(&f);
    }

    #[test]
    fn the_default_folder_follows_localappdata() {
        let p = default_dir();
        assert!(
            p.ends_with("LDFStudio\\logs")
                || p.ends_with("LDFStudio/logs")
                || std::env::var_os("LDF_STUDIO_LOG_DIR").is_some()
        );
    }
}
