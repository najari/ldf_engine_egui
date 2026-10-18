//! LDF 프로젝트 저장소(SQLite). 리비전 단위 스냅샷으로 `LdfModel`을 손실 없이 저장·복원한다.
//!
//! UI와 엔진은 이 크레이트를 통해서만 SQL을 만난다(`cargo deny`가 강제한다).

mod error;
mod load;
mod save;

pub use error::{Result, StoreError};

use ldf_model::{Diagnostic, LdfModel};
use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};
use std::path::Path;

static MIGRATION_LIST: &[M<'static>] = &[
    M::up(include_str!("../migrations/0001_init.sql")),
    M::up(include_str!("../migrations/0002_lower_notes.sql")),
    M::up(include_str!("../migrations/0003_leading_comment.sql")),
];
static MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_LIST);

/// 새 리비전으로 저장할 내용.
pub struct NewRevision<'a> {
    /// 소속 프로젝트.
    pub project_id: i64,
    /// 부모 리비전(편집·재임포트의 원본).
    pub parent_id: Option<i64>,
    /// 사용자가 붙인 라벨.
    pub label: &'a str,
    /// 출처 파일 이름.
    pub source_name: Option<&'a str>,
    /// 원본 바이트 전체.
    pub source: &'a [u8],
    /// 모델.
    pub model: &'a LdfModel,
    /// 진단(캐시).
    pub diagnostics: &'a [Diagnostic],
}

/// 리비전 목록용 요약.
#[derive(Clone, Debug, PartialEq)]
pub struct RevisionInfo {
    /// 리비전 ID.
    pub id: i64,
    /// 프로젝트 ID.
    pub project_id: i64,
    /// 부모 리비전.
    pub parent_id: Option<i64>,
    /// 라벨.
    pub label: String,
    /// 출처 파일 이름.
    pub source_name: Option<String>,
    /// 생성 시각(UTC, `YYYY-MM-DD HH:MM:SS`).
    pub created_at: String,
    /// `LIN_protocol_version`.
    pub protocol_version: Option<String>,
    /// `LIN_language_version`.
    pub language_version: Option<String>,
    /// 속도(bit/s).
    pub speed_bps: Option<u32>,
    /// 원본 문자 인코딩.
    pub encoding: String,
}

/// DB에서 복원한 리비전.
#[derive(Clone, Debug)]
pub struct StoredRevision {
    /// 요약.
    pub info: RevisionInfo,
    /// 모델.
    pub model: LdfModel,
    /// 저장된 진단.
    pub diagnostics: Vec<Diagnostic>,
    /// 원본 바이트.
    pub source: Vec<u8>,
}

/// 프로젝트 DB 연결. 쓰기는 한 연결에서만 한다(엔진 워커).
pub struct Store {
    conn: Connection,
}

impl Store {
    /// 파일을 열고(없으면 만들고) 최신 스키마로 올린다.
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?, true)
    }

    /// 메모리 DB(테스트용).
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, false)
    }

    fn init(mut conn: Connection, wal: bool) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if wal {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self { conn })
    }

    /// 프로젝트를 만든다.
    pub fn create_project(&self, name: &str) -> Result<i64> {
        self.conn
            .execute("INSERT INTO project (name) VALUES (?1)", params![name])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 첫 번째 프로젝트의 ID. 없으면 `name`으로 만든다.
    pub fn ensure_project(&self, name: &str) -> Result<i64> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM project ORDER BY id LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        match id {
            Some(id) => Ok(id),
            None => self.create_project(name),
        }
    }

    /// 리비전을 저장한다. 단일 트랜잭션이므로 도중에 죽어도 DB는 일관된다.
    pub fn save_revision(&mut self, rev: &NewRevision<'_>) -> Result<i64> {
        save::save(&mut self.conn, rev)
    }

    /// 프로젝트의 리비전 목록(오래된 순).
    pub fn list_revisions(&self, project_id: i64) -> Result<Vec<RevisionInfo>> {
        load::list(&self.conn, project_id)
    }

    /// 리비전을 모델로 복원한다.
    pub fn load_revision(&self, id: i64) -> Result<StoredRevision> {
        load::load(&self.conn, id)
    }

    /// 원본 바이트가 완전히 같은 리비전을 찾는다(가장 최근 것).
    pub fn find_revision_by_source(&self, project_id: i64, source: &[u8]) -> Result<Option<i64>> {
        let hash = save::source_hash(source);
        let mut st = self.conn.prepare_cached(
            "SELECT id, source_text FROM ldf_revision \
             WHERE project_id = ?1 AND source_hash = ?2 ORDER BY id DESC",
        )?;
        let mut rows = st.query(params![project_id, hash])?;
        while let Some(row) = rows.next()? {
            let text: Vec<u8> = row.get(1)?;
            if text == source {
                return Ok(Some(row.get(0)?));
            }
        }
        Ok(None)
    }

    /// 리비전을 지운다. 자식 리비전의 부모 연결은 끊긴다.
    pub fn delete_revision(&self, id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM ldf_revision WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("리비전 {id}")));
        }
        Ok(())
    }

    /// `PRAGMA integrity_check`와 `foreign_key_check`가 모두 통과하는지(FR-IMP-05).
    pub fn check_integrity(&self) -> Result<()> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if result != "ok" {
            return Err(StoreError::Corrupt(format!("integrity_check: {result}")));
        }
        let mut st = self.conn.prepare("PRAGMA foreign_key_check")?;
        let mut rows = st.query([])?;
        if let Some(row) = rows.next()? {
            let table: String = row.get(0)?;
            return Err(StoreError::Corrupt(format!(
                "foreign_key_check 실패: {table}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
