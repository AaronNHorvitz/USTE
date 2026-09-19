use super::*;
use uste_crypto::{CryptoError, EntropyFailure, KeyAdapter, SecretKeyMaterial};
use uste_types::{DatabaseId, NamespaceId};

struct Adapter;
impl KeyAdapter for Adapter {
    type Envelope = [u8; 32];
    fn wrap(
        &mut self,
        _: DatabaseId,
        key: &SecretKeyMaterial,
        _: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(*key.expose_to_adapter())
    }
    fn unwrap(
        &mut self,
        _: DatabaseId,
        wrapped: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*wrapped))
    }
}
struct Entropy(u64);
impl EntropySource for Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        for (i, chunk) in out.chunks_mut(8).enumerate() {
            chunk.copy_from_slice(&(self.0 + i as u64).to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
fn context() -> PackedRootContext {
    PackedRootContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        profile: [3; 32],
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([4; 16]),
        object: [5; 16],
    }
}
fn claims() -> PackedRootClaims {
    PackedRootClaims {
        revision: CommitRevision::new(9).unwrap(),
        generation: 7,
        certificate_digest: [6; 32],
        reducer_profile: [7; 32],
        state_commitment_profile: [8; 32],
        state_digest: [9; 32],
    }
}
fn family(id: u8) -> PackedRootFamily {
    PackedRootFamily {
        family: id,
        commitment: logical::empty_commitment(
            CommitmentContext::new(context().scope, context().profile, id).unwrap(),
        ),
        root: None,
    }
}
fn populated() -> PackedRootFamily {
    let mut bytes = [0; 54];
    bytes[..16].fill(10);
    bytes[23] = 8;
    bytes[31] = 1;
    bytes[32..48].fill(11);
    bytes[51] = 3;
    bytes[53] = 4;
    let root = PackedLocator::decode_fixed(&bytes, context().owner(claims().revision, 1)).unwrap();
    let commitment = logical::leaf_commitment(
        CommitmentContext::new(context().scope, context().profile, 1).unwrap(),
        logical::LeafProof {
            key: b"a",
            value: logical::value_commitment(&[]).unwrap(),
        },
    )
    .unwrap();
    PackedRootFamily {
        family: 1,
        commitment,
        root: Some(root),
    }
}

#[test]
fn packed_root_manifest_literal_layout_and_all_family_slots() {
    let families = [populated(), family(2)];
    let actual = encode_plain(context(), claims(), &families).unwrap();
    let mut expected = vec![0; 2048];
    expected[..8].copy_from_slice(&[85, 80, 82, 84, 2, 0, 2, 0]);
    expected[8..24].fill(2);
    expected[31] = 9;
    expected[39] = 7;
    expected[40..72].fill(6);
    expected[72..104].fill(7);
    expected[104..136].fill(8);
    expected[136..168].fill(9);
    expected[168..200].fill(3);
    expected[200..216].fill(5);
    expected[224] = 1;
    expected[239] = 1;
    expected[247] = 1;
    expected[248..280].copy_from_slice(families[0].commitment.digest());
    expected[280] = 1;
    expected[281..297].fill(10);
    expected[304] = 8;
    expected[312] = 1;
    expected[313..329].fill(11);
    expected[332] = 3;
    expected[334] = 4;
    expected[336] = 2;
    expected[360..392].copy_from_slice(families[1].commitment.digest());
    assert_eq!(actual.as_slice(), expected);
    let decoded = decode_plain(context(), &actual).unwrap();
    assert!(decoded.context() == context());
    assert!(decoded.claims() == claims());
    assert!(decoded.families() == families);
    let all: Vec<_> = (1..=16).map(family).collect();
    assert!(
        decode_plain(context(), &encode_plain(context(), claims(), &all).unwrap())
            .unwrap()
            .families()
            == all
    );
    let mut too_many = all;
    too_many.push(family(17));
    assert_eq!(
        encode_plain(context(), claims(), &too_many).err(),
        Some(StorageError::ResourceLimit)
    );
    assert_eq!(
        encode_plain(context(), claims(), &[]).err(),
        Some(StorageError::InvalidState)
    );
}

#[test]
fn packed_root_manifest_closed_grammar_rejects_lengths_reserved_and_false_claims() {
    let bytes = encode_plain(context(), claims(), &[populated(), family(2)]).unwrap();
    for length in 0..bytes.len() {
        assert!(decode_plain(context(), &bytes[..length]).is_err());
    }
    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(decode_plain(context(), &trailing).is_err());
    let reserved = std::iter::once(7)
        .chain(216..224)
        .chain(225..232)
        .chain(std::iter::once(335))
        .chain(337..344)
        .chain(std::iter::once(447))
        .chain(448..2048);
    for offset in reserved {
        let mut bad = bytes.to_vec();
        bad[offset] = 1;
        assert!(decode_plain(context(), &bad).is_err(), "reserved {offset}");
    }
    for (offset, value) in [
        (0, 0),
        (4, 1),
        (5, 1),
        (6, 0),
        (6, 17),
        (8, 9),
        (31, 0),
        (39, 0),
        (168, 9),
        (200, 9),
        (224, 0),
        (336, 1),
        (336, 0),
        (280, 2),
        (280, 0),
        (392, 1),
        (239, 0),
        (247, 0),
        (304, 10),
        (312, 0),
        (334, 128),
        (360, 9),
    ] {
        let mut bad = bytes.to_vec();
        bad[offset] = value;
        assert!(
            decode_plain(context(), &bad).is_err(),
            "offset {offset} value {value}"
        );
    }
    for range in [281..297, 329..333] {
        let mut bad = bytes.to_vec();
        bad[range.clone()].fill(if range.start == 281 { 0 } else { 255 });
        assert!(decode_plain(context(), &bad).is_err());
    }
    for malformed in [
        vec![family(2), populated()],
        vec![family(2), family(2)],
        vec![PackedRootFamily {
            root: None,
            ..populated()
        }],
        vec![PackedRootFamily {
            root: populated().root,
            ..family(1)
        }],
    ] {
        assert!(encode_plain(context(), claims(), &malformed).is_err());
    }
}

#[test]
fn packed_root_manifest_encryption_binds_context_and_refuses_every_ciphertext_mutation() {
    let c = context();
    let f = [populated(), family(2)];
    let mut vault = KeyVault::create(c.scope.database(), &mut Adapter, Entropy(10)).unwrap();
    let encoded = seal_manifest(&mut vault, c, claims(), &f).unwrap();
    assert_eq!(encoded.len(), 4161);
    assert!(open_manifest(&vault, c, &encoded).unwrap().families() == f);
    for length in 0..encoded.len() {
        assert!(open_manifest(&vault, c, &encoded[..length]).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(open_manifest(&vault, c, &trailing).is_err());
    for offset in 0..encoded.len() {
        let mut changed = encoded.clone();
        changed[offset] ^= 1;
        assert!(open_manifest(&vault, c, &changed).is_err(), "byte {offset}");
    }
    for changed in [
        PackedRootContext {
            scope: NamespaceRef::new(DatabaseId::from_bytes([9; 16]), c.scope.namespace()),
            ..c
        },
        PackedRootContext {
            scope: NamespaceRef::new(c.scope.database(), NamespaceId::from_bytes([9; 16])),
            ..c
        },
        PackedRootContext {
            profile: [9; 32],
            ..c
        },
        PackedRootContext {
            object: [9; 16],
            ..c
        },
        PackedRootContext {
            writer: WriterIncarnationId::from_bytes([9; 16]),
            ..c
        },
        PackedRootContext {
            epoch: KeyEpoch::new(2).unwrap(),
            ..c
        },
    ] {
        assert!(open_manifest(&vault, changed, &encoded).is_err());
    }
    let other = KeyVault::create(c.scope.database(), &mut Adapter, Entropy(100)).unwrap();
    assert!(open_manifest(&other, c, &encoded).is_err());
    let plaintext = encode_plain(c, claims(), &f).unwrap();
    let mut malformed = plaintext.to_vec();
    malformed[216] = 1;
    let forged = vault
        .encrypt(c.crypto(), &malformed)
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        open_manifest(&vault, c, &forged).err(),
        Some(StorageError::IntegrityFailure)
    );
    for crypto in [
        CryptoContext::new(
            c.scope.database(),
            Scope::Namespace(c.scope.namespace()),
            c.epoch,
            ObjectRole::IndexPage,
            CryptoObjectId::from_bytes(c.object),
            1,
            c.writer,
            2,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            c.scope.database(),
            Scope::Namespace(c.scope.namespace()),
            c.epoch,
            ObjectRole::IndexPage,
            CryptoObjectId::from_bytes(c.object),
            0,
            c.writer,
            1,
            0,
            FrameClass::Small4KiB,
        ),
    ] {
        let wrong = vault.encrypt(crypto, &plaintext).unwrap().encode().unwrap();
        assert!(open_manifest(&vault, c, &wrong).is_err());
    }
    vault.lock();
    assert_eq!(
        open_manifest(&vault, c, &encoded).err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    assert_eq!(
        seal_manifest(&mut vault, c, claims(), &f).err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    assert_eq!(
        seal_manifest(
            &mut vault,
            PackedRootContext {
                object: [0; 16],
                ..c
            },
            claims(),
            &f
        )
        .err(),
        Some(StorageError::InvalidState)
    );
    assert_eq!(
        seal_manifest(
            &mut vault,
            c,
            PackedRootClaims {
                generation: 0,
                ..claims()
            },
            &f
        )
        .err(),
        Some(StorageError::InvalidState)
    );
}
