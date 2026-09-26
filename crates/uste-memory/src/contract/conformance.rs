//! Synthetic conformance fixtures for `uste-memory-consumer` 1.0.
//!
//! [`run_conformance`] drives any [`MemoryConsumerContract`] implementation through a fixed,
//! ordered scenario and checks every outcome. [`fixture_digest`] pins that scenario: step names,
//! expected outcomes and the canonical bytes of every fixture record. All data is synthetic.

use sha2::{Digest, Sha256};
use uste_policy::{NamespacePolicy, PolicyVersion};
use uste_txn::{Cancellation, NeverCancel};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant};

use super::{
    ContractContent, ContractError, ContractSource, ContractVersion, ContractWrite,
    MEMORY_CONTRACT_NAME, MEMORY_CONTRACT_VERSION, MemoryConsumerContract, OperationId,
    negotiate_contract, source_record,
};
use crate::research::{
    CitationInput, ClaimInput, ClaimStatus, EdgeInput, EdgeKind, EffectiveSupport, FetchOutcome,
    Freshness, FreshnessState, ResearchKnowledge, ResearchReadOutput, ResearchReadRequest,
    ResearchRecord, SourceKind, SupportKind, encode_research_record,
};
use crate::{SourceLocator, SourceVersionId};

/// Pinned SHA-256 of the `uste-memory-consumer` 1.0 fixture ([`fixture_digest`]); the candidate
/// contract revision recorded in Decision 0282.
pub const CONFORMANCE_FIXTURE_DIGEST: &str =
    "b145b45ebb5668947ffb160a95a5aee8d93364bc4879f7428f31a97a196ef034";

/// Record identity whose `ReadRecord` the consumer-supplied restricted policy must deny.
pub const CONFORMANCE_DENIED_SOURCE: [u8; 16] = [0xA2; 16];

const RETRIEVED_AT: i64 = 1_790_000_000;
const FIRST_TEXT: &str = "conformance source one states the cache is derived";
const SECOND_TEXT: &str = "conformance source two is restricted";

/// Every step, in order, with its expected outcome label. Changing either changes the digest.
pub const CONFORMANCE_STEPS: &[(&str, &str)] = &[
    ("negotiate-current-version", "ok"),
    ("negotiate-future-minor", "unsupported-version"),
    ("negotiate-other-major", "unsupported-version"),
    ("negotiate-other-name", "unsupported-version"),
    ("read-before-any-generation", "stale-generation"),
    ("begin-generation-1", "ok"),
    ("read-while-rebuilding", "rebuilding"),
    ("put-source-1", "ok"),
    ("retry-put-source-1", "same-receipt"),
    ("inaccessible-source-with-bytes", "invalid-request"),
    ("put-source-2", "ok"),
    ("put-claim-1", "ok"),
    ("citation-beyond-source", "invalid-request"),
    ("put-claim-3", "ok"),
    ("complete-generation-1", "ok"),
    ("get-claim-1", "ok-direct-quote-fresh"),
    ("search-claim-1", "ok-one-result"),
    ("correct-claim-1", "ok"),
    ("read-through-old-view", "stale-view"),
    ("get-superseded-claim-1", "ok-superseded"),
    ("put-edge", "ok"),
    ("edges-from-claim-2", "ok-one-edge"),
    ("revoke-source-1", "ok"),
    ("get-revoked-claim-2", "ok-revoked-excerpt-withheld"),
    ("cancelled-write", "cancelled"),
    ("cancelled-write-left-no-record", "not-found"),
    ("foreign-scope-search", "unauthorized"),
    ("zero-result-budget", "resource-limit"),
    ("replace-policy", "ok-no-revision"),
    ("read-after-policy-change", "stale-policy"),
    ("get-claim-3-denied-source", "ok-no-visible-citation"),
    ("begin-generation-2", "ok"),
    ("read-old-generation", "stale-generation"),
    ("read-new-generation", "rebuilding"),
];

/// Consumer-supplied inputs the fixture cannot fabricate.
pub struct ConformanceInputs {
    /// The namespace the implementation under test owns.
    pub scope: NamespaceRef,
    /// A namespace the principal has no grant in.
    pub foreign_scope: NamespaceRef,
    /// The current policy version and a replacement that keeps every permission of the current
    /// grant but denies `ReadRecord` on [`CONFORMANCE_DENIED_SOURCE`].
    pub restricted_policy: (PolicyVersion, NamespacePolicy),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceFailure {
    pub step: &'static str,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceReport {
    pub contract: ContractVersion,
    pub steps_passed: usize,
    pub fixture_digest: [u8; 32],
}

fn record(scope: NamespaceRef, bytes: [u8; 16]) -> RecordRef {
    RecordRef::new(
        scope.database(),
        scope.namespace(),
        RecordId::from_bytes(bytes),
    )
}

fn at(seconds: i64) -> UtcInstant {
    UtcInstant::new(seconds, 0).unwrap_or_else(|_| unreachable!("fixture instants are in range"))
}

fn operation(sequence: u8) -> OperationId {
    OperationId {
        idempotency_key: [sequence; 16],
        transaction_id: [sequence.wrapping_add(0x80); 16],
    }
}

fn source_id(scope: NamespaceRef, bytes: [u8; 16]) -> SourceVersionId {
    SourceVersionId {
        source: record(scope, bytes),
        version: 1,
    }
}

fn fixture_source(scope: NamespaceRef, bytes: [u8; 16], text: &str) -> ContractSource {
    ContractSource {
        id: source_id(scope, bytes),
        kind: SourceKind::DocPackPage,
        locator_text: "conformance/guide.html".to_owned(),
        version_label: "synthetic-1".to_owned(),
        retrieved_at: at(RETRIEVED_AT),
        run_identity: [0x42; 16],
        route_label: "conformance-fixture".to_owned(),
        outcome: FetchOutcome::Complete,
        license_label: "synthetic".to_owned(),
        redistributable: true,
        freshness: Freshness::MaxAge { seconds: 3_600 },
        content: Some(ContractContent {
            media_type: "text/plain; charset=utf-8".to_owned(),
            bytes: text.as_bytes().to_vec(),
        }),
    }
}

fn quote(
    scope: NamespaceRef,
    claim: u8,
    source: [u8; 16],
    text: &str,
    start: usize,
    end: usize,
    corrects: Option<u8>,
) -> ClaimInput {
    let excerpt = &text[start..end];
    ClaimInput {
        id: record(scope, [claim; 16]),
        subject: "conformance".to_owned(),
        predicate: "states".to_owned(),
        value: excerpt.to_owned(),
        support: SupportKind::DirectQuote,
        citations: vec![CitationInput {
            source: source_id(scope, source),
            locator: SourceLocator::ByteRange {
                start: start as u64,
                end: end as u64,
            },
            excerpt_digest: Sha256::digest(excerpt.as_bytes()).into(),
            excerpt: Some(excerpt.to_owned()),
        }],
        valid_from: None,
        valid_until: None,
        corrects: corrects.map(|value| record(scope, [value; 16])),
    }
}

struct Fixture {
    first: ContractSource,
    second: ContractSource,
    claim_one: ClaimInput,
    claim_two: ClaimInput,
    claim_three: ClaimInput,
    beyond: ClaimInput,
    cancelled: ClaimInput,
    edge: EdgeInput,
}

impl Fixture {
    fn new(scope: NamespaceRef) -> Self {
        let one = [0xA1; 16];
        let two = CONFORMANCE_DENIED_SOURCE;
        let mut beyond = quote(scope, 0xC9, one, FIRST_TEXT, 0, 4, None);
        beyond.citations[0].locator = SourceLocator::ByteRange {
            start: 0,
            end: FIRST_TEXT.len() as u64 + 1,
        };
        beyond.citations[0].excerpt = None;
        Self {
            first: fixture_source(scope, one, FIRST_TEXT),
            second: fixture_source(scope, two, SECOND_TEXT),
            // "the cache is derived"
            claim_one: quote(scope, 0xC1, one, FIRST_TEXT, 30, 50, None),
            // "source one states"
            claim_two: quote(scope, 0xC2, one, FIRST_TEXT, 12, 29, Some(0xC1)),
            claim_three: quote(scope, 0xC3, two, SECOND_TEXT, 23, 36, None),
            beyond,
            cancelled: quote(scope, 0xC4, one, FIRST_TEXT, 0, 11, None),
            edge: EdgeInput {
                id: record(scope, [0xE1; 16]),
                kind: EdgeKind::Corrects,
                from: record(scope, [0xC2; 16]),
                to: record(scope, [0xC1; 16]),
                asserted_by: record(scope, [0xC2; 16]),
                valid_from: None,
                valid_until: None,
            },
        }
    }
}

/// SHA-256 pinning the fixture: step names and expectations, then the canonical research-record
/// bytes of every fixture record at a fixed synthetic scope, with source content bound by digest.
#[must_use]
pub fn fixture_digest() -> [u8; 32] {
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes([0x55; 16]),
        NamespaceId::from_bytes([0x66; 16]),
    );
    let fixture = Fixture::new(scope);
    let mut digest = Sha256::new();
    digest.update(MEMORY_CONTRACT_NAME.as_bytes());
    digest.update(MEMORY_CONTRACT_VERSION.major.to_be_bytes());
    digest.update(MEMORY_CONTRACT_VERSION.minor.to_be_bytes());
    for (name, expected) in CONFORMANCE_STEPS {
        for part in [name, expected] {
            digest.update(u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
            digest.update(part.as_bytes());
        }
    }
    let mut records = Vec::new();
    for source in [&fixture.first, &fixture.second] {
        records.push(ResearchRecord::Source(source_record(source, None)));
        let content = source.content.as_ref();
        digest.update(content.map_or(&[][..], |content| content.media_type.as_bytes()));
        digest.update(Sha256::digest(
            content.map_or(&[][..], |content| &content.bytes[..]),
        ));
    }
    for claim in [
        &fixture.claim_one,
        &fixture.claim_two,
        &fixture.claim_three,
        &fixture.beyond,
        &fixture.cancelled,
    ] {
        records.push(ResearchRecord::Claim(claim.clone()));
    }
    records.push(ResearchRecord::Edge(fixture.edge.clone()));
    for record in &records {
        // The beyond-source claim is valid as a record; its refusal is a reducer rule. Sources
        // are encoded without their producer-assigned blob, which a complete source requires,
        // so their descriptive fields are hashed through a content-free inaccessible form.
        let bytes = match record {
            ResearchRecord::Source(source) => {
                let mut described = source.clone();
                described.outcome = FetchOutcome::Inaccessible {
                    reason: "content bound by digest".to_owned(),
                };
                described.content = None;
                encode_research_record(scope, &ResearchRecord::Source(described))
            }
            other => encode_research_record(scope, other),
        }
        .unwrap_or_default();
        digest.update(u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
        digest.update(&bytes);
    }
    digest.finalize().into()
}

struct Cancelled;

impl Cancellation for Cancelled {
    fn is_cancelled(&self) -> bool {
        true
    }
}

struct Runner {
    executed: usize,
}

impl Runner {
    fn check(
        &mut self,
        step: &'static str,
        outcome: Result<(), String>,
    ) -> Result<(), ConformanceFailure> {
        let expected = CONFORMANCE_STEPS.get(self.executed).map(|(name, _)| *name);
        if expected != Some(step) {
            return Err(ConformanceFailure {
                step,
                detail: format!("fixture order mismatch; expected {expected:?}"),
            });
        }
        outcome.map_err(|detail| ConformanceFailure { step, detail })?;
        self.executed += 1;
        Ok(())
    }
}

fn expect_error<T: core::fmt::Debug>(
    actual: Result<T, ContractError>,
    expected: ContractError,
) -> Result<(), String> {
    match actual {
        Err(error) if error == expected => Ok(()),
        other => Err(format!("expected {expected:?}, got {other:?}")),
    }
}

fn ok<T: core::fmt::Debug>(actual: Result<T, ContractError>) -> Result<T, String> {
    actual.map_err(|error| format!("unexpected {error:?}"))
}

fn get_claim(generation: u64, claim: RecordRef) -> ResearchReadRequest {
    ResearchReadRequest::GetClaim {
        authority_generation: generation,
        claim,
        knowledge: ResearchKnowledge::Current,
        evaluated_at: at(RETRIEVED_AT + 60),
        maximum_output_bytes: 64 * 1024,
    }
}

fn claim_view(
    output: Result<ResearchReadOutput, ContractError>,
) -> Result<crate::research::ClaimView, String> {
    match ok(output)? {
        ResearchReadOutput::Claim(view) => Ok(view),
        other => Err(format!("expected a claim, got {other:?}")),
    }
}

/// Run the complete synthetic scenario against `contract`, which must own `inputs.scope` with a
/// fresh, empty store.
pub fn run_conformance<C: MemoryConsumerContract>(
    contract: &mut C,
    inputs: ConformanceInputs,
) -> Result<ConformanceReport, ConformanceFailure> {
    let scope = inputs.scope;
    let fixture = Fixture::new(scope);
    let never = NeverCancel;
    let mut run = Runner { executed: 0 };
    let current = ContractVersion {
        major: MEMORY_CONTRACT_VERSION.major,
        minor: MEMORY_CONTRACT_VERSION.minor,
    };

    run.check(
        "negotiate-current-version",
        match negotiate_contract(MEMORY_CONTRACT_NAME, current) {
            Ok(version) if version == contract.contract_version() => Ok(()),
            other => Err(format!("{other:?}")),
        },
    )?;
    run.check(
        "negotiate-future-minor",
        expect_error(
            negotiate_contract(
                MEMORY_CONTRACT_NAME,
                ContractVersion {
                    minor: current.minor + 1,
                    ..current
                },
            ),
            ContractError::UnsupportedVersion,
        ),
    )?;
    run.check(
        "negotiate-other-major",
        expect_error(
            negotiate_contract(
                MEMORY_CONTRACT_NAME,
                ContractVersion {
                    major: current.major + 1,
                    minor: 0,
                },
            ),
            ContractError::UnsupportedVersion,
        ),
    )?;
    run.check(
        "negotiate-other-name",
        expect_error(
            negotiate_contract("another-contract", current),
            ContractError::UnsupportedVersion,
        ),
    )?;

    let claim_one = fixture.claim_one.id;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "read-before-any-generation",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "read-before-any-generation",
        expect_error(
            contract.read(&view, &get_claim(1, claim_one), &never),
            ContractError::StaleGeneration,
        ),
    )?;
    run.check(
        "begin-generation-1",
        ok(contract.write(
            operation(1),
            &ContractWrite::BeginGeneration { next_generation: 1 },
            &never,
        ))
        .map(drop),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "read-while-rebuilding",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "read-while-rebuilding",
        expect_error(
            contract.read(&view, &get_claim(1, claim_one), &never),
            ContractError::Rebuilding,
        ),
    )?;
    let put_first = ContractWrite::PutSource {
        generation: 1,
        source: fixture.first.clone(),
    };
    let first_receipt = ok(contract.write(operation(2), &put_first, &never));
    run.check("put-source-1", first_receipt.clone().map(drop))?;
    run.check(
        "retry-put-source-1",
        match (
            contract.write(operation(2), &put_first, &never),
            first_receipt,
        ) {
            (Ok(retry), Ok(original)) if retry == original => Ok(()),
            (retry, original) => Err(format!("retry {retry:?} original {original:?}")),
        },
    )?;
    let mut inaccessible = fixture.second.clone();
    inaccessible.outcome = FetchOutcome::Inaccessible {
        reason: "fixture unavailable".to_owned(),
    };
    run.check(
        "inaccessible-source-with-bytes",
        expect_error(
            contract.write(
                operation(3),
                &ContractWrite::PutSource {
                    generation: 1,
                    source: inaccessible,
                },
                &never,
            ),
            ContractError::InvalidRequest,
        ),
    )?;
    for (step, sequence, request) in [
        (
            "put-source-2",
            4,
            ContractWrite::PutSource {
                generation: 1,
                source: fixture.second.clone(),
            },
        ),
        (
            "put-claim-1",
            5,
            ContractWrite::PutClaim {
                generation: 1,
                claim: fixture.claim_one.clone(),
            },
        ),
    ] {
        run.check(
            step,
            ok(contract.write(operation(sequence), &request, &never)).map(drop),
        )?;
    }
    run.check(
        "citation-beyond-source",
        expect_error(
            contract.write(
                operation(6),
                &ContractWrite::PutClaim {
                    generation: 1,
                    claim: fixture.beyond.clone(),
                },
                &never,
            ),
            ContractError::InvalidRequest,
        ),
    )?;
    for (step, sequence, request) in [
        (
            "put-claim-3",
            7,
            ContractWrite::PutClaim {
                generation: 1,
                claim: fixture.claim_three.clone(),
            },
        ),
        (
            "complete-generation-1",
            8,
            ContractWrite::CompleteGeneration { generation: 1 },
        ),
    ] {
        run.check(
            step,
            ok(contract.write(operation(sequence), &request, &never)).map(drop),
        )?;
    }

    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "get-claim-1",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "get-claim-1",
        claim_view(contract.read(&view, &get_claim(1, claim_one), &never)).and_then(|claim| {
            let citation = claim.citations.first();
            if claim.status == ClaimStatus::Active
                && claim.effective_support == EffectiveSupport::AsRecorded(SupportKind::DirectQuote)
                && citation.is_some_and(|citation| {
                    citation.freshness == FreshnessState::Fresh
                        && citation.excerpt.as_deref() == Some("the cache is derived")
                })
            {
                Ok(())
            } else {
                Err(format!("{claim:?}"))
            }
        }),
    )?;
    run.check(
        "search-claim-1",
        match ok(contract.read(
            &view,
            &ResearchReadRequest::Search {
                scope,
                authority_generation: 1,
                terms: vec!["DERIVED".to_owned()],
                knowledge: ResearchKnowledge::Current,
                evaluated_at: at(RETRIEVED_AT + 60),
                maximum_candidates: 64,
                maximum_results: 8,
                maximum_output_bytes: 64 * 1024,
            },
            &never,
        )) {
            Ok(ResearchReadOutput::Search(results))
                if results.items.iter().map(|claim| claim.id).eq([claim_one]) =>
            {
                Ok(())
            }
            other => Err(format!("{other:?}")),
        },
    )?;
    run.check(
        "correct-claim-1",
        ok(contract.write(
            operation(9),
            &ContractWrite::PutClaim {
                generation: 1,
                claim: fixture.claim_two.clone(),
            },
            &never,
        ))
        .map(drop),
    )?;
    run.check(
        "read-through-old-view",
        expect_error(
            contract.read(&view, &get_claim(1, claim_one), &never),
            ContractError::StaleView,
        ),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "get-superseded-claim-1",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "get-superseded-claim-1",
        claim_view(contract.read(&view, &get_claim(1, claim_one), &never)).and_then(|claim| {
            if matches!(claim.status, ClaimStatus::Superseded(_)) {
                Ok(())
            } else {
                Err(format!("{claim:?}"))
            }
        }),
    )?;
    run.check(
        "put-edge",
        ok(contract.write(
            operation(10),
            &ContractWrite::PutEdge {
                generation: 1,
                edge: fixture.edge.clone(),
            },
            &never,
        ))
        .map(drop),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "edges-from-claim-2",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "edges-from-claim-2",
        match ok(contract.read(
            &view,
            &ResearchReadRequest::EdgesFrom {
                authority_generation: 1,
                from: fixture.claim_two.id,
                kind: Some(EdgeKind::Corrects),
                knowledge: ResearchKnowledge::Current,
                maximum_candidates: 8,
                maximum_results: 8,
            },
            &never,
        )) {
            Ok(ResearchReadOutput::Edges(results))
                if results
                    .items
                    .iter()
                    .map(|edge| edge.id)
                    .eq([fixture.edge.id]) =>
            {
                Ok(())
            }
            other => Err(format!("{other:?}")),
        },
    )?;
    run.check(
        "revoke-source-1",
        ok(contract.write(
            operation(11),
            &ContractWrite::RevokeSource {
                generation: 1,
                source: fixture.first.id,
            },
            &never,
        ))
        .map(drop),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "get-revoked-claim-2",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "get-revoked-claim-2",
        claim_view(contract.read(&view, &get_claim(1, fixture.claim_two.id), &never)).and_then(
            |claim| {
                if matches!(claim.effective_support, EffectiveSupport::Revoked(_))
                    && claim
                        .citations
                        .iter()
                        .all(|citation| citation.excerpt.is_none())
                {
                    Ok(())
                } else {
                    Err(format!("{claim:?}"))
                }
            },
        ),
    )?;
    run.check(
        "cancelled-write",
        expect_error(
            contract.write(
                operation(12),
                &ContractWrite::PutClaim {
                    generation: 1,
                    claim: fixture.cancelled.clone(),
                },
                &Cancelled,
            ),
            ContractError::Cancelled,
        ),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "cancelled-write-left-no-record",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "cancelled-write-left-no-record",
        expect_error(
            contract.read(&view, &get_claim(1, fixture.cancelled.id), &never),
            ContractError::NotFound,
        ),
    )?;
    run.check(
        "foreign-scope-search",
        expect_error(
            contract.read(
                &view,
                &ResearchReadRequest::Search {
                    scope: inputs.foreign_scope,
                    authority_generation: 1,
                    terms: vec!["cache".to_owned()],
                    knowledge: ResearchKnowledge::Current,
                    evaluated_at: at(RETRIEVED_AT),
                    maximum_candidates: 8,
                    maximum_results: 8,
                    maximum_output_bytes: 4096,
                },
                &never,
            ),
            ContractError::Unauthorized,
        ),
    )?;
    run.check(
        "zero-result-budget",
        expect_error(
            contract.read(
                &view,
                &ResearchReadRequest::SourceVersions {
                    authority_generation: 1,
                    source: fixture.first.id.source,
                    knowledge: ResearchKnowledge::Current,
                    evaluated_at: at(RETRIEVED_AT),
                    maximum_results: 0,
                },
                &never,
            ),
            ContractError::ResourceLimit,
        ),
    )?;
    let (expected_version, next) = inputs.restricted_policy;
    run.check(
        "replace-policy",
        match contract.write(
            operation(13),
            &ContractWrite::ReplacePolicy {
                expected_version,
                next,
            },
            &never,
        ) {
            Ok(receipt) if receipt.revision.is_none() => Ok(()),
            other => Err(format!("{other:?}")),
        },
    )?;
    run.check(
        "read-after-policy-change",
        expect_error(
            contract.read(&view, &get_claim(1, fixture.claim_three.id), &never),
            ContractError::StalePolicy,
        ),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "get-claim-3-denied-source",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "get-claim-3-denied-source",
        claim_view(contract.read(&view, &get_claim(1, fixture.claim_three.id), &never)).and_then(
            |claim| {
                if claim.citations.is_empty()
                    && claim.effective_support == EffectiveSupport::NoVisibleCitation
                {
                    Ok(())
                } else {
                    Err(format!("{claim:?}"))
                }
            },
        ),
    )?;
    run.check(
        "begin-generation-2",
        ok(contract.write(
            operation(14),
            &ContractWrite::BeginGeneration { next_generation: 2 },
            &never,
        ))
        .map(drop),
    )?;
    let view = contract.view().map_err(|error| ConformanceFailure {
        step: "read-old-generation",
        detail: format!("view {error:?}"),
    })?;
    run.check(
        "read-old-generation",
        expect_error(
            contract.read(&view, &get_claim(1, fixture.claim_three.id), &never),
            ContractError::StaleGeneration,
        ),
    )?;
    run.check(
        "read-new-generation",
        expect_error(
            contract.read(&view, &get_claim(2, fixture.claim_three.id), &never),
            ContractError::Rebuilding,
        ),
    )?;
    if run.executed != CONFORMANCE_STEPS.len() {
        return Err(ConformanceFailure {
            step: "complete",
            detail: format!("{} of {} steps ran", run.executed, CONFORMANCE_STEPS.len()),
        });
    }
    Ok(ConformanceReport {
        contract: MEMORY_CONTRACT_VERSION,
        steps_passed: run.executed,
        fixture_digest: fixture_digest(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_digest_matches_the_pinned_candidate_revision() {
        let hex: String = fixture_digest()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(hex, CONFORMANCE_FIXTURE_DIGEST);
        assert_eq!(CONFORMANCE_STEPS.len(), 34);
        let mut names: Vec<_> = CONFORMANCE_STEPS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CONFORMANCE_STEPS.len());
    }

    #[test]
    fn negotiation_is_fail_closed() {
        assert_eq!(
            negotiate_contract(MEMORY_CONTRACT_NAME, MEMORY_CONTRACT_VERSION),
            Ok(MEMORY_CONTRACT_VERSION)
        );
        for (name, major, minor) in [
            (MEMORY_CONTRACT_NAME, 0, 0),
            (MEMORY_CONTRACT_NAME, 2, 0),
            (MEMORY_CONTRACT_NAME, 1, 1),
            ("uste-memory-consumer-v2", 1, 0),
        ] {
            assert_eq!(
                negotiate_contract(name, ContractVersion { major, minor }),
                Err(ContractError::UnsupportedVersion)
            );
        }
        assert!(ContractError::Unavailable.is_retryable());
        assert!(!ContractError::Conflict.is_retryable());
    }
}
