use super::*;
use std::{cell::Cell, rc::Rc};
use uste_crypto::{CryptoError, EntropyFailure, KeyAdapter, SecretKeyMaterial};
use uste_types::{DatabaseId, NamespaceId};

// Synthetic test-only adapter, never a production key wrapper.
struct TestAdapter;
impl KeyAdapter for TestAdapter {
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
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*envelope))
    }
}
struct TestEntropy {
    next: u64,
    fail: Rc<Cell<bool>>,
}
impl EntropySource for TestEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), EntropyFailure> {
        if self.fail.get() {
            return Err(EntropyFailure);
        }
        self.next += 1;
        for (i, chunk) in output.chunks_mut(8).enumerate() {
            chunk.copy_from_slice(&(self.next + i as u64).to_be_bytes()[..chunk.len()]);
        }
        Ok(())
    }
}
fn context() -> PackedPageContext {
    PackedPageContext {
        scope: NamespaceRef::new(
            DatabaseId::from_bytes([1; 16]),
            NamespaceId::from_bytes([2; 16]),
        ),
        epoch: KeyEpoch::FIRST,
        writer: WriterIncarnationId::from_bytes([3; 16]),
        creation_revision: CommitRevision::FIRST,
        profile: [4; 32],
        family: 5,
        object: [6; 16],
        page: 7,
    }
}
fn vault(seed: u64) -> (KeyVault<[u8; 32], TestEntropy>, Rc<Cell<bool>>) {
    let fail = Rc::new(Cell::new(false));
    let entropy = TestEntropy {
        next: seed,
        fail: fail.clone(),
    };
    (
        KeyVault::create(context().scope.database(), &mut TestAdapter, entropy).unwrap(),
        fail,
    )
}
fn builder() -> PackedPageBuilder {
    let mut page = PackedPageBuilder::new(context()).unwrap();
    assert_eq!(
        page.push(PackedRecordKind::TreeNode, b"synthetic-node")
            .unwrap(),
        0
    );
    assert_eq!(
        page.push(PackedRecordKind::ValueChunk, b"synthetic-chunk")
            .unwrap(),
        1
    );
    page
}

#[test]
fn packed_page_round_trip_fixed_size_and_independent_literal_layout() {
    let page = builder();
    // Construct the expected framing independently, without builder/parser helpers.
    let mut expected = vec![0; 16_384];
    expected[0..8].copy_from_slice(&[85, 73, 67, 80, 2, 0, 5, 0]);
    expected[15] = 1;
    expected[23] = 7;
    expected[24..28].copy_from_slice(&[0, 2, 2, 111]); // 623 bytes used.
    expected[32..64].fill(4);
    expected[64..80].fill(6);
    expected[80..88].copy_from_slice(&[2, 80, 0, 15, 2, 95, 0, 16]);
    expected[592] = 1;
    expected[593..607].copy_from_slice(b"synthetic-node");
    expected[607] = 2;
    expected[608..623].copy_from_slice(b"synthetic-chunk");
    assert_eq!(page.bytes.as_slice(), expected);
    let (mut vault, _) = vault(10);
    let encoded = page.seal(&mut vault).unwrap();
    assert_eq!(encoded.len(), 20_545);
    let opened = PackedPage::open(&vault, context(), &encoded).unwrap();
    assert_eq!(opened.record_count(), 2);
    assert_eq!(opened.record(0).unwrap().kind, PackedRecordKind::TreeNode);
    assert_eq!(opened.record(0).unwrap().payload, b"synthetic-node");
    assert_eq!(opened.record(1).unwrap().kind, PackedRecordKind::ValueChunk);
    assert_eq!(opened.record(1).unwrap().payload, b"synthetic-chunk");
    assert!(opened.record(2).is_none());
    assert!(opened.record(u16::MAX).is_none());
    assert_ne!(encoded, page.seal(&mut vault).unwrap());
}

#[test]
fn packed_page_exact_byte_slot_limits_and_refusals_are_atomic() {
    let mut page = PackedPageBuilder::new(context()).unwrap();
    let (mut vault, _) = vault(10);
    assert_eq!(
        page.seal(&mut vault).err(),
        Some(StorageError::InvalidState)
    );
    let before = page.bytes.to_vec();
    assert_eq!(
        page.push(PackedRecordKind::TreeNode, b"").err(),
        Some(StorageError::InvalidState)
    );
    assert_eq!(
        page.push(PackedRecordKind::TreeNode, &vec![0; MAX_RECORD_PAYLOAD + 1])
            .err(),
        Some(StorageError::ResourceLimit)
    );
    assert_eq!(page.bytes.as_slice(), before);
    let payload = vec![91; MAX_RECORD_PAYLOAD];
    page.push(PackedRecordKind::ValueChunk, &payload).unwrap();
    let before = page.bytes.to_vec();
    assert_eq!(
        page.push(PackedRecordKind::TreeNode, b"x").err(),
        Some(StorageError::ResourceLimit)
    );
    assert_eq!(page.bytes.as_slice(), before);
    let encoded = page.seal(&mut vault).unwrap();
    let opened = PackedPage::open(&vault, context(), &encoded).unwrap();
    assert_eq!(opened.record(0).unwrap().payload, payload);
    let mut page = PackedPageBuilder::new(context()).unwrap();
    for slot in 0..MAX_SLOTS {
        assert_eq!(
            page.push(PackedRecordKind::TreeNode, &[slot as u8])
                .unwrap(),
            slot
        );
    }
    let before = page.bytes.to_vec();
    assert_eq!(
        page.push(PackedRecordKind::TreeNode, b"x").err(),
        Some(StorageError::ResourceLimit)
    );
    assert_eq!(page.bytes.as_slice(), before);
    let encoded = page.seal(&mut vault).unwrap();
    let opened = PackedPage::open(&vault, context(), &encoded).unwrap();
    for slot in 0..MAX_SLOTS {
        assert_eq!(opened.record(slot).unwrap().payload, &[slot as u8]);
    }
    for bad in [
        PackedPageContext {
            family: 0,
            ..context()
        },
        PackedPageContext {
            object: [0; 16],
            ..context()
        },
        PackedPageContext {
            page: MAX_PAGES,
            ..context()
        },
    ] {
        assert!(PackedPageBuilder::new(bad).is_err());
        assert!(PackedPage::open(&vault, bad, &encoded).is_err());
    }
    assert!(
        PackedPageBuilder::new(PackedPageContext {
            page: MAX_PAGES - 1,
            ..context()
        })
        .is_ok()
    );
}

#[test]
fn packed_page_every_plaintext_truncation_and_nonpayload_mutation_fails_closed() {
    let page = builder();
    for length in 0..PAGE_BYTES {
        assert!(validate_plaintext(context(), &page.bytes[..length]).is_err());
    }
    let mut trailing = page.bytes.to_vec();
    trailing.push(0);
    assert!(validate_plaintext(context(), &trailing).is_err());
    for index in 0..PAGE_BYTES {
        if (593..607).contains(&index) || (608..623).contains(&index) {
            continue;
        }
        let mut bad = page.bytes.to_vec();
        bad[index] ^= 1;
        assert!(validate_plaintext(context(), &bad).is_err(), "byte {index}");
    }
    // Authenticated lengths/offsets are hostile too, including wrapped end calculations.
    for offset in [24, 26, 80, 82, 84, 86] {
        for value in [0, 1, 127, 128, 129, 591, 592, 623, 16_384, u16::MAX] {
            let mut bad = page.bytes.to_vec();
            bad[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
            if bad.as_slice() != page.bytes.as_slice() {
                assert!(
                    validate_plaintext(context(), &bad).is_err(),
                    "offset {offset} value {value}"
                );
            }
        }
    }
}

#[test]
fn packed_page_authenticated_malformed_framing_and_context_substitution_are_rejected() {
    let page = builder();
    let (mut vault, _) = vault(10);
    let encoded = page.seal(&mut vault).unwrap();
    let c = context();
    for altered in [
        PackedPageContext {
            scope: NamespaceRef::new(DatabaseId::from_bytes([9; 16]), c.scope.namespace()),
            ..c
        },
        PackedPageContext {
            scope: NamespaceRef::new(c.scope.database(), NamespaceId::from_bytes([9; 16])),
            ..c
        },
        PackedPageContext {
            epoch: KeyEpoch::new(2).unwrap(),
            ..c
        },
        PackedPageContext {
            writer: WriterIncarnationId::from_bytes([9; 16]),
            ..c
        },
        PackedPageContext {
            object: [9; 16],
            ..c
        },
        PackedPageContext { page: 8, ..c },
        PackedPageContext {
            creation_revision: CommitRevision::new(2).unwrap(),
            ..c
        },
        PackedPageContext {
            profile: [9; 32],
            ..c
        },
        PackedPageContext { family: 6, ..c },
    ] {
        assert!(PackedPage::open(&vault, altered, &encoded).is_err());
    }
    for offset in [
        0, 4, 5, 6, 7, 8, 16, 24, 26, 28, 32, 64, 80, 82, 84, 86, 88, 592, 607, 623, 16_383,
    ] {
        let mut bad = page.bytes.to_vec();
        bad[offset] ^= 1;
        let encoded = vault.encrypt(c.crypto(), &bad).unwrap().encode().unwrap();
        assert_eq!(
            PackedPage::open(&vault, c, &encoded).err(),
            Some(StorageError::IntegrityFailure)
        );
    }
    let old_context = CryptoContext::new(
        c.scope.database(),
        Scope::Namespace(c.scope.namespace()),
        c.epoch,
        ObjectRole::IndexPage,
        CryptoObjectId::from_bytes(c.object),
        c.page + 1,
        c.writer,
        1,
        0,
        FrameClass::Small4KiB,
    );
    let old_encoded = vault
        .encrypt(old_context, &page.bytes)
        .unwrap()
        .encode()
        .unwrap();
    assert!(PackedPage::open(&vault, c, &old_encoded).is_err());
}

#[test]
fn packed_page_ciphertext_mutations_truncations_keys_lock_and_entropy_fail_closed() {
    let page = builder();
    let (mut vault, fail) = vault(10);
    let encoded = page.seal(&mut vault).unwrap();
    for length in 0..encoded.len() {
        assert!(PackedPage::open(&vault, context(), &encoded[..length]).is_err());
    }
    let mut bad = encoded.clone();
    bad.push(0);
    assert!(PackedPage::open(&vault, context(), &bad).is_err());
    for index in 0..encoded.len() {
        let mut bad = encoded.clone();
        bad[index] ^= 1;
        assert!(PackedPage::open(&vault, context(), &bad).is_err());
    }
    fail.set(true);
    assert_eq!(
        page.seal(&mut vault).err(),
        Some(StorageError::Crypto(CryptoError::RetryableUnavailable))
    );
    assert!(PackedPage::open(&vault, context(), &encoded).is_ok());
    vault.lock();
    assert_eq!(
        page.seal(&mut vault).err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    assert_eq!(
        PackedPage::open(&vault, context(), &encoded).err(),
        Some(StorageError::Crypto(CryptoError::Locked))
    );
    let (wrong, _) = self::vault(100);
    assert!(PackedPage::open(&wrong, context(), &encoded).is_err());
}
