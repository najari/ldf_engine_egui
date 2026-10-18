//! 내비게이션 트리(Frames/Commands, Nodes/Frames)를 egui와 무관하게 만든다.

use super::derive::{self, uint};
use super::{FrameGroup, NavTab, NodeGroup, Selection};
use ldf_model::{FrameId, FrameKind, LdfModel, NodeId, NodeRole, Ref, SignalId};

/// 트리 노드 ID. 같은 시그널이 여러 노드 아래 나올 수 있으므로 위치 정보를 포함한다.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NavKey {
    /// 탭의 루트(클러스터).
    Root(NavTab),
    /// 프레임 그룹 폴더.
    Group(FrameGroup),
    /// 프레임.
    Frame(FrameId),
    /// "Predefined Commands" 폴더.
    CommandsDir,
    /// 미리 정의된 명령.
    Command(&'static str),
    /// 노드.
    Node(NodeId),
    /// 노드 아래 그룹.
    NodeGroup(NodeId, NodeGroup),
    /// 노드 아래 그룹의 항목(그룹 안 순번).
    NodeItem(NodeId, NodeGroup, u32),
}

/// 화면에 그릴 트리 노드.
#[derive(Clone, Debug, PartialEq)]
pub struct NavNode {
    /// 노드 ID.
    pub key: NavKey,
    /// 표시 이름.
    pub label: String,
    /// 폴더인지(자식을 가질 수 있는지).
    pub dir: bool,
    /// 이름을 못 찾은 참조인지(빨갛게 표시).
    pub unresolved: bool,
    /// 자식.
    pub children: Vec<NavNode>,
}

impl NavNode {
    fn dir(key: NavKey, label: impl Into<String>, children: Vec<NavNode>) -> Self {
        Self {
            key,
            label: label.into(),
            dir: true,
            unresolved: false,
            children,
        }
    }

    fn leaf(key: NavKey, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            dir: false,
            unresolved: false,
            children: Vec::new(),
        }
    }

    /// 첫 번째 잎(검색 후 Enter로 선택할 대상).
    pub fn first_leaf(&self) -> Option<&NavKey> {
        if !self.dir {
            return Some(&self.key);
        }
        self.children.iter().find_map(NavNode::first_leaf)
    }
}

impl NavKey {
    /// 화면 선택에 대응하는 트리 항목(트리에 없는 선택이면 `None`).
    pub fn of_selection(sel: &Selection) -> Option<NavKey> {
        match sel {
            Selection::Group(g) => Some(NavKey::Group(*g)),
            Selection::Frame(f) => Some(NavKey::Frame(*f)),
            Selection::Command(c) => Some(NavKey::Command(c)),
            Selection::Node(n) => Some(NavKey::Node(*n)),
            Selection::NodeGroup(n, g) => Some(NavKey::NodeGroup(*n, *g)),
            _ => None,
        }
    }

    /// 트리 선택을 화면 선택으로 바꾼다.
    pub fn selection(&self, m: &LdfModel) -> Selection {
        match self {
            NavKey::Root(_) => Selection::Cluster,
            NavKey::Group(g) => Selection::Group(*g),
            NavKey::Frame(f) => Selection::Frame(*f),
            NavKey::CommandsDir => Selection::None,
            NavKey::Command(c) => Selection::Command(c),
            NavKey::Node(n) => Selection::Node(*n),
            NavKey::NodeGroup(n, g) => Selection::NodeGroup(*n, *g),
            NavKey::NodeItem(n, g, i) => {
                let i = *i as usize;
                match g {
                    NodeGroup::PublishedSignals | NodeGroup::SubscribedSignals => {
                        let list = node_signals(m, *n, *g == NodeGroup::PublishedSignals);
                        list.get(i)
                            .map_or(Selection::None, |s| Selection::Signal(*s))
                    }
                    NodeGroup::PublishedFrames | NodeGroup::SubscribedFrames => {
                        let list = node_frames(m, *n, *g == NodeGroup::PublishedFrames);
                        list.get(i)
                            .map_or(Selection::None, |f| Selection::Frame(*f))
                    }
                }
            }
        }
    }
}

/// 노드가 발행(또는 구독)하는 시그널.
pub fn node_signals(m: &LdfModel, node: NodeId, published: bool) -> Vec<SignalId> {
    m.signals
        .iter()
        .filter(|(_, s)| {
            if published {
                s.publisher.as_ref().and_then(Ref::id) == Some(node)
            } else {
                s.subscribers.iter().any(|r| r.id() == Some(node))
            }
        })
        .map(|(id, _)| id)
        .collect()
}

/// 노드가 발행(또는 구독)하는 프레임.
pub fn node_frames(m: &LdfModel, node: NodeId, published: bool) -> Vec<FrameId> {
    let name = m.nodes.get(node).map(|n| n.name.clone());
    m.frames
        .iter()
        .filter(|(id, f)| {
            let Some(name) = &name else { return false };
            if published {
                match &f.kind {
                    FrameKind::Unconditional { publisher, .. } => publisher.id() == Some(node),
                    _ => derive::frame_publishers(m, *id)
                        .iter()
                        .any(|p| &p.text == name),
                }
            } else {
                derive::frame_subscribers(m, *id)
                    .iter()
                    .any(|p| &p.text == name)
            }
        })
        .map(|(id, _)| id)
        .collect()
}

fn frame_label(m: &LdfModel, id: FrameId, hex: bool) -> String {
    let Some(f) = m.frames.get(id) else {
        return "?".to_owned();
    };
    match f.lin_id() {
        Some(lin) => format!("{} ({})", f.name, uint(lin, hex)),
        None => f.name.clone(),
    }
}

/// 이 프로그램이 "미리 정의된 명령"으로 보여주는 목록.
pub const COMMANDS: [&str; 7] = [
    "AssignNAD",
    "AssignFrameId",
    "AssignFrameIdRange",
    "ConditionalChangeNAD",
    "DataDump",
    "SaveConfiguration",
    "FreeFormat",
];

fn group_of(kind: &FrameKind) -> FrameGroup {
    match kind {
        FrameKind::Unconditional { .. } => FrameGroup::Unconditional,
        FrameKind::Diagnostic { .. } => FrameGroup::Diagnostic,
        FrameKind::EventTriggered { .. } => FrameGroup::EventTriggered,
        FrameKind::Sporadic { .. } => FrameGroup::Sporadic,
    }
}

fn root_label(m: &LdfModel) -> String {
    m.header
        .channel_name
        .clone()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "LIN".to_owned())
}

/// Frames/Commands 탭의 트리.
pub fn frames_tree(m: &LdfModel, hex: bool) -> NavNode {
    let mut groups = Vec::new();
    for g in [
        FrameGroup::Unconditional,
        FrameGroup::Diagnostic,
        FrameGroup::EventTriggered,
        FrameGroup::Sporadic,
    ] {
        let children: Vec<NavNode> = m
            .frames
            .iter()
            .filter(|(_, f)| group_of(&f.kind) == g)
            .map(|(id, _)| NavNode::leaf(NavKey::Frame(id), frame_label(m, id, hex)))
            .collect();
        groups.push(NavNode::dir(NavKey::Group(g), g.label(), children));
    }
    let commands = COMMANDS
        .iter()
        .map(|c| NavNode::leaf(NavKey::Command(c), *c))
        .collect();
    groups.push(NavNode::dir(
        NavKey::CommandsDir,
        "Predefined Commands",
        commands,
    ));
    NavNode::dir(NavKey::Root(NavTab::Frames), root_label(m), groups)
}

/// Nodes/Frames 탭의 트리.
pub fn nodes_tree(m: &LdfModel, hex: bool) -> NavNode {
    let mut nodes = Vec::new();
    for (nid, n) in m.nodes.iter() {
        let mut groups = Vec::new();
        for g in NodeGroup::ALL {
            let children: Vec<NavNode> = match g {
                NodeGroup::PublishedSignals | NodeGroup::SubscribedSignals => {
                    node_signals(m, nid, g == NodeGroup::PublishedSignals)
                        .into_iter()
                        .enumerate()
                        .map(|(i, s)| {
                            let name = m.signals.get(s).map_or("?", |s| s.name.as_str());
                            NavNode::leaf(NavKey::NodeItem(nid, g, i as u32), name)
                        })
                        .collect()
                }
                NodeGroup::PublishedFrames | NodeGroup::SubscribedFrames => {
                    node_frames(m, nid, g == NodeGroup::PublishedFrames)
                        .into_iter()
                        .enumerate()
                        .map(|(i, f)| {
                            NavNode::leaf(
                                NavKey::NodeItem(nid, g, i as u32),
                                frame_label(m, f, hex),
                            )
                        })
                        .collect()
                }
            };
            groups.push(NavNode::dir(NavKey::NodeGroup(nid, g), g.label(), children));
        }
        let label = match n.role {
            NodeRole::Master => format!("{} (Master)", n.name),
            NodeRole::Slave => n.name.clone(),
        };
        nodes.push(NavNode::dir(NavKey::Node(nid), label, groups));
    }
    NavNode::dir(NavKey::Root(NavTab::Nodes), root_label(m), nodes)
}

/// 대소문자를 구분하지 않는 부분 일치로 트리를 거른다. 자신이나 자손이 일치하면 남긴다.
/// 질의가 비어 있으면 그대로 돌려준다.
pub fn filter(node: &NavNode, query: &str) -> Option<NavNode> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(node.clone());
    }
    filter_inner(node, &q)
}

fn filter_inner(node: &NavNode, q: &str) -> Option<NavNode> {
    let children: Vec<NavNode> = node
        .children
        .iter()
        .filter_map(|c| filter_inner(c, q))
        .collect();
    if !children.is_empty() || (!node.dir && node.label.to_lowercase().contains(q)) {
        Some(NavNode {
            children,
            ..node.clone()
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> LdfModel {
        ldf_model::load(
            "LIN_description_file; Channel_name = \"Body\";\n\
             Nodes { Master: GW, 5 ms, 0.1 ms; Slaves: A, B; }\n\
             Signals { S1: 4, 0, A, GW, B; S2: 8, 0, B, GW; }\n\
             Frames { FA: 0x10, A, 1 { S1, 0; } FB: 0x11, B, 1 { S2, 0; } }\n\
             Sporadic_frames { Sp: FA, FB; }\n\
             Diagnostic_signals { D0: 8, 0; }\n\
             Diagnostic_frames { MasterReq: 0x3c { D0, 0; } }\n",
        )
        .model
    }

    fn labels(n: &NavNode) -> Vec<String> {
        n.children.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn frames_tree_groups_frames_by_kind() {
        let m = model();
        let t = frames_tree(&m, false);
        assert_eq!(t.label, "Body");
        assert_eq!(
            labels(&t),
            [
                "Unconditional Frames",
                "Diagnostic Frames",
                "Event-Triggered Frames",
                "Sporadic Frames",
                "Predefined Commands"
            ]
        );
        assert_eq!(labels(&t.children[0]), ["FA (16)", "FB (17)"]);
        assert_eq!(labels(&t.children[1]), ["MasterReq (60)"]);
        assert_eq!(labels(&t.children[3]), ["Sp"]);
        assert_eq!(t.children[4].children.len(), COMMANDS.len());
        // 16진 표시
        assert_eq!(
            labels(&frames_tree(&m, true).children[0]),
            ["FA (0x10)", "FB (0x11)"]
        );
    }

    #[test]
    fn tree_keys_are_unique() {
        let m = model();
        for t in [frames_tree(&m, false), nodes_tree(&m, false)] {
            let mut keys = Vec::new();
            fn walk(n: &NavNode, keys: &mut Vec<NavKey>) {
                keys.push(n.key.clone());
                n.children.iter().for_each(|c| walk(c, keys));
            }
            walk(&t, &mut keys);
            let unique: std::collections::HashSet<_> = keys.iter().collect();
            assert_eq!(unique.len(), keys.len(), "중복 키: {keys:?}");
        }
    }

    #[test]
    fn nodes_tree_lists_published_and_subscribed() {
        let m = model();
        let t = nodes_tree(&m, false);
        assert_eq!(labels(&t), ["GW (Master)", "A", "B"]);
        let a = &t.children[1];
        assert_eq!(
            labels(a),
            [
                "Published Signals",
                "Subscribed Signals",
                "Published Frames",
                "Subscribed Frames"
            ]
        );
        assert_eq!(labels(&a.children[0]), ["S1"]);
        assert!(a.children[1].children.is_empty());
        assert_eq!(labels(&a.children[2]), ["FA (16)"]);
        let gw = &t.children[0];
        assert_eq!(labels(&gw.children[1]), ["S1", "S2"]);
    }

    #[test]
    fn tree_selection_maps_back_to_model_objects() {
        let m = model();
        let t = nodes_tree(&m, false);
        let a = &t.children[1];
        let item = &a.children[0].children[0];
        let Selection::Signal(sid) = item.key.selection(&m) else {
            panic!("signal expected")
        };
        assert_eq!(m.signals.get(sid).map(|s| s.name.as_str()), Some("S1"));

        let fitem = &a.children[2].children[0];
        let Selection::Frame(fid) = fitem.key.selection(&m) else {
            panic!("frame expected")
        };
        assert_eq!(m.frames.get(fid).map(|f| f.name.as_str()), Some("FA"));
        assert_eq!(
            NavKey::Root(NavTab::Nodes).selection(&m),
            Selection::Cluster
        );
        assert_eq!(
            NavKey::NodeItem(NodeId(0), NodeGroup::PublishedSignals, 99).selection(&m),
            Selection::None
        );
    }

    #[test]
    fn filter_keeps_matching_leaves_and_their_ancestors() {
        let m = model();
        let t = frames_tree(&m, false);
        let f = filter(&t, "fb").expect("match");
        assert_eq!(labels(&f), ["Unconditional Frames"]);
        assert_eq!(labels(&f.children[0]), ["FB (17)"]);
        assert_eq!(f.first_leaf(), Some(&NavKey::Frame(frame_id(&m, "FB"))));
        assert!(filter(&t, "zzz").is_none());
        assert_eq!(filter(&t, "  ").expect("all"), t);
    }

    fn frame_id(m: &LdfModel, name: &str) -> FrameId {
        m.frames
            .iter()
            .find(|(_, f)| f.name == name)
            .map(|(i, _)| i)
            .expect("frame")
    }
}
