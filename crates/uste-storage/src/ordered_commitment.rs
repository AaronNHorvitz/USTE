//! Canonical ordered content commitments for a separately versioned future disk index.
//!
//! These pure, storage-free primitives neither authorize access nor admit caller-supplied roots
//! as canonical state. Callers require an independently trusted canonical base. No v1 hash or
//! persisted format uses this module. See Decision 0128.
use sha2::{Digest, Sha256};
use uste_types::NamespaceRef;

pub const MAX_KEY_BYTES: usize = 4096;
pub const MAX_VALUE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ENTRIES: u64 = 1_000_000_000;
pub const MAX_BRANCH_BITS: u32 = 9 * MAX_KEY_BYTES as u32;
pub const MAX_PROOF_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const NODE_DOMAIN: &[u8] = b"USTE-ORDERED-COMMITMENT-V1\0";
const VALUE_DOMAIN: &[u8] = b"USTE-ORDERED-COMMITMENT-VALUE-V1\0";

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitmentError {
    InvalidProof,
    Conflict,
    ResourceLimit,
}

#[derive(Clone, Copy)]
pub struct CommitmentContext {
    scope: NamespaceRef,
    profile: [u8; 32],
    family: u8,
}

impl CommitmentContext {
    pub fn new(
        scope: NamespaceRef,
        profile: [u8; 32],
        family: u8,
    ) -> Result<Self, CommitmentError> {
        if family == 0 {
            return Err(CommitmentError::InvalidProof);
        }
        Ok(Self {
            scope,
            profile,
            family,
        })
    }

    fn hasher(self, kind: u8) -> Sha256 {
        let mut hash = Sha256::new();
        hash.update(NODE_DOMAIN);
        hash.update(self.scope.database().as_bytes());
        hash.update(self.scope.namespace().as_bytes());
        hash.update(self.profile);
        hash.update([self.family, kind]);
        hash
    }
}

/// Logical content only; never a physical root or an authorization capability.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct OrderedCommitment {
    entries: u64,
    logical_bytes: u64,
    digest: [u8; 32],
}

impl core::fmt::Debug for OrderedCommitment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OrderedCommitment")
            .field("entries", &self.entries)
            .field("logical_bytes", &self.logical_bytes)
            .field("digest", &"[REDACTED]")
            .finish()
    }
}

impl OrderedCommitment {
    pub fn entries(self) -> u64 {
        self.entries
    }
    pub fn logical_bytes(self) -> u64 {
        self.logical_bytes
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    fn feed(self, hash: &mut Sha256) {
        hash.update(self.entries.to_be_bytes());
        hash.update(self.logical_bytes.to_be_bytes());
        hash.update(self.digest);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct ValueCommitment {
    pub length: u64,
    pub digest: [u8; 32],
}

/// Borrowed proof input. Deserializers must enforce their own bounds before allocating it.
#[derive(Clone, Copy)]
pub struct LeafProof<'a> {
    pub key: &'a [u8],
    pub value: ValueCommitment,
}

#[derive(Clone, Copy)]
pub struct BranchProof {
    pub bit: u32,
    pub sibling: OrderedCommitment,
}

#[derive(Clone, Copy)]
pub struct LookupProof<'a> {
    pub leaf: Option<LeafProof<'a>>,
    pub branches: &'a [BranchProof],
}

#[derive(Clone, Copy)]
pub struct CommitmentLimits {
    pub maximum_branches: u32,
    pub maximum_input_bytes: u64,
}

fn key_valid(key: &[u8]) -> Result<(), CommitmentError> {
    if key.is_empty() {
        return Err(CommitmentError::InvalidProof);
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(CommitmentError::ResourceLimit);
    }
    Ok(())
}

pub fn value_commitment(value: &[u8]) -> Result<ValueCommitment, CommitmentError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(CommitmentError::ResourceLimit);
    }
    let length = value.len() as u64;
    let mut hash = Sha256::new();
    hash.update(VALUE_DOMAIN);
    hash.update(length.to_be_bytes());
    hash.update(value);
    Ok(ValueCommitment {
        length,
        digest: hash.finalize().into(),
    })
}

pub fn empty_commitment(context: CommitmentContext) -> OrderedCommitment {
    OrderedCommitment {
        entries: 0,
        logical_bytes: 0,
        digest: context.hasher(0).finalize().into(),
    }
}

pub fn leaf_commitment(
    context: CommitmentContext,
    leaf: LeafProof<'_>,
) -> Result<OrderedCommitment, CommitmentError> {
    key_valid(leaf.key)?;
    if leaf.value.length > MAX_VALUE_BYTES as u64 {
        return Err(CommitmentError::ResourceLimit);
    }
    let mut hash = context.hasher(1);
    hash.update((leaf.key.len() as u32).to_be_bytes());
    hash.update(leaf.key);
    hash.update(leaf.value.length.to_be_bytes());
    hash.update(leaf.value.digest);
    Ok(OrderedCommitment {
        entries: 1,
        logical_bytes: leaf.key.len() as u64 + leaf.value.length,
        digest: hash.finalize().into(),
    })
}

/// Low-level hash composition, not proof that two arbitrary subtrees form a canonical branch.
pub fn branch_commitment(
    context: CommitmentContext,
    bit: u32,
    left: OrderedCommitment,
    right: OrderedCommitment,
) -> Result<OrderedCommitment, CommitmentError> {
    if bit >= MAX_BRANCH_BITS || left.entries == 0 || right.entries == 0 {
        return Err(CommitmentError::InvalidProof);
    }
    let entries = left
        .entries
        .checked_add(right.entries)
        .filter(|n| *n <= MAX_ENTRIES)
        .ok_or(CommitmentError::ResourceLimit)?;
    let logical_bytes = left
        .logical_bytes
        .checked_add(right.logical_bytes)
        .ok_or(CommitmentError::ResourceLimit)?;
    let mut hash = context.hasher(2);
    hash.update(bit.to_be_bytes());
    left.feed(&mut hash);
    right.feed(&mut hash);
    Ok(OrderedCommitment {
        entries,
        logical_bytes,
        digest: hash.finalize().into(),
    })
}

fn bit(key: &[u8], position: u32) -> bool {
    let byte = position as usize / 9;
    let offset = position % 9;
    key.get(byte)
        .is_some_and(|value| offset == 0 || value & (1 << (8 - offset)) != 0)
}

fn first_difference(a: &[u8], b: &[u8]) -> Option<u32> {
    for (index, (left, right)) in a.iter().zip(b).enumerate() {
        if left != right {
            return Some(index as u32 * 9 + 1 + (left ^ right).leading_zeros());
        }
    }
    (a.len() != b.len()).then_some(a.len().min(b.len()) as u32 * 9)
}

fn fold(
    context: CommitmentContext,
    key: &[u8],
    mut child: OrderedCommitment,
    path: &[BranchProof],
) -> Result<OrderedCommitment, CommitmentError> {
    for step in path.iter().rev() {
        child = if bit(key, step.bit) {
            branch_commitment(context, step.bit, step.sibling, child)?
        } else {
            branch_commitment(context, step.bit, child, step.sibling)?
        };
    }
    Ok(child)
}

fn admit(
    key: &[u8],
    proof: LookupProof<'_>,
    limits: CommitmentLimits,
    extra_bytes: u64,
) -> Result<(), CommitmentError> {
    key_valid(key)?;
    if limits.maximum_branches > MAX_BRANCH_BITS
        || proof.branches.len() > limits.maximum_branches as usize
        || limits.maximum_input_bytes > MAX_PROOF_INPUT_BYTES
    {
        return Err(CommitmentError::ResourceLimit);
    }
    let mut bytes = (proof.branches.len() as u64)
        .checked_mul(52)
        .and_then(|n| n.checked_add(key.len() as u64))
        .and_then(|n| n.checked_add(extra_bytes))
        .ok_or(CommitmentError::ResourceLimit)?;
    if let Some(leaf) = proof.leaf {
        key_valid(leaf.key)?;
        if leaf.value.length > MAX_VALUE_BYTES as u64 {
            return Err(CommitmentError::ResourceLimit);
        }
        bytes = bytes
            .checked_add(leaf.key.len() as u64 + 40)
            .ok_or(CommitmentError::ResourceLimit)?;
    }
    if bytes > limits.maximum_input_bytes {
        return Err(CommitmentError::ResourceLimit);
    }
    Ok(())
}

fn verify(
    context: CommitmentContext,
    expected: OrderedCommitment,
    key: &[u8],
    proof: LookupProof<'_>,
) -> Result<Option<ValueCommitment>, CommitmentError> {
    let Some(leaf) = proof.leaf else {
        return if proof.branches.is_empty() && expected == empty_commitment(context) {
            Ok(None)
        } else {
            Err(CommitmentError::InvalidProof)
        };
    };
    let mut previous = None;
    for step in proof.branches {
        if previous.is_some_and(|prior| prior >= step.bit)
            || step.bit > leaf.key.len() as u32 * 9
            || bit(key, step.bit) != bit(leaf.key, step.bit)
        {
            return Err(CommitmentError::InvalidProof);
        }
        previous = Some(step.bit);
    }
    if fold(
        context,
        key,
        leaf_commitment(context, leaf)?,
        proof.branches,
    )? != expected
    {
        return Err(CommitmentError::InvalidProof);
    }
    Ok((leaf.key == key).then_some(leaf.value))
}

/// Verify against an independently trusted canonical root; absence is distinct from invalid proof.
pub fn verify_lookup(
    context: CommitmentContext,
    expected: OrderedCommitment,
    key: &[u8],
    proof: LookupProof<'_>,
    limits: CommitmentLimits,
) -> Result<Option<ValueCommitment>, CommitmentError> {
    admit(key, proof, limits, 0)?;
    verify(context, expected, key, proof)
}

/// Compute only a new logical root, with no I/O, allocations, publication or caller-state mutation.
/// The before-value is an exact compare-and-swap precondition, not a hint for rebuilding a root.
#[allow(clippy::too_many_arguments)]
pub fn apply_delta(
    context: CommitmentContext,
    expected: OrderedCommitment,
    key: &[u8],
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    proof: LookupProof<'_>,
    limits: CommitmentLimits,
) -> Result<OrderedCommitment, CommitmentError> {
    if before.is_some_and(|value| value.len() > MAX_VALUE_BYTES)
        || after.is_some_and(|value| value.len() > MAX_VALUE_BYTES)
    {
        return Err(CommitmentError::ResourceLimit);
    }
    let bytes = before.map_or(0, |v| v.len() as u64) + after.map_or(0, |v| v.len() as u64);
    admit(key, proof, limits, bytes)?;
    let found = verify(context, expected, key, proof)?;
    if found != before.map(value_commitment).transpose()? {
        return Err(CommitmentError::Conflict);
    }
    let Some(value) = after else {
        if found.is_none() {
            return Ok(expected);
        }
        return match proof.branches.split_last() {
            None => Ok(empty_commitment(context)),
            Some((last, parents)) => fold(context, key, last.sibling, parents),
        };
    };
    let new = leaf_commitment(
        context,
        LeafProof {
            key,
            value: value_commitment(value)?,
        },
    )?;
    let Some(old_leaf) = proof.leaf else {
        return Ok(new);
    };
    if found.is_some() {
        return fold(context, key, new, proof.branches);
    }
    let split = first_difference(key, old_leaf.key).ok_or(CommitmentError::InvalidProof)?;
    let at = proof.branches.partition_point(|step| step.bit < split);
    if proof.branches.get(at).is_some_and(|step| step.bit == split) {
        return Err(CommitmentError::InvalidProof);
    }
    let old = fold(
        context,
        key,
        leaf_commitment(context, old_leaf)?,
        &proof.branches[at..],
    )?;
    let split_root = if bit(key, split) {
        branch_commitment(context, split, old, new)?
    } else {
        branch_commitment(context, split, new, old)?
    };
    fold(context, key, split_root, &proof.branches[..at])
}
