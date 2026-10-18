//! 워커 스레드와 `Command`/`Event` 채널. UI는 여기만 호출하고, 결과는 새 스냅샷을 담은 이벤트로 받는다.

use crate::error::{EngineError, Result};
use crate::export::{ExportOptions, ExportReport};
use crate::session::{ModelSnapshot, Session};
use crossbeam_channel::{Receiver, Sender, unbounded};
use ldf_model::EditOp;
use ldf_store::RevisionInfo;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// UI가 워커에 보내는 명령.
#[derive(Debug)]
pub enum Command {
    /// 프로젝트 파일을 연다(없으면 만든다).
    OpenProject(PathBuf),
    /// 디스크의 `.ldf`를 임포트한다.
    Import(PathBuf),
    /// 메모리의 바이트를 임포트한다(드래그 앤 드롭, 테스트).
    ImportBytes {
        /// 파일 이름.
        name: String,
        /// 파일 내용.
        bytes: Vec<u8>,
    },
    /// 편집한 텍스트를 새 리비전으로 저장하고 다시 검사한다(Checker 창의 Run Checks).
    ApplyText {
        /// 파일 이름(리비전 라벨).
        name: String,
        /// 편집한 전체 텍스트.
        text: String,
        /// 원본 인코딩 이름.
        encoding: String,
        /// 부모 리비전.
        parent: Option<i64>,
    },
    /// 리비전을 연다.
    OpenRevision(i64),
    /// 리비전 목록을 다시 보낸다.
    RefreshRevisions,
    /// 편집 연산을 적용한다(FR-EDT). 대화상자 OK 한 번은 `EditOp::Batch` 하나다.
    Edit(EditOp),
    /// 한 단계 되돌린다.
    Undo,
    /// 되돌린 것을 다시 적용한다.
    Redo,
    /// 편집한 내용을 새 리비전으로 저장한다.
    SaveEdits,
    /// 리비전을 LDF 파일로 내보낸다(FR-EXP).
    Export {
        /// 내보낼 리비전.
        revision: i64,
        /// 대상 파일.
        path: PathBuf,
        /// 옵션.
        options: ExportOptions,
    },
}

/// 워커가 UI에 돌려주는 이벤트.
#[derive(Debug)]
pub enum Event {
    /// 프로젝트가 열렸다.
    ProjectOpened {
        /// 파일 경로(임시 프로젝트면 `None`).
        path: Option<PathBuf>,
        /// 리비전 목록.
        revisions: Vec<RevisionInfo>,
    },
    /// 새 스냅샷이 준비되었다.
    Loaded {
        /// 스냅샷.
        snapshot: Arc<ModelSnapshot>,
        /// 갱신된 리비전 목록.
        revisions: Vec<RevisionInfo>,
        /// 임포트한 내용이 이미 있어 새 리비전을 만들지 않고 기존 것을 열었는지(OpenRevision은 항상 alse).
        reused: bool,
    },
    /// 편집(또는 Undo/Redo)으로 새 스냅샷이 준비되었다. 열린 리비전은 그대로라 화면 상태(선택 등)를 유지해도 된다.
    Edited(Arc<ModelSnapshot>),
    /// 편집 내용이 새 리비전으로 저장되었다.
    Saved {
        /// 저장한 리비전의 스냅샷.
        snapshot: Arc<ModelSnapshot>,
        /// 갱신된 리비전 목록.
        revisions: Vec<RevisionInfo>,
    },
    /// 리비전 목록만 갱신되었다.
    Revisions(Vec<RevisionInfo>),
    /// 내보내기를 마쳤다.
    Exported(ExportReport),
    /// 오류 진단이 남아 있어 내보내기를 멈췄다. 확인을 받으면 llow_errors로 다시 보낸다(FR-EXP-04).
    ExportBlocked {
        /// 내보내려던 리비전.
        revision: i64,
        /// 대상 파일.
        path: PathBuf,
        /// 옵션.
        options: ExportOptions,
        /// 남은 오류 개수.
        errors: usize,
    },
    /// 명령이 실패했다. 상태는 바뀌지 않았다.
    Failed(String),
    /// 워커가 패닉했다(버그). 다시 시작이 필요하다.
    Fatal(String),
}

/// 워커 스레드 핸들.
pub struct Engine {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Engine {
    /// 워커를 시작한다. `project`가 `None`이면 저장하지 않는 임시 프로젝트다.
    /// `repaint`는 이벤트를 보낸 직후 호출되어 UI를 깨운다(`egui::Context::request_repaint`).
    pub fn spawn(project: Option<PathBuf>, repaint: impl Fn() + Send + 'static) -> Engine {
        let (tx, cmd_rx) = unbounded::<Command>();
        let (ev_tx, rx) = unbounded::<Event>();

        let handle = std::thread::Builder::new()
            .name("ldf-engine".into())
            .spawn(move || run(project, &cmd_rx, &ev_tx, &repaint))
            .ok();
        Engine { tx, rx, handle }
    }

    /// 명령을 보낸다. 워커가 이미 끝났으면 오류.
    pub fn send(&self, cmd: Command) -> Result<()> {
        self.tx.send(cmd).map_err(|_| EngineError::WorkerGone)
    }

    /// 쌓인 이벤트를 하나 꺼낸다(UI 프레임마다 비울 때 쓴다).
    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// 이벤트를 기다린다(CLI·테스트용).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Event> {
        self.rx.recv_timeout(timeout).ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // 송신 쪽을 닫으면 워커 루프가 끝난다.
        let (dead, _) = unbounded();
        self.tx = dead;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn open_session(project: Option<&PathBuf>) -> (Session, Option<String>) {
    match project {
        Some(path) => match Session::open(path) {
            Ok(s) => (s, None),
            Err(e) => match Session::open_in_memory() {
                Ok(s) => (
                    s,
                    Some(format!(
                        "프로젝트를 열지 못해 임시 프로젝트로 시작한다: {e}"
                    )),
                ),
                Err(e2) => fatal_fallback(e2),
            },
        },
        None => match Session::open_in_memory() {
            Ok(s) => (s, None),
            Err(e) => fatal_fallback(e),
        },
    }
}

/// 메모리 DB조차 못 여는 경우는 SQLite 번들이 깨진 것이므로 계속할 수 없다.
fn fatal_fallback(e: EngineError) -> ! {
    // 워커 스레드 안에서만 호출된다. 패닉은 `run`의 catch_unwind가 Fatal 이벤트로 바꾼다.
    #[allow(clippy::panic)]
    {
        panic!("임시 프로젝트를 만들 수 없다: {e}")
    }
}

fn run(
    project: Option<PathBuf>,
    cmd_rx: &Receiver<Command>,
    ev_tx: &Sender<Event>,
    repaint: &(impl Fn() + Send),
) {
    let send = |e: Event| {
        let _ = ev_tx.send(e);
        repaint();
    };

    let started = catch_unwind(AssertUnwindSafe(|| open_session(project.as_ref())));
    let (mut session, warning) = match started {
        Ok(v) => v,
        Err(p) => {
            send(Event::Fatal(panic_text(&p)));
            return;
        }
    };
    if let Some(w) = warning {
        send(Event::Failed(w));
    }
    send(Event::ProjectOpened {
        path: session.path().map(ToOwned::to_owned),
        revisions: session.revisions().unwrap_or_default(),
    });

    let mut current: Option<Arc<ModelSnapshot>> = None;
    while let Ok(cmd) = cmd_rx.recv() {
        let result = catch_unwind(AssertUnwindSafe(|| handle(&mut session, &current, cmd)));
        match result {
            Ok(Ok(events)) => {
                for e in &events {
                    match e {
                        Event::Loaded { snapshot, .. }
                        | Event::Saved { snapshot, .. }
                        | Event::Edited(snapshot) => current = Some(Arc::clone(snapshot)),
                        Event::ProjectOpened { .. } => current = None,
                        _ => {}
                    }
                }
                events.into_iter().for_each(&send);
            }
            Ok(Err(e)) => send(Event::Failed(e.to_string())),
            Err(p) => {
                send(Event::Fatal(panic_text(&p)));
                return;
            }
        }
    }
}

fn handle(
    session: &mut Session,
    current: &Option<Arc<ModelSnapshot>>,
    cmd: Command,
) -> Result<Vec<Event>> {
    match cmd {
        Command::OpenProject(path) => {
            *session = Session::open(&path)?;
            Ok(vec![Event::ProjectOpened {
                path: session.path().map(ToOwned::to_owned),
                revisions: session.revisions()?,
            }])
        }
        Command::Import(path) => {
            let out = session.import_file(&path)?;
            loaded(session, out)
        }
        Command::ImportBytes { name, bytes } => {
            let out = session.import_bytes(&name, &bytes)?;
            loaded(session, out)
        }
        Command::ApplyText {
            name,
            text,
            encoding,
            parent,
        } => {
            let out = session.apply_text(&name, &text, &encoding, parent)?;
            loaded(session, out)
        }
        Command::OpenRevision(id) => {
            let snapshot = session.open_revision(id)?;
            Ok(vec![Event::Loaded {
                snapshot,
                revisions: session.revisions()?,
                reused: false,
            }])
        }
        Command::RefreshRevisions => Ok(vec![Event::Revisions(session.revisions()?)]),
        Command::Edit(op) => {
            let snap = open_snapshot(current)?;
            Ok(vec![Event::Edited(session.edit(snap, &op)?)])
        }
        Command::Undo => {
            let snap = open_snapshot(current)?;
            Ok(session.undo(snap).map(Event::Edited).into_iter().collect())
        }
        Command::Redo => {
            let snap = open_snapshot(current)?;
            Ok(session.redo(snap).map(Event::Edited).into_iter().collect())
        }
        Command::SaveEdits => {
            let snap = open_snapshot(current)?;
            let out = session.save_edits(snap)?;
            Ok(vec![Event::Saved {
                snapshot: out.snapshot,
                revisions: session.revisions()?,
            }])
        }
        Command::Export {
            revision,
            path,
            options,
        } => match session.export(revision, &path, &options) {
            Ok(report) => Ok(vec![Event::Exported(report)]),
            Err(EngineError::ExportBlocked { errors }) => Ok(vec![Event::ExportBlocked {
                revision,
                path,
                options,
                errors,
            }]),
            Err(e) => Err(e),
        },
    }
}

fn open_snapshot(current: &Option<Arc<ModelSnapshot>>) -> Result<&ModelSnapshot> {
    current.as_deref().ok_or(EngineError::NothingOpen)
}

fn loaded(session: &Session, out: crate::session::ImportOutcome) -> Result<Vec<Event>> {
    Ok(vec![Event::Loaded {
        snapshot: out.snapshot,
        revisions: session.revisions()?,
        reused: out.reused,
    }])
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "알 수 없는 패닉".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const LDF: &str = "LIN_description_file;\nNodes { Master: M, 5 ms, 0.1 ms; Slaves: A; }\n\
                       Signals { S: 8, 0, A, M; }\nFrames { F: 1, A, 1 { S, 0; } }\n";

    fn wait(engine: &Engine) -> Event {
        engine
            .recv_timeout(Duration::from_secs(10))
            .expect("이벤트가 왔어야 한다")
    }

    #[test]
    fn worker_imports_and_wakes_the_ui() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&wakes);
        let engine = Engine::spawn(None, move || {
            w.fetch_add(1, Ordering::SeqCst);
        });

        assert!(matches!(
            wait(&engine),
            Event::ProjectOpened { path: None, .. }
        ));
        engine
            .send(Command::ImportBytes {
                name: "a.ldf".into(),
                bytes: LDF.as_bytes().to_vec(),
            })
            .expect("send");
        let Event::Loaded {
            snapshot,
            revisions,
            reused,
        } = wait(&engine)
        else {
            panic!("Loaded 이벤트여야 한다")
        };
        assert!(!reused);
        assert_eq!(snapshot.model.frames.len(), 1);
        assert_eq!(revisions.len(), 1);
        assert!(wakes.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn failures_are_reported_and_the_worker_keeps_going() {
        let engine = Engine::spawn(None, || {});
        let _ = wait(&engine);
        engine
            .send(Command::Import(PathBuf::from("Z:/nope/missing.ldf")))
            .expect("send");
        assert!(matches!(wait(&engine), Event::Failed(m) if m.contains("읽을 수 없다")));

        engine.send(Command::OpenRevision(12345)).expect("send");
        assert!(matches!(wait(&engine), Event::Failed(_)));

        engine
            .send(Command::ImportBytes {
                name: "a.ldf".into(),
                bytes: LDF.as_bytes().to_vec(),
            })
            .expect("send");
        assert!(matches!(wait(&engine), Event::Loaded { .. }));
    }

    #[test]
    fn reopening_a_revision_and_refreshing_the_list() {
        let engine = Engine::spawn(None, || {});
        let _ = wait(&engine);
        engine
            .send(Command::ImportBytes {
                name: "a.ldf".into(),
                bytes: LDF.as_bytes().to_vec(),
            })
            .expect("send");
        let Event::Loaded { snapshot, .. } = wait(&engine) else {
            panic!("loaded")
        };
        let id = snapshot.revision.id;

        engine.send(Command::OpenRevision(id)).expect("send");
        let Event::Loaded {
            snapshot: again,
            reused,
            ..
        } = wait(&engine)
        else {
            panic!("loaded")
        };
        assert!(!reused, "리비전을 직접 여는 것은 중복 임포트가 아니다");
        assert_eq!(again.revision.id, id);
        assert!(again.version > snapshot.version);

        engine.send(Command::RefreshRevisions).expect("send");
        assert!(matches!(wait(&engine), Event::Revisions(r) if r.len() == 1));
    }

    #[test]
    fn project_file_persists_across_engines() {
        let dir = std::env::temp_dir().join(format!("ldfengine-test-{}", std::process::id()));
        let path = dir.join("p.ldfdb");
        let _ = std::fs::remove_dir_all(&dir);

        {
            let engine = Engine::spawn(Some(path.clone()), || {});
            let _ = wait(&engine);
            engine
                .send(Command::ImportBytes {
                    name: "a.ldf".into(),
                    bytes: LDF.as_bytes().to_vec(),
                })
                .expect("send");
            assert!(matches!(wait(&engine), Event::Loaded { .. }));
        }
        let engine = Engine::spawn(Some(path), || {});
        let Event::ProjectOpened { path, revisions } = wait(&engine) else {
            panic!("opened")
        };
        assert!(path.is_some());
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].label, "a.ldf");

        drop(engine);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_the_engine_stops_the_worker() {
        let engine = Engine::spawn(None, || {});
        let _ = wait(&engine);
        drop(engine); // join 하므로 멈추지 않고 돌아와야 한다
    }
}
