//! Test-only sorted-key reconstruction, with separately encoded hash frames and explicit bits.
use super::*;

pub enum Tree {
    Empty(OrderedCommitment),
    Leaf {
        key: Vec<u8>,
        value: ValueCommitment,
        root: OrderedCommitment,
    },
    Branch {
        bit: usize,
        left: Box<Tree>,
        right: Box<Tree>,
        root: OrderedCommitment,
    },
}

fn bits(key: &[u8]) -> Vec<bool> {
    let mut out = Vec::new();
    for byte in key {
        out.push(true);
        for shift in (0..8).rev() {
            out.push((byte >> shift) & 1 != 0);
        }
    }
    out.push(false);
    out
}
fn prefix(c: CommitmentContext, kind: u8) -> Vec<u8> {
    let mut frame = b"USTE-ORDERED-COMMITMENT-V1\0".to_vec();
    frame.extend(c.scope.database().as_bytes());
    frame.extend(c.scope.namespace().as_bytes());
    frame.extend(c.profile);
    frame.extend([c.family, kind]);
    frame
}
fn summary_bytes(root: OrderedCommitment) -> Vec<u8> {
    let mut out = root.entries.to_be_bytes().to_vec();
    out.extend(root.logical_bytes.to_be_bytes());
    out.extend(root.digest);
    out
}

impl Tree {
    pub fn build(c: CommitmentContext, map: &BTreeMap<Vec<u8>, Vec<u8>>) -> Self {
        Self::sorted(c, &map.iter().collect::<Vec<_>>())
    }
    fn sorted(c: CommitmentContext, entries: &[(&Vec<u8>, &Vec<u8>)]) -> Self {
        if entries.is_empty() {
            return Self::Empty(OrderedCommitment {
                entries: 0,
                logical_bytes: 0,
                digest: Sha256::digest(prefix(c, 0)).into(),
            });
        }
        if entries.len() == 1 {
            let (key, bytes) = entries[0];
            let mut value = b"USTE-ORDERED-COMMITMENT-VALUE-V1\0".to_vec();
            value.extend((bytes.len() as u64).to_be_bytes());
            value.extend(bytes);
            let value = ValueCommitment {
                length: bytes.len() as u64,
                digest: Sha256::digest(value).into(),
            };
            let mut frame = prefix(c, 1);
            frame.extend((key.len() as u32).to_be_bytes());
            frame.extend(key);
            frame.extend(value.length.to_be_bytes());
            frame.extend(value.digest);
            return Self::Leaf {
                key: key.clone(),
                value,
                root: OrderedCommitment {
                    entries: 1,
                    logical_bytes: (key.len() + bytes.len()) as u64,
                    digest: Sha256::digest(frame).into(),
                },
            };
        }
        let left_bits = bits(entries[0].0);
        let right_bits = bits(entries[entries.len() - 1].0);
        let bit = (0..left_bits.len().max(right_bits.len()))
            .find(|i| {
                left_bits.get(*i).copied().unwrap_or(false)
                    != right_bits.get(*i).copied().unwrap_or(false)
            })
            .unwrap();
        let split = entries
            .iter()
            .position(|(key, _)| bits(key).get(bit).copied().unwrap_or(false))
            .unwrap();
        assert!(split > 0);
        let left = Box::new(Self::sorted(c, &entries[..split]));
        let right = Box::new(Self::sorted(c, &entries[split..]));
        let mut frame = prefix(c, 2);
        frame.extend((bit as u32).to_be_bytes());
        frame.extend(summary_bytes(left.root()));
        frame.extend(summary_bytes(right.root()));
        let root = OrderedCommitment {
            entries: left.root().entries + right.root().entries,
            logical_bytes: left.root().logical_bytes + right.root().logical_bytes,
            digest: Sha256::digest(frame).into(),
        };
        Self::Branch {
            bit,
            left,
            right,
            root,
        }
    }
    pub fn root(&self) -> OrderedCommitment {
        match self {
            Self::Empty(root) | Self::Leaf { root, .. } | Self::Branch { root, .. } => *root,
        }
    }
    pub fn proof(&self, key: &[u8]) -> (Option<LeafProof<'_>>, Vec<BranchProof>) {
        let query = bits(key);
        let mut node = self;
        let mut path = Vec::new();
        loop {
            match node {
                Self::Empty(_) => return (None, path),
                Self::Leaf { key, value, .. } => {
                    return (Some(LeafProof { key, value: *value }), path);
                }
                Self::Branch {
                    bit, left, right, ..
                } => {
                    let direction = query.get(*bit).copied().unwrap_or(false);
                    path.push(BranchProof {
                        bit: *bit as u32,
                        sibling: if direction { left.root() } else { right.root() },
                    });
                    node = if direction { right } else { left };
                }
            }
        }
    }
}
