//! Lightweight, in-window terminal session tree for `agenterm-con`.
//!
//! This deliberately owns only tab identity and parentage. PTYs, rendering,
//! persistence, and any background authority remain outside this type: the
//! standalone console host must stay one GUI process with bounded local state.

pub const MAX_TABS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TabId(u64);

impl TabId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabNode {
    pub id: TabId,
    pub parent: Option<TabId>,
    pub title: String,
}

/// The lightweight host's complete tab tree.
///
/// Parent cycles are impossible because a node can only be created beneath an
/// existing node. Closing a parent promotes its direct children, preserving
/// their sessions instead of treating hierarchy as ownership of a PTY.
#[derive(Debug)]
pub struct Workspace {
    nodes: Vec<TabNode>,
    depths: Vec<u32>,
    active: Option<TabId>,
    next_id: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            depths: Vec::new(),
            active: None,
            next_id: 1,
        }
    }
}

impl Workspace {
    pub fn nodes(&self) -> &[TabNode] {
        &self.nodes
    }

    pub fn depths(&self) -> &[u32] {
        &self.depths
    }

    pub const fn active(&self) -> Option<TabId> {
        self.active
    }

    pub fn node(&self, id: TabId) -> Option<&TabNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn set_active(&mut self, id: TabId) -> bool {
        if self.node(id).is_none() {
            return false;
        }
        self.active = Some(id);
        true
    }

    pub fn add_root(&mut self, title: String) -> Option<TabId> {
        self.add(None, title, 0)
    }

    pub fn add_child(&mut self, parent: TabId, title: String) -> Option<TabId> {
        let parent_index = self.nodes.iter().position(|node| node.id == parent)?;
        let depth = self.depths[parent_index].saturating_add(1);
        self.add(Some(parent), title, depth)
    }

    pub fn close(&mut self, id: TabId) -> Option<TabNode> {
        let index = self.nodes.iter().position(|node| node.id == id)?;
        let removed = self.nodes.remove(index);
        for node in &mut self.nodes {
            if node.parent == Some(id) {
                node.parent = removed.parent;
            }
        }
        self.depths = agenterm_ui_core::compute_tree_depths_by(
            &self.nodes,
            |node| node.id,
            |node| node.parent,
        )
        .unwrap_or_else(|_| vec![0; self.nodes.len()]);
        if self.active == Some(id) {
            self.active = self
                .nodes
                .get(index)
                .or_else(|| self.nodes.last())
                .map(|node| node.id);
        }
        Some(removed)
    }

    fn add(&mut self, parent: Option<TabId>, title: String, depth: u32) -> Option<TabId> {
        if self.nodes.len() >= MAX_TABS || self.next_id == u64::MAX {
            return None;
        }
        let id = TabId(self.next_id);
        self.next_id = self.next_id.checked_add(1)?;
        self.nodes.push(TabNode { id, parent, title });
        self.depths.push(depth);
        self.active = Some(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_parent_promotes_direct_children_and_keeps_them_live() {
        let mut workspace = Workspace::default();
        let root = workspace.add_root("root".into()).unwrap();
        let parent = workspace.add_child(root, "parent".into()).unwrap();
        let child = workspace.add_child(parent, "child".into()).unwrap();
        let grandchild = workspace.add_child(child, "grandchild".into()).unwrap();

        workspace.close(parent).unwrap();

        assert_eq!(workspace.node(child).unwrap().parent, Some(root));
        assert_eq!(workspace.node(grandchild).unwrap().parent, Some(child));
        assert_eq!(workspace.depths(), &[0, 1, 2]);
    }

    #[test]
    fn closing_active_tab_selects_a_remaining_neighbor() {
        let mut workspace = Workspace::default();
        let first = workspace.add_root("first".into()).unwrap();
        let second = workspace.add_root("second".into()).unwrap();
        workspace.close(second);
        assert_eq!(workspace.active(), Some(first));
    }

    #[test]
    fn creation_limits_fail_without_mutating_tree_or_active_tab() {
        let mut workspace = Workspace::default();
        for index in 0..MAX_TABS {
            assert!(workspace.add_root(format!("tab {index}")).is_some());
        }
        let active = workspace.active();
        assert_eq!(workspace.add_root("overflow".into()), None);
        assert_eq!(workspace.nodes().len(), MAX_TABS);
        assert_eq!(workspace.active(), active);

        workspace.close(active.unwrap());
        let node_count = workspace.nodes().len();
        let active = workspace.active();
        workspace.next_id = u64::MAX;
        assert_eq!(workspace.add_root("exhausted".into()), None);
        assert_eq!(workspace.nodes().len(), node_count);
        assert_eq!(workspace.active(), active);
    }
}
