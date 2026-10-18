//! 모델에서 화면용 값을 계산한다(egui 무관). 구독자 합집합, 프레임 크기, CS 모델, 슬롯 시간, 비트 배치 등.

use ldf_model::{
    FrameId, FrameKind, InitValue, LdfModel, NodeId, NodeRole, Ref, ScheduleId, SignalId,
    SlotEntry, frame_time_max_ms, frame_time_nominal_ms,
};

/// 이름 하나. 해석되지 않은 참조는 빨갛게 보여주기 위해 표시한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// 표시할 이름.
    pub text: String,
    /// 이름을 못 찾은 참조인지.
    pub unresolved: bool,
}

impl Named {
    fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            unresolved: false,
        }
    }
}

/// 숫자 표시: 16진이면 `0x3C`.
pub fn uint(v: u32, hex: bool) -> String {
    if hex {
        format!("0x{v:02X}")
    } else {
        v.to_string()
    }
}

/// 시간(ms) 표시: 소수 둘째 자리.
pub fn ms(v: f64) -> String {
    format!("{v:.2}")
}

/// 속도 표시: `19.200 kbps`.
pub fn speed(bps: u32) -> String {
    format!("{:.3} kbps", f64::from(bps) / 1000.0)
}

/// 시그널 초기값 표시.
pub fn init_text(v: &InitValue, hex: bool) -> String {
    match v {
        InitValue::Scalar(n) => {
            if hex {
                format!("0x{n:X}")
            } else {
                n.to_string()
            }
        }
        InitValue::Array(bytes) => {
            let items: Vec<String> = bytes
                .iter()
                .map(|b| {
                    if hex {
                        format!("0x{b:02X}")
                    } else {
                        b.to_string()
                    }
                })
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

// ---- 이름 해석 -------------------------------------------------------------------------------

pub fn node_ref(m: &LdfModel, r: &Ref<NodeId>) -> Named {
    match r {
        Ref::Resolved(id) => Named::ok(
            m.nodes
                .get(*id)
                .map_or_else(|| "?".to_owned(), |n| n.name.clone()),
        ),
        Ref::Unresolved(n) => Named {
            text: n.clone(),
            unresolved: true,
        },
    }
}

pub fn signal_ref(m: &LdfModel, r: &Ref<SignalId>) -> Named {
    match r {
        Ref::Resolved(id) => Named::ok(
            m.signals
                .get(*id)
                .map_or_else(|| "?".to_owned(), |s| s.name.clone()),
        ),
        Ref::Unresolved(n) => Named {
            text: n.clone(),
            unresolved: true,
        },
    }
}

pub fn frame_ref(m: &LdfModel, r: &Ref<FrameId>) -> Named {
    match r {
        Ref::Resolved(id) => Named::ok(
            m.frames
                .get(*id)
                .map_or_else(|| "?".to_owned(), |f| f.name.clone()),
        ),
        Ref::Unresolved(n) => Named {
            text: n.clone(),
            unresolved: true,
        },
    }
}

pub fn encoding_ref(m: &LdfModel, r: &Ref<ldf_model::EncodingId>) -> Named {
    match r {
        Ref::Resolved(id) => Named::ok(
            m.encodings
                .get(*id)
                .map_or_else(|| "?".to_owned(), |e| e.name.clone()),
        ),
        Ref::Unresolved(n) => Named {
            text: n.clone(),
            unresolved: true,
        },
    }
}

fn push_unique(out: &mut Vec<Named>, n: Named) {
    if !out.contains(&n) {
        out.push(n);
    }
}

fn master_name(m: &LdfModel) -> Option<Named> {
    m.nodes
        .iter()
        .find(|(_, n)| n.role == NodeRole::Master)
        .map(|(_, n)| Named::ok(n.name.clone()))
}

// ---- 프레임 -----------------------------------------------------------------------------------

/// 프레임의 발행자. 이벤트 트리거는 연관 프레임 발행자의 합집합, 스포라딕은 마스터다.
pub fn frame_publishers(m: &LdfModel, id: FrameId) -> Vec<Named> {
    let Some(f) = m.frames.get(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match &f.kind {
        FrameKind::Unconditional { publisher, .. } => out.push(node_ref(m, publisher)),
        FrameKind::Diagnostic { id: lin, .. } => {
            // MasterReq(0x3C)는 마스터가, SlaveResp(0x3D)는 슬레이브가 보낸다
            if *lin == 0x3c {
                out.extend(master_name(m));
            } else {
                out.extend(
                    m.nodes
                        .iter()
                        .filter(|(_, n)| n.role == NodeRole::Slave)
                        .map(|(_, n)| Named::ok(n.name.clone())),
                );
            }
        }
        FrameKind::Sporadic { .. } => out.extend(master_name(m)),
        FrameKind::EventTriggered { associated, .. } => {
            for a in associated.iter().filter_map(Ref::id) {
                for p in frame_publishers(m, a) {
                    push_unique(&mut out, p);
                }
            }
        }
    }
    out
}

/// 프레임의 구독자: 배치된 시그널 구독자의 합집합(LDF는 프레임 구독자를 따로 정의하지 않는다).
pub fn frame_subscribers(m: &LdfModel, id: FrameId) -> Vec<Named> {
    let Some(f) = m.frames.get(id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match &f.kind {
        FrameKind::Unconditional { signals, .. } | FrameKind::Diagnostic { signals, .. } => {
            if matches!(f.kind, FrameKind::Diagnostic { .. }) && signals.is_empty() {
                // 진단 프레임은 구독자를 시그널로 정의하지 않는다
                if f.lin_id() == Some(0x3d) {
                    out.extend(master_name(m));
                } else {
                    out.extend(
                        m.nodes
                            .iter()
                            .filter(|(_, n)| n.role == NodeRole::Slave)
                            .map(|(_, n)| Named::ok(n.name.clone())),
                    );
                }
            }
            for fs in signals {
                let Some(sig) = fs.signal.id().and_then(|s| m.signals.get(s)) else {
                    continue;
                };
                for sub in &sig.subscribers {
                    push_unique(&mut out, node_ref(m, sub));
                }
            }
        }
        FrameKind::Sporadic { associated } | FrameKind::EventTriggered { associated, .. } => {
            for a in associated.iter().filter_map(Ref::id) {
                for s in frame_subscribers(m, a) {
                    push_unique(&mut out, s);
                }
            }
        }
    }
    out
}

/// 프레임 크기(byte). 이벤트 트리거·스포라딕은 연관 프레임 중 가장 큰 값.
pub fn frame_size(m: &LdfModel, id: FrameId) -> Option<u32> {
    let f = m.frames.get(id)?;
    match &f.kind {
        FrameKind::Unconditional { size, .. } => Some(*size),
        FrameKind::Diagnostic { .. } => Some(8),
        FrameKind::Sporadic { associated } | FrameKind::EventTriggered { associated, .. } => {
            associated
                .iter()
                .filter_map(Ref::id)
                .filter_map(|a| frame_size(m, a))
                .max()
        }
    }
}

/// 체크섬 모델.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsModel {
    /// 클래식(데이터만).
    Classic,
    /// 향상(PID 포함).
    Enhanced,
}

impl CsModel {
    /// 화면 표시.
    pub fn label(self) -> &'static str {
        match self {
            CsModel::Classic => "Classic",
            CsModel::Enhanced => "Enhanced",
        }
    }
}

fn is_classic_version(v: &str) -> bool {
    v.trim() == "1.3"
}

/// 프레임의 체크섬 모델: 진단 프레임은 항상 클래식, 그 외는 발행 노드의 `LIN_protocol`
/// (없으면 클러스터 프로토콜)이 1.3이면 클래식, 아니면 향상.
pub fn cs_model(m: &LdfModel, id: FrameId) -> CsModel {
    let Some(f) = m.frames.get(id) else {
        return CsModel::Enhanced;
    };
    if matches!(f.kind, FrameKind::Diagnostic { .. }) {
        return CsModel::Classic;
    }
    let node_version = match &f.kind {
        FrameKind::Unconditional { publisher, .. } => publisher.id().and_then(|n| {
            m.node_attrs
                .iter()
                .find(|a| a.node.id() == Some(n))
                .and_then(|a| a.lin_protocol.clone())
        }),
        _ => None,
    };
    let version = node_version
        .or_else(|| m.header.protocol_version.clone())
        .unwrap_or_default();
    if is_classic_version(&version) {
        CsModel::Classic
    } else {
        CsModel::Enhanced
    }
}

/// 프레임 종류 표시.
pub fn frame_type_label(m: &LdfModel, id: FrameId) -> &'static str {
    match m.frames.get(id).map(|f| &f.kind) {
        Some(FrameKind::Unconditional { .. }) => "Unconditional",
        Some(FrameKind::Sporadic { .. }) => "Sporadic",
        Some(FrameKind::EventTriggered { .. }) => "Event-Triggered",
        Some(FrameKind::Diagnostic { .. }) => "Diagnostic",
        None => "?",
    }
}

// ---- 스케줄 -----------------------------------------------------------------------------------

/// 슬롯 한 줄에 필요한 값.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotInfo {
    /// 프레임 이름 또는 명령 이름.
    pub name: Named,
    /// 종류 표시(`Unconditional`, `AssignNAD` 등).
    pub kind: String,
    /// 슬롯이 가리키는 프레임.
    pub frame: Option<FrameId>,
    /// delay(ms).
    pub delay_ms: f64,
    /// 최소 프레임 시간(ms) = 공칭.
    pub min_ms: Option<f64>,
    /// 최대 프레임 시간(ms) = 공칭의 1.4배 + 마스터 지터.
    pub max_ms: Option<f64>,
}

fn master_jitter(m: &LdfModel) -> f64 {
    m.nodes
        .iter()
        .find(|(_, n)| n.role == NodeRole::Master)
        .and_then(|(_, n)| n.jitter_ms)
        .unwrap_or(0.0)
}

fn times(m: &LdfModel, size: Option<u32>) -> (Option<f64>, Option<f64>) {
    let (Some(size), Some(bps)) = (size, m.header.speed_bps) else {
        return (None, None);
    };
    (
        frame_time_nominal_ms(size, bps),
        frame_time_max_ms(size, bps).map(|t| t + master_jitter(m)),
    )
}

pub fn slot_info(m: &LdfModel, sched: ScheduleId, index: usize) -> Option<SlotInfo> {
    let slot = m.schedules.get(sched)?.slots.get(index)?;
    Some(match &slot.entry {
        SlotEntry::Frame(r) => {
            let name = frame_ref(m, r);
            let (frame, kind, size) = match r.id() {
                Some(id) => (
                    Some(id),
                    frame_type_label(m, id).to_owned(),
                    frame_size(m, id),
                ),
                None => (None, "?".to_owned(), None),
            };
            let (min_ms, max_ms) = times(m, size);
            SlotInfo {
                name,
                kind,
                frame,
                delay_ms: slot.delay_ms,
                min_ms,
                max_ms,
            }
        }
        other => {
            // 진단 명령은 8바이트 MasterReq 프레임으로 나간다
            let (min_ms, max_ms) = times(m, Some(8));
            SlotInfo {
                name: Named::ok(command_name(other)),
                kind: command_name(other).to_owned(),
                frame: None,
                delay_ms: slot.delay_ms,
                min_ms,
                max_ms,
            }
        }
    })
}

/// 슬롯 명령 이름.
pub fn command_name(e: &SlotEntry) -> &'static str {
    match e {
        SlotEntry::Frame(_) => "Frame",
        SlotEntry::AssignNad { .. } => "AssignNAD",
        SlotEntry::ConditionalChangeNad { .. } => "ConditionalChangeNAD",
        SlotEntry::DataDump { .. } => "DataDump",
        SlotEntry::SaveConfiguration { .. } => "SaveConfiguration",
        SlotEntry::AssignFrameIdRange { .. } => "AssignFrameIdRange",
        SlotEntry::AssignFrameId { .. } => "AssignFrameId",
        SlotEntry::FreeFormat { .. } => "FreeFormat",
        SlotEntry::Unknown(_) => "Unknown",
    }
}

/// 스케줄 테이블 통계.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScheduleStats {
    /// 슬롯 delay 합(ms).
    pub total_cycle_ms: f64,
    /// 버스 점유율(%) = 슬롯 공칭 프레임 시간 합 / 주기. 속도를 모르면 `None`.
    pub bus_load_pct: Option<f64>,
}

pub fn schedule_stats(m: &LdfModel, sched: ScheduleId) -> ScheduleStats {
    let Some(t) = m.schedules.get(sched) else {
        return ScheduleStats {
            total_cycle_ms: 0.0,
            bus_load_pct: None,
        };
    };
    let total: f64 = t.slots.iter().map(|s| s.delay_ms).sum();
    let busy: Option<f64> = (0..t.slots.len())
        .map(|i| slot_info(m, sched, i).and_then(|s| s.min_ms))
        .sum();
    let load = match busy {
        Some(b) if total > 0.0 => Some(b / total * 100.0),
        _ => None,
    };
    ScheduleStats {
        total_cycle_ms: total,
        bus_load_pct: load,
    }
}

// ---- 비트 배치 --------------------------------------------------------------------------------

/// 프레임의 비트 배치(FR-UI-08): 비트마다 점유한 시그널.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BitMap {
    /// 프레임 크기(byte).
    pub bytes: u32,
    /// 비트별 점유 시그널(길이 `bytes * 8`, 인덱스 = 비트 번호).
    pub cells: Vec<Vec<SignalId>>,
    /// 프레임 크기를 넘는 시그널.
    pub overflow: Vec<SignalId>,
}

impl BitMap {
    /// 두 시그널 이상이 겹친 비트가 있는지.
    pub fn has_overlap(&self) -> bool {
        self.cells.iter().any(|c| c.len() > 1)
    }
}

pub fn bit_map(m: &LdfModel, id: FrameId) -> Option<BitMap> {
    let f = m.frames.get(id)?;
    let bytes = f.size_bytes()?;
    let nbits = bytes.checked_mul(8)? as usize;
    let mut map = BitMap {
        bytes,
        cells: vec![Vec::new(); nbits],
        overflow: Vec::new(),
    };
    for fs in f.signals() {
        let Some(sid) = fs.signal.id() else { continue };
        let Some(sig) = m.signals.get(sid) else {
            continue;
        };
        let start = fs.offset as usize;
        // 극단적으로 큰 길이로 메모리·시간을 낭비하지 않도록 프레임 안쪽만 순회한다
        let end = start.saturating_add(sig.size_bits as usize);
        for bit in start..end.min(nbits) {
            if let Some(cell) = map.cells.get_mut(bit) {
                cell.push(sid);
            }
        }
        if end > nbits && !map.overflow.contains(&sid) {
            map.overflow.push(sid);
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> LdfModel {
        ldf_model::load(
            "LIN_description_file;\n\
             LIN_protocol_version = \"2.1\"; LIN_language_version = \"2.1\"; LIN_speed = 19.2 kbps;\n\
             Nodes { Master: GW, 5 ms, 0.1 ms; Slaves: A, B; }\n\
             Signals { S1: 4, 0, A, GW, B; S2: 8, 0, A, GW; S3: 8, 0, B, GW; Big: 16, 0, GW, A; }\n\
             Frames { FA: 1, A, 2 { S1, 0; S2, 8; } FB: 2, B, 1 { S3, 0; } FG: 3, GW, 2 { Big, 0; } \
                      Over: 4, A, 1 { S2, 4; } Clash: 5, A, 2 { S1, 0; S2, 2; } }\n\
             Sporadic_frames { Sp: FA, FB; }\n\
             Event_triggered_frames { Etf: Col, 9, FA, FB; }\n\
             Diagnostic_signals { MasterReqB0: 8, 0; }\n\
             Diagnostic_frames { MasterReq: 0x3c { MasterReqB0, 0; } SlaveResp: 0x3d { } }\n\
             Schedule_tables {\n\
               Col { FA delay 10 ms; FB delay 10 ms; }\n\
               Main { FA delay 20 ms; Etf delay 20 ms; AssignNAD { A } delay 10 ms; Nope delay 5 ms; }\n\
             }\n",
        )
        .model
    }

    fn fid(m: &LdfModel, name: &str) -> FrameId {
        m.frames
            .iter()
            .find(|(_, f)| f.name == name)
            .map(|(i, _)| i)
            .expect(name)
    }

    #[test]
    fn number_formats() {
        assert_eq!(uint(60, false), "60");
        assert_eq!(uint(60, true), "0x3C");
        assert_eq!(uint(5, true), "0x05");
        assert_eq!(ms(6.4583), "6.46");
        assert_eq!(speed(19_200), "19.200 kbps");
        assert_eq!(
            init_text(&InitValue::Array(vec![1, 255]), true),
            "{0x01, 0xFF}"
        );
        assert_eq!(init_text(&InitValue::Scalar(255), true), "0xFF");
        assert_eq!(init_text(&InitValue::Scalar(255), false), "255");
    }

    #[test]
    fn frame_subscribers_are_the_union_of_signal_subscribers() {
        let m = model();
        let subs: Vec<_> = frame_subscribers(&m, fid(&m, "FA"))
            .into_iter()
            .map(|n| n.text)
            .collect();
        assert_eq!(subs, ["GW", "B"]);
        // 이벤트 트리거는 연관 프레임의 합집합
        let etf: Vec<_> = frame_subscribers(&m, fid(&m, "Etf"))
            .into_iter()
            .map(|n| n.text)
            .collect();
        assert_eq!(etf, ["GW", "B"]);
    }

    #[test]
    fn publishers_by_frame_kind() {
        let m = model();
        let names = |n: &str| -> Vec<String> {
            frame_publishers(&m, fid(&m, n))
                .into_iter()
                .map(|p| p.text)
                .collect()
        };
        assert_eq!(names("FA"), ["A"]);
        assert_eq!(names("Sp"), ["GW"]);
        assert_eq!(names("Etf"), ["A", "B"]);
        assert_eq!(names("MasterReq"), ["GW"]);
        assert_eq!(names("SlaveResp"), ["A", "B"]);
    }

    #[test]
    fn frame_sizes() {
        let m = model();
        assert_eq!(frame_size(&m, fid(&m, "FA")), Some(2));
        assert_eq!(frame_size(&m, fid(&m, "MasterReq")), Some(8));
        assert_eq!(frame_size(&m, fid(&m, "Etf")), Some(2));
        assert_eq!(frame_size(&m, fid(&m, "Sp")), Some(2));
    }

    #[test]
    fn checksum_model_follows_protocol_version() {
        let mut m = model();
        assert_eq!(cs_model(&m, fid(&m, "FA")), CsModel::Enhanced);
        assert_eq!(cs_model(&m, fid(&m, "MasterReq")), CsModel::Classic);
        m.header.protocol_version = Some("1.3".into());
        assert_eq!(cs_model(&m, fid(&m, "FA")), CsModel::Classic);
    }

    #[test]
    fn slot_times_match_the_reference_numbers() {
        let m = model();
        let main = ScheduleId(1);
        let fa = slot_info(&m, main, 0).expect("slot");
        assert_eq!(fa.kind, "Unconditional");
        // 2바이트: 공칭 (34 + 30) 비트 = 3.33 ms
        assert!((fa.min_ms.unwrap_or(0.0) - 64.0 * 1000.0 / 19_200.0).abs() < 1e-9);
        // 최대 = 공칭 x 1.4 + 마스터 지터 0.1
        assert!((fa.max_ms.unwrap_or(0.0) - (64.0 * 1000.0 / 19_200.0 * 1.4 + 0.1)).abs() < 1e-9);

        let cmd = slot_info(&m, main, 2).expect("slot");
        assert_eq!(cmd.kind, "AssignNAD");
        assert!((cmd.min_ms.unwrap_or(0.0) - 124.0 * 1000.0 / 19_200.0).abs() < 1e-9);

        let broken = slot_info(&m, main, 3).expect("slot");
        assert!(broken.name.unresolved);
        assert_eq!(broken.min_ms, None);
        assert!(slot_info(&m, main, 99).is_none());
    }

    #[test]
    fn schedule_statistics() {
        let m = model();
        let s = schedule_stats(&m, ScheduleId(0));
        assert_eq!(s.total_cycle_ms, 20.0);
        // FA(2B) + FB(1B) 공칭 시간 / 20 ms
        let busy = 64.0 * 1000.0 / 19_200.0 + 54.0 * 1000.0 / 19_200.0;
        assert!((s.bus_load_pct.unwrap_or(0.0) - busy / 20.0 * 100.0).abs() < 1e-9);
        // 깨진 슬롯이 있으면 점유율을 단정하지 않는다
        assert_eq!(schedule_stats(&m, ScheduleId(1)).bus_load_pct, None);
        assert_eq!(schedule_stats(&m, ScheduleId(1)).total_cycle_ms, 55.0);
    }

    #[test]
    fn bit_map_marks_overlap_and_overflow() {
        let m = model();
        let ok = bit_map(&m, fid(&m, "FA")).expect("map");
        assert_eq!(ok.bytes, 2);
        assert_eq!(ok.cells.len(), 16);
        assert!(!ok.has_overlap());
        assert_eq!(ok.cells[0].len(), 1);
        assert!(ok.cells[4].is_empty());

        // S1(0..4)와 S2(2..10)가 비트 2,3에서 겹친다
        let clash = bit_map(&m, fid(&m, "Clash")).expect("map");
        assert!(clash.has_overlap());
        assert_eq!(clash.cells[2].len(), 2);
        assert_eq!(clash.cells[1].len(), 1);

        // S2(8비트)를 오프셋 4에 두면 1바이트 프레임을 넘는다
        let over = bit_map(&m, fid(&m, "Over")).expect("map");
        assert_eq!(over.overflow.len(), 1);

        // 스포라딕은 비트 배치가 없다
        assert!(bit_map(&m, fid(&m, "Sp")).is_none());
    }

    #[test]
    fn absurd_signal_sizes_do_not_blow_up() {
        let m = ldf_model::load(
            "Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\n\
             Signals { Huge: 4294967295, 0, A, M; }\n\
             Frames { F: 1, A, 1 { Huge, 4294967295; } }",
        )
        .model;
        let id = m.frames.iter().next().map(|(i, _)| i).expect("frame");
        let map = bit_map(&m, id).expect("map");
        assert_eq!(map.cells.len(), 8);
        assert_eq!(map.overflow.len(), 1);
    }
}
