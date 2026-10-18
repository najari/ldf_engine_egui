//! 화면에 무엇을 보여줄지 정하는 순수 로직(egui 무관, 단위 테스트 대상).
//! UI 패널(`ui/`)은 여기서 만든 값을 그리기만 한다.

pub mod checker;
pub mod context;
pub mod derive;
pub mod nav;
pub mod props;

use ldf_model::{FrameId, NodeId, ScheduleId, SignalId};

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
