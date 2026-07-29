use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TabTreeNode {
    pub(crate) id: u64,
    pub(crate) parent_id: Option<u64>,
    pub(crate) sort_key: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TabTreeRow {
    pub(crate) id: u64,
    pub(crate) depth: usize,
    pub(crate) is_last: bool,
    pub(crate) ancestors: Vec<u64>,
    pub(crate) guides: Vec<bool>,
}

pub(crate) fn tree_rows(nodes: &[TabTreeNode]) -> Vec<TabTreeRow> {
    fn visit(
        nodes: &[TabTreeNode],
        id: u64,
        is_last: bool,
        ancestors: &[u64],
        guides: &[bool],
        visited: &mut HashSet<u64>,
        rows: &mut Vec<TabTreeRow>,
    ) {
        if !visited.insert(id) {
            return;
        }
        let depth = ancestors.len();
        rows.push(TabTreeRow {
            id,
            depth,
            is_last,
            ancestors: ancestors.to_vec(),
            guides: guides.to_vec(),
        });

        let mut children = nodes
            .iter()
            .filter(|node| node.parent_id == Some(id))
            .copied()
            .collect::<Vec<_>>();
        children.sort_by_key(|node| node.sort_key);
        let child_count = children.len();
        for (position, child) in children.into_iter().enumerate() {
            let mut child_ancestors = ancestors.to_vec();
            child_ancestors.push(id);
            let mut child_guides = guides.to_vec();
            if depth > 0 {
                child_guides.push(!is_last);
            }
            visit(
                nodes,
                child.id,
                position + 1 == child_count,
                &child_ancestors,
                &child_guides,
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
            position + 1 == root_count,
            &[],
            &[],
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
        visit(nodes, node.id, true, &[], &[], &mut visited, &mut rows);
    }
    rows
}

/// Filter preorder tree rows to those visible when ancestor tabs are collapsed.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn visible_tree_rows(
    all_rows: &[TabTreeRow],
    collapsed_ids: &HashSet<u64>,
) -> Vec<TabTreeRow> {
    all_rows
        .iter()
        .filter(|row| !row.ancestors.iter().any(|id| collapsed_ids.contains(id)))
        .cloned()
        .collect()
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
                parent_id: Some(3),
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
        assert!(rows[1].is_last);
        assert!(rows[2].is_last);
        assert_eq!(rows[2].ancestors, vec![1, 3]);
        assert_eq!(rows[2].guides, vec![false]);
    }

    #[test]
    fn visible_tree_rows_hides_descendants_of_collapsed_ancestors() {
        let rows = vec![
            TabTreeRow {
                id: 1,
                depth: 0,
                is_last: false,
                ancestors: vec![],
                guides: vec![],
            },
            TabTreeRow {
                id: 2,
                depth: 1,
                is_last: true,
                ancestors: vec![1],
                guides: vec![false],
            },
            TabTreeRow {
                id: 3,
                depth: 0,
                is_last: true,
                ancestors: vec![],
                guides: vec![],
            },
        ];
        let collapsed = HashSet::from([1]);
        let visible = visible_tree_rows(&rows, &collapsed);
        assert_eq!(
            visible.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
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
