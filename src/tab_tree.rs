use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabTreeNode {
    pub(crate) id: u64,
    pub(crate) parent_id: Option<u64>,
    pub(crate) sort_key: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabTreeRow {
    pub(crate) id: u64,
    pub(crate) depth: usize,
    pub(crate) is_last: bool,
}

pub(crate) fn tree_rows(nodes: &[TabTreeNode]) -> Vec<TabTreeRow> {
    fn visit(
        nodes: &[TabTreeNode],
        id: u64,
        depth: usize,
        is_last: bool,
        visited: &mut HashSet<u64>,
        rows: &mut Vec<TabTreeRow>,
    ) {
        if !visited.insert(id) {
            return;
        }
        rows.push(TabTreeRow { id, depth, is_last });

        let mut children = nodes
            .iter()
            .filter(|node| node.parent_id == Some(id))
            .copied()
            .collect::<Vec<_>>();
        children.sort_by_key(|node| node.sort_key);
        let child_count = children.len();
        for (position, child) in children.into_iter().enumerate() {
            visit(
                nodes,
                child.id,
                depth + 1,
                position + 1 == child_count,
                visited,
                rows,
            );
        }
    }

    let ids = nodes.iter().map(|node| node.id).collect::<HashSet<_>>();
    let mut roots = nodes
        .iter()
        .filter(|node| {
            node.parent_id
                .is_none_or(|parent_id| !ids.contains(&parent_id) || parent_id == node.id)
        })
        .copied()
        .collect::<Vec<_>>();
    roots.sort_by_key(|node| node.sort_key);

    let mut visited = HashSet::new();
    let mut rows = Vec::with_capacity(nodes.len());
    let root_count = roots.len();
    for (position, root) in roots.into_iter().enumerate() {
        visit(
            nodes,
            root.id,
            0,
            position + 1 == root_count,
            &mut visited,
            &mut rows,
        );
    }

    let mut remaining = nodes
        .iter()
        .filter(|node| !visited.contains(&node.id))
        .copied()
        .collect::<Vec<_>>();
    remaining.sort_by_key(|node| node.sort_key);
    for node in remaining {
        visit(nodes, node.id, 0, true, &mut visited, &mut rows);
    }
    rows
}

pub(crate) fn would_create_cycle(nodes: &[TabTreeNode], child_id: u64, parent_id: u64) -> bool {
    if child_id == parent_id {
        return true;
    }
    let mut current = Some(parent_id);
    for _ in 0..=nodes.len() {
        let Some(id) = current else {
            return false;
        };
        if id == child_id {
            return true;
        }
        current = nodes
            .iter()
            .find(|node| node.id == id)
            .and_then(|node| node.parent_id);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_roots_and_children_as_a_preorder_tree() {
        let nodes = [
            TabTreeNode {
                id: 4,
                parent_id: Some(1),
                sort_key: 3,
            },
            TabTreeNode {
                id: 1,
                parent_id: None,
                sort_key: 0,
            },
            TabTreeNode {
                id: 3,
                parent_id: Some(1),
                sort_key: 2,
            },
            TabTreeNode {
                id: 2,
                parent_id: None,
                sort_key: 1,
            },
        ];
        let rows = tree_rows(&nodes);
        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![1, 3, 4, 2]
        );
        assert_eq!(rows[1].depth, 1);
        assert!(!rows[1].is_last);
        assert!(rows[2].is_last);
    }

    #[test]
    fn rejects_parent_cycles() {
        let nodes = [
            TabTreeNode {
                id: 1,
                parent_id: None,
                sort_key: 0,
            },
            TabTreeNode {
                id: 2,
                parent_id: Some(1),
                sort_key: 1,
            },
        ];
        assert!(would_create_cycle(&nodes, 1, 2));
        assert!(would_create_cycle(&nodes, 1, 1));
        assert!(!would_create_cycle(&nodes, 2, 1));
    }
}
