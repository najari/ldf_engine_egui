//! 동기 세션: 프로젝트 DB 하나와 그 위의 임포트·열기. 워커 스레드, CLI, 테스트가 함께 쓴다.

use crate::analysis::{analyze, analyze_text};
use crate::decode;
use crate::error::{EngineError, Result};
use ldf_model::{Diagnostic, EditOp, LdfModel, LineIndex};
use ldf_store::{NewRevision, RevisionInfo, Store, StoredRevision};
use ldf_writer::{Eol, WriteOptions, write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 한 시점의 모델. UI는 이것만 읽고(`Arc`로 공유), 편집이 일어나면 워커가 새 스냅샷을 보낸다.
#[derive(Debug)]
pub struct ModelSnapshot {
    /// 스냅샷 번호. 교체될 때마다 커진다(UI 캐시 무효화용).
    pub version: u64,
    /// 리비전 요약.
    pub revision: RevisionInfo,
    /// 모델.
    pub model: LdfModel,
    /// 진단.
    pub diagnostics: Vec<Diagnostic>,
    /// 디코딩한 원문(BOM 제외). 진단의 `Span`이 가리키는 텍스트다.
    pub source: String,
    /// `Span` -> 줄·열 변환용 색인.
    pub line_index: LineIndex,
    /// 마지막으로 저장(또는 연) 이후 편집이 있는지.
    pub dirty: bool,
    /// Undo할 수 있는지.
    pub can_undo: bool,
    /// Redo할 수 있는지.
    pub can_redo: bool,
}

impl ModelSnapshot {
    /// 원문 바이트 오프셋의 1부터 시작하는 (줄, 열).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        self.line_index.line_col(&self.source, offset)
    }
}

/// 임포트 결과.
#[derive(Debug)]
pub struct ImportOutcome {
    /// 열린 스냅샷.
    pub snapshot: Arc<ModelSnapshot>,
    /// 같은 내용이 이미 있어 새 리비전을 만들지 않고 기존 것을 열었는지(FR-IMP-06).
    pub reused: bool,
}

/// 프로젝트 DB 하나에 대한 작업 세션.
pub struct Session {
    store: Store,
    project_id: i64,
    path: Option<PathBuf>,
    version: u64,
    working: Option<Working>,
}

/// 편집 중인 상태. Undo/Redo는 편집 전후의 텍스트를 보관해 처리한다: 되돌린 결과는 그 텍스트를 다시 읽은 것과 같고,
/// 텍스트가 같으면 모델·진단·위치가 모두 같으므로 어떤 연산이든 대칭이 보장된다.
struct Working {
    /// 편집의 바탕이 된 리비전.
    base: i64,
    /// 저장(또는 열기) 시점의 텍스트. 현재 텍스트와 같으면 변경이 없는 것이다.
    saved: String,
    undo: Vec<String>,
    redo: Vec<String>,
    /// 현재 텍스트(스냅샷의 `source`와 같다).
    current: String,
}

/// Undo 단계 수의 상한(PRD FR-EDT-08: 최소 100단계).
const MAX_UNDO: usize = 200;
/// Undo 스택이 쓰는 텍스트 총량의 상한(바이트). 이를 넘으면 오래된 단계부터 버린다.
const MAX_UNDO_BYTES: usize = 128 * 1024 * 1024;

impl Session {
    /// 프로젝트 파일을 열고(없으면 만들고) 첫 프로젝트를 준비한다.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|source| EngineError::Io {
                path: dir.to_owned(),
                source,
            })?;
        }
        let store = Store::open(path)?;
        let project_id = store.ensure_project("LDF 프로젝트")?;
        Ok(Self {
            store,
            project_id,
            path: Some(path.to_owned()),
            version: 0,
            working: None,
        })
    }

    /// 저장하지 않는 임시 세션.
    pub fn open_in_memory() -> Result<Self> {
        let store = Store::open_in_memory()?;
        let project_id = store.ensure_project("LDF 프로젝트")?;
        Ok(Self {
            store,
            project_id,
            path: None,
            version: 0,
            working: None,
        })
    }

    /// 프로젝트 파일 경로(임시 세션이면 `None`).
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// 프로젝트의 리비전 목록(오래된 순).
    pub fn revisions(&self) -> Result<Vec<RevisionInfo>> {
        Ok(self.store.list_revisions(self.project_id)?)
    }

    /// 디스크의 `.ldf`를 임포트한다.
    pub fn import_file(&mut self, path: &Path) -> Result<ImportOutcome> {
        let bytes = std::fs::read(path).map_err(|source| EngineError::Io {
            path: path.to_owned(),
            source,
        })?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.import_bytes(&name, &bytes)
    }

    /// 바이트를 임포트한다: 인코딩 판별 → 파싱 → 검사 → 단일 트랜잭션으로 저장.
    ///
    /// 같은 프로젝트에 원본 바이트가 완전히 같은 리비전이 있으면 새로 만들지 않고 그것을 연다.
    /// 같은 파일 이름의 최근 리비전이 있으면 그 리비전을 부모로 연결한다.
    pub fn import_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<ImportOutcome> {
        self.store_bytes(name, bytes, None)
    }

    /// 편집한 텍스트를 새 리비전으로 저장한다(FR-CHK-05). 원본 인코딩으로 다시 인코딩하고,
    /// `parent`를 부모 리비전으로 연결한다. 같은 바이트가 이미 있으면 그것을 연다.
    pub fn apply_text(
        &mut self,
        name: &str,
        text: &str,
        encoding: &str,
        parent: Option<i64>,
    ) -> Result<ImportOutcome> {
        let bytes = decode::encode(text, encoding);
        self.store_bytes(name, &bytes, Some(parent))
    }

    /// `parent`가 `None`이면 같은 파일 이름의 최근 리비전을, `Some(p)`이면 `p`를 부모로 쓴다.
    fn store_bytes(
        &mut self,
        name: &str,
        bytes: &[u8],
        parent: Option<Option<i64>>,
    ) -> Result<ImportOutcome> {
        if let Some(id) = self.store.find_revision_by_source(self.project_id, bytes)? {
            let snapshot = self.open_revision(id)?;
            return Ok(ImportOutcome {
                snapshot,
                reused: true,
            });
        }

        let analysis = analyze(bytes);
        let parent_id = match parent {
            Some(p) => p,
            None => self
                .store
                .list_revisions(self.project_id)?
                .iter()
                .rev()
                .find(|r| r.source_name.as_deref() == Some(name))
                .map(|r| r.id),
        };

        let id = self.store.save_revision(&NewRevision {
            project_id: self.project_id,
            parent_id,
            label: name,
            source_name: Some(name),
            source: bytes,
            model: &analysis.model,
            diagnostics: &analysis.diagnostics,
        })?;
        let snapshot = self.open_revision(id)?;
        Ok(ImportOutcome {
            snapshot,
            reused: false,
        })
    }
    /// 리비전을 열어 스냅샷을 만든다.
    pub fn open_revision(&mut self, id: i64) -> Result<Arc<ModelSnapshot>> {
        let StoredRevision {
            info,
            model,
            diagnostics,
            source,
        } = self.store.load_revision(id)?;
        let text = decode::decode(&source, &info.encoding);
        self.version += 1;
        self.working = Some(Working {
            base: info.id,
            saved: text.clone(),
            undo: Vec::new(),
            redo: Vec::new(),
            current: text.clone(),
        });
        Ok(Arc::new(ModelSnapshot {
            version: self.version,
            line_index: LineIndex::new(&text),
            source: text,
            revision: info,
            model,
            diagnostics,
            dirty: false,
            can_undo: false,
            can_redo: false,
        }))
    }

    /// 편집 상태가 `snap`의 것인지 확인하고, 아니면 `snap`을 바탕으로 새로 시작한다.
    fn working_for(&mut self, snap: &ModelSnapshot) -> &mut Working {
        let stale = self
            .working
            .as_ref()
            .is_none_or(|w| w.base != snap.revision.id || w.current != snap.source);
        if stale {
            self.working = Some(Working {
                base: snap.revision.id,
                saved: snap.source.clone(),
                undo: Vec::new(),
                redo: Vec::new(),
                current: snap.source.clone(),
            });
        }
        self.working
            .get_or_insert_with(|| unreachable_working(snap))
    }

    /// 텍스트로 스냅샷을 만든다(편집·Undo·Redo가 공유한다).
    fn snapshot_from_text(&mut self, base: &ModelSnapshot, text: String) -> Arc<ModelSnapshot> {
        let encoding = match base.revision.encoding.as_str() {
            decode::CP1252 => decode::CP1252,
            decode::UTF8_BOM => decode::UTF8_BOM,
            _ => decode::UTF8,
        };
        let analysis = analyze_text(text, encoding);
        self.version += 1;
        let (dirty, can_undo, can_redo) =
            self.working.as_ref().map_or((false, false, false), |w| {
                (w.current != w.saved, !w.undo.is_empty(), !w.redo.is_empty())
            });
        Arc::new(ModelSnapshot {
            version: self.version,
            line_index: LineIndex::new(&analysis.text),
            source: analysis.text,
            revision: base.revision.clone(),
            model: analysis.model,
            diagnostics: analysis.diagnostics,
            dirty,
            can_undo,
            can_redo,
        })
    }

    /// 편집 연산을 적용한다. 모델을 고쳐 텍스트로 쓰고 다시 읽어, 위치와 진단이 맞는 새 스냅샷을 만든다.
    /// 실패하면 아무것도 바뀌지 않는다.
    pub fn edit(&mut self, snap: &ModelSnapshot, op: &EditOp) -> Result<Arc<ModelSnapshot>> {
        let mut model = snap.model.clone();
        op.apply(&mut model)?;
        let eol = if snap.source.contains("\r\n") {
            Eol::CrLf
        } else {
            Eol::Lf
        };
        let text = write(
            &model,
            &WriteOptions {
                eol,
                ..WriteOptions::default()
            },
        )
        .text;
        let w = self.working_for(snap);
        w.undo.push(std::mem::replace(&mut w.current, text.clone()));
        w.redo.clear();
        trim_undo(&mut w.undo);
        Ok(self.snapshot_from_text(snap, text))
    }

    /// 한 단계 되돌린다. 되돌릴 것이 없으면 `None`.
    pub fn undo(&mut self, snap: &ModelSnapshot) -> Option<Arc<ModelSnapshot>> {
        let w = self.working_for(snap);
        let prev = w.undo.pop()?;
        w.redo.push(std::mem::replace(&mut w.current, prev.clone()));
        Some(self.snapshot_from_text(snap, prev))
    }

    /// 되돌린 것을 다시 적용한다. 다시 할 것이 없으면 `None`.
    pub fn redo(&mut self, snap: &ModelSnapshot) -> Option<Arc<ModelSnapshot>> {
        let w = self.working_for(snap);
        let next = w.redo.pop()?;
        w.undo.push(std::mem::replace(&mut w.current, next.clone()));
        Some(self.snapshot_from_text(snap, next))
    }

    /// 편집한 내용을 새 리비전으로 저장한다(바탕 리비전이 부모). Undo 기록은 유지한다.
    pub fn save_edits(&mut self, snap: &ModelSnapshot) -> Result<ImportOutcome> {
        let name = snap
            .revision
            .source_name
            .clone()
            .unwrap_or_else(|| snap.revision.label.clone());
        let (undo, redo) = self.working.as_ref().map_or((Vec::new(), Vec::new()), |w| {
            (w.undo.clone(), w.redo.clone())
        });
        let out = self.apply_text(
            &name,
            &snap.source,
            &snap.revision.encoding,
            Some(snap.revision.id),
        )?;
        // `apply_text`가 연 새 리비전에서 이어서 편집할 수 있게 기록을 옮긴다
        self.working = Some(Working {
            base: out.snapshot.revision.id,
            saved: out.snapshot.source.clone(),
            undo,
            redo,
            current: out.snapshot.source.clone(),
        });
        Ok(out)
    }

    /// 저장소 무결성 검사.
    pub fn check_integrity(&self) -> Result<()> {
        Ok(self.store.check_integrity()?)
    }
}

/// `working_for`가 방금 값을 채웠으므로 비어 있을 수 없지만, 패닉하지 않고 새로 만든다.
fn unreachable_working(snap: &ModelSnapshot) -> Working {
    Working {
        base: snap.revision.id,
        saved: snap.source.clone(),
        undo: Vec::new(),
        redo: Vec::new(),
        current: snap.source.clone(),
    }
}

/// 단계 수와 총 크기 상한을 넘으면 오래된 단계부터 버린다.
fn trim_undo(undo: &mut Vec<String>) {
    while undo.len() > MAX_UNDO {
        undo.remove(0);
    }
    let mut bytes: usize = undo.iter().map(String::len).sum();
    // 최소 100단계는 지키되, 그 이상은 크기 상한에 맞춘다
    while bytes > MAX_UNDO_BYTES && undo.len() > 100 {
        bytes -= undo.remove(0).len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LDF: &str = "LIN_description_file;\n\
        LIN_protocol_version = \"2.1\";\n\
        LIN_language_version = \"2.1\";\n\
        LIN_speed = 19.2 kbps;\n\
        // 주석\n\
        Nodes { Master: M, 5 ms, 0.1 ms; Slaves: A; }\n\
        Signals { S: 8, 0, A, M; }\n\
        Frames { F: 1, A, 1 { S, 0; } }\n";

    #[test]
    fn import_creates_a_revision_with_a_usable_snapshot() {
        let mut s = Session::open_in_memory().expect("session");
        let out = s.import_bytes("a.ldf", LDF.as_bytes()).expect("import");
        assert!(!out.reused);
        let snap = &out.snapshot;
        assert_eq!(snap.revision.label, "a.ldf");
        assert_eq!(snap.model.frames.len(), 1);
        assert_eq!(snap.model.header.encoding, "utf-8");
        assert_eq!(snap.source, LDF);
        // 진단 위치를 줄·열로 바꿀 수 있다
        assert_eq!(snap.line_col(LDF.find("Nodes").unwrap_or(0)), (6, 1));
        assert_eq!(s.revisions().expect("list").len(), 1);
    }

    #[test]
    fn importing_the_same_bytes_again_reuses_the_revision() {
        let mut s = Session::open_in_memory().expect("session");
        let first = s.import_bytes("a.ldf", LDF.as_bytes()).expect("import");
        let again = s
            .import_bytes("renamed.ldf", LDF.as_bytes())
            .expect("import");
        assert!(again.reused);
        assert_eq!(again.snapshot.revision.id, first.snapshot.revision.id);
        assert_eq!(s.revisions().expect("list").len(), 1);
        // 열 때마다 스냅샷 번호가 커진다
        assert!(again.snapshot.version > first.snapshot.version);
    }

    #[test]
    fn a_changed_file_with_the_same_name_becomes_a_child_revision() {
        let mut s = Session::open_in_memory().expect("session");
        let a = s.import_bytes("a.ldf", LDF.as_bytes()).expect("import");
        let changed = LDF.replace("19.2 kbps", "10.4 kbps");
        let b = s.import_bytes("a.ldf", changed.as_bytes()).expect("import");
        assert!(!b.reused);
        assert_eq!(b.snapshot.revision.parent_id, Some(a.snapshot.revision.id));
        assert_eq!(b.snapshot.revision.speed_bps, Some(10_400));
        // 다른 이름이면 부모가 없다
        let c = s
            .import_bytes("other.ldf", changed.replace("5 ms", "6 ms").as_bytes())
            .expect("import");
        assert_eq!(c.snapshot.revision.parent_id, None);
    }

    #[test]
    fn cp1252_files_keep_their_encoding_and_text() {
        let mut bytes = b"LIN_description_file;\n// T".to_vec();
        bytes.extend_from_slice(&[0xFC]); // ü
        bytes.extend_from_slice(b"r\nNodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n");
        let mut s = Session::open_in_memory().expect("session");
        let out = s.import_bytes("de.ldf", &bytes).expect("import");
        assert_eq!(out.snapshot.revision.encoding, "windows-1252");
        assert!(out.snapshot.source.contains("Tür"));
        assert_eq!(out.snapshot.model.nodes.len(), 2);
    }

    #[test]
    fn bom_is_stripped_from_the_snapshot_text_so_spans_match() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(LDF.as_bytes());
        let mut s = Session::open_in_memory().expect("session");
        let snap = s.import_bytes("bom.ldf", &bytes).expect("import").snapshot;
        assert_eq!(snap.revision.encoding, "utf-8-bom");
        assert!(snap.source.starts_with("LIN_description_file"));
        let node = snap
            .model
            .nodes
            .iter()
            .next()
            .and_then(|(_, n)| n.span)
            .expect("span");
        assert!(node.text(&snap.source).starts_with("Master"));
    }

    #[test]
    fn garbage_imports_without_error_and_stays_consistent() {
        let mut s = Session::open_in_memory().expect("session");
        let out = s
            .import_bytes("junk.ldf", b"\x00\xff\xfe @@@ { { Nodes {")
            .expect("import");
        assert!(!out.snapshot.diagnostics.is_empty());
        s.check_integrity().expect("integrity");
    }

    #[test]
    fn applying_edited_text_creates_a_child_revision_in_the_original_encoding() {
        let mut bytes = b"LIN_description_file;\nLIN_speed = 19.2 kbps;\n// T".to_vec();
        bytes.push(0xFC); // ü (CP1252)
        bytes.extend_from_slice(b"r\nNodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n");
        let mut s = Session::open_in_memory().expect("session");
        let first = s.import_bytes("de.ldf", &bytes).expect("import").snapshot;
        assert_eq!(first.revision.encoding, "windows-1252");

        let edited = first.source.replace("19.2 kbps", "10.4 kbps");
        let out = s
            .apply_text(
                "de.ldf",
                &edited,
                &first.revision.encoding,
                Some(first.revision.id),
            )
            .expect("apply");
        assert!(!out.reused);
        let second = &out.snapshot;
        assert_eq!(second.revision.parent_id, Some(first.revision.id));
        assert_eq!(second.revision.speed_bps, Some(10_400));
        assert_eq!(second.revision.encoding, "windows-1252");
        // 한 글자만 바뀌었으므로 ü는 같은 CP1252 바이트로 저장된다
        assert!(second.source.contains("Tür"));

        // 같은 텍스트를 다시 적용하면 새 리비전이 생기지 않는다
        let again = s
            .apply_text("de.ldf", &edited, "windows-1252", Some(second.revision.id))
            .expect("apply");
        assert!(again.reused);
        assert_eq!(s.revisions().expect("list").len(), 2);
    }

    /// 프레임 `n`개짜리 LDF를 만든다(프레임당 시그널 2개).
    fn synthetic_ldf(n: usize) -> String {
        let mut s = String::from(
            "LIN_description_file;\nLIN_protocol_version = \"2.1\";\n\
             LIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
             Nodes { Master: GW, 5 ms, 0.1 ms; Slaves: N1, N2, N3, N4; }\nSignals {\n",
        );
        for i in 0..n * 2 {
            s.push_str(&format!("  Sig_{i}: 8, 0, N{}, GW;\n", (i / 2) % 4 + 1));
        }
        s.push_str("}\nFrames {\n");
        for j in 0..n {
            s.push_str(&format!(
                "  F_{j}: {}, N{}, 2 {{ Sig_{}, 0; Sig_{}, 8; }}\n",
                j % 60,
                j % 4 + 1,
                2 * j,
                2 * j + 1
            ));
        }
        s.push_str("}\nSchedule_tables {\n  T {\n");
        for j in 0..n.min(500) {
            s.push_str(&format!("    F_{j} delay 10 ms;\n"));
        }
        s.push_str("  }\n}\n");
        s
    }

    /// NFR-01: 100 KB LDF 임포트(+ 저장) 1초 이내. 릴리스 빌드 기준이라 평소에는 건너뛴다.
    /// 실행: `cargo test --release -p ldf-engine -- --ignored --nocapture`
    #[test]
    #[ignore = "성능 측정: 릴리스 빌드에서 --ignored 로 실행"]
    fn import_of_a_100kb_file_takes_under_a_second() {
        let ldf = synthetic_ldf(900);
        assert!(ldf.len() > 90_000, "{} bytes", ldf.len());
        let mut s = Session::open_in_memory().expect("session");
        let t = std::time::Instant::now();
        let out = s.import_bytes("big.ldf", ldf.as_bytes()).expect("import");
        let elapsed = t.elapsed();
        eprintln!("{} KB 임포트: {elapsed:?}", ldf.len() / 1024);
        assert_eq!(out.snapshot.model.frames.len(), 900);
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");

        // 5천 프레임(약 550 KB)도 같이 보고한다
        let big = synthetic_ldf(5000);
        let t = std::time::Instant::now();
        s.import_bytes("huge.ldf", big.as_bytes()).expect("import");
        eprintln!("{} KB 임포트: {:?}", big.len() / 1024, t.elapsed());
    }

    #[test]
    fn missing_file_is_an_io_error() {
        let mut s = Session::open_in_memory().expect("session");
        let err = s
            .import_file(Path::new("Z:/definitely/not/here.ldf"))
            .expect_err("error");
        assert!(matches!(err, EngineError::Io { .. }));
    }
}
