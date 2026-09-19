use super::*;
use crate::{
    FileSystem,
    memory::MemoryFileSystem,
    packed_index_pack::{ImmutablePackWriter, PackWriteLimits, read_record_page},
    packed_index_page::ENCODED_PAGE_BYTES,
};
use uste_crypto::{
    CryptoError, EntropyFailure, EntropySource, KeyAdapter, KeyVault, SecretKeyMaterial,
};

struct Entropy(u64);
impl EntropySource for Entropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), EntropyFailure> {
        self.0 += 1;
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = (self.0.wrapping_add(i as u64) & 255) as u8;
        }
        Ok(())
    }
}
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
        envelope: &Self::Envelope,
    ) -> Result<SecretKeyMaterial, CryptoError> {
        Ok(SecretKeyMaterial::from_adapter_bytes(*envelope))
    }
}

#[test]
fn packed_tree_record_typed_nodes_and_value_survive_encrypted_pack_restart() {
    let mut fs = MemoryFileSystem::default();
    let directory = fs.root();
    let c = owner();
    let mut vault = KeyVault::create(c.scope.database(), &mut Adapter, Entropy(10)).unwrap();
    let mut writer = ImmutablePackWriter::create(
        &mut fs,
        &directory,
        PackedPageContext {
            object: [0; 16],
            page: 0,
            ..c
        },
        PackWriteLimits {
            maximum_pages: 1,
            maximum_records: 4,
            maximum_payload_bytes: 512,
        },
        &mut Entropy(100),
    )
    .unwrap();
    let context = writer.context();
    let bytes = ValueChunk {
        remaining: 1,
        next: None,
        data: b"A",
    }
    .encode(context)
    .unwrap();
    let chunk_address = writer
        .append(&mut fs, &mut vault, PackedRecordKind::ValueChunk, &bytes)
        .unwrap();
    let left = TreeNode::Leaf {
        key: b"k",
        value: logical::value_commitment(b"A").unwrap(),
        first_chunk: Some(PackedLocator::from_address(chunk_address)),
    };
    let bytes = left.encode(context).unwrap();
    let left_address = writer
        .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, &bytes)
        .unwrap();
    let right = TreeNode::Leaf {
        key: b"z",
        value: logical::value_commitment(b"").unwrap(),
        first_chunk: None,
    };
    let bytes = right.encode(context).unwrap();
    let right_address = writer
        .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, &bytes)
        .unwrap();
    let branch = TreeNode::Branch {
        bit: 4,
        left: ChildReference {
            location: PackedLocator::from_address(left_address),
            claimed: left.commitment(context).unwrap(),
        },
        right: ChildReference {
            location: PackedLocator::from_address(right_address),
            claimed: right.commitment(context).unwrap(),
        },
    };
    let expected = branch.commitment(context).unwrap();
    let bytes = branch.encode(context).unwrap();
    let root_address = writer
        .append(&mut fs, &mut vault, PackedRecordKind::TreeNode, &bytes)
        .unwrap();
    let pack = writer.finish(&mut fs, &mut vault).unwrap();
    assert_eq!((pack.pages(), pack.records()), (1, 4));
    fs.restart().unwrap();
    let directory = fs.root();
    let (page, report) = read_record_page(
        &mut fs,
        &directory,
        &vault,
        pack,
        root_address,
        ENCODED_PAGE_BYTES as u64,
    )
    .unwrap();
    assert_eq!(report.pages, 1);
    let actual = TreeNode::decode(
        root_address.context(),
        page.record(root_address.slot()).unwrap(),
    )
    .unwrap();
    assert_eq!(actual.commitment(root_address.context()).unwrap(), expected);
    let TreeNode::Branch { left, right, .. } = actual else {
        panic!("branch")
    };
    for (claimed, address) in [(left, left_address), (right, right_address)] {
        assert!(claimed.location.page_context(context) == address.context());
        assert_eq!(claimed.location.slot(), address.slot());
        let child =
            TreeNode::decode(address.context(), page.record(address.slot()).unwrap()).unwrap();
        assert_eq!(
            child.commitment(address.context()).unwrap(),
            claimed.claimed
        );
    }
    let TreeNode::Leaf {
        value, first_chunk, ..
    } = TreeNode::decode(
        left_address.context(),
        page.record(left_address.slot()).unwrap(),
    )
    .unwrap()
    else {
        panic!("leaf")
    };
    assert!(first_chunk.unwrap().page_context(context) == chunk_address.context());
    let chunk = ValueChunk::decode(
        chunk_address.context(),
        page.record(chunk_address.slot()).unwrap(),
    )
    .unwrap();
    assert_eq!(chunk.remaining as u64, value.length);
    assert!(logical::value_commitment(chunk.data).unwrap() == value);
    assert!(chunk.next.is_none());
}
