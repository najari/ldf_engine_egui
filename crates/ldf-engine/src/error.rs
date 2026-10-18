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
    /// 편집 연산을 적용하지 못했다.
    #[error(transparent)]
    Edit(#[from] ldf_model::EditError),
    /// 오류 진단이 남아 있어 내보내기를 막았다(확인 후 llow_errors로 다시 시도).
    #[error("오류 진단이 {errors}개 남아 있다")]
    ExportBlocked {
        /// 남은 오류 개수.
        errors: usize,
    },
    /// 열린 LDF가 없어 편집할 수 없다.
    #[error("열린 LDF가 없다")]
    NothingOpen,
    /// 작업자 스레드가 이미 끝났다.
    #[error("엔진 작업자가 종료되었다")]
    WorkerGone,
}
