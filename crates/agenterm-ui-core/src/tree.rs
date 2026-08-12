//! Host-neutral tree geometry inputs that do not depend on product identity.

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
    Id: Copy + Ord,
{
    compute_tree_depths_by(nodes, |node| node.id, |node| node.parent)
}

pub fn compute_tree_depths_by<Node, Id, IdOf, ParentOf>(
    nodes: &[Node],
    id_of: IdOf,
    parent_of: ParentOf,
) -> Result<Vec<u32>, TreeDepthError<Id>>
where
    Id: Copy + Ord,
    IdOf: Fn(&Node) -> Id,
    ParentOf: Fn(&Node) -> Option<Id>,
{
    let mut ids = Vec::with_capacity(nodes.len());
    let mut parents = Vec::with_capacity(nodes.len());
    for node in nodes {
        ids.push(id_of(node));
        parents.push(parent_of(node));
    }

    let mut indexes: Vec<(Id, usize)> = ids.iter().copied().zip(0..nodes.len()).collect();
    sort_index_pairs(&mut indexes);
    for duplicate in indexes.windows(2) {
        if duplicate[0].0 == duplicate[1].0 {
            return Err(TreeDepthError::DuplicateId {
                id: duplicate[1].0,
                index: duplicate[1].1,
            });
        }
    }

    let index_of = |id: Id| {
        indexes
            .binary_search_by_key(&id, |(candidate, _)| *candidate)
            .ok()
            .map(|index| indexes[index].1)
    };

    for (index, parent) in parents.iter().copied().enumerate() {
        if let Some(parent) = parent
            && index_of(parent).is_none()
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
                Some(parent) => current = index_of(parent).expect("validated parent index"),
                None => break,
            }
        }

        for &index in path.iter().rev() {
            depths[index] = parents[index]
                .map(|parent| {
                    depths[index_of(parent).expect("validated parent index")].saturating_add(1)
                })
                .unwrap_or(0);
            state[index] = 2;
        }
    }

    Ok(depths)
}

fn sort_index_pairs<Id: Ord>(values: &mut [(Id, usize)]) {
    let len = values.len();
    for root in (0..len / 2).rev() {
        sift_down(values, root, len);
    }
    for end in (1..len).rev() {
        values.swap(0, end);
        sift_down(values, 0, end);
    }
}

fn sift_down<Id: Ord>(values: &mut [(Id, usize)], mut root: usize, end: usize) {
    loop {
        let left = root * 2 + 1;
        if left >= end {
            return;
        }
        let right = left + 1;
        let child = if right < end && values[left] < values[right] {
            right
        } else {
            left
        };
        if values[root] >= values[child] {
            return;
        }
        values.swap(root, child);
        root = child;
    }
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
