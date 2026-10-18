//! 엔진 오류.

use ldf_store::StoreError;
use std::path::PathBuf;
use thiserror::Error;

/// 엔진 결과.
pub type Result<T, E = EngineError> = std::result::Result<T, E>;

/// 엔진 오류. LDF 내용의 문제는 오류가 아니라 진단(`Diagnostic`)이다.
#[derive(Debug, Error)]
pub enum EngineError {
    /// 파일을 읽지 못했다.
    #[error("파일을 읽을 수 없다: {path}: {source}")]
    Io {
        /// 파일 경로.
        path: PathBuf,
        /// 원인.
        #[source]
        source: std::io::Error,
    },
    /// 저장소 오류.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// 작업자 스레드가 이미 끝났다.
    #[error("엔진 작업자가 종료되었다")]
    WorkerGone,
}
