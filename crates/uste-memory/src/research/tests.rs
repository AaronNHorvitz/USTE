use sha2::{Digest, Sha256};
use uste_storage::{BlobId, BlobReference};
use uste_types::{DatabaseId, NamespaceId, NamespaceRef, RecordId, RecordRef, UtcInstant};

use super::*;
use crate::{SourceLocator, SourceVersionId};

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([2; 16]),
    )
}

fn other_scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([1; 16]),
        NamespaceId::from_bytes([3; 16]),
    )
}

fn id(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn instant(seconds: i64) -> UtcInstant {
    UtcInstant::new(seconds, 0).unwrap()
}

fn content(text: &str, scope: NamespaceRef) -> RetainedContent {
    RetainedContent {
        blob: BlobReference::new(
            scope,
            BlobId::from_bytes([9; 16]),
            u64::try_from(text.len()).unwrap(),
            1,
            Sha256::digest(text.as_bytes()).into(),
        )
        .unwrap(),
        media_type: "text/html; charset=utf-8".to_owned(),
    }
}

fn source_version() -> SourceVersionId {
    SourceVersionId {
        source: id(10),
        version: 1,
    }
}

fn source() -> SourceRecordInput {
    SourceRecordInput {
        id: source_version(),
        kind: SourceKind::DocPackPage,
        locator_text: "guide/storage.html".to_owned(),
        version_label: "1.95.0".to_owned(),
        retrieved_at: instant(1_790_000_000),
        run_identity: [7; 16],
        route_label: "offline-doc-pack".to_owned(),
        outcome: FetchOutcome::Complete,
        content: Some(content("<p>synthetic page</p>", scope())),
        license_label: "MIT OR Apache-2.0".to_owned(),
        redistributable: true,
        freshness: Freshness::Pinned,
    }
}

fn artifact() -> ArtifactInput {
    ArtifactInput {
        id: id(20),
        inputs: vec![
            source_version(),
            SourceVersionId {
                source: id(11),
                version: 3,
            },
        ],
        producer: ProducerIdentity {
            name: "synthetic-html-text".to_owned(),
            revision: "0.1.0".to_owned(),
            configuration_digest: [5; 32],
        },
        content: None,
        complete_coverage: false,
        limitations: "tables omitted".to_owned(),
    }
}

fn claim() -> ClaimInput {
    let excerpt = "synthetic page";
    ClaimInput {
        id: id(30),
        subject: "storage guide".to_owned(),
        predicate: "states".to_owned(),
        value: "synthetic page".to_owned(),
        support: SupportKind::DirectQuote,
        citations: vec![CitationInput {
            source: source_version(),
            locator: SourceLocator::ByteRange { start: 3, end: 17 },
            excerpt_digest: Sha256::digest(excerpt.as_bytes()).into(),
            excerpt: Some(excerpt.to_owned()),
        }],
        valid_from: Some(instant(1_700_000_000)),
        valid_until: None,
        corrects: Some(id(29)),
    }
}

fn edge() -> EdgeInput {
    EdgeInput {
        id: id(40),
        kind: EdgeKind::Contradicts,
        from: id(30),
        to: id(31),
        asserted_by: id(20),
        valid_from: None,
        valid_until: Some(instant(1_800_000_000)),
    }
}

fn fixtures() -> Vec<ResearchRecord> {
    let mut inaccessible = source();
    inaccessible.id.version = 2;
    inaccessible.kind = SourceKind::WebPage;
    inaccessible.outcome = FetchOutcome::Inaccessible {
        reason: "consumer reported HTTP 404".to_owned(),
    };
    inaccessible.content = None;
    inaccessible.freshness = Freshness::MaxAge { seconds: 86_400 };
    let mut truncated = source();
    truncated.outcome = FetchOutcome::Truncated { limit_bytes: 64 };
    let mut unsupported = claim();
    unsupported.support = SupportKind::Unsupported;
    unsupported.citations.clear();
    unsupported.corrects = None;
    vec![
        ResearchRecord::Source(source()),
        ResearchRecord::Source(inaccessible),
        ResearchRecord::Source(truncated),
        ResearchRecord::Artifact(artifact()),
        ResearchRecord::Claim(claim()),
        ResearchRecord::Claim(unsupported),
        ResearchRecord::Edge(edge()),
    ]
}

fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").unwrap();
    }
    output
}

#[test]
fn every_fixture_round_trips_to_identical_bytes() {
    for record in fixtures() {
        let bytes = encode_research_record(scope(), &record).unwrap();
        let (decoded_scope, decoded) = decode_research_record(&bytes).unwrap();
        assert_eq!(decoded_scope, scope());
        assert_eq!(decoded, record);
        assert_eq!(encode_research_record(scope(), &decoded).unwrap(), bytes);
    }
}

#[test]
fn edge_layout_matches_the_specified_bytes_exactly() {
    let mut expected = Vec::new();
    expected.extend_from_slice(b"URSM");
    expected.extend_from_slice(&[1, 0, 4, 0]);
    expected.extend_from_slice(&[1; 16]);
    expected.extend_from_slice(&[2; 16]);
    expected.extend_from_slice(&[40; 16]);
    expected.push(2);
    expected.extend_from_slice(&[30; 16]);
    expected.extend_from_slice(&[31; 16]);
    expected.extend_from_slice(&[20; 16]);
    expected.push(0);
    expected.push(1);
    expected.extend_from_slice(&1_800_000_000_i64.to_be_bytes());
    expected.extend_from_slice(&0_u32.to_be_bytes());
    assert_eq!(expected.len(), RESEARCH_HEADER_BYTES + 16 + 1 + 48 + 1 + 13);
    assert_eq!(
        encode_research_record(scope(), &ResearchRecord::Edge(edge())).unwrap(),
        expected
    );
}

#[test]
fn golden_encodings_are_pinned() {
    let digests: Vec<String> = fixtures()
        .iter()
        .map(|record| {
            hex(&Sha256::digest(
                encode_research_record(scope(), record).unwrap(),
            ))
        })
        .collect();
    assert_eq!(digests, GOLDEN);
}

/// SHA-256 of each fixture's canonical encoding, in `fixtures()` order. Pinned from the first
/// reviewed run; the complete-source (first) and edge (last) digests were also reproduced by an
/// independent reimplementation of the specified byte layout before pinning.
const GOLDEN: [&str; 7] = [
    "4d1acdb3f3636a35261910ef5ee6ae07f1f5c185cea07ab0959cd199dfba44dd",
    "7ae03f6dacaa2c21a5abbf1e4f9773dec24497fa023b671914af7453936540ad",
    "333ad44112e336d3778b3f7d300233edb9f578fb92a40ce5560901bb9da458da",
    "f3cbc682dac7732036727b9700d706b73d25ad9b00a5c0a547ed26e3adb37aa6",
    "04d04d41f05e91f8f0e3f97d158e16ff13a0289e1aeb4d6857ad22e7788532ca",
    "9e65610e0ff193e0a7125e03058f9ea06209bbe59ce07af608e26fc663ac78b4",
    "a7a7f38426c1d20ca4322c9c07f1635f6e8135ad3e7e7ffd7d2478af13877a33",
];

#[test]
fn every_truncation_and_every_trailing_byte_fails_closed() {
    for record in fixtures() {
        let bytes = encode_research_record(scope(), &record).unwrap();
        for length in 0..bytes.len() {
            assert!(
                decode_research_record(&bytes[..length]).is_err(),
                "{length}"
            );
        }
        let mut extended = bytes.clone();
        extended.push(0);
        assert_eq!(
            decode_research_record(&extended),
            Err(ResearchCodecError::Invalid)
        );
    }
}

#[test]
fn versions_kinds_and_reserved_bytes_fail_closed() {
    let bytes = encode_research_record(scope(), &ResearchRecord::Edge(edge())).unwrap();
    for (offset, value, expected) in [
        (0, b'X', ResearchCodecError::UnsupportedVersion),
        (4, 2, ResearchCodecError::UnsupportedVersion),
        (5, 1, ResearchCodecError::UnsupportedVersion),
        (6, 0, ResearchCodecError::UnsupportedVersion),
        (6, 5, ResearchCodecError::UnsupportedVersion),
        (7, 1, ResearchCodecError::Invalid),
    ] {
        let mut changed = bytes.clone();
        changed[offset] = value;
        assert_eq!(decode_research_record(&changed), Err(expected), "{offset}");
    }
}

#[test]
fn every_single_byte_change_is_refused_or_changes_the_record() {
    for record in fixtures() {
        let bytes = encode_research_record(scope(), &record).unwrap();
        for offset in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[offset] ^= 0x80;
            if let Ok((changed_scope, decoded)) = decode_research_record(&changed) {
                assert!(changed_scope != scope() || decoded != record, "{offset}");
                assert_eq!(
                    encode_research_record(changed_scope, &decoded).unwrap(),
                    changed
                );
            }
        }
    }
}

#[test]
fn semantic_rules_are_enforced_on_encode_and_decode() {
    let invalid = |record: ResearchRecord, expected: ResearchCodecError| {
        assert_eq!(encode_research_record(scope(), &record), Err(expected));
    };
    let mut value = source();
    value.content = None;
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.outcome = FetchOutcome::Inaccessible {
        reason: "gone".to_owned(),
    };
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.outcome = FetchOutcome::Truncated { limit_bytes: 4 };
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.freshness = Freshness::MaxAge { seconds: 0 };
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.id.version = 0;
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.locator_text.clear();
    invalid(ResearchRecord::Source(value), ResearchCodecError::Invalid);
    let mut value = source();
    value.content = Some(content("x", other_scope()));
    invalid(
        ResearchRecord::Source(value),
        ResearchCodecError::ScopeMismatch,
    );
    let mut value = source();
    value.locator_text = "a".repeat(RESEARCH_PROFILE.maximum_locator_text_bytes + 1);
    invalid(
        ResearchRecord::Source(value),
        ResearchCodecError::ResourceLimit,
    );

    let mut value = artifact();
    value.inputs.reverse();
    invalid(ResearchRecord::Artifact(value), ResearchCodecError::Invalid);
    let mut value = artifact();
    value.inputs.clear();
    invalid(ResearchRecord::Artifact(value), ResearchCodecError::Invalid);
    let mut value = artifact();
    value.inputs = (1..=17)
        .map(|version| SourceVersionId {
            source: id(10),
            version,
        })
        .collect();
    invalid(
        ResearchRecord::Artifact(value),
        ResearchCodecError::ResourceLimit,
    );

    let mut value = claim();
    value.support = SupportKind::Unsupported;
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.citations.clear();
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.citations[0].excerpt = Some("different text".to_owned());
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.citations.push(value.citations[0].clone());
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.citations[0].locator = SourceLocator::ByteRange { start: 5, end: 5 };
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.citations[0].locator = SourceLocator::Utf8Lines {
        start: 0,
        end: 5,
        first_line: 0,
        last_line: 1,
    };
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.valid_until = value.valid_from;
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.corrects = Some(value.id);
    invalid(ResearchRecord::Claim(value), ResearchCodecError::Invalid);
    let mut value = claim();
    value.corrects = Some(RecordRef::new(
        other_scope().database(),
        other_scope().namespace(),
        RecordId::from_bytes([29; 16]),
    ));
    invalid(
        ResearchRecord::Claim(value),
        ResearchCodecError::ScopeMismatch,
    );
    let mut value = claim();
    value.subject = "s".repeat(RESEARCH_PROFILE.maximum_claim_field_bytes + 1);
    invalid(
        ResearchRecord::Claim(value),
        ResearchCodecError::ResourceLimit,
    );

    let mut value = edge();
    value.to = value.from;
    invalid(ResearchRecord::Edge(value), ResearchCodecError::Invalid);
    let mut value = edge();
    value.valid_from = Some(instant(1_900_000_000));
    invalid(ResearchRecord::Edge(value), ResearchCodecError::Invalid);
}

#[test]
fn decoded_bytes_are_revalidated_and_strict_flags_are_canonical() {
    // A claim encoded as unsupported with no citation, then relabelled as a direct quote in the
    // byte stream, decodes structurally but fails semantic revalidation.
    let mut unsupported = claim();
    unsupported.support = SupportKind::Unsupported;
    unsupported.citations.clear();
    let bytes = encode_research_record(scope(), &ResearchRecord::Claim(unsupported)).unwrap();
    let support_offset = bytes.len() - 17 - 13 - 1 - 2 - 1;
    assert_eq!(bytes[support_offset], 4);
    let mut relabelled = bytes.clone();
    relabelled[support_offset] = 1;
    assert_eq!(
        decode_research_record(&relabelled),
        Err(ResearchCodecError::Invalid)
    );
    // Presence and boolean bytes accept exactly 0 or 1.
    let edge_bytes = encode_research_record(scope(), &ResearchRecord::Edge(edge())).unwrap();
    let presence = RESEARCH_HEADER_BYTES + 16 + 1 + 48;
    assert_eq!(edge_bytes[presence], 0);
    let mut changed = edge_bytes.clone();
    changed[presence] = 2;
    assert_eq!(
        decode_research_record(&changed),
        Err(ResearchCodecError::Invalid)
    );
}

#[test]
fn oversized_inputs_are_refused_before_parsing() {
    let oversized = vec![0; RESEARCH_PROFILE.maximum_request_bytes + 1];
    assert_eq!(
        decode_research_record(&oversized),
        Err(ResearchCodecError::ResourceLimit)
    );
}
