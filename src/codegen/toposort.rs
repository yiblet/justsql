use std::collections::BTreeSet;

/// returns a topologically sorted vector of input value references
/// returns None if the graph contains a cycle
pub fn topological_sort<'a, T: Ord + 'a, I: Iterator<Item = &'a (T, T)>>(
    iter: I,
) -> Option<Vec<&'a T>> {
    let mut parent_of_relations = BTreeSet::new();
    let mut child_of_relations = BTreeSet::new();
    let mut all_nodes = BTreeSet::new();

    let mut min = None;
    let mut max = None;

    for (from_node, to_node) in iter {
        parent_of_relations.insert((to_node, from_node));
        child_of_relations.insert((from_node, to_node));
        all_nodes.insert(to_node);
        all_nodes.insert(from_node);

        if let Some(min_val) = min {
            min = Some(std::cmp::min(min_val, std::cmp::min(from_node, to_node)));
        } else {
            min = Some(std::cmp::min(from_node, to_node));
        }
        if let Some(max_val) = max {
            max = Some(std::cmp::max(max_val, std::cmp::max(from_node, to_node)));
        } else {
            max = Some(std::cmp::max(from_node, to_node));
        }
    }

    if min.is_none() {
        // the degenerate case
        // only happens if the input iter is empty
        return Some(vec![]);
    }

    let min = min?; // we can ensure min is Some
    let max = max?; // same for max

    // generate the first set of nodes that have adjacencies:
    let mut leaves: Vec<_> = parent_of_relations
        .iter()
        .map(|edge| edge.1)
        .fold(all_nodes, |mut nodes, val| {
            nodes.remove(val);
            nodes
        })
        .into_iter()
        .collect();

    let mut res = vec![];

    let mut from_nodes = vec![];
    while let Some(node) = leaves.pop() {
        res.push(node);

        from_nodes.extend(
            parent_of_relations
                .range((node, min)..=(node, max))
                .map(|(_, from_node)| *from_node),
        );

        for from_node in from_nodes.drain(..) {
            parent_of_relations.remove(&(node, from_node));
            child_of_relations.remove(&(from_node, node));

            if child_of_relations
                .range((from_node, min)..=(from_node, max))
                .count()
                == 0
            {
                leaves.push(from_node)
            }
        }
    }

    if parent_of_relations.len() != 0 {
        None // some edges where not traversed despite all nodes with no
             // outbound edges having been removed. This means there must exist
             // at least one cycle in the remaining subgraph.
    } else {
        Some(res) // the topological ordering.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_sort_test() {
        let val = [(1, 2), (2, 3)].iter();
        let res = topological_sort(val);
        assert_eq!(res, Some(vec![&3, &2, &1]));

        let val = [(1, 2), (2, 3)].iter();
        let res = topological_sort(val);
        assert_eq!(res, Some(vec![&3, &2, &1]));

        let val = [(1, 2), (4, 3), (2, 3)].iter();
        let res = topological_sort(val);
        assert_eq!(res, Some(vec![&3, &4, &2, &1]));

        // directed diamond
        let val = [(1, 2), (5, 1), (5, 4), (4, 3), (2, 3)].iter();
        let res = topological_sort(val);
        assert_eq!(res, Some(vec![&3, &4, &2, &1, &5]));
    }
}
