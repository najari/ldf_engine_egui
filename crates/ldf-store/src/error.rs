//! 저장소 오류.

use thiserror::Error;

/// 저장소 결과.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// 저장소 오류.
#[derive(Debug, Error)]
pub enum StoreError {
    /// SQLite 오류(디스크 부족, 잠금 등).
    #[error("SQLite 오류: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// 마이그레이션 실패.
    #[error("스키마 마이그레이션 실패: {0}")]
    Migration(String),
    /// 이 프로그램이 아는 것보다 새로운 스키마의 DB.
    #[error("이 프로그램보다 새로운 버전의 프로젝트 파일이다")]
    SchemaTooNew,
    /// JSON 직렬화 오류.
    #[error("JSON 오류: {0}")]
    Json(#[from] serde_json::Error),
    /// DB 내용이 모델로 복원되지 않는다.
    #[error("프로젝트 파일 내용이 손상되었다: {0}")]
    Corrupt(String),
    /// 없는 리비전·프로젝트.
    #[error("찾을 수 없다: {0}")]
    NotFound(String),
}

impl From<rusqlite_migration::Error> for StoreError {
    fn from(e: rusqlite_migration::Error) -> Self {
        use rusqlite_migration::{Error, MigrationDefinitionError};
        match e {
            Error::MigrationDefinition(MigrationDefinitionError::DatabaseTooFarAhead) => {
                StoreError::SchemaTooNew
            }
            other => StoreError::Migration(other.to_string()),
        }
    }
}

pub(crate) fn corrupt(msg: impl Into<String>) -> StoreError {
    StoreError::Corrupt(msg.into())
}
