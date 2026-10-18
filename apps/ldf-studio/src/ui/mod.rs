//! egui 패널. 무엇을 보여줄지는 `view`가 정하고, 여기서는 그리고 입력을 상태에 반영한다.

pub mod bits;
pub mod context;
pub mod nav;
pub mod props;
pub mod schedule;
pub mod status;

use crate::view::context::Table;
use crate::view::nav::{NavKey, NavNode};
use crate::view::{NavTab, Selection};
use egui_ltreeview::TreeViewState;
use ldf_model::ScheduleId;
use std::collections::HashSet;
use std::sync::Arc;

/// 키가 같으면 이전 계산 결과를 재사용하는 한 칸짜리 캐시.
/// egui는 매 프레임 화면을 다시 그리므로, 모델이 큰 경우 표·트리를 프레임마다 만들면 16 ms를 넘긴다.
pub struct Cache<K, V> {
    slot: Option<(K, Arc<V>)>,
}

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self { slot: None }
    }
}

impl<K: PartialEq, V> Cache<K, V> {
    /// 키가 바뀌었을 때만 `make`를 실행한다.
    pub fn get(&mut self, key: K, make: impl FnOnce() -> V) -> Arc<V> {
        if let Some((k, v)) = &self.slot
            && *k == key
        {
            return Arc::clone(v);
        }
        let v = Arc::new(make());
        self.slot = Some((key, Arc::clone(&v)));
        v
    }
}

/// 내비게이션 트리 캐시 키: (스냅샷 번호, 16진 여부, 탭, 검색어).
pub type NavKeyCache = (u64, bool, NavTab, String);
/// 컨텍스트 표 캐시 키: (스냅샷 번호, 선택, 16진 여부).
pub type CtxKeyCache = (u64, Selection, bool);

/// 저장하지 않는 화면 상태(선택, 검색어, 펼침 등).
#[derive(Default)]
pub struct UiState {
    /// 현재 선택.
    pub selection: Selection,
    /// 내비게이션 탭.
    pub nav_tab: NavTab,
    /// Frames/Commands 트리 상태.
    pub tree_frames: TreeViewState<NavKey>,
    /// Nodes/Frames 트리 상태.
    pub tree_nodes: TreeViewState<NavKey>,
    /// 이름 검색어.
    pub search: String,
    /// 다음 프레임에 검색창으로 포커스를 옮길지(Ctrl+F).
    pub focus_search: bool,
    /// 펼친 스케줄 테이블.
    pub sched_open: HashSet<ScheduleId>,
    /// 마지막으로 선택한 스케줄 테이블(다른 것을 골라도 위쪽 속성 창이 유지된다).
    pub last_schedule: Option<ScheduleId>,
    /// 거른 내비게이션 트리 캐시.
    pub nav_cache: Cache<NavKeyCache, Option<NavNode>>,
    /// 컨텍스트 표 캐시.
    pub ctx_cache: Cache<CtxKeyCache, Table>,
}

impl UiState {
    /// 새 모델이 열렸을 때: 이전 모델의 ID를 가리키는 상태를 모두 버린다.
    pub fn reset_for_new_model(&mut self) {
        self.selection = Selection::Cluster;
        self.tree_frames = TreeViewState::default();
        self.tree_nodes = TreeViewState::default();
        self.sched_open.clear();
        self.last_schedule = None;
        self.search.clear();
    }
}
