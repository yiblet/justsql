use std::collections::BTreeSet;

/// returns a topologically sorted vector of input value references
/// returns None if the graph contains a cycle
pub fn topological_sort<
    'a,
    T: Ord + 'a,
    N: Iterator<Item = &'a T>,
    E: Iterator<Item = &'a (T, T)>,
>(
    nodes: N,
    edges: E,
) -> (Vec<&'a T>, Option<BTreeSet<&'a T>>) {
    let mut parent_of_relations = BTreeSet::new();
    let mut child_of_relations = BTreeSet::new();
    let mut all_nodes: BTreeSet<_> = nodes.collect();

    let mut min = None;
    let mut max = None;

    for (from_node, to_node) in edges {
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
        // only happens if there are no edges
        return (all_nodes.into_iter().collect(), None);
    }

    let min = min.unwrap(); // we can ensure min is Some
    let max = max.unwrap(); // same for max

    // generate the first set of nodes that have adjacencies:
    // goes through all u <- v relations, and removes v from the set of all nodes
    // leaving only the nodes that have no parents
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
        // some edges where not traversed despite all nodes with no
        // outbound edges having been removed. This means there must exist
        // at least one cycle in the remaining subgraph.

        let cycle = parent_of_relations
            .into_iter()
            .flat_map(|(v1, v2)| vec![v1, v2].into_iter())
            .chain(
                child_of_relations
                    .into_iter()
                    .flat_map(|(v1, v2)| vec![v1, v2].into_iter()),
            )
            .collect();
        return (res, Some(cycle));
    } else {
        return (res, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topological_sort_no_cycle_test() {
        let val = [(1, 2), (2, 3)].iter();
        let res = topological_sort([1, 2, 3].iter(), val);
        assert_eq!(res.0, (vec![&3, &2, &1]));

        let val = [(1, 2), (2, 3)].iter();
        let res = topological_sort([1, 2, 3].iter(), val);
        assert_eq!(res.0, (vec![&3, &2, &1]));

        let val = [(1, 2), (4, 3), (2, 3)].iter();
        let res = topological_sort([1, 2, 3, 4].iter(), val);
        assert_eq!(res.0, (vec![&3, &4, &2, &1]));

        // directed diamond
        let nodes = (1..=5).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (5, 1), (5, 4), (4, 3), (2, 3)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.0, (vec![&3, &4, &2, &1, &5]));

        let nodes = (1..=5).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (5, 1), (5, 4), (4, 3), (2, 3)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.0, (vec![&3, &4, &2, &1, &5]));
    }

    #[test]
    fn topological_sort_degenerate_test() {
        let nodes = (1..=4).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (2, 3)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.0, (vec![&4, &3, &2, &1]));

        let nodes = (1..=4).into_iter().collect::<Vec<_>>();
        let val = [].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.0, (vec![&1, &2, &3, &4]));
    }

    #[test]
    fn topological_sort_cycle_test() {
        let nodes = (1..=2).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (2, 1)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.1, Some([1, 2].iter().collect()));

        let nodes = (1..=3).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (2, 1), (2, 3)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.1, Some([1, 2].iter().collect()));

        let nodes = (1..=4).into_iter().collect::<Vec<_>>();
        let val = [(1, 2), (2, 3), (3, 1), (3, 4)].iter();
        let res = topological_sort(nodes.iter(), val);
        assert_eq!(res.1, Some([1, 2, 3].iter().collect()));
    }
}
