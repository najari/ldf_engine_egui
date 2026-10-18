//! 내비게이션 뷰: Frames/Commands, Nodes/Frames 두 탭의 트리와 이름 검색.

use super::UiState;
use crate::view::nav::{self, NavKey, NavNode};
use crate::view::{NavTab, Selection};
use egui::{Id, Key, ScrollArea, Ui};
use egui_ltreeview::{Action, NodeBuilder, TreeView, TreeViewBuilder, TreeViewState};
use ldf_engine::ModelSnapshot;

/// 내비게이션 패널 전체를 그린다. 트리는 (스냅샷, 16진, 탭, 검색어)가 바뀔 때만 다시 만든다.
pub fn show(ui: &mut Ui, snap: &ModelSnapshot, st: &mut UiState, hex: bool) {
    let model = &snap.model;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut st.nav_tab, NavTab::Frames, "Frames/Commands");
        ui.selectable_value(&mut st.nav_tab, NavTab::Nodes, "Nodes/Frames");
    });

    let search = ui.add(
        egui::TextEdit::singleline(&mut st.search)
            .hint_text("이름 검색 (Ctrl+F)")
            .desired_width(f32::INFINITY),
    );
    if st.focus_search {
        search.request_focus();
        st.focus_search = false;
    }
    let enter = search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    ui.separator();

    let tab = st.nav_tab;
    let query = st.search.trim().to_lowercase();
    let cached = st
        .nav_cache
        .get((snap.version, hex, tab, query.clone()), || {
            let full = match tab {
                NavTab::Frames => nav::frames_tree(model, hex),
                NavTab::Nodes => nav::nodes_tree(model, hex),
            };
            nav::filter(&full, &query)
        });
    let Some(root) = (*cached).as_ref() else {
        ui.weak("일치하는 항목이 없다");
        return;
    };
    let searching = !query.is_empty();

    // 검색 후 Enter: 첫 결과를 선택한다
    if enter && let Some(key) = root.first_leaf().cloned() {
        st.selection = key.selection(model);
        match tab {
            NavTab::Frames => st.tree_frames.set_one_selected(key),
            NavTab::Nodes => st.tree_nodes.set_one_selected(key),
        }
    }

    let UiState {
        tree_frames,
        tree_nodes,
        selection,
        activate,
        delete,
        ..
    } = st;
    let state: &mut TreeViewState<NavKey> = match tab {
        NavTab::Frames => tree_frames,
        NavTab::Nodes => tree_nodes,
    };
    if searching {
        open_all(state, root);
    }

    ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let (response, actions) =
            TreeView::new(Id::new(("nav-tree", tab))).show_state(ui, state, |b| draw(b, root, 0));
        for action in actions {
            match action {
                Action::SetSelected(keys) => {
                    if let Some(key) = keys.first() {
                        *selection = key.selection(model);
                    }
                }
                Action::Activate(_) => *activate = true,
                _ => {}
            }
        }
        // 트리에 포커스가 있을 때의 Delete는 선택한 항목의 삭제 요청이다
        if response.has_focus() && ui.input(|i| i.key_pressed(Key::Delete)) {
            *delete = true;
        }
    });
}
fn draw(b: &mut TreeViewBuilder<'_, NavKey>, n: &NavNode, depth: usize) {
    if n.dir {
        b.node(
            NodeBuilder::dir(n.key.clone())
                .label(n.label.clone())
                .default_open(depth < 2),
        );
        for c in &n.children {
            draw(b, c, depth + 1);
        }
        b.close_dir();
    } else {
        b.leaf(n.key.clone(), n.label.clone());
    }
}

fn open_all(state: &mut TreeViewState<NavKey>, n: &NavNode) {
    if n.dir {
        state.set_openness(n.key.clone(), true);
        for c in &n.children {
            open_all(state, c);
        }
    }
}

/// 컨텍스트·스케줄 뷰에서 프레임을 골랐을 때 트리에서도 그 프레임을 선택 표시한다.
pub fn sync_tree_selection(st: &mut UiState, sel: &Selection) {
    if let Selection::Frame(id) = sel {
        st.tree_frames.set_one_selected(NavKey::Frame(*id));
    }
}
