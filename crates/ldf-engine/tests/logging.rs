//! 로그에는 LDF의 내용(시그널·프레임·노드 이름, 파일 이름, 경로)이 남지 않는다(NFR-05, NFR-08).
//!
//! 이 파일은 프로세스 전체의 로그를 가로채므로 다른 시험과 따로 돌도록 별도 통합 시험 파일이다.

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{Command, EditOp, Engine, Event, SignalDraft};
use ldf_model::InitValue;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

const SECRET_FILE: &str = "CustomerSecretCluster.ldf";
const LDF: &str = "LIN_description_file;\nLIN_protocol_version = \"2.1\";\nLIN_language_version = \"2.1\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: ZxMasterZx, 1 ms, 0 ms; Slaves: ZxSlaveZx; }\nSignals { ZxSignalZx: 8, 0, ZxSlaveZx, ZxMasterZx; }\n\
Frames { ZxFrameZx: 1, ZxSlaveZx, 1 { ZxSignalZx, 0; } }\nSchedule_tables { ZxTableZx { ZxFrameZx delay 10 ms; } }\n";

#[test]
fn logs_record_what_happened_but_never_the_contents() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(capture.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("전역 구독자");

    let engine = Engine::spawn(None, || {});
    let wait = || {
        engine
            .recv_timeout(Duration::from_secs(10))
            .expect("이벤트")
    };
    assert!(matches!(wait(), Event::ProjectOpened { .. }));
    engine
        .send(Command::ImportBytes {
            name: SECRET_FILE.into(),
            bytes: LDF.as_bytes().to_vec(),
        })
        .expect("send");
    assert!(matches!(wait(), Event::Loaded { .. }));
    engine
        .send(Command::Edit(EditOp::AddSignal(SignalDraft {
            name: "ZxNewSignalZx".into(),
            size_bits: 8,
            init: InitValue::Scalar(0),
            publisher: Some("ZxSlaveZx".into()),
            subscribers: vec!["ZxMasterZx".into()],
            encoding: None,
        })))
        .expect("send");
    assert!(matches!(wait(), Event::Edited(_)));
    // 실패하는 명령(없는 시그널 삭제): 오류 메시지에는 이름이 있지만 로그에는 종류만 남는다
    engine
        .send(Command::Edit(EditOp::DeleteSignal("ZxMissingZx".into())))
        .expect("send");
    assert!(matches!(wait(), Event::Failed(m) if m.contains("ZxMissingZx")));
    engine.send(Command::Undo).expect("send");
    assert!(matches!(wait(), Event::Edited(_)));
    drop(engine);

    let log = String::from_utf8(capture.0.lock().expect("lock").clone()).expect("utf8");
    // 무슨 일이 있었는지는 남는다
    assert!(
        log.contains("리비전 저장") && log.contains("bytes="),
        "{log}"
    );
    assert!(log.contains("add_signal") && log.contains("undo"), "{log}");
    assert!(
        log.contains("명령 실패") && log.contains("error=\"edit\""),
        "{log}"
    );
    // 내용은 남지 않는다
    for secret in [SECRET_FILE, "Zx", "CustomerSecret"] {
        assert!(
            !log.contains(secret),
            "로그에 `{secret}`이(가) 들어 있다:\n{log}"
        );
    }
}
