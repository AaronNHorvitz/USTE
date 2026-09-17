use std::collections::VecDeque;

use sha2::{Digest, Sha256};
use uste_crypto::{
    CryptoContext, CryptoError, CryptoObjectId, EncryptedEnvelope, EntropyFailure, EntropySource,
    FrameClass, KeyAdapter, KeyEpoch, KeyVault, MAX_NONCES_PER_WRITER_SESSION, MAX_PLAINTEXT_BYTES,
    MAX_RECOVERY_PASSWORD_BYTES, OBJECT_ENVELOPE_HEADER_BYTES, ObjectRole, PortableRecoveryAdapter,
    RECOVERY_ENVELOPE_HEADER_BYTES, RecoveryEnvelope, RecoveryPassword, Scope, SecretKeyMaterial,
    WriterIncarnationId,
};
use uste_types::{DatabaseId, NamespaceId};

const SECURITY_VECTORS: &str = include_str!("../../../acceptance/r0/security-lifecycle.tsv");
const CRYPTO_VECTORS: &str = include_str!("../../../acceptance/r1/crypto-v1.tsv");

#[test]
fn object_envelope_round_trips_exact_bytes_and_padding_classes() {
    let database = database(1);
    let mut vault = test_vault(database, vec![[0x21; 24], [0x22; 24], [0x23; 24]]);
    for (index, (frame, plaintext)) in [
        (FrameClass::Small4KiB, Vec::new()),
        (FrameClass::Small4KiB, b"exact plaintext".to_vec()),
        (FrameClass::Blob64KiB, vec![0xa5; 65_537]),
    ]
    .into_iter()
    .enumerate()
    {
        let context = context(database, frame, index as u64);
        let envelope = vault.encrypt(context, &plaintext).unwrap();
        let encoded = envelope.encode().unwrap();
        assert_eq!(encoded.len(), 49 + envelope.ciphertext().len());
        assert_eq!((envelope.ciphertext().len() - 16) % frame.bytes(), 0);
        if !plaintext.is_empty() {
            assert!(
                !encoded
                    .windows(plaintext.len())
                    .any(|window| window == plaintext)
            );
        }
        let decoded = EncryptedEnvelope::decode(&encoded).unwrap();
        let decrypted = vault.decrypt(context, &decoded).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }
}

#[test]
fn wrong_key_context_nonce_ciphertext_tag_and_truncation_fail_closed() {
    let database = database(2);
    let mut vault = test_vault(database, vec![[0x31; 24]]);
    let base = context(database, FrameClass::Small4KiB, 7);
    let envelope = vault.encrypt(base, b"classified canary").unwrap();
    let encoded = envelope.encode().unwrap();

    let mut wrong_key = KeyVault::from_locked(database, [0x99; 32], ScriptedEntropy::new(vec![]));
    wrong_key.unlock(&mut TestAdapter).unwrap();
    assert_eq!(
        wrong_key.decrypt(base, &envelope).unwrap_err(),
        CryptoError::IntegrityFailure
    );

    for wrong in changed_contexts(database) {
        assert_eq!(
            vault.decrypt(wrong, &envelope).unwrap_err(),
            CryptoError::IntegrityFailure
        );
    }

    for index in 17..encoded.len() {
        let mut damaged = encoded.clone();
        damaged[index] ^= 0x80;
        match EncryptedEnvelope::decode(&damaged) {
            Ok(damaged) => assert_eq!(
                vault.decrypt(base, &damaged).unwrap_err(),
                CryptoError::IntegrityFailure,
                "accepted mutation {index}"
            ),
            Err(error) => assert!(matches!(
                error,
                CryptoError::IntegrityFailure
                    | CryptoError::InvalidEnvelope
                    | CryptoError::ResourceLimit
            )),
        }
    }
    for cut in 0..encoded.len() {
        assert!(
            EncryptedEnvelope::decode(&encoded[..cut]).is_err(),
            "cut {cut}"
        );
    }
    let mut appended = encoded;
    appended.push(0);
    assert_eq!(
        EncryptedEnvelope::decode(&appended).unwrap_err(),
        CryptoError::IntegrityFailure
    );
}

#[test]
fn entropy_failure_duplicate_nonce_and_locked_state_match_literal_vectors() {
    let database = database(3);
    let mut adapter = TestAdapter;
    let entropy = ScriptedEntropy::with_failure(vec![vec![0x41; 32]]);
    let mut vault = KeyVault::create(database, &mut adapter, entropy).unwrap();
    assert_eq!(
        vault
            .encrypt(context(database, FrameClass::Small4KiB, 1), b"x")
            .unwrap_err(),
        CryptoError::RetryableUnavailable
    );

    let mut vault = test_vault(database, vec![[0x42; 24], [0x42; 24]]);
    let context = context(database, FrameClass::Small4KiB, 2);
    vault.encrypt(context, b"first").unwrap();
    assert_eq!(
        vault.encrypt(context, b"second").unwrap_err(),
        CryptoError::IntegrityFailure
    );
    vault.lock();
    assert!(vault.is_locked());
    assert_eq!(
        vault.encrypt(context, b"locked").unwrap_err(),
        CryptoError::Locked
    );
    assert_eq!(
        vault
            .decrypt(
                context,
                &EncryptedEnvelope::decode(&valid_fixture()).unwrap()
            )
            .unwrap_err(),
        CryptoError::Locked
    );
    vault.lock();
    assert!(vault.is_locked());

    for (case, expected) in [
        ("wrong_key", "IntegrityFailure"),
        ("wrong_context", "IntegrityFailure"),
        ("truncated_tag", "IntegrityFailure"),
        ("entropy_failure", "RetryableUnavailable"),
        ("duplicate_nonce", "IntegrityFailure"),
        ("key_loss", "KeyUnavailable"),
    ] {
        let row = SECURITY_VECTORS
            .lines()
            .find(|line| line.starts_with(case))
            .unwrap();
        assert_eq!(row.split('\t').next_back(), Some(expected));
    }
}

#[test]
fn envelope_versions_lengths_and_caps_are_rejected_before_allocation() {
    let fixture = valid_fixture();
    for (index, value) in [(4, 0xff), (5, 2), (6, 1), (7, 2), (8, 0xff)] {
        let mut mutated = fixture.clone();
        mutated[index] = value;
        assert_eq!(
            EncryptedEnvelope::decode(&mutated).unwrap_err(),
            CryptoError::UnsupportedProfile
        );
    }
    let mut zero_epoch = fixture.clone();
    zero_epoch[9..17].fill(0);
    assert_eq!(
        EncryptedEnvelope::decode(&zero_epoch).unwrap_err(),
        CryptoError::InvalidEnvelope
    );
    let mut huge = fixture;
    huge[41..49].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(matches!(
        EncryptedEnvelope::decode(&huge),
        Err(CryptoError::ResourceLimit | CryptoError::InvalidEnvelope)
    ));
}

#[test]
fn deterministic_object_envelope_has_a_pinned_digest() {
    let database = database(4);
    let mut vault = test_vault(database, vec![[0x55; 24]]);
    let envelope = vault
        .encrypt(
            context(database, FrameClass::Small4KiB, 0x0102_0304_0506_0708),
            b"USTE crypto-v1 golden",
        )
        .unwrap();
    let digest = Sha256::digest(envelope.encode().unwrap());
    assert_eq!(
        digest.as_slice(),
        &[
            58, 226, 40, 43, 142, 8, 188, 5, 100, 189, 134, 60, 11, 16, 103, 85, 8, 244, 207, 100,
            59, 60, 233, 5, 201, 183, 215, 206, 168, 121, 108, 70,
        ]
    );
    assert!(CRYPTO_VECTORS.contains(
        "object_golden\tsha256\tmaster=11x32;nonce=55x24;database=04x16;namespace=02x16;role=blob;object=03x16;sequence=0102030405060708;writer=04x16;format=1.0;frame=4KiB;plaintext=USTE_crypto-v1_golden\t3ae2282b8e08bc0564bd863c0b10675508f4cf643b3ce905c9b7d7cea8796c46"
    ));
}

#[test]
fn public_opaque_identifier_has_a_pinned_domain_and_rejects_other_roles() {
    let database = database(0x4a);
    let vault = test_vault(database, vec![]);
    let context = CryptoContext::new(
        database,
        Scope::Database,
        KeyEpoch::FIRST,
        ObjectRole::BlobInventoryName,
        CryptoObjectId::from_bytes([0; 16]),
        0,
        WriterIncarnationId::from_bytes([0x55; 16]),
        1,
        0,
        FrameClass::Small4KiB,
    );
    assert_eq!(
        vault
            .derive_opaque_identifier(context, &[0x66; 32])
            .unwrap(),
        [
            0xc2, 0xd4, 0xec, 0x31, 0x45, 0x43, 0xb5, 0x87, 0xf6, 0xb2, 0xf5, 0x85, 0x4c, 0x4c,
            0x21, 0x31, 0x77, 0x6b, 0x97, 0x3d, 0xa4, 0xf1, 0x19, 0xda, 0x59, 0x03, 0xb2, 0x41,
            0x3f, 0xad, 0xab, 0x4b,
        ]
    );
    assert_eq!(
        vault
            .derive_opaque_identifier(
                CryptoContext::new(
                    database,
                    Scope::Database,
                    KeyEpoch::FIRST,
                    ObjectRole::BlobInventory,
                    CryptoObjectId::from_bytes([0; 16]),
                    0,
                    WriterIncarnationId::from_bytes([0x55; 16]),
                    1,
                    0,
                    FrameClass::Small4KiB,
                ),
                &[0x66; 32],
            )
            .unwrap_err(),
        CryptoError::InvalidContext
    );
}

#[test]
fn literal_crypto_profile_matches_public_bounds() {
    let expected = |case: &str| {
        CRYPTO_VECTORS
            .lines()
            .skip(1)
            .find_map(|line| {
                let mut fields = line.split('\t');
                (fields.next() == Some(case)).then(|| fields.nth(2).unwrap())
            })
            .unwrap()
    };
    assert_eq!(
        expected("object_header").parse::<usize>().unwrap(),
        OBJECT_ENVELOPE_HEADER_BYTES
    );
    assert_eq!(
        expected("object_plaintext_cap").parse::<usize>().unwrap(),
        MAX_PLAINTEXT_BYTES
    );
    assert_eq!(
        expected("small_frame").parse::<usize>().unwrap(),
        FrameClass::Small4KiB.bytes()
    );
    assert_eq!(
        expected("blob_frame").parse::<usize>().unwrap(),
        FrameClass::Blob64KiB.bytes()
    );
    assert_eq!(
        expected("nonce_session_cap").parse::<usize>().unwrap(),
        MAX_NONCES_PER_WRITER_SESSION
    );
    assert_eq!(
        expected("recovery_header").parse::<usize>().unwrap(),
        RECOVERY_ENVELOPE_HEADER_BYTES
    );
    assert_eq!(
        expected("recovery_memory").parse::<u32>().unwrap(),
        uste_crypto::ARGON2_MEMORY_KIB
    );
    assert_eq!(
        expected("recovery_iterations").parse::<u32>().unwrap(),
        uste_crypto::ARGON2_ITERATIONS
    );
    assert_eq!(
        expected("recovery_lanes").parse::<u32>().unwrap(),
        uste_crypto::ARGON2_LANES
    );
    assert_eq!(
        expected("recovery_password_cap").parse::<usize>().unwrap(),
        MAX_RECOVERY_PASSWORD_BYTES
    );
    assert_eq!(CRYPTO_VECTORS.lines().count(), 12);
}

#[test]
fn writer_incarnation_separates_same_master_nonce_and_plaintext() {
    let database = database(44);
    let mut left = KeyVault::from_locked(
        database,
        [0x11; 32],
        ScriptedEntropy::new(vec![vec![0x45; 24]]),
    );
    let mut right = KeyVault::from_locked(
        database,
        [0x11; 32],
        ScriptedEntropy::new(vec![vec![0x45; 24]]),
    );
    left.unlock(&mut TestAdapter).unwrap();
    right.unlock(&mut TestAdapter).unwrap();
    let left_context = context(database, FrameClass::Small4KiB, 1);
    let right_context = CryptoContext::new(
        database,
        Scope::Namespace(namespace(2)),
        KeyEpoch::FIRST,
        ObjectRole::BlobChunk,
        CryptoObjectId::from_bytes([3; 16]),
        1,
        WriterIncarnationId::from_bytes([0x46; 16]),
        1,
        0,
        FrameClass::Small4KiB,
    );
    let left_envelope = left.encrypt(left_context, b"same").unwrap();
    let right_envelope = right.encrypt(right_context, b"same").unwrap();
    assert_ne!(
        left_envelope.encode().unwrap(),
        right_envelope.encode().unwrap()
    );
    assert_eq!(
        left.decrypt(right_context, &left_envelope).unwrap_err(),
        CryptoError::IntegrityFailure
    );
    assert_eq!(
        right
            .decrypt(right_context, &right_envelope)
            .unwrap()
            .as_slice(),
        b"same"
    );
}

#[test]
fn object_plaintext_cap_is_inclusive_and_overage_precedes_entropy() {
    let database = database(45);
    let mut vault = test_vault(database, vec![[0x47; 24]]);
    let maximum = vec![0x48; MAX_PLAINTEXT_BYTES];
    let context = context(database, FrameClass::Blob64KiB, 1);
    let envelope = vault.encrypt(context, &maximum).unwrap();
    let plaintext = vault.decrypt(context, &envelope).unwrap();
    assert_eq!(plaintext.as_slice().len(), MAX_PLAINTEXT_BYTES);
    assert!(plaintext.as_slice().iter().all(|byte| *byte == 0x48));
    assert_eq!(
        vault
            .encrypt(context, &vec![0; MAX_PLAINTEXT_BYTES + 1])
            .unwrap_err(),
        CryptoError::ResourceLimit
    );
}

#[test]
fn portable_recovery_uses_real_fixed_argon2_profile_and_lock_unlock() {
    let database = database(5);
    let password = RecoveryPassword::new(b"correct horse battery staple".to_vec()).unwrap();
    let mut adapter = PortableRecoveryAdapter::new(password);
    let entropy = ScriptedEntropy::new(vec![
        vec![0x61; 32],
        vec![0x62; 16],
        vec![0x63; 24],
        vec![0x64; 24],
    ]);
    let mut vault = KeyVault::create(database, &mut adapter, entropy).unwrap();
    let wrapped_bytes = vault.wrapped().encode().unwrap();
    assert_eq!(
        wrapped_bytes.len(),
        RECOVERY_ENVELOPE_HEADER_BYTES + vault.wrapped().ciphertext().len()
    );
    let mut downgrade = wrapped_bytes.clone();
    downgrade[9..13].copy_from_slice(&1024_u32.to_be_bytes());
    assert_eq!(
        RecoveryEnvelope::decode(&downgrade).unwrap_err(),
        CryptoError::UnsupportedProfile
    );
    let context = context(database, FrameClass::Small4KiB, 11);
    let object = vault.encrypt(context, b"after recovery").unwrap();
    vault.lock();
    assert_eq!(
        vault.decrypt(context, &object).unwrap_err(),
        CryptoError::Locked
    );
    vault.unlock(&mut adapter).unwrap();
    assert_eq!(
        vault.decrypt(context, &object).unwrap().as_slice(),
        b"after recovery"
    );

    let wrapped = RecoveryEnvelope::decode(&wrapped_bytes).unwrap();
    let mut wrong = KeyVault::from_locked(database, wrapped, ScriptedEntropy::new(vec![]));
    let mut wrong_adapter =
        PortableRecoveryAdapter::new(RecoveryPassword::new(b"wrong password".to_vec()).unwrap());
    assert_eq!(
        wrong.unlock(&mut wrong_adapter),
        Err(CryptoError::KeyUnavailable)
    );
    assert!(wrong.is_locked());

    let wrong_database_envelope = RecoveryEnvelope::decode(&wrapped_bytes).unwrap();
    let mut wrong_database = KeyVault::from_locked(
        DatabaseId::from_bytes([6; 16]),
        wrong_database_envelope,
        ScriptedEntropy::new(vec![]),
    );
    assert_eq!(
        wrong_database.unlock(&mut adapter),
        Err(CryptoError::KeyUnavailable)
    );
    assert!(wrong_database.is_locked());

    let mut tampered_bytes = wrapped_bytes;
    let last = tampered_bytes.len() - 1;
    tampered_bytes[last] ^= 1;
    let tampered = RecoveryEnvelope::decode(&tampered_bytes).unwrap();
    let mut tampered_vault =
        KeyVault::from_locked(database, tampered, ScriptedEntropy::new(vec![]));
    assert_eq!(
        tampered_vault.unlock(&mut adapter),
        Err(CryptoError::KeyUnavailable)
    );
    assert!(tampered_vault.is_locked());
}

#[test]
fn secret_and_error_diagnostics_are_redacted() {
    let canaries = [
        "classified canary",
        "correct horse battery staple",
        "0505050505050505",
    ];
    let secret = SecretKeyMaterial::from_adapter_bytes([5; 32]);
    let password = RecoveryPassword::new(b"correct horse battery staple".to_vec()).unwrap();
    let rendered = format!(
        "{secret:?} {password:?} {:?} {}",
        CryptoError::IntegrityFailure,
        CryptoError::IntegrityFailure
    );
    for canary in canaries {
        assert!(!rendered.contains(canary));
    }
    assert!(rendered.contains("REDACTED"));
    assert!(rendered.contains("USTE_CRYPTO_INTEGRITY_FAILURE"));
    assert_eq!(
        RecoveryPassword::new(Vec::new()).unwrap_err(),
        CryptoError::InvalidCredential
    );
    assert_eq!(
        RecoveryPassword::new(vec![0x7f; MAX_RECOVERY_PASSWORD_BYTES + 1]).unwrap_err(),
        CryptoError::InvalidCredential
    );
    assert!(RecoveryPassword::new(vec![0x7f; MAX_RECOVERY_PASSWORD_BYTES]).is_ok());
}

fn changed_contexts(db: DatabaseId) -> Vec<CryptoContext> {
    vec![
        CryptoContext::new(
            database(99),
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(9)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::new(2).unwrap(),
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::Snapshot,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([8; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        context(db, FrameClass::Small4KiB, 8),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([9; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            2,
            0,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            1,
            FrameClass::Small4KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Namespace(namespace(2)),
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Blob64KiB,
        ),
        CryptoContext::new(
            db,
            Scope::Database,
            KeyEpoch::FIRST,
            ObjectRole::BlobChunk,
            CryptoObjectId::from_bytes([3; 16]),
            7,
            WriterIncarnationId::from_bytes([4; 16]),
            1,
            0,
            FrameClass::Small4KiB,
        ),
    ]
}

fn context(database: DatabaseId, frame: FrameClass, sequence: u64) -> CryptoContext {
    CryptoContext::new(
        database,
        Scope::Namespace(namespace(2)),
        KeyEpoch::FIRST,
        ObjectRole::BlobChunk,
        CryptoObjectId::from_bytes([3; 16]),
        sequence,
        WriterIncarnationId::from_bytes([4; 16]),
        1,
        0,
        frame,
    )
}

fn test_vault(database: DatabaseId, nonces: Vec<[u8; 24]>) -> KeyVault<[u8; 32], ScriptedEntropy> {
    let mut outputs = vec![vec![0x11; 32]];
    outputs.extend(nonces.into_iter().map(Vec::from));
    KeyVault::create(database, &mut TestAdapter, ScriptedEntropy::new(outputs)).unwrap()
}

fn valid_fixture() -> Vec<u8> {
    let database = database(8);
    test_vault(database, vec![[0x71; 24]])
        .encrypt(context(database, FrameClass::Small4KiB, 1), b"fixture")
        .unwrap()
        .encode()
        .unwrap()
}

fn database(byte: u8) -> DatabaseId {
    DatabaseId::from_bytes([byte; 16])
}

fn namespace(byte: u8) -> NamespaceId {
    NamespaceId::from_bytes([byte; 16])
}

struct TestAdapter;

impl KeyAdapter for TestAdapter {
    type Envelope = [u8; 32];

    fn wrap(
        &mut self,
        _database: DatabaseId,
        key: &SecretKeyMaterial,
        _entropy: &mut dyn EntropySource,
    ) -> Result<Self::Envelope, CryptoError> {
        Ok(*key.expose_to_adapter())
    }

    fn unwrap(
        &mut self,
        _database: DatabaseId,
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*envelope))
    }
}

struct ScriptedEntropy {
    outputs: VecDeque<Result<Vec<u8>, EntropyFailure>>,
}

impl ScriptedEntropy {
    fn new(outputs: Vec<Vec<u8>>) -> Self {
        Self {
            outputs: outputs.into_iter().map(Ok).collect(),
        }
    }

    fn with_failure(outputs: Vec<Vec<u8>>) -> Self {
        let mut outputs: VecDeque<_> = outputs.into_iter().map(Ok).collect();
        outputs.push_back(Err(EntropyFailure));
        Self { outputs }
    }
}

impl EntropySource for ScriptedEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        let bytes = self.outputs.pop_front().ok_or(EntropyFailure)??;
        if bytes.len() != output.len() {
            return Err(EntropyFailure);
        }
        output.copy_from_slice(&bytes);
        Ok(())
    }
}
