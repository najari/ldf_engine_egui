//! 규칙 설정(FR-CHK-06): 끄면 다음 검사부터 나오지 않고, 프로젝트 파일에 남아 다시 열어도 유지된다.

#![allow(clippy::expect_used, clippy::panic)]

use ldf_engine::{Command, Engine, Event, RuleSetting, Session};
use ldf_model::Severity;
use std::time::Duration;

/// 0406(쓰지 않는 바이트)이 나는 최소 LDF.
const LDF: &str = "LIN_description_file;\nLIN_protocol_version = \"1.3\";\nLIN_language_version = \"1.3\";\nLIN_speed = 19.2 kbps;\n\
Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\nSignals { S: 8, 0, M, A; }\nFrames { F: 1, M, 4 { S, 0; } }\nSchedule_tables { T { F delay 10 ms; } }\n";

fn count(snap: &ldf_engine::ModelSnapshot, code: u16) -> usize {
    snap.diagnostics.iter().filter(|d| d.code == code).count()
}

#[test]
fn a_disabled_rule_stays_quiet_and_survives_reopening_the_project() {
    let dir = std::env::temp_dir().join(format!("ldf-rules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("p.ldfdb");
    {
        let mut s = Session::open(&path).expect("open");
        let first = s
            .import_bytes("a.ldf", LDF.as_bytes())
            .expect("import")
            .snapshot;
        assert_eq!(count(&first, 406), 1);

        s.set_rule_setting(
            406,
            RuleSetting {
                enabled: false,
                severity: None,
            },
        )
        .expect("set");
        // 열려 있는 파일은 다시 검사해야 반영된다
        let again = s.recheck(&first);
        assert_eq!(count(&again, 406), 0);
        // 다른 규칙은 그대로다
        assert_eq!(again.diagnostics.len(), first.diagnostics.len() - 1);
    }
    // 같은 프로젝트 파일을 다시 열면(다른 PC에서 열어도) 설정이 유지된다
    let mut s = Session::open(&path).expect("reopen");
    assert!(!s.rule_settings().get(406).enabled);
    let reopened = s
        .open_revision(s.revisions().expect("list")[0].id)
        .expect("open");
    assert_eq!(
        count(&reopened, 406),
        0,
        "저장된 진단이 아니라 지금 설정으로 다시 낸다"
    );
    // 새로 임포트해도 나오지 않는다
    let second = s
        .import_bytes("b.ldf", format!("{LDF}// b\n").as_bytes())
        .expect("import")
        .snapshot;
    assert_eq!(count(&second, 406), 0);

    // 켜면 다시 나온다
    s.set_rule_setting(406, RuleSetting::default())
        .expect("set");
    assert_eq!(count(&s.recheck(&second), 406), 1);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn severity_overrides_are_applied_and_persisted() {
    let mut s = Session::open_in_memory().expect("session");
    let snap = s
        .import_bytes("a.ldf", LDF.as_bytes())
        .expect("import")
        .snapshot;
    let sev = |snap: &ldf_engine::ModelSnapshot| {
        snap.diagnostics
            .iter()
            .find(|d| d.code == 406)
            .map(|d| d.severity)
    };
    assert_eq!(sev(&snap), Some(Severity::Warning));
    s.set_rule_setting(
        406,
        RuleSetting {
            enabled: true,
            severity: Some(Severity::Error),
        },
    )
    .expect("set");
    assert_eq!(sev(&s.recheck(&snap)), Some(Severity::Error));
}

#[test]
fn the_worker_applies_a_rule_change_to_the_open_file() {
    let engine = Engine::spawn(None, || {});
    let wait = || {
        engine
            .recv_timeout(Duration::from_secs(10))
            .expect("이벤트")
    };
    assert!(matches!(wait(), Event::ProjectOpened { .. }));
    engine
        .send(Command::ImportBytes {
            name: "a.ldf".into(),
            bytes: LDF.as_bytes().to_vec(),
        })
        .expect("send");
    let Event::Loaded { snapshot, .. } = wait() else {
        panic!("loaded")
    };
    assert_eq!(count(&snapshot, 406), 1);

    engine
        .send(Command::SetRule {
            code: 406,
            enabled: false,
            severity: None,
        })
        .expect("send");
    let Event::RuleSettings(rules) = wait() else {
        panic!("설정 이벤트")
    };
    assert!(!rules.get(406).enabled);
    let Event::Edited(rechecked) = wait() else {
        panic!("다시 검사한 스냅샷")
    };
    assert_eq!(count(&rechecked, 406), 0);
    assert_eq!(rechecked.revision.id, snapshot.revision.id);
    assert!(!rechecked.dirty);
}
