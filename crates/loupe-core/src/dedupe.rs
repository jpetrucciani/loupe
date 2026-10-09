//! Connected components and deterministic representatives for finite input.

use crate::error::{Error, ErrorKind, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::str::FromStr;

pub mod streaming;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Keep {
    #[default]
    First,
    Last,
    Longest,
}
impl FromStr for Keep {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "first" => Ok(Self::First),
            "last" => Ok(Self::Last),
            "longest" => Ok(Self::Longest),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "dedupe --keep must be first, last or longest",
            )),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub index: u64,
    pub characters: usize,
}
#[derive(Debug, Serialize)]
pub struct Cluster {
    pub id: u64,
    pub representative: u64,
    pub members: Vec<u64>,
}
pub struct Forest {
    members: Vec<Member>,
    positions: BTreeMap<u64, usize>,
    parents: Vec<usize>,
    ranks: Vec<usize>,
}
impl Forest {
    pub fn new(members: Vec<Member>) -> Result<Self> {
        if members.iter().any(|member| member.index == 0)
            || members
                .windows(2)
                .any(|pair| pair[0].index >= pair[1].index)
        {
            return Err(Error::new(
                ErrorKind::Input,
                "dedupe members require increasing positive source indices",
            ));
        }
        let positions = members
            .iter()
            .enumerate()
            .map(|(position, member)| (member.index, position))
            .collect();
        let count = members.len();
        Ok(Self {
            members,
            positions,
            parents: (0..count).collect(),
            ranks: vec![0; count],
        })
    }
    pub fn connect(&mut self, left: u64, right: u64) -> Result<bool> {
        let left = *self
            .positions
            .get(&left)
            .ok_or_else(|| invalid("unknown left dedupe member"))?;
        let right = *self
            .positions
            .get(&right)
            .ok_or_else(|| invalid("unknown right dedupe member"))?;
        let mut left = self.root(left)?;
        let mut right = self.root(right)?;
        if left == right {
            return Ok(false);
        }
        let left_rank = *self
            .ranks
            .get(left)
            .ok_or_else(|| invalid("invalid dedupe rank"))?;
        let right_rank = *self
            .ranks
            .get(right)
            .ok_or_else(|| invalid("invalid dedupe rank"))?;
        if left_rank < right_rank {
            std::mem::swap(&mut left, &mut right);
        }
        *self
            .parents
            .get_mut(right)
            .ok_or_else(|| invalid("invalid dedupe parent"))? = left;
        if left_rank == right_rank {
            let rank = self
                .ranks
                .get_mut(left)
                .ok_or_else(|| invalid("invalid dedupe rank"))?;
            *rank = rank
                .checked_add(1)
                .ok_or_else(|| invalid("dedupe rank overflow"))?;
        }
        Ok(true)
    }
    pub fn clusters(&mut self, keep: Keep) -> Result<Vec<Cluster>> {
        let mut groups = BTreeMap::<usize, Vec<Member>>::new();
        for position in 0..self.members.len() {
            let root = self.root(position)?;
            let member = *self
                .members
                .get(position)
                .ok_or_else(|| invalid("dedupe member missing"))?;
            groups.entry(root).or_default().push(member);
        }
        let mut clusters = Vec::with_capacity(groups.len());
        for members in groups.into_values() {
            let first = members
                .first()
                .ok_or_else(|| invalid("empty dedupe component"))?;
            let representative = match keep {
                Keep::First => first,
                Keep::Last => members
                    .last()
                    .ok_or_else(|| invalid("empty dedupe component"))?,
                Keep::Longest => members
                    .iter()
                    .max_by(|left, right| {
                        left.characters
                            .cmp(&right.characters)
                            .then_with(|| right.index.cmp(&left.index))
                    })
                    .ok_or_else(|| invalid("empty dedupe component"))?,
            };
            clusters.push(Cluster {
                id: first.index,
                representative: representative.index,
                members: members.iter().map(|member| member.index).collect(),
            });
        }
        clusters.sort_by_key(|cluster| cluster.representative);
        Ok(clusters)
    }
    fn root(&mut self, position: usize) -> Result<usize> {
        let mut root = position;
        loop {
            let parent = *self
                .parents
                .get(root)
                .ok_or_else(|| invalid("dedupe node outside forest"))?;
            if parent == root {
                break;
            }
            root = parent;
        }
        let mut cursor = position;
        while cursor != root {
            let parent = self
                .parents
                .get_mut(cursor)
                .ok_or_else(|| invalid("dedupe node outside forest"))?;
            let next = *parent;
            *parent = root;
            cursor = next;
        }
        Ok(root)
    }
}
pub fn pair_count(members: usize) -> u128 {
    (members as u128) * (members.saturating_sub(1) as u128) / 2
}
fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::Input, message)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::dedupe::{Forest, Keep, Member, pair_count};
    fn forest() -> Forest {
        Forest::new(vec![
            Member {
                index: 1,
                characters: 10,
            },
            Member {
                index: 3,
                characters: 30,
            },
            Member {
                index: 7,
                characters: 20,
            },
            Member {
                index: 9,
                characters: 40,
            },
        ])
        .unwrap()
    }
    #[test]
    fn bridge_edges_merge_full_components_and_all_representatives_are_source_ordered() {
        let mut forest = forest();
        assert!(forest.connect(1, 3).unwrap());
        assert!(forest.connect(3, 7).unwrap());
        assert!(!forest.connect(1, 7).unwrap());
        for (keep, expected) in [
            (Keep::First, vec![1, 9]),
            (Keep::Last, vec![7, 9]),
            (Keep::Longest, vec![3, 9]),
        ] {
            let clusters = forest.clusters(keep).unwrap();
            assert_eq!(
                clusters
                    .iter()
                    .map(|cluster| cluster.representative)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(clusters[0].members, vec![1, 3, 7]);
            assert_eq!(clusters[0].id, 1);
        }
    }
    #[test]
    fn edge_order_reversal_duplicate_edges_and_longest_ties_keep_the_same_partition() {
        let mut left = forest();
        let mut right = forest();
        for (a, b) in [(1, 3), (7, 9), (3, 7), (1, 9)] {
            left.connect(a, b).unwrap();
        }
        for (a, b) in [(9, 1), (7, 3), (9, 7), (3, 1), (3, 1)] {
            right.connect(a, b).unwrap();
        }
        assert_eq!(
            serde_json::to_value(left.clusters(Keep::Longest).unwrap()).unwrap(),
            serde_json::to_value(right.clusters(Keep::Longest).unwrap()).unwrap()
        );
        let mut tied = Forest::new(vec![
            Member {
                index: 1,
                characters: 4,
            },
            Member {
                index: 2,
                characters: 4,
            },
        ])
        .unwrap();
        tied.connect(1, 2).unwrap();
        assert_eq!(tied.clusters(Keep::Longest).unwrap()[0].representative, 1);
    }
    #[test]
    fn invalid_sources_empty_singleton_and_quadratic_pair_counts_are_explicit() {
        assert!(
            Forest::new(vec![Member {
                index: 0,
                characters: 0
            }])
            .is_err()
        );
        assert!(
            Forest::new(vec![
                Member {
                    index: 2,
                    characters: 0
                },
                Member {
                    index: 1,
                    characters: 0
                }
            ])
            .is_err()
        );
        assert!(
            Forest::new(vec![])
                .unwrap()
                .clusters(Keep::First)
                .unwrap()
                .is_empty()
        );
        let mut forest = forest();
        assert!(forest.connect(1, 99).is_err());
        assert_eq!(forest.clusters(Keep::First).unwrap().len(), 4);
        assert_eq!(pair_count(0), 0);
        assert_eq!(pair_count(1), 0);
        assert_eq!(pair_count(3), 3);
        assert_eq!(pair_count(1000), 499500);
    }
}
