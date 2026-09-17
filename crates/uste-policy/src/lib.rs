//! Storage-independent, default-deny authorization and quota policy for trusted USTE adapters.

#![forbid(unsafe_code)]

use core::{fmt, num::NonZeroU64};
use std::{collections::BTreeMap, sync::Arc};

use uste_types::{NamespaceRef, RecordId, RecordRef};

pub const MAX_PRINCIPALS_PER_NAMESPACE: usize = 65_536;
pub const MAX_RECORD_RULES_PER_GRANT: usize = 100_000;
pub const MAX_AUTHORIZATION_REQUIREMENTS_PER_REQUEST: usize = 100_000;
pub const HARD_MAX_REQUEST_BYTES: u64 = 16 * 1024 * 1024;
pub const HARD_MAX_STAGED_BLOB_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const HARD_MAX_COMMITTED_BLOB_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
pub const HARD_MAX_LIVE_UPLOADS: u32 = 32;
pub const HARD_MAX_BLOB_READ_BYTES: u32 = 1024 * 1024;

/// Stable digest produced by a trusted authentication adapter. It is not a credential.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PrincipalDigest([u8; 32]);

impl PrincipalDigest {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Debug for PrincipalDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PrincipalDigest([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PolicyVersion(NonZeroU64);

impl PolicyVersion {
    pub const fn new(value: u64) -> Result<Self, PolicyError> {
        match NonZeroU64::new(value) {
            Some(value) => Ok(Self(value)),
            None => Err(PolicyError::InvalidPolicy),
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// Independently authorized operation classes. Permissions never imply another class.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Action {
    ReadRecord,
    ReadHistory,
    ExpandGraph,
    Search,
    Aggregate,
    ReadBlob,
    Export,
    Commit,
    StartUpload,
    ResumeUpload,
    WriteUpload,
    FinishUpload,
    AbortUpload,
    ReadOwnOutcome,
    ReadAllOutcomes,
    InspectQuota,
    EvaluateBranch,
    Subscribe,
    ManagePolicy,
    ManageSchema,
    Import,
    Promote,
    ManageRetention,
    ManageKeys,
    ExecuteProcedure,
}

const ACTION_COUNT: u32 = Action::ExecuteProcedure as u32 + 1;
const VALID_PERMISSION_BITS: u64 = (1_u64 << ACTION_COUNT) - 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PermissionSet(u64);

impl PermissionSet {
    #[must_use]
    pub fn from_actions(actions: impl IntoIterator<Item = Action>) -> Self {
        let mut bits = 0_u64;
        for action in actions {
            bits |= 1_u64 << u32::from(action as u8);
        }
        Self(bits)
    }

    #[must_use]
    pub const fn contains(self, action: Action) -> bool {
        self.0 & (1_u64 << action as u8) != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn from_bits(bits: u64) -> Result<Self, PolicyError> {
        if bits & !VALID_PERMISSION_BITS == 0 {
            Ok(Self(bits))
        } else {
            Err(PolicyError::InvalidPolicy)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaLimits {
    max_request_bytes: u64,
    max_staged_blob_bytes: u64,
    max_committed_blob_bytes: u64,
    max_live_uploads: u32,
    max_blob_read_bytes_per_call: u32,
}

impl QuotaLimits {
    pub const fn new(
        max_request_bytes: u64,
        max_staged_blob_bytes: u64,
        max_committed_blob_bytes: u64,
        max_live_uploads: u32,
        max_blob_read_bytes_per_call: u32,
    ) -> Result<Self, PolicyError> {
        let limits = Self {
            max_request_bytes,
            max_staged_blob_bytes,
            max_committed_blob_bytes,
            max_live_uploads,
            max_blob_read_bytes_per_call,
        };
        if limits.within_hard_caps() {
            Ok(limits)
        } else {
            Err(PolicyError::InvalidPolicy)
        }
    }

    const fn within_hard_caps(self) -> bool {
        self.max_request_bytes <= HARD_MAX_REQUEST_BYTES
            && self.max_staged_blob_bytes <= HARD_MAX_STAGED_BLOB_BYTES
            && self.max_committed_blob_bytes <= HARD_MAX_COMMITTED_BLOB_BYTES
            && self.max_live_uploads <= HARD_MAX_LIVE_UPLOADS
            && self.max_blob_read_bytes_per_call <= HARD_MAX_BLOB_READ_BYTES
    }

    #[must_use]
    pub const fn min(self, other: Self) -> Self {
        Self {
            max_request_bytes: if self.max_request_bytes < other.max_request_bytes {
                self.max_request_bytes
            } else {
                other.max_request_bytes
            },
            max_staged_blob_bytes: if self.max_staged_blob_bytes < other.max_staged_blob_bytes {
                self.max_staged_blob_bytes
            } else {
                other.max_staged_blob_bytes
            },
            max_committed_blob_bytes: if self.max_committed_blob_bytes
                < other.max_committed_blob_bytes
            {
                self.max_committed_blob_bytes
            } else {
                other.max_committed_blob_bytes
            },
            max_live_uploads: if self.max_live_uploads < other.max_live_uploads {
                self.max_live_uploads
            } else {
                other.max_live_uploads
            },
            max_blob_read_bytes_per_call: if self.max_blob_read_bytes_per_call
                < other.max_blob_read_bytes_per_call
            {
                self.max_blob_read_bytes_per_call
            } else {
                other.max_blob_read_bytes_per_call
            },
        }
    }

    #[must_use]
    pub const fn max_request_bytes(self) -> u64 {
        self.max_request_bytes
    }

    #[must_use]
    pub const fn max_staged_blob_bytes(self) -> u64 {
        self.max_staged_blob_bytes
    }

    #[must_use]
    pub const fn max_committed_blob_bytes(self) -> u64 {
        self.max_committed_blob_bytes
    }

    #[must_use]
    pub const fn max_live_uploads(self) -> u32 {
        self.max_live_uploads
    }

    #[must_use]
    pub const fn max_blob_read_bytes_per_call(self) -> u32 {
        self.max_blob_read_bytes_per_call
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordRule {
    pub deny: PermissionSet,
}

#[derive(Clone, Eq, PartialEq)]
pub struct NamespaceGrant {
    permissions: PermissionSet,
    quotas: QuotaLimits,
    record_rules: BTreeMap<RecordId, RecordRule>,
}

impl fmt::Debug for NamespaceGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NamespaceGrant")
            .field("permissions", &self.permissions)
            .field("quotas", &self.quotas)
            .field("record_rule_count", &self.record_rules.len())
            .finish()
    }
}

impl NamespaceGrant {
    #[must_use]
    pub const fn new(permissions: PermissionSet, quotas: QuotaLimits) -> Self {
        Self {
            permissions,
            quotas,
            record_rules: BTreeMap::new(),
        }
    }

    pub fn deny_record(
        &mut self,
        record: RecordId,
        deny: PermissionSet,
    ) -> Result<(), PolicyError> {
        if !self.record_rules.contains_key(&record)
            && self.record_rules.len() >= MAX_RECORD_RULES_PER_GRANT
        {
            return Err(PolicyError::ResourceLimit);
        }
        self.record_rules.insert(record, RecordRule { deny });
        Ok(())
    }

    #[must_use]
    pub const fn permissions(&self) -> PermissionSet {
        self.permissions
    }

    #[must_use]
    pub const fn quotas(&self) -> QuotaLimits {
        self.quotas
    }

    pub fn record_rules(&self) -> impl ExactSizeIterator<Item = (RecordId, PermissionSet)> + '_ {
        self.record_rules
            .iter()
            .map(|(record, rule)| (*record, rule.deny))
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct NamespacePolicy {
    scope: NamespaceRef,
    version: PolicyVersion,
    quotas: QuotaLimits,
    grants: BTreeMap<PrincipalDigest, NamespaceGrant>,
}

impl fmt::Debug for NamespacePolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NamespacePolicy")
            .field("scope", &"[REDACTED]")
            .field("version", &self.version)
            .field("quotas", &self.quotas)
            .field("grant_count", &self.grants.len())
            .finish()
    }
}

impl NamespacePolicy {
    #[must_use]
    pub const fn new(scope: NamespaceRef, version: PolicyVersion, quotas: QuotaLimits) -> Self {
        Self {
            scope,
            version,
            quotas,
            grants: BTreeMap::new(),
        }
    }

    pub fn grant(
        &mut self,
        principal: PrincipalDigest,
        grant: NamespaceGrant,
    ) -> Result<(), PolicyError> {
        if !self.grants.contains_key(&principal)
            && self.grants.len() >= MAX_PRINCIPALS_PER_NAMESPACE
        {
            return Err(PolicyError::ResourceLimit);
        }
        if grant.quotas.min(self.quotas) != grant.quotas {
            return Err(PolicyError::InvalidPolicy);
        }
        self.grants.insert(principal, grant);
        Ok(())
    }

    #[must_use]
    pub const fn scope(&self) -> NamespaceRef {
        self.scope
    }

    #[must_use]
    pub const fn version(&self) -> PolicyVersion {
        self.version
    }

    #[must_use]
    pub const fn quotas(&self) -> QuotaLimits {
        self.quotas
    }

    pub fn grants(&self) -> impl ExactSizeIterator<Item = (PrincipalDigest, &NamespaceGrant)> + '_ {
        self.grants
            .iter()
            .map(|(principal, grant)| (*principal, grant))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Namespace(NamespaceRef),
    Record(RecordRef),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationRequirement {
    pub action: Action,
    pub target: Target,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthorizationRequirements(Vec<AuthorizationRequirement>);

impl AuthorizationRequirements {
    pub fn new(
        requirements: impl IntoIterator<Item = AuthorizationRequirement>,
    ) -> Result<Self, PolicyError> {
        let mut bounded = Vec::new();
        for requirement in requirements {
            if bounded.len() == MAX_AUTHORIZATION_REQUIREMENTS_PER_REQUEST {
                return Err(PolicyError::ResourceLimit);
            }
            bounded
                .try_reserve(1)
                .map_err(|_| PolicyError::ResourceLimit)?;
            bounded.push(requirement);
        }
        Ok(Self(bounded))
    }

    pub fn iter(&self) -> impl Iterator<Item = AuthorizationRequirement> + '_ {
        self.0.iter().copied()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Target {
    #[must_use]
    pub const fn scope(self) -> NamespaceRef {
        match self {
            Self::Namespace(scope) => scope,
            Self::Record(record) => NamespaceRef::new(record.database(), record.namespace()),
        }
    }
}

pub trait TrustedPrincipalAdapter {
    type Credential;

    fn authenticate(
        &mut self,
        credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticationError;

struct PolicyAuthority;

#[derive(Clone)]
pub struct AuthenticatedPrincipal {
    digest: PrincipalDigest,
    authority: Arc<PolicyAuthority>,
}

impl AuthenticatedPrincipal {
    #[must_use]
    pub const fn digest(&self) -> PrincipalDigest {
        self.digest
    }
}

impl fmt::Debug for AuthenticatedPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticatedPrincipal([REDACTED])")
    }
}

impl PartialEq for AuthenticatedPrincipal {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest && Arc::ptr_eq(&self.authority, &other.authority)
    }
}

impl Eq for AuthenticatedPrincipal {}

#[derive(Clone)]
pub struct AuthorizationLease {
    principal: PrincipalDigest,
    scope: NamespaceRef,
    action: Action,
    version: PolicyVersion,
    authority: Arc<PolicyAuthority>,
}

impl fmt::Debug for AuthorizationLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationLease([REDACTED])")
    }
}

impl PartialEq for AuthorizationLease {
    fn eq(&self, other: &Self) -> bool {
        self.principal == other.principal
            && self.scope == other.scope
            && self.action == other.action
            && self.version == other.version
            && Arc::ptr_eq(&self.authority, &other.authority)
    }
}

impl Eq for AuthorizationLease {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
    Unauthorized,
    ResourceLimit,
    InvalidPolicy,
    StalePolicy,
}

#[derive(Clone)]
pub struct PolicyKernel {
    policies: BTreeMap<NamespaceRef, NamespacePolicy>,
    authority: Arc<PolicyAuthority>,
}

impl fmt::Debug for PolicyKernel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PolicyKernel")
            .field("namespace_count", &self.policies.len())
            .finish()
    }
}

impl PolicyKernel {
    #[must_use]
    pub fn new() -> Self {
        Self {
            policies: BTreeMap::new(),
            authority: Arc::new(PolicyAuthority),
        }
    }

    pub fn authenticate<A: TrustedPrincipalAdapter>(
        &self,
        adapter: &mut A,
        credential: &A::Credential,
    ) -> Result<AuthenticatedPrincipal, AuthenticationError> {
        adapter
            .authenticate(credential)
            .map(|digest| AuthenticatedPrincipal {
                digest,
                authority: Arc::clone(&self.authority),
            })
    }

    /// Install the trusted adapter's current policy while creating/opening a coordinator.
    pub fn install_initial_policy(&mut self, policy: NamespacePolicy) -> Result<(), PolicyError> {
        if self.policies.contains_key(&policy.scope) {
            return Err(PolicyError::InvalidPolicy);
        }
        self.policies.insert(policy.scope, policy);
        Ok(())
    }

    pub fn authorize(
        &self,
        principal: &AuthenticatedPrincipal,
        action: Action,
        target: Target,
    ) -> Result<AuthorizationLease, PolicyError> {
        if !Arc::ptr_eq(&self.authority, &principal.authority) {
            return Err(PolicyError::Unauthorized);
        }
        let scope = target.scope();
        let policy = self.policies.get(&scope).ok_or(PolicyError::Unauthorized)?;
        let grant = policy
            .grants
            .get(&principal.digest)
            .ok_or(PolicyError::Unauthorized)?;
        if !grant.permissions.contains(action) {
            return Err(PolicyError::Unauthorized);
        }
        if let Target::Record(record) = target
            && grant
                .record_rules
                .get(&record.record())
                .is_some_and(|rule| rule.deny.contains(action))
        {
            return Err(PolicyError::Unauthorized);
        }
        Ok(AuthorizationLease {
            principal: principal.digest,
            scope,
            action,
            version: policy.version,
            authority: Arc::clone(&self.authority),
        })
    }

    pub fn revalidate(&self, lease: &AuthorizationLease) -> Result<(), PolicyError> {
        if !Arc::ptr_eq(&self.authority, &lease.authority) {
            return Err(PolicyError::Unauthorized);
        }
        let policy = self
            .policies
            .get(&lease.scope)
            .ok_or(PolicyError::Unauthorized)?;
        if policy.version != lease.version {
            return Err(PolicyError::StalePolicy);
        }
        let grant = policy
            .grants
            .get(&lease.principal)
            .ok_or(PolicyError::Unauthorized)?;
        if !grant.permissions.contains(lease.action) {
            return Err(PolicyError::Unauthorized);
        }
        Ok(())
    }

    pub fn quotas(
        &self,
        principal: &AuthenticatedPrincipal,
        scope: NamespaceRef,
    ) -> Result<QuotaLimits, PolicyError> {
        if !Arc::ptr_eq(&self.authority, &principal.authority) {
            return Err(PolicyError::Unauthorized);
        }
        let policy = self.policies.get(&scope).ok_or(PolicyError::Unauthorized)?;
        let grant = policy
            .grants
            .get(&principal.digest)
            .ok_or(PolicyError::Unauthorized)?;
        Ok(policy.quotas.min(grant.quotas))
    }

    /// Trusted coordinator access to namespace-wide admission limits.
    pub fn namespace_quotas(&self, scope: NamespaceRef) -> Result<QuotaLimits, PolicyError> {
        self.policies
            .get(&scope)
            .map(|policy| policy.quotas)
            .ok_or(PolicyError::Unauthorized)
    }

    #[must_use]
    pub fn namespace_policy(&self, scope: NamespaceRef) -> Option<&NamespacePolicy> {
        self.policies.get(&scope)
    }

    pub fn replace_namespace_policy(
        &mut self,
        authority: &AuthenticatedPrincipal,
        expected: PolicyVersion,
        next: NamespacePolicy,
    ) -> Result<(), PolicyError> {
        if !Arc::ptr_eq(&self.authority, &authority.authority) {
            return Err(PolicyError::Unauthorized);
        }
        let current = self
            .policies
            .get(&next.scope)
            .ok_or(PolicyError::Unauthorized)?;
        if current.version != expected || next.version <= current.version {
            return Err(PolicyError::StalePolicy);
        }
        let grant = current
            .grants
            .get(&authority.digest)
            .ok_or(PolicyError::Unauthorized)?;
        if !grant.permissions.contains(Action::ManagePolicy) {
            return Err(PolicyError::Unauthorized);
        }
        self.policies.insert(next.scope, next);
        Ok(())
    }
}

impl Default for PolicyKernel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uste_types::{DatabaseId, NamespaceId};

    struct Adapter;

    impl TrustedPrincipalAdapter for Adapter {
        type Credential = u8;

        fn authenticate(
            &mut self,
            credential: &Self::Credential,
        ) -> Result<PrincipalDigest, AuthenticationError> {
            Ok(PrincipalDigest::from_bytes([*credential; 32]))
        }
    }

    fn scope(value: u8) -> NamespaceRef {
        NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([value; 16]),
        )
    }

    fn quotas() -> QuotaLimits {
        QuotaLimits::new(1024, 2048, 4096, 2, 512).unwrap()
    }

    #[test]
    fn default_deny_distinct_actions_record_narrowing_and_redaction() {
        let mut kernel = PolicyKernel::new();
        let alice = kernel.authenticate(&mut Adapter, &7).unwrap();
        assert_eq!(
            kernel.authorize(&alice, Action::ReadRecord, Target::Namespace(scope(2))),
            Err(PolicyError::Unauthorized)
        );
        let mut grant = NamespaceGrant::new(
            PermissionSet::from_actions([Action::ReadRecord, Action::ReadBlob]),
            quotas(),
        );
        let denied = RecordId::from_bytes([9; 16]);
        grant
            .deny_record(denied, PermissionSet::from_actions([Action::ReadRecord]))
            .unwrap();
        let mut policy = NamespacePolicy::new(scope(2), PolicyVersion::new(1).unwrap(), quotas());
        policy.grant(alice.digest(), grant).unwrap();
        kernel.install_initial_policy(policy).unwrap();
        assert!(
            kernel
                .authorize(&alice, Action::ReadBlob, Target::Namespace(scope(2)))
                .is_ok()
        );
        assert_eq!(
            kernel.authorize(&alice, Action::ReadHistory, Target::Namespace(scope(2))),
            Err(PolicyError::Unauthorized)
        );
        assert_eq!(
            kernel.authorize(
                &alice,
                Action::ReadRecord,
                Target::Record(RecordRef::new(
                    scope(2).database(),
                    scope(2).namespace(),
                    denied,
                )),
            ),
            Err(PolicyError::Unauthorized)
        );
        assert!(!format!("{alice:?}").contains('7'));
    }

    #[test]
    fn replacement_invalidates_leases_and_cannot_raise_hard_caps() {
        assert_eq!(
            QuotaLimits::new(HARD_MAX_REQUEST_BYTES + 1, 1, 1, 1, 1),
            Err(PolicyError::InvalidPolicy)
        );
        let mut kernel = PolicyKernel::new();
        let admin = kernel.authenticate(&mut Adapter, &3).unwrap();
        let permissions = PermissionSet::from_actions([Action::ReadBlob, Action::ManagePolicy]);
        let mut first = NamespacePolicy::new(scope(4), PolicyVersion::new(1).unwrap(), quotas());
        first
            .grant(admin.digest(), NamespaceGrant::new(permissions, quotas()))
            .unwrap();
        kernel.install_initial_policy(first).unwrap();
        let lease = kernel
            .authorize(&admin, Action::ReadBlob, Target::Namespace(scope(4)))
            .unwrap();
        let mut second = NamespacePolicy::new(scope(4), PolicyVersion::new(2).unwrap(), quotas());
        second
            .grant(admin.digest(), NamespaceGrant::new(permissions, quotas()))
            .unwrap();
        kernel
            .replace_namespace_policy(&admin, PolicyVersion::new(1).unwrap(), second)
            .unwrap();
        assert_eq!(kernel.revalidate(&lease), Err(PolicyError::StalePolicy));
    }

    #[test]
    fn authenticated_principal_is_bound_to_the_issuing_kernel() {
        let mut first = PolicyKernel::new();
        let mut second = PolicyKernel::new();
        for kernel in [&mut first, &mut second] {
            let mut namespace =
                NamespacePolicy::new(scope(6), PolicyVersion::new(1).unwrap(), quotas());
            namespace
                .grant(
                    PrincipalDigest::from_bytes([8; 32]),
                    NamespaceGrant::new(
                        PermissionSet::from_actions([Action::ReadRecord]),
                        quotas(),
                    ),
                )
                .unwrap();
            kernel.install_initial_policy(namespace).unwrap();
        }
        let forged_for_first = second.authenticate(&mut Adapter, &8).unwrap();
        assert_eq!(
            first.authorize(
                &forged_for_first,
                Action::ReadRecord,
                Target::Namespace(scope(6)),
            ),
            Err(PolicyError::Unauthorized)
        );
    }

    #[test]
    fn authorization_requirements_reject_the_first_entry_over_the_bound() {
        let requirement = AuthorizationRequirement {
            action: Action::ExpandGraph,
            target: Target::Namespace(scope(5)),
        };
        let exact = AuthorizationRequirements::new(core::iter::repeat_n(
            requirement,
            MAX_AUTHORIZATION_REQUIREMENTS_PER_REQUEST,
        ))
        .unwrap();
        assert_eq!(
            exact.iter().count(),
            MAX_AUTHORIZATION_REQUIREMENTS_PER_REQUEST
        );
        assert_eq!(
            AuthorizationRequirements::new(core::iter::repeat_n(
                requirement,
                MAX_AUTHORIZATION_REQUIREMENTS_PER_REQUEST + 1,
            )),
            Err(PolicyError::ResourceLimit)
        );
    }
}
