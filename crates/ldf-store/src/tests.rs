use super::*;
use ldf_model::{FrameKind, NodeId, Ref};

/// 대부분의 섹션과 깨진 참조를 포함한 샘플.
const SAMPLE: &str = r#"
LIN_description_file;
LIN_protocol_version = "2.1";
LIN_language_version = "2.1";
LIN_speed = 19.2 kbps;
Channel_name = "Body";
Nodes { Master: Gateway, 5 ms, 0.1 ms; Slaves: Door, Lamp; }
Node_composition { configuration Cfg { Cluster: Door, Lamp; } }
Signals {
  DoorState: 2, 0, Door, Gateway;
  LampCmd: 8, 0xFFFFFFFFFFFFFFFF, Gateway, Lamp, Door;
  LampPos: 32, {1, 2, 3, 4}, Lamp, Gateway;
  Fault: 1, 0, Lamp, Gateway;
  Orphan: 4, 0, Ghost, Gateway;
}
Diagnostic_signals { MasterReqB0: 8, 0; SlaveRespB0: 8, 0; }
Frames {
  DoorFrame: 0x10, Door, 1 { DoorState, 0; }
  LampFrame: 0x11, Gateway, 8 { LampCmd, 0; LampPos, 8; }
  LampStatus: 0x12, Lamp, 2 { Fault, 0; Missing, 8; }
}
Sporadic_frames { Sporadic1: LampFrame, DoorFrame; }
Event_triggered_frames { Etf1: Collision, 0x20, DoorFrame, LampStatus; }
Diagnostic_frames { MasterReq: 0x3c { MasterReqB0, 0; } SlaveResp: 0x3d { SlaveRespB0, 0; } }
Node_attributes {
  Lamp {
    LIN_protocol = "2.1"; configured_NAD = 0x20; initial_NAD = 0x20;
    product_id = 0x1a, 0x2b, 3; response_error = Fault; fault_state_signals = Fault;
    P2_min = 50 ms; ST_min = 0 ms; N_As_timeout = 1000 ms; N_Cr_timeout = 1000 ms;
    wakeup_time = 50 ms;
    configurable_frames { LampFrame; LampStatus = 0x1234; }
  }
}
Schedule_tables {
  Normal { LampFrame delay 10 ms; Etf1 delay 20 ms; MasterReq delay 10 ms; }
  Collision { DoorFrame delay 10 ms; LampStatus delay 10 ms; }
  Config { AssignNAD { Lamp } delay 10 ms; FreeFormat { 1, 2, 3, 4, 5, 6, 7, 8 } delay 10 ms; }
}
Signal_encoding_types {
  LampEnc { logical_value, 0, "off"; logical_value, 1, "on"; physical_value, 0, 100, 0.5, 0, "percent"; bcd_value; ascii_value; }
}
Signal_representation { LampEnc: LampCmd, Ghost; }
Vendor_extension { x = 1; }
"#;

/// 필드별로 비교해 어디가 다른지 바로 보이게 한다.
fn assert_same(a: &ldf_model::LdfModel, b: &ldf_model::LdfModel) {
    assert_eq!(a.header, b.header, "header");
    assert_eq!(a.nodes, b.nodes, "nodes");
    assert_eq!(a.node_attrs, b.node_attrs, "node_attrs");
    assert_eq!(a.signals, b.signals, "signals");
    assert_eq!(a.frames, b.frames, "frames");
    assert_eq!(a.encodings, b.encodings, "encodings");
    assert_eq!(a.schedules, b.schedules, "schedules");
    assert_eq!(a.composition, b.composition, "composition");
    assert_eq!(a.dynamic_frames, b.dynamic_frames, "dynamic_frames");
    assert_eq!(a.signal_groups, b.signal_groups, "signal_groups");
    assert_eq!(a.unknown_sections, b.unknown_sections, "unknown_sections");
    assert_eq!(a.section_order, b.section_order, "section_order");
}

fn save(
    store: &mut Store,
    project: i64,
    parent: Option<i64>,
    src: &str,
) -> (i64, ldf_model::Loaded) {
    let loaded = ldf_model::load(src);
    let id = store
        .save_revision(&NewRevision {
            project_id: project,
            parent_id: parent,
            label: "test",
            source_name: Some("sample.ldf"),
            source: src.as_bytes(),
            model: &loaded.model,
            diagnostics: &loaded.diagnostics,
        })
        .expect("save");
    (id, loaded)
}

#[test]
fn save_then_load_restores_an_identical_model() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (id, loaded) = save(&mut store, project, None, SAMPLE);

    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert_eq!(back.diagnostics, loaded.diagnostics);
    assert_eq!(back.source, SAMPLE.as_bytes());
    assert_eq!(back.info.label, "test");
    assert_eq!(back.info.source_name.as_deref(), Some("sample.ldf"));
    store.check_integrity().expect("integrity");
    assert!(
        back.model.composition.is_some(),
        "노드 구성이 모델에 있어야 한다"
    );
}

#[test]
fn composition_survives_in_both_forms() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    for src in [
        "composite { configuration c { P { A, B; } } configuration d { Q { A; } } }",
        "Node_composition { configuration c { P: A, B; } }",
    ] {
        let (id, loaded) = save(&mut store, project, None, src);
        let back = store.load_revision(id).expect("load");
        assert_same(&back.model, &loaded.model);
        assert!(back.model.composition.is_some());
    }
    // 노드 구성이 없는 리비전은 없는 채로 돌아온다
    let (id, _) = save(
        &mut store,
        project,
        None,
        "Nodes { Master: M, 1 ms, 0 ms; }",
    );
    assert!(
        store
            .load_revision(id)
            .expect("load")
            .model
            .composition
            .is_none()
    );
}

#[test]
fn dynamic_frames_survive_with_and_without_ids() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    // ID 목록은 모델로, 그렇지 않은 모양은 원문 보관 섹션으로 돌아온다
    let (id, loaded) = save(&mut store, project, None, "dynamic_frames { 1, 0x2 }");
    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert_eq!(back.model.dynamic_frames.map(|d| d.ids), Some(vec![1, 2]));
    let (id, loaded) = save(&mut store, project, None, "dynamic_frames { Dyn: 0x10; }");
    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert!(back.model.dynamic_frames.is_none());
    assert_eq!(back.model.unknown_sections.len(), 1);
}

#[test]
fn unresolved_refs_and_big_unsigned_values_survive() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (id, _) = save(&mut store, project, None, SAMPLE);
    let m = store.load_revision(id).expect("load").model;

    let orphan = m
        .signals
        .iter()
        .find(|(_, s)| s.name == "Orphan")
        .map(|(_, s)| s)
        .expect("sig");
    assert_eq!(orphan.publisher, Some(Ref::Unresolved("Ghost".into())));
    let cmd = m
        .signals
        .iter()
        .find(|(_, s)| s.name == "LampCmd")
        .map(|(_, s)| s)
        .expect("sig");
    assert_eq!(cmd.init, ldf_model::InitValue::Scalar(u64::MAX));
    let status = m
        .frames
        .iter()
        .find(|(_, f)| f.name == "LampStatus")
        .map(|(_, f)| f)
        .expect("f");
    assert_eq!(
        status.signals()[1].signal,
        Ref::Unresolved("Missing".into())
    );
}

#[test]
fn revisions_are_listed_in_order_with_parent_links() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (a, _) = save(&mut store, project, None, SAMPLE);
    let (b, _) = save(&mut store, project, Some(a), "LIN_description_file;");
    let list = store.list_revisions(project).expect("list");
    assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), [a, b]);
    assert_eq!(list[1].parent_id, Some(a));
    assert_eq!(list[0].protocol_version.as_deref(), Some("2.1"));
    assert_eq!(list[0].speed_bps, Some(19_200));
}

#[test]
fn identical_source_is_found_again() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (a, _) = save(&mut store, project, None, SAMPLE);
    assert_eq!(
        store
            .find_revision_by_source(project, SAMPLE.as_bytes())
            .expect("find"),
        Some(a)
    );
    assert_eq!(
        store
            .find_revision_by_source(project, b"other")
            .expect("find"),
        None
    );
}

#[test]
fn deleting_a_revision_removes_its_rows_and_keeps_the_rest_consistent() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (a, _) = save(&mut store, project, None, SAMPLE);
    let (b, loaded_b) = save(&mut store, project, Some(a), SAMPLE);

    store.delete_revision(a).expect("delete");
    store.check_integrity().expect("integrity");
    assert!(matches!(
        store.load_revision(a),
        Err(StoreError::NotFound(_))
    ));
    let kept = store.load_revision(b).expect("load");
    assert_same(&kept.model, &loaded_b.model);
    assert_eq!(kept.info.parent_id, None);
    assert!(matches!(
        store.delete_revision(a),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn a_failed_save_rolls_back_completely() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");

    // 존재하지 않는 노드를 가리키는 해석된 참조: 외래키 위반으로 저장이 실패해야 한다
    let mut loaded = ldf_model::load(SAMPLE);
    if let Some(s) = loaded.model.signals.get_mut(ldf_model::SignalId(0)) {
        s.publisher = Some(Ref::Resolved(NodeId(999)));
    }
    let result = store.save_revision(&NewRevision {
        project_id: project,
        parent_id: None,
        label: "bad",
        source: SAMPLE.as_bytes(),
        source_name: None,
        model: &loaded.model,
        diagnostics: &[],
    });
    assert!(result.is_err());
    assert!(store.list_revisions(project).expect("list").is_empty());
    store.check_integrity().expect("integrity");
}

#[test]
fn newer_schema_is_refused() {
    let conn = Connection::open_in_memory().expect("conn");
    conn.pragma_update(None, "user_version", 99)
        .expect("pragma");
    assert!(matches!(
        Store::init(conn, false),
        Err(StoreError::SchemaTooNew)
    ));
}

#[test]
fn database_file_reopens_with_its_revisions() {
    let dir = std::env::temp_dir().join(format!("ldfstore-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("p.ldfdb");
    let _ = std::fs::remove_file(&path);

    let (project, id, loaded) = {
        let mut store = Store::open(&path).expect("open");
        let project = store.ensure_project("p").expect("project");
        let (id, loaded) = save(&mut store, project, None, SAMPLE);
        (project, id, loaded)
    };
    let store = Store::open(&path).expect("reopen");
    assert_eq!(store.ensure_project("ignored").expect("project"), project);
    assert_same(&store.load_revision(id).expect("load").model, &loaded.model);
    store.check_integrity().expect("integrity");

    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn frame_kinds_round_trip() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (id, _) = save(&mut store, project, None, SAMPLE);
    let m = store.load_revision(id).expect("load").model;
    let kinds: Vec<_> = m
        .frames
        .iter()
        .map(|(_, f)| match f.kind {
            FrameKind::Unconditional { .. } => "u",
            FrameKind::Sporadic { .. } => "s",
            FrameKind::EventTriggered { .. } => "e",
            FrameKind::Diagnostic { .. } => "d",
        })
        .collect();
    assert_eq!(kinds, ["u", "u", "u", "s", "e", "d", "d"]);
}

#[test]
fn rule_settings_persist_in_the_project_file() {
    let dir = std::env::temp_dir().join(format!("ldfstore-rules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("p.ldfdb");
    {
        let store = Store::open(&path).expect("open");
        let project = store.ensure_project("p").expect("project");
        assert!(store.rule_settings(project).expect("list").is_empty());
        let off = crate::RuleSettingRow {
            code: 406,
            enabled: false,
            severity: None,
        };
        let sev = crate::RuleSettingRow {
            code: 301,
            enabled: true,
            severity: Some("info".into()),
        };
        store.set_rule_setting(project, &off).expect("set");
        store.set_rule_setting(project, &sev).expect("set");
        // 같은 코드를 다시 저장하면 바뀐다
        store
            .set_rule_setting(
                project,
                &crate::RuleSettingRow {
                    code: 406,
                    enabled: true,
                    severity: Some("error".into()),
                },
            )
            .expect("set");
    }
    // 다시 열어도(다른 PC에서 파일을 열어도) 유지된다
    let store = Store::open(&path).expect("reopen");
    let project = store.ensure_project("p").expect("project");
    let rows = store.rule_settings(project).expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].code, 301);
    assert_eq!(
        rows[1],
        crate::RuleSettingRow {
            code: 406,
            enabled: true,
            severity: Some("error".into())
        }
    );
    store.clear_rule_setting(project, 301).expect("clear");
    assert_eq!(store.rule_settings(project).expect("list").len(), 1);
    store.check_integrity().expect("integrity");
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signal_groups_survive_with_and_without_groups() {
    let mut store = Store::open_in_memory().expect("open");
    let project = store.ensure_project("p").expect("project");
    let (id, loaded) = save(
        &mut store,
        project,
        None,
        "Signal_groups { G: 16 { S1, 0; S2, 8; } }",
    );
    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert_eq!(back.model.signal_groups.map(|g| g.groups.len()), Some(1));
    // 비어 있는 섹션과 원문으로 남은 모양
    let (id, loaded) = save(&mut store, project, None, "Signal_groups { }");
    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert_eq!(back.model.signal_groups.map(|g| g.groups.len()), Some(0));
    let (id, loaded) = save(&mut store, project, None, "Signal_groups { G { S, 0; } }");
    let back = store.load_revision(id).expect("load");
    assert_same(&back.model, &loaded.model);
    assert!(back.model.signal_groups.is_none());
    assert_eq!(back.model.unknown_sections.len(), 1);
}
