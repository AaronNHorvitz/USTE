use sha2::{Digest, Sha256};
use uste_policy::{Action, Target};
use uste_storage::{BlobId, BlobInventory, BlobReference};
use uste_txn::{ApplyError, AuthorizedTransactionState, TransactionState};
use uste_types::{
    CommitRevision, DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant,
};

use super::*;
use crate::research::{
    ArtifactInput, CitationInput, ClaimInput, EdgeInput, EdgeKind, FetchOutcome, Freshness,
    ProducerIdentity, ResearchRecord, RetainedContent, SourceKind, SourceRecordInput, SupportKind,
};
use crate::{SourceLocator, SourceVersionId};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn blob(seed: u8, length: u64) -> BlobReference {
    BlobReference::new(
        scope(),
        BlobId::from_bytes([seed; 16]),
        length,
        1,
        Sha256::digest([seed]).into(),
    )
    .unwrap()
}

fn source(source: u8, version: u32, length: Option<u64>) -> SourceRecordInput {
    SourceRecordInput {
        id: SourceVersionId {
            source: id(source),
            version,
        },
        kind: SourceKind::WebPage,
        locator_text: format!("https://example.invalid/{source}"),
        version_label: "unversioned".to_owned(),
        retrieved_at: UtcInstant::new(1_790_000_000, 0).unwrap(),
        run_identity: [3; 16],
        route_label: "fake-provider".to_owned(),
        outcome: match length {
            Some(_) => FetchOutcome::Complete,
            None => FetchOutcome::Inaccessible {
                reason: "fake 404".to_owned(),
            },
        },
        content: length.map(|length| RetainedContent {
            blob: blob(source.wrapping_mul(31).wrapping_add(version as u8), length),
            media_type: "text/html".to_owned(),
        }),
        license_label: "synthetic".to_owned(),
        redistributable: false,
        freshness: Freshness::MaxAge { seconds: 3_600 },
    }
}

fn claim(claim: u8, citation: Option<(u8, u32, u64)>, corrects: Option<u8>) -> ClaimInput {
    let excerpt_digest: [u8; 32] = Sha256::digest([claim]).into();
    ClaimInput {
        id: id(claim),
        subject: "subject".to_owned(),
        predicate: "states".to_owned(),
        value: format!("value {claim}"),
        support: if citation.is_some() {
            SupportKind::Paraphrase
        } else {
            SupportKind::Unsupported
        },
        citations: citation
            .map(|(source, version, end)| CitationInput {
                source: SourceVersionId {
                    source: id(source),
                    version,
                },
                locator: SourceLocator::ByteRange { start: 0, end },
                excerpt_digest,
                excerpt: None,
            })
            .into_iter()
            .collect(),
        valid_from: None,
        valid_until: None,
        corrects: corrects.map(id),
    }
}

fn artifact(artifact: u8, input: (u8, u32)) -> ArtifactInput {
    ArtifactInput {
        id: id(artifact),
        inputs: vec![SourceVersionId {
            source: id(input.0),
            version: input.1,
        }],
        producer: ProducerIdentity {
            name: "fake-extractor".to_owned(),
            revision: "1".to_owned(),
            configuration_digest: [0; 32],
        },
        content: None,
        complete_coverage: true,
        limitations: String::new(),
    }
}

fn edge(edge: u8, from: u8, to: u8, asserted_by: u8) -> EdgeInput {
    EdgeInput {
        id: id(edge),
        kind: EdgeKind::Supports,
        from: id(from),
        to: id(to),
        asserted_by: id(asserted_by),
        valid_from: None,
        valid_until: None,
    }
}

fn encode(generation: u64, mutation: ResearchMutation) -> Vec<u8> {
    encode_research_transaction(&ResearchTransaction {
        scope: scope(),
        generation,
        mutation,
    })
    .unwrap()
}

fn inventory_for(mutation: &ResearchMutation) -> Option<BlobInventory> {
    let content = match mutation {
        ResearchMutation::Put(record) => match &**record {
            ResearchRecord::Source(source) => source.content.as_ref(),
            ResearchRecord::Artifact(artifact) => artifact.content.as_ref(),
            _ => None,
        },
        _ => None,
    };
    content.map(|content| BlobInventory::new(scope(), [content.blob]).unwrap())
}

/// Minimal harness mirroring the coordinator: a failed prepare consumes no revision.
struct Harness {
    state: ResearchState,
    next: u64,
}

impl Harness {
    fn begun() -> Self {
        let mut harness = Self {
            state: ResearchState::new(scope()),
            next: 1,
        };
        harness
            .apply(ResearchMutation::BeginRebuild { next_generation: 1 })
            .unwrap();
        harness
    }

    fn apply(&mut self, mutation: ResearchMutation) -> Result<CommitRevision, ApplyError> {
        let inventory = inventory_for(&mutation);
        let bytes = encode(1, mutation);
        let revision = CommitRevision::new(self.next).unwrap();
        let prepared = self.state.prepare(&bytes, inventory.as_ref(), revision)?;
        assert_eq!(
            ResearchState::result_digest(&prepared),
            prepared.state_digest()
        );
        self.state.publish(prepared);
        self.next += 1;
        Ok(revision)
    }
}

fn put(record: ResearchRecord) -> ResearchMutation {
    ResearchMutation::Put(Box::new(record))
}

#[test]
fn transactions_round_trip_and_fail_closed() {
    let mutations = [
        ResearchMutation::BeginRebuild { next_generation: 2 },
        put(ResearchRecord::Source(source(10, 1, Some(64)))),
        put(ResearchRecord::Claim(claim(20, Some((10, 1, 8)), None))),
        ResearchMutation::RetractClaim { target: id(20) },
        ResearchMutation::ExpireClaim { target: id(21) },
        ResearchMutation::RevokeSource {
            source: SourceVersionId {
                source: id(10),
                version: 1,
            },
        },
        ResearchMutation::CompleteRebuild,
    ];
    for mutation in mutations {
        let bytes = encode(2, mutation.clone());
        let decoded = decode_research_transaction(&bytes).unwrap();
        assert_eq!(decoded.mutation, mutation);
        assert_eq!(decoded.generation, 2);
        for length in 0..bytes.len() {
            assert!(decode_research_transaction(&bytes[..length]).is_err());
        }
        let mut extended = bytes.clone();
        extended.push(0);
        assert!(decode_research_transaction(&extended).is_err());
    }
    let bytes = encode(1, ResearchMutation::CompleteRebuild);
    for (offset, value, expected) in [
        (0, b'X', ResearchCodecError::UnsupportedVersion),
        (4, 2, ResearchCodecError::UnsupportedVersion),
        (5, 1, ResearchCodecError::UnsupportedVersion),
        (6, 9, ResearchCodecError::UnsupportedVersion),
        (7, 1, ResearchCodecError::Invalid),
    ] {
        let mut changed = bytes.clone();
        changed[offset] = value;
        assert_eq!(decode_research_transaction(&changed), Err(expected));
    }
    // A record whose own scope differs from the transaction scope is refused.
    let mut foreign = encode(1, put(ResearchRecord::Edge(edge(40, 30, 31, 20))));
    let record_start = RESEARCH_TRANSACTION_HEADER_BYTES + 4;
    foreign[record_start + 24] ^= 1;
    assert_eq!(
        decode_research_transaction(&foreign),
        Err(ResearchCodecError::ScopeMismatch)
    );
}

#[test]
fn lifecycle_versions_corrections_and_revocation_are_exact() {
    let mut harness = Harness::begun();
    let first = harness
        .apply(put(ResearchRecord::Source(source(10, 1, Some(64)))))
        .unwrap();
    // Versions are sequential and a second version supersedes the first.
    assert_eq!(
        harness.apply(put(ResearchRecord::Source(source(10, 3, Some(64))))),
        Err(ApplyError::Conflict)
    );
    let second = harness
        .apply(put(ResearchRecord::Source(source(10, 2, None))))
        .unwrap();
    let sources = harness.state.sources();
    assert_eq!(
        sources[&SourceVersionId {
            source: id(10),
            version: 1
        }]
            .superseded_revision,
        Some(second)
    );
    assert_eq!(harness.state.retained_bytes(), 64);
    // Citations must name an existing, accessible, unrevoked version within its bytes.
    assert_eq!(
        harness.apply(put(ResearchRecord::Claim(claim(
            20,
            Some((10, 2, 8)),
            None
        )))),
        Err(ApplyError::Conflict)
    );
    assert_eq!(
        harness.apply(put(ResearchRecord::Claim(claim(
            20,
            Some((10, 1, 65)),
            None
        )))),
        Err(ApplyError::InvalidRequest)
    );
    let recorded = harness
        .apply(put(ResearchRecord::Claim(claim(
            20,
            Some((10, 1, 64)),
            None,
        ))))
        .unwrap();
    // Identities are unique across kinds.
    assert_eq!(
        harness.apply(put(ResearchRecord::Source(source(20, 1, None)))),
        Err(ApplyError::Conflict)
    );
    // Correction supersedes the active predecessor exactly once.
    let corrected = harness
        .apply(put(ResearchRecord::Claim(claim(
            21,
            Some((10, 1, 4)),
            Some(20),
        ))))
        .unwrap();
    assert_eq!(
        harness.state.claims()[&id(20)].status,
        ClaimStatus::Superseded(corrected)
    );
    assert_eq!(harness.state.claims()[&id(20)].recorded_revision, recorded);
    assert_eq!(
        harness.apply(put(ResearchRecord::Claim(claim(22, None, Some(20))))),
        Err(ApplyError::Conflict)
    );
    assert_eq!(
        harness.apply(ResearchMutation::RetractClaim { target: id(20) }),
        Err(ApplyError::Conflict)
    );
    let retracted = harness
        .apply(ResearchMutation::RetractClaim { target: id(21) })
        .unwrap();
    assert_eq!(
        harness.state.claims()[&id(21)].status,
        ClaimStatus::Retracted(retracted)
    );
    harness
        .apply(put(ResearchRecord::Claim(claim(23, None, None))))
        .unwrap();
    let expired = harness
        .apply(ResearchMutation::ExpireClaim { target: id(23) })
        .unwrap();
    assert_eq!(
        harness.state.claims()[&id(23)].status,
        ClaimStatus::Expired(expired)
    );
    // Artifacts and edges need their references; edges need a claim or artifact assertion.
    harness
        .apply(put(ResearchRecord::Artifact(artifact(30, (10, 1)))))
        .unwrap();
    assert_eq!(
        harness.apply(put(ResearchRecord::Edge(edge(40, 20, 99, 30)))),
        Err(ApplyError::Conflict)
    );
    assert_eq!(
        harness.apply(put(ResearchRecord::Edge(edge(40, 20, 21, 10)))),
        Err(ApplyError::Conflict)
    );
    harness
        .apply(put(ResearchRecord::Edge(edge(40, 20, 21, 30))))
        .unwrap();
    // Revocation is terminal and blocks new citations of that version.
    let revoked = harness
        .apply(ResearchMutation::RevokeSource {
            source: SourceVersionId {
                source: id(10),
                version: 1,
            },
        })
        .unwrap();
    assert_eq!(
        harness.state.sources()[&SourceVersionId {
            source: id(10),
            version: 1
        }]
            .revoked_revision,
        Some(revoked)
    );
    assert_eq!(
        harness.apply(put(ResearchRecord::Claim(claim(
            24,
            Some((10, 1, 4)),
            None
        )))),
        Err(ApplyError::Conflict)
    );
    assert_eq!(
        harness.apply(put(ResearchRecord::Artifact(artifact(31, (10, 1))))),
        Err(ApplyError::Conflict)
    );
    assert_eq!(first.get(), 2);
    harness.apply(ResearchMutation::CompleteRebuild).unwrap();
    assert!(harness.state.is_ready());
    assert_eq!(
        harness.apply(ResearchMutation::CompleteRebuild),
        Err(ApplyError::Conflict)
    );
}

#[test]
fn inventories_generations_and_scope_are_enforced() {
    let mut harness = Harness::begun();
    let with_content = encode(1, put(ResearchRecord::Source(source(10, 1, Some(8)))));
    let revision = CommitRevision::new(harness.next).unwrap();
    assert_eq!(
        harness
            .state
            .prepare(&with_content, None, revision)
            .unwrap_err(),
        ApplyError::InvalidRequest
    );
    let wrong = BlobInventory::new(scope(), [blob(200, 8)]).unwrap();
    assert_eq!(
        harness
            .state
            .prepare(&with_content, Some(&wrong), revision)
            .unwrap_err(),
        ApplyError::InvalidRequest
    );
    let claim_bytes = encode(1, put(ResearchRecord::Claim(claim(20, None, None))));
    assert_eq!(
        harness
            .state
            .prepare(&claim_bytes, Some(&wrong), revision)
            .unwrap_err(),
        ApplyError::InvalidRequest
    );
    let stale = encode(2, put(ResearchRecord::Claim(claim(20, None, None))));
    assert_eq!(
        harness.state.prepare(&stale, None, revision).unwrap_err(),
        ApplyError::SourceChanged
    );
    let foreign = ResearchState::new(NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([9; 16]),
    ));
    assert_eq!(
        foreign.prepare(&claim_bytes, None, revision).unwrap_err(),
        ApplyError::InvalidRequest
    );
    assert_eq!(
        harness
            .apply(ResearchMutation::BeginRebuild { next_generation: 3 })
            .unwrap_err(),
        ApplyError::Conflict
    );
    harness
        .apply(put(ResearchRecord::Claim(claim(20, None, None))))
        .unwrap();
    let before = harness.state.state_digest();
    // A rebuild clears derived state and starts the next generation only.
    let bytes = encode(2, ResearchMutation::BeginRebuild { next_generation: 2 });
    let rebuilt = harness
        .state
        .prepare(&bytes, None, CommitRevision::new(harness.next).unwrap())
        .unwrap();
    assert!(rebuilt.claims().is_empty());
    assert_eq!(rebuilt.generation(), Some(2));
    assert_ne!(rebuilt.state_digest(), before);
}

#[test]
fn edge_fan_out_is_bounded_per_record() {
    let mut harness = Harness::begun();
    harness
        .apply(put(ResearchRecord::Claim(claim(1, None, None))))
        .unwrap();
    let maximum = RESEARCH_PROFILE.maximum_edges_per_record;
    let mut next_id = 2_u16;
    let record = |value: u16| {
        let bytes = value.to_be_bytes();
        RecordRef::new(
            scope().database(),
            scope().namespace(),
            RecordId::from_bytes([bytes[0], bytes[1], 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7]),
        )
    };
    for _ in 0..maximum {
        let target = record(next_id);
        let mut target_claim = claim(0, None, None);
        target_claim.id = target;
        harness
            .apply(put(ResearchRecord::Claim(target_claim)))
            .unwrap();
        let mut link = edge(0, 1, 0, 1);
        link.id = record(next_id + 1);
        link.to = target;
        harness.apply(put(ResearchRecord::Edge(link))).unwrap();
        next_id += 2;
    }
    let target = record(next_id);
    let mut target_claim = claim(0, None, None);
    target_claim.id = target;
    harness
        .apply(put(ResearchRecord::Claim(target_claim)))
        .unwrap();
    let mut link = edge(0, 1, 0, 1);
    link.id = record(next_id + 1);
    link.to = target;
    assert_eq!(
        harness.apply(put(ResearchRecord::Edge(link))),
        Err(ApplyError::ResourceLimit)
    );
}

#[test]
fn authorization_requirements_cover_every_referenced_record() {
    let requirements = |mutation| {
        ResearchState::authorization_requirements(&encode(1, mutation), None)
            .unwrap()
            .iter()
            .map(|requirement| (requirement.action, requirement.target))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        requirements(ResearchMutation::CompleteRebuild),
        [(Action::ManageSchema, Target::Namespace(scope()))]
    );
    assert_eq!(
        requirements(put(ResearchRecord::Claim(claim(
            20,
            Some((10, 1, 4)),
            Some(19)
        )))),
        [
            (Action::Commit, Target::Record(id(20))),
            (Action::ReadRecord, Target::Record(id(10))),
            (Action::Commit, Target::Record(id(19))),
        ]
    );
    assert_eq!(
        requirements(put(ResearchRecord::Edge(edge(40, 20, 21, 30)))),
        [
            (Action::Commit, Target::Record(id(40))),
            (Action::ReadRecord, Target::Record(id(20))),
            (Action::ReadRecord, Target::Record(id(21))),
            (Action::ReadRecord, Target::Record(id(30))),
        ]
    );
    assert_eq!(
        requirements(put(ResearchRecord::Artifact(artifact(30, (10, 2))))),
        [
            (Action::Commit, Target::Record(id(30))),
            (Action::ReadRecord, Target::Record(id(10))),
        ]
    );
    assert_eq!(
        requirements(ResearchMutation::RevokeSource {
            source: SourceVersionId {
                source: id(10),
                version: 1
            }
        }),
        [(Action::ManageRetention, Target::Record(id(10)))]
    );
}

/// Independent reference model written directly from the specification's admission rules.
#[derive(Default)]
struct Model {
    /// (source, version, retained length or `None` when inaccessible, revoked, superseded)
    sources: Vec<(u8, u32, Option<u64>, bool, bool)>,
    /// (claim, status: 0 active, 1 superseded, 2 retracted, 3 expired)
    claims: Vec<(u8, u8)>,
    artifacts: Vec<u8>,
    edges: Vec<u8>,
}

enum Operation {
    Source(u8, Option<u64>),
    Claim(u8, Option<(u8, u32, u64)>, Option<u8>),
    Artifact(u8, (u8, u32)),
    Edge(u8, u8, u8, u8),
    Retract(u8),
    Expire(u8),
    Revoke(u8, u32),
}

impl Model {
    fn exists(&self, id: u8) -> bool {
        self.sources.iter().any(|entry| entry.0 == id)
            || self.claims.iter().any(|entry| entry.0 == id)
            || self.artifacts.contains(&id)
            || self.edges.contains(&id)
    }

    fn usable(&self, source: u8, version: u32) -> Option<Option<u64>> {
        self.sources
            .iter()
            .find(|entry| entry.0 == source && entry.1 == version && !entry.3)
            .map(|entry| entry.2)
    }

    fn apply(&mut self, operation: &Operation) -> Result<(), ApplyError> {
        match *operation {
            Operation::Source(source, length) => {
                let latest = self
                    .sources
                    .iter()
                    .filter(|entry| entry.0 == source)
                    .map(|entry| entry.1)
                    .max();
                if latest.is_none() && self.exists(source) {
                    return Err(ApplyError::Conflict);
                }
                for entry in &mut self.sources {
                    if entry.0 == source && Some(entry.1) == latest {
                        entry.4 = true;
                    }
                }
                self.sources
                    .push((source, latest.unwrap_or(0) + 1, length, false, false));
            }
            Operation::Claim(claim, citation, corrects) => {
                if self.exists(claim) {
                    return Err(ApplyError::Conflict);
                }
                if let Some((source, version, end)) = citation {
                    match self.usable(source, version) {
                        None | Some(None) => return Err(ApplyError::Conflict),
                        Some(Some(length)) if end > length => {
                            return Err(ApplyError::InvalidRequest);
                        }
                        Some(Some(_)) => {}
                    }
                }
                if let Some(predecessor) = corrects {
                    let entry = self
                        .claims
                        .iter_mut()
                        .find(|entry| entry.0 == predecessor && entry.1 == 0)
                        .ok_or(ApplyError::Conflict)?;
                    entry.1 = 1;
                }
                self.claims.push((claim, 0));
            }
            Operation::Artifact(artifact, (source, version)) => {
                if self.exists(artifact) || self.usable(source, version).is_none() {
                    return Err(ApplyError::Conflict);
                }
                self.artifacts.push(artifact);
            }
            Operation::Edge(edge, from, to, asserted_by) => {
                let asserts = self.claims.iter().any(|entry| entry.0 == asserted_by)
                    || self.artifacts.contains(&asserted_by);
                if self.exists(edge) || !self.exists(from) || !self.exists(to) || !asserts {
                    return Err(ApplyError::Conflict);
                }
                self.edges.push(edge);
            }
            Operation::Retract(claim) | Operation::Expire(claim) => {
                let status = if matches!(operation, Operation::Retract(_)) {
                    2
                } else {
                    3
                };
                let entry = self
                    .claims
                    .iter_mut()
                    .find(|entry| entry.0 == claim && entry.1 == 0)
                    .ok_or(ApplyError::Conflict)?;
                entry.1 = status;
            }
            Operation::Revoke(source, version) => {
                let entry = self
                    .sources
                    .iter_mut()
                    .find(|entry| entry.0 == source && entry.1 == version && !entry.3)
                    .ok_or(ApplyError::Conflict)?;
                entry.3 = true;
            }
        }
        Ok(())
    }

    fn matches(&self, state: &ResearchState) {
        assert_eq!(self.sources.len(), state.sources().len());
        for &(source, version, _, revoked, superseded) in &self.sources {
            let entry = &state.sources()[&SourceVersionId {
                source: id(source),
                version,
            }];
            assert_eq!(entry.revoked_revision.is_some(), revoked);
            assert_eq!(entry.superseded_revision.is_some(), superseded);
        }
        assert_eq!(self.claims.len(), state.claims().len());
        for &(claim, status) in &self.claims {
            let actual = match state.claims()[&id(claim)].status {
                ClaimStatus::Active => 0,
                ClaimStatus::Superseded(_) => 1,
                ClaimStatus::Retracted(_) => 2,
                ClaimStatus::Expired(_) => 3,
            };
            assert_eq!(actual, status);
        }
        assert_eq!(self.artifacts.len(), state.artifacts().len());
        assert_eq!(self.edges.len(), state.edges().len());
    }
}

fn operation(random: &mut u64) -> Operation {
    let mut next = || {
        *random ^= *random << 13;
        *random ^= *random >> 7;
        *random ^= *random << 17;
        *random
    };
    let small = |value: u64| u8::try_from(value % 12).unwrap() + 1;
    match next() % 7 {
        0 => Operation::Source(small(next()), (next() % 3 != 0).then_some(16 + next() % 32)),
        1 => {
            let claim = small(next());
            let citation = (next() % 4 != 0).then(|| {
                (
                    small(next()),
                    u32::try_from(next() % 3).unwrap() + 1,
                    1 + next() % 48,
                )
            });
            // A claim cannot correct itself; that is a codec-level refusal, not a reducer case.
            let corrects = (next() % 3 == 0)
                .then(|| small(next()))
                .filter(|predecessor| *predecessor != claim);
            Operation::Claim(claim, citation, corrects)
        }
        2 => Operation::Artifact(
            small(next()),
            (small(next()), u32::try_from(next() % 3).unwrap() + 1),
        ),
        3 => {
            let from = small(next());
            // Self-loops are refused by the codec; keep generated edges codec-valid.
            let to = from % 12 + 1;
            Operation::Edge(small(next()), from, to, small(next()))
        }
        4 => Operation::Retract(small(next())),
        5 => Operation::Expire(small(next())),
        _ => Operation::Revoke(small(next()), u32::try_from(next() % 3).unwrap() + 1),
    }
}

fn mutation(operation: &Operation) -> ResearchMutation {
    match *operation {
        // The harness assigns the next version before submission.
        Operation::Source(source_id, length) => {
            put(ResearchRecord::Source(source(source_id, 0, length)))
        }
        Operation::Claim(claim_id, citation, corrects) => {
            put(ResearchRecord::Claim(claim(claim_id, citation, corrects)))
        }
        Operation::Artifact(artifact_id, input) => {
            put(ResearchRecord::Artifact(artifact(artifact_id, input)))
        }
        Operation::Edge(edge_id, from, to, asserted_by) => {
            put(ResearchRecord::Edge(edge(edge_id, from, to, asserted_by)))
        }
        Operation::Retract(claim_id) => ResearchMutation::RetractClaim {
            target: id(claim_id),
        },
        Operation::Expire(claim_id) => ResearchMutation::ExpireClaim {
            target: id(claim_id),
        },
        Operation::Revoke(source_id, version) => ResearchMutation::RevokeSource {
            source: SourceVersionId {
                source: id(source_id),
                version,
            },
        },
    }
}

#[test]
fn reducer_matches_independent_reference_model_and_replays_identically() {
    let mut accepted_total = 0;
    for seed in 1..=40_u64 {
        let mut random = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut harness = Harness::begun();
        let mut model = Model::default();
        let mut accepted = Vec::new();
        for _ in 0..200 {
            let operation = operation(&mut random);
            let mut mutation = mutation(&operation);
            // Sources are always offered as the next version for the model's own view; the
            // model's reference outcome decides whether that is an identity conflict.
            if let ResearchMutation::Put(record) = &mut mutation
                && let ResearchRecord::Source(source) = &mut **record
            {
                let latest = harness
                    .state
                    .sources()
                    .keys()
                    .filter(|key| key.source == source.id.source)
                    .map(|key| key.version)
                    .max();
                source.id.version = latest.unwrap_or(0) + 1;
                if let Some(content) = &mut source.content {
                    content.blob = blob(
                        u8::try_from(harness.next % 251).unwrap(),
                        content.blob.byte_len(),
                    );
                }
            }
            let expected = model.apply(&operation);
            let actual = harness.apply(mutation.clone()).map(|_| ());
            assert_eq!(actual, expected, "seed {seed}");
            if actual.is_ok() {
                accepted.push(mutation);
            }
            model.matches(&harness.state);
        }
        accepted_total += accepted.len();
        // Replaying exactly the accepted transactions from empty state reproduces the digest.
        let mut replay = Harness::begun();
        for mutation in accepted {
            replay.apply(mutation).unwrap();
        }
        assert_eq!(replay.state, harness.state);
        assert_eq!(replay.state.state_digest(), harness.state.state_digest());
    }
    assert!(accepted_total > 1_000, "{accepted_total}");
}
