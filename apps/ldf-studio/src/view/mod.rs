//! 화면에 무엇을 보여줄지 정하는 순수 로직(egui 무관, 단위 테스트 대상).
//! UI 패널(`ui/`)은 여기서 만든 값을 그리기만 한다.

pub mod checker;
pub mod context;
pub mod derive;
pub mod edit;
pub mod nav;
pub mod props;

use ldf_model::{FrameId, LdfModel, NodeId, ScheduleId, SignalId};

/// 프레임 그룹(내비게이션 Frames/Commands 탭의 폴더).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrameGroup {
    /// Unconditional Frames.
    Unconditional,
    /// Diagnostic Frames.
    Diagnostic,
    /// Event-Triggered Frames.
    EventTriggered,
    /// Sporadic Frames.
    Sporadic,
}

impl FrameGroup {
    /// 폴더 이름.
    pub fn label(self) -> &'static str {
        match self {
            FrameGroup::Unconditional => "Unconditional Frames",
            FrameGroup::Diagnostic => "Diagnostic Frames",
            FrameGroup::EventTriggered => "Event-Triggered Frames",
            FrameGroup::Sporadic => "Sporadic Frames",
        }
    }
}

/// 노드 아래의 네 그룹(내비게이션 Nodes/Frames 탭).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeGroup {
    /// Published Signals.
    PublishedSignals,
    /// Subscribed Signals.
    SubscribedSignals,
    /// Published Frames.
    PublishedFrames,
    /// Subscribed Frames.
    SubscribedFrames,
}

impl NodeGroup {
    /// 모든 그룹(표시 순서).
    pub const ALL: [NodeGroup; 4] = [
        NodeGroup::PublishedSignals,
        NodeGroup::SubscribedSignals,
        NodeGroup::PublishedFrames,
        NodeGroup::SubscribedFrames,
    ];

    /// 폴더 이름.
    pub fn label(self) -> &'static str {
        match self {
            NodeGroup::PublishedSignals => "Published Signals",
            NodeGroup::SubscribedSignals => "Subscribed Signals",
            NodeGroup::PublishedFrames => "Published Frames",
            NodeGroup::SubscribedFrames => "Subscribed Frames",
        }
    }
}

/// 내비게이션 탭.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NavTab {
    /// Frames/Commands.
    #[default]
    Frames,
    /// Nodes/Frames.
    Nodes,
}

/// 사용자가 고른 대상. 컨텍스트 뷰와 속성 창이 이것을 따른다.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Selection {
    /// 선택 없음.
    #[default]
    None,
    /// 클러스터 전체(트리 루트).
    Cluster,
    /// 프레임 그룹 폴더.
    Group(FrameGroup),
    /// 프레임.
    Frame(FrameId),
    /// 미리 정의된 명령(`AssignNAD` 등).
    Command(&'static str),
    /// 노드.
    Node(NodeId),
    /// 노드 아래 그룹.
    NodeGroup(NodeId, NodeGroup),
    /// 시그널.
    Signal(SignalId),
    /// 스케줄 테이블.
    Schedule(ScheduleId),
    /// 스케줄 슬롯(테이블, 슬롯 번호).
    Slot(ScheduleId, usize),
}

/// 이름으로 적은 선택. 편집으로 ID가 당겨지거나 모델을 다시 읽어도 같은 객체를 가리키게 한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelKey {
    /// 선택 없음.
    None,
    /// 클러스터.
    Cluster,
    /// 프레임 그룹.
    Group(FrameGroup),
    /// 프레임.
    Frame(String),
    /// 미리 정의된 명령.
    Command(&'static str),
    /// 노드.
    Node(String),
    /// 노드 아래 그룹.
    NodeGroup(String, NodeGroup),
    /// 시그널.
    Signal(String),
    /// 스케줄 테이블.
    Schedule(String),
    /// 스케줄 슬롯(테이블 이름, 번호).
    Slot(String, usize),
}

impl Selection {
    /// 이름으로 바꾼다.
    pub fn key(&self, m: &LdfModel) -> SelKey {
        let name = |s: Option<&str>| s.unwrap_or_default().to_owned();
        match self {
            Selection::None => SelKey::None,
            Selection::Cluster => SelKey::Cluster,
            Selection::Group(g) => SelKey::Group(*g),
            Selection::Frame(id) => SelKey::Frame(name(m.frames.get(*id).map(|f| f.name.as_str()))),
            Selection::Command(c) => SelKey::Command(c),
            Selection::Node(id) => SelKey::Node(name(m.nodes.get(*id).map(|n| n.name.as_str()))),
            Selection::NodeGroup(id, g) => {
                SelKey::NodeGroup(name(m.nodes.get(*id).map(|n| n.name.as_str())), *g)
            }
            Selection::Signal(id) => {
                SelKey::Signal(name(m.signals.get(*id).map(|s| s.name.as_str())))
            }
            Selection::Schedule(id) => {
                SelKey::Schedule(name(m.schedules.get(*id).map(|s| s.name.as_str())))
            }
            Selection::Slot(id, i) => {
                SelKey::Slot(name(m.schedules.get(*id).map(|s| s.name.as_str())), *i)
            }
        }
    }
}

impl SelKey {
    /// 모델에서 같은 이름의 객체를 찾아 선택으로 바꾼다. 없어졌으면 클러스터 선택으로 돌아간다.
    pub fn resolve(&self, m: &LdfModel) -> Selection {
        let found = match self {
            SelKey::None => Some(Selection::None),
            SelKey::Cluster => Some(Selection::Cluster),
            SelKey::Group(g) => Some(Selection::Group(*g)),
            SelKey::Command(c) => Some(Selection::Command(c)),
            SelKey::Frame(n) => m.find_frame(n).map(Selection::Frame),
            SelKey::Node(n) => m.find_node(n).map(Selection::Node),
            SelKey::NodeGroup(n, g) => m.find_node(n).map(|id| Selection::NodeGroup(id, *g)),
            SelKey::Signal(n) => m.find_signal(n).map(Selection::Signal),
            SelKey::Schedule(n) => m.find_schedule(n).map(Selection::Schedule),
            SelKey::Slot(n, i) => m.find_schedule(n).and_then(|id| {
                m.schedules
                    .get(id)
                    .filter(|t| *i < t.slots.len())
                    .map(|_| Selection::Slot(id, *i))
            }),
        };
        found.unwrap_or(Selection::Cluster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ldf_model::{EditOp, load};

    const SRC: &str = "Nodes { Master: M, 1 ms, 0 ms; Slaves: A, B; }\n\
        Signals { S1: 8, 0, M, A; S2: 8, 0, M, B; }\n\
        Frames { F1: 1, M, 1 { S1, 0; } F2: 2, M, 1 { S2, 0; } }\n\
        Schedule_tables { T { F1 delay 10 ms; F2 delay 10 ms; } }";

    #[test]
    fn selection_survives_id_shifts_by_name() {
        let mut m = load(SRC).model;
        let sel = Selection::Signal(m.find_signal("S2").expect("S2"));
        let key = sel.key(&m);
        EditOp::DeleteSignal("S1".into())
            .apply(&mut m)
            .expect("delete");
        // S2의 ID가 1에서 0으로 당겨졌지만 이름으로 같은 객체를 찾는다
        let back = key.resolve(&m);
        assert_eq!(back, Selection::Signal(m.find_signal("S2").expect("S2")));
        assert_ne!(back, sel);
    }

    #[test]
    fn deleted_objects_fall_back_to_the_cluster() {
        let mut m = load(SRC).model;
        let key = Selection::Frame(m.find_frame("F1").expect("F1")).key(&m);
        EditOp::DeleteFrame("F1".into())
            .apply(&mut m)
            .expect("delete");
        assert_eq!(key.resolve(&m), Selection::Cluster);
    }

    #[test]
    fn every_selection_kind_round_trips() {
        let m = load(SRC).model;
        let t = m.find_schedule("T").expect("T");
        for sel in [
            Selection::None,
            Selection::Cluster,
            Selection::Group(FrameGroup::Sporadic),
            Selection::Frame(m.find_frame("F2").expect("F2")),
            Selection::Command("AssignNAD"),
            Selection::Node(m.find_node("B").expect("B")),
            Selection::NodeGroup(m.find_node("A").expect("A"), NodeGroup::PublishedFrames),
            Selection::Signal(m.find_signal("S1").expect("S1")),
            Selection::Schedule(t),
            Selection::Slot(t, 1),
        ] {
            assert_eq!(sel.key(&m).resolve(&m), sel);
        }
        // 없는 슬롯 번호는 클러스터로
        assert_eq!(SelKey::Slot("T".into(), 9).resolve(&m), Selection::Cluster);
    }
}
