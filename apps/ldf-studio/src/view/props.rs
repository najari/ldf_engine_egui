//! 속성 창 내용. 읽기 전용(M1)이며, 계산된 값은 회색(`Derived`)으로 구분한다.

use super::Selection;
use super::derive::{self, uint};
use ldf_model::{
    FrameId, FrameKind, LdfModel, NodeId, NodeRole, Ref, ScheduleId, SignalId, protected_id,
};

/// 속성 값의 성격.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    /// LDF에 적힌 값.
    Normal,
    /// 계산한 값(회색, 편집 불가).
    Derived,
    /// 이름을 못 찾은 참조(빨간색).
    Unresolved,
}

/// 속성 한 줄.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    /// 속성 이름.
    pub key: &'static str,
    /// 값.
    pub value: String,
    /// 성격.
    pub kind: PropKind,
}

/// 속성 묶음.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropSection {
    /// 제목.
    pub title: String,
    /// 속성.
    pub props: Vec<Prop>,
}

fn p(key: &'static str, value: impl Into<String>) -> Prop {
    Prop {
        key,
        value: value.into(),
        kind: PropKind::Normal,
    }
}

fn d(key: &'static str, value: impl Into<String>) -> Prop {
    Prop {
        key,
        value: value.into(),
        kind: PropKind::Derived,
    }
}

fn named(key: &'static str, n: &derive::Named) -> Prop {
    Prop {
        key,
        value: n.text.clone(),
        kind: if n.unresolved {
            PropKind::Unresolved
        } else {
            PropKind::Normal
        },
    }
}

fn joined(list: &[derive::Named]) -> derive::Named {
    derive::Named {
        text: list
            .iter()
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        unresolved: list.iter().any(|n| n.unresolved),
    }
}

fn opt_ms(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_owned(), derive::ms)
}

// ---- 스케줄 창 --------------------------------------------------------------------------------

/// 위쪽 속성 창: 스케줄 테이블 또는 슬롯.
pub fn schedule_props(m: &LdfModel, sel: &Selection) -> Option<PropSection> {
    let (sched, slot) = match sel {
        Selection::Schedule(s) => (*s, None),
        Selection::Slot(s, i) => (*s, Some(*i)),
        _ => return None,
    };
    let t = m.schedules.get(sched)?;
    let stats = derive::schedule_stats(m, sched);
    let collision_for: Vec<&str> = m
        .frames
        .iter()
        .filter_map(|(_, f)| match &f.kind {
            FrameKind::EventTriggered {
                collision_table: Some(Ref::Resolved(c)),
                ..
            } if *c == sched => Some(f.name.as_str()),
            _ => None,
        })
        .collect();

    let mut props = vec![
        p("Name", t.name.clone()),
        d("Table Id", sched.0.to_string()),
        d("Total Cycle Time [ms]", derive::ms(stats.total_cycle_ms)),
        d(
            "Bus Load [%]",
            stats
                .bus_load_pct
                .map_or_else(|| "—".to_owned(), |v| format!("{v:.1}")),
        ),
        d("Slots", t.slots.len().to_string()),
        d(
            "Resolves collision of",
            if collision_for.is_empty() {
                "—".to_owned()
            } else {
                collision_for.join(", ")
            },
        ),
    ];
    let mut title = "Schedule Table Properties".to_owned();
    if let Some(info) = slot.and_then(|i| derive::slot_info(m, sched, i)) {
        title = "Schedule Slot Properties".to_owned();
        props = vec![
            named("Slot", &info.name),
            d("Type", info.kind.clone()),
            p("Delay [ms]", derive::ms(info.delay_ms)),
            d("Max. Frame Time [ms]", opt_ms(info.max_ms)),
            d("Min. Frame Time [ms]", opt_ms(info.min_ms)),
            d("Table", t.name.clone()),
        ];
    }
    Some(PropSection { title, props })
}

// ---- 객체 창 ----------------------------------------------------------------------------------

/// 아래쪽 속성 창: 프레임, 시그널, 노드, 클러스터.
pub fn object_props(m: &LdfModel, sel: &Selection, hex: bool) -> Option<PropSection> {
    match sel {
        Selection::Cluster => Some(cluster_props(m)),
        Selection::Frame(f) => frame_props(m, *f, hex),
        Selection::Slot(s, i) => {
            let frame = derive::slot_info(m, *s, *i)?.frame?;
            frame_props(m, frame, hex)
        }
        Selection::Signal(s) => signal_props(m, *s, hex),
        Selection::Node(n) => node_props(m, *n, hex),
        _ => None,
    }
}

fn cluster_props(m: &LdfModel) -> PropSection {
    let h = &m.header;
    let master = m
        .nodes
        .iter()
        .find(|(_, n)| n.role == NodeRole::Master)
        .map(|(_, n)| n);
    let mut props = vec![
        p(
            "Channel",
            h.channel_name.clone().unwrap_or_else(|| "—".to_owned()),
        ),
        p(
            "LIN Protocol",
            h.protocol_version.clone().unwrap_or_else(|| "—".to_owned()),
        ),
        p(
            "LIN Language",
            h.language_version.clone().unwrap_or_else(|| "—".to_owned()),
        ),
        p(
            "LIN Speed",
            h.speed_bps.map_or_else(|| "—".to_owned(), derive::speed),
        ),
        p(
            "Master",
            master.map_or_else(|| "—".to_owned(), |n| n.name.clone()),
        ),
        p("Timebase [ms]", opt_ms(master.and_then(|n| n.timebase_ms))),
        p("Jitter [ms]", opt_ms(master.and_then(|n| n.jitter_ms))),
        d("Nodes", m.nodes.len().to_string()),
        d("Signals", m.signals.len().to_string()),
        d("Frames", m.frames.len().to_string()),
        d("Schedule Tables", m.schedules.len().to_string()),
        d("File Encoding", h.encoding.clone()),
    ];
    if let Some(rev) = &h.ldf_file_revision {
        props.insert(3, p("LDF File Revision", rev.clone()));
    }
    PropSection {
        title: "LIN Cluster Properties".to_owned(),
        props,
    }
}

fn frame_props(m: &LdfModel, id: FrameId, hex: bool) -> Option<PropSection> {
    let f = m.frames.get(id)?;
    let mut props = vec![
        p("Name", f.name.clone()),
        d("Type", derive::frame_type_label(m, id)),
    ];
    if let Some(lin) = f.lin_id() {
        props.push(p("Frame ID", uint(lin, hex)));
        let pid = u8::try_from(lin)
            .ok()
            .filter(|v| *v < 64)
            .map(|v| format!("0x{:02X}", protected_id(v)));
        props.push(d("Protected ID", pid.unwrap_or_else(|| "—".to_owned())));
    }
    props.push(d(
        "Size [Byte]",
        derive::frame_size(m, id).map_or_else(|| "—".to_owned(), |v| v.to_string()),
    ));
    props.push(d("CS Model", derive::cs_model(m, id).label()));
    props.push(named(
        "Publisher",
        &joined(&derive::frame_publishers(m, id)),
    ));
    props.push(named(
        "Subscribers",
        &joined(&derive::frame_subscribers(m, id)),
    ));
    if let FrameKind::EventTriggered {
        collision_table: Some(t),
        ..
    } = &f.kind
    {
        let name = match t {
            Ref::Resolved(s) => derive::Named {
                text: m
                    .schedules
                    .get(*s)
                    .map_or_else(|| "?".to_owned(), |x| x.name.clone()),
                unresolved: false,
            },
            Ref::Unresolved(n) => derive::Named {
                text: n.clone(),
                unresolved: true,
            },
        };
        props.push(named("Collision Resolving Table", &name));
    }
    if matches!(
        f.kind,
        FrameKind::Unconditional { .. } | FrameKind::Diagnostic { .. }
    ) {
        props.push(d("Signals", f.signals().len().to_string()));
    }
    Some(PropSection {
        title: "Frame Properties".to_owned(),
        props,
    })
}

fn signal_props(m: &LdfModel, id: SignalId, hex: bool) -> Option<PropSection> {
    let s = m.signals.get(id)?;
    let subs: Vec<derive::Named> = s
        .subscribers
        .iter()
        .map(|r| derive::node_ref(m, r))
        .collect();
    let publisher = s.publisher.as_ref().map_or_else(
        || derive::Named {
            text: "—".to_owned(),
            unresolved: false,
        },
        |r| derive::node_ref(m, r),
    );
    let encoding = s.encoding().map_or_else(
        || derive::Named {
            text: "—".to_owned(),
            unresolved: false,
        },
        |r| derive::encoding_ref(m, r),
    );
    let kind = match s.kind {
        ldf_model::SignalKind::Normal => "Scalar / Byte array",
        ldf_model::SignalKind::Diagnostic => "Diagnostic",
    };
    Some(PropSection {
        title: "Signal Properties".to_owned(),
        props: vec![
            p("Name", s.name.clone()),
            d("Kind", kind),
            p("Length [Bit]", s.size_bits.to_string()),
            p("Init Value", derive::init_text(&s.init, hex)),
            named("Publisher", &publisher),
            named("Subscribers", &joined(&subs)),
            named("Encoding", &encoding),
        ],
    })
}

fn node_props(m: &LdfModel, id: NodeId, hex: bool) -> Option<PropSection> {
    let n = m.nodes.get(id)?;
    let role = match n.role {
        NodeRole::Master => "Master",
        NodeRole::Slave => "Slave",
    };
    let mut props = vec![p("Name", n.name.clone()), d("Role", role)];
    if n.role == NodeRole::Master {
        props.push(p("Timebase [ms]", opt_ms(n.timebase_ms)));
        props.push(p("Jitter [ms]", opt_ms(n.jitter_ms)));
    }
    if let Some(a) = m.node_attrs.iter().find(|a| a.node.id() == Some(id)) {
        let dash = || "—".to_owned();
        props.push(p(
            "LIN Protocol",
            a.lin_protocol.clone().unwrap_or_else(dash),
        ));
        props.push(p(
            "Configured NAD",
            a.configured_nad.map_or_else(dash, |v| uint(v, hex)),
        ));
        props.push(p(
            "Initial NAD",
            a.initial_nad.map_or_else(dash, |v| uint(v, hex)),
        ));
        props.push(p(
            "Product ID",
            a.product_id.as_ref().map_or_else(dash, |x| {
                let variant = x
                    .variant
                    .map_or_else(String::new, |v| format!(", {}", uint(v, hex)));
                format!(
                    "{}, {}{variant}",
                    uint(x.supplier, hex),
                    uint(x.function, hex)
                )
            }),
        ));
        props.push(p("P2min [ms]", opt_ms(a.p2_min_ms)));
        props.push(p("STmin [ms]", opt_ms(a.st_min_ms)));
        props.push(p("N_As Timeout [ms]", opt_ms(a.n_as_timeout_ms)));
        props.push(p("N_Cr Timeout [ms]", opt_ms(a.n_cr_timeout_ms)));
        if let Some(r) = &a.response_error {
            props.push(named("Response Error", &derive::signal_ref(m, r)));
        }
        props.push(d(
            "Configurable Frames",
            a.configurable_frames.len().to_string(),
        ));
    }
    Some(PropSection {
        title: "Node Properties".to_owned(),
        props,
    })
}

/// 슬롯이 가리키는 프레임을 반환한다(속성 창이 슬롯 선택 시 프레임 속성을 보여줄 때 쓴다).
pub fn slot_frame(m: &LdfModel, sched: ScheduleId, index: usize) -> Option<FrameId> {
    derive::slot_info(m, sched, index)?.frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> LdfModel {
        ldf_model::load(
            "LIN_description_file; LIN_protocol_version = \"2.1\"; LIN_speed = 19.2 kbps; Channel_name = \"Body\";\n\
             Nodes { Master: GW, 5 ms, 0.1 ms; Slaves: A; }\n\
             Signals { S1: 4, 0x5, A, GW; Lost: 2, 0, Ghost, GW; }\n\
             Frames { FA: 0x2c, A, 1 { S1, 0; } }\n\
             Event_triggered_frames { Etf: Col, 9, FA; }\n\
             Schedule_tables { Col { FA delay 10 ms; } Main { FA delay 20 ms; Etf delay 20 ms; } }\n\
             Node_attributes { A { LIN_protocol = \"2.1\"; configured_NAD = 0x20; initial_NAD = 0x21;\n\
               product_id = 0x1a, 0x2b; P2_min = 50 ms; } }\n",
        )
        .model
    }

    fn get<'a>(s: &'a PropSection, key: &str) -> &'a Prop {
        s.props
            .iter()
            .find(|p| p.key == key)
            .unwrap_or_else(|| panic!("속성 {key} 없음"))
    }

    #[test]
    fn frame_properties_include_derived_values() {
        let m = model();
        let fa = m.frames.iter().next().map(|(i, _)| i).expect("frame");
        let s = object_props(&m, &Selection::Frame(fa), true).expect("props");
        assert_eq!(s.title, "Frame Properties");
        assert_eq!(get(&s, "Frame ID").value, "0x2C");
        assert_eq!(get(&s, "Protected ID").value, "0xEC");
        assert_eq!(get(&s, "Protected ID").kind, PropKind::Derived);
        assert_eq!(get(&s, "CS Model").value, "Enhanced");
        assert_eq!(get(&s, "Publisher").value, "A");
        assert_eq!(get(&s, "Subscribers").value, "GW");
    }

    #[test]
    fn out_of_range_frame_id_has_no_protected_id() {
        let m = ldf_model::load(
            "Nodes { Master: M, 1 ms, 0 ms; Slaves: A; }\nFrames { F: 99, A, 1 { } }",
        )
        .model;
        let f = m.frames.iter().next().map(|(i, _)| i).expect("frame");
        let s = object_props(&m, &Selection::Frame(f), false).expect("props");
        assert_eq!(get(&s, "Protected ID").value, "—");
    }

    #[test]
    fn event_triggered_frame_shows_its_collision_table() {
        let m = model();
        let etf = m
            .frames
            .iter()
            .find(|(_, f)| f.name == "Etf")
            .map(|(i, _)| i)
            .expect("frame");
        let s = object_props(&m, &Selection::Frame(etf), false).expect("props");
        assert_eq!(get(&s, "Collision Resolving Table").value, "Col");
    }

    #[test]
    fn signal_properties_flag_unresolved_publishers() {
        let m = model();
        let lost = m
            .signals
            .iter()
            .find(|(_, s)| s.name == "Lost")
            .map(|(i, _)| i)
            .expect("sig");
        let s = object_props(&m, &Selection::Signal(lost), false).expect("props");
        assert_eq!(get(&s, "Publisher").value, "Ghost");
        assert_eq!(get(&s, "Publisher").kind, PropKind::Unresolved);
        assert_eq!(get(&s, "Encoding").value, "—");
    }

    #[test]
    fn node_properties_show_attributes_for_slaves_and_timing_for_the_master() {
        let m = model();
        let master = object_props(&m, &Selection::Node(NodeId(0)), false).expect("props");
        assert_eq!(get(&master, "Timebase [ms]").value, "5.00");
        let slave = object_props(&m, &Selection::Node(NodeId(1)), true).expect("props");
        assert_eq!(get(&slave, "Configured NAD").value, "0x20");
        assert_eq!(get(&slave, "Product ID").value, "0x1A, 0x2B");
        assert_eq!(get(&slave, "P2min [ms]").value, "50.00");
    }

    #[test]
    fn schedule_and_slot_properties() {
        let m = model();
        let t = schedule_props(&m, &Selection::Schedule(ScheduleId(0))).expect("props");
        assert_eq!(t.title, "Schedule Table Properties");
        assert_eq!(get(&t, "Name").value, "Col");
        assert_eq!(get(&t, "Resolves collision of").value, "Etf");
        assert_eq!(get(&t, "Total Cycle Time [ms]").value, "10.00");
        let main = schedule_props(&m, &Selection::Schedule(ScheduleId(1))).expect("props");
        assert_eq!(get(&main, "Slots").value, "2");

        let slot = schedule_props(&m, &Selection::Slot(ScheduleId(1), 0)).expect("props");
        assert_eq!(slot.title, "Schedule Slot Properties");
        assert_eq!(get(&slot, "Slot").value, "FA");
        assert_eq!(get(&slot, "Delay [ms]").value, "20.00");
        // 1바이트 프레임: (34 + 20) 비트 / 19200 bps = 2.81 ms
        assert_eq!(get(&slot, "Min. Frame Time [ms]").value, "2.81");
        // 슬롯을 고르면 아래 창에는 그 프레임의 속성이 나온다
        let obj = object_props(&m, &Selection::Slot(ScheduleId(1), 0), false).expect("props");
        assert_eq!(obj.title, "Frame Properties");
    }

    #[test]
    fn cluster_properties_summarize_the_header() {
        let m = model();
        let s = object_props(&m, &Selection::Cluster, false).expect("props");
        assert_eq!(get(&s, "Channel").value, "Body");
        assert_eq!(get(&s, "LIN Speed").value, "19.200 kbps");
        assert_eq!(get(&s, "Master").value, "GW");
        assert_eq!(get(&s, "Frames").value, "2");
    }

    #[test]
    fn unselectable_things_have_no_properties() {
        let m = model();
        assert!(object_props(&m, &Selection::None, false).is_none());
        assert!(object_props(&m, &Selection::Command("AssignNAD"), false).is_none());
        assert!(schedule_props(&m, &Selection::Frame(FrameId(0))).is_none());
        assert!(schedule_props(&m, &Selection::Schedule(ScheduleId(99))).is_none());
    }
}
