//! Host-neutral tree geometry inputs that do not depend on product identity.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TreeDepthNode<Id> {
    pub id: Id,
    pub parent: Option<Id>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TreeDepthError<Id> {
    DuplicateId { id: Id, index: usize },
    MissingParent { id: Id, parent: Id, index: usize },
    Cycle { id: Id, index: usize },
}

pub fn compute_tree_depths<Id>(nodes: &[TreeDepthNode<Id>]) -> Result<Vec<u32>, TreeDepthError<Id>>
where
    Id: Copy + Eq + Hash,
{
    compute_tree_depths_by(nodes, |node| node.id, |node| node.parent)
}

pub fn compute_tree_depths_by<Node, Id, IdOf, ParentOf>(
    nodes: &[Node],
    id_of: IdOf,
    parent_of: ParentOf,
) -> Result<Vec<u32>, TreeDepthError<Id>>
where
    Id: Copy + Eq + Hash,
    IdOf: Fn(&Node) -> Id,
    ParentOf: Fn(&Node) -> Option<Id>,
{
    let mut ids = Vec::with_capacity(nodes.len());
    let mut parents = Vec::with_capacity(nodes.len());
    for node in nodes {
        ids.push(id_of(node));
        parents.push(parent_of(node));
    }

    let mut indexes = HashMap::with_capacity(nodes.len());
    for (index, id) in ids.iter().copied().enumerate() {
        if indexes.insert(id, index).is_some() {
            return Err(TreeDepthError::DuplicateId { id, index });
        }
    }

    for (index, parent) in parents.iter().copied().enumerate() {
        if let Some(parent) = parent
            && !indexes.contains_key(&parent)
        {
            return Err(TreeDepthError::MissingParent {
                id: ids[index],
                parent,
                index,
            });
        }
    }

    let mut depths = vec![0_u32; nodes.len()];
    let mut state = vec![0_u8; nodes.len()];
    let mut path = Vec::with_capacity(nodes.len());

    for start in 0..nodes.len() {
        if state[start] != 0 {
            continue;
        }

        path.clear();
        let mut current = start;
        loop {
            match state[current] {
                0 => {}
                1 => {
                    return Err(TreeDepthError::Cycle {
                        id: ids[current],
                        index: current,
                    });
                }
                _ => break,
            }

            state[current] = 1;
            path.push(current);
            match parents[current] {
                Some(parent) => current = indexes[&parent],
                None => break,
            }
        }

        for &index in path.iter().rev() {
            depths[index] = parents[index]
                .map(|parent| depths[indexes[&parent]].saturating_add(1))
                .unwrap_or(0);
            state[index] = 2;
        }
    }

    Ok(depths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, parent: Option<u32>) -> TreeDepthNode<u32> {
        TreeDepthNode { id, parent }
    }

    #[test]
    fn ordinary_tree_keeps_input_order() {
        let nodes = [
            node(1, None),
            node(2, Some(1)),
            node(3, Some(1)),
            node(4, Some(2)),
        ];
        assert_eq!(compute_tree_depths(&nodes), Ok(vec![0, 1, 1, 2]));
    }

    #[test]
    fn parent_may_appear_after_child() {
        let nodes = [node(3, Some(2)), node(1, None), node(2, Some(1))];
        assert_eq!(compute_tree_depths(&nodes), Ok(vec![2, 0, 1]));
    }

    #[test]
    fn deep_chain_is_iterative() {
        let mut nodes = Vec::with_capacity(20_000);
        for id in 0..20_000_u32 {
            let parent = if id == 0 { None } else { Some(id - 1) };
            nodes.push(node(id, parent));
        }

        let depths = compute_tree_depths(&nodes).unwrap();
        assert_eq!(depths[0], 0);
        assert_eq!(depths[19_999], 19_999);
    }

    #[test]
    fn missing_parent_is_typed() {
        let error = compute_tree_depths(&[node(1, Some(9))]).unwrap_err();
        assert_eq!(
            error,
            TreeDepthError::MissingParent {
                id: 1,
                parent: 9,
                index: 0,
            }
        );
    }

    #[test]
    fn duplicate_id_is_typed() {
        let error = compute_tree_depths(&[node(1, None), node(1, None)]).unwrap_err();
        assert_eq!(error, TreeDepthError::DuplicateId { id: 1, index: 1 });
    }

    #[test]
    fn self_cycle_is_typed() {
        let error = compute_tree_depths(&[node(7, Some(7))]).unwrap_err();
        assert_eq!(error, TreeDepthError::Cycle { id: 7, index: 0 });
    }

    #[test]
    fn multi_node_cycle_is_typed_without_recursion() {
        let nodes = [node(1, Some(3)), node(2, Some(1)), node(3, Some(2))];
        assert!(matches!(
            compute_tree_depths(&nodes),
            Err(TreeDepthError::Cycle { .. })
        ));
    }
}
