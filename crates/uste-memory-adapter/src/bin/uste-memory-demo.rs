#![forbid(unsafe_code)]
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

use uste_crypto::RecoveryPassword;
use uste_memory::{
    EventTimeFilter, FactInput, KnowledgeAt, MemoryReadOutput, MemoryReadRequest, SourceLocator,
    SourceVersionId,
};
use uste_memory_adapter::{
    ConsumerAuthority, ConsumerCheckpoint, LocalAdapterConfig, LocalAdapterError,
    LocalMemoryAdapter, OperationIdentity, PendingSourceUpload, SourceEncoding,
};
use uste_policy::{
    Action, AuthenticationError, NamespaceGrant, NamespacePolicy, PermissionSet, PolicyKernel,
    PolicyVersion, PrincipalDigest, QuotaLimits, TrustedPrincipalAdapter,
};
use uste_storage::{BLOB_CHUNK_BYTES, BlobUploadToken, EntryName, linux::LinuxFilesystemProfile};
use uste_types::{
    DatabaseId, IdempotencyKey, NamespaceId, NamespaceRef, RecordId, RecordRef, TransactionId,
    UtcInstant,
};

const PASSWORD: &[u8] = b"uste-m1-synthetic-demo-password";
const CHECKPOINT: &str = "consumer-checkpoint-v1.bin";
const CHECKPOINT_TEMP: &str = "consumer-checkpoint-v1.pending";
const SOURCE: &str = "consumer-source-v1.txt";
const CHECKPOINT_MAGIC: &[u8; 4] = b"UMCP";

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [root] => run(Path::new(root)),
        [command, root] if command == "crash-probe" => crash_probe(Path::new(root)),
        [command, root] if command == "verify-crash" => verify_crash(Path::new(root), false),
        [command, root] if command == "verify-wrong-key" => verify_crash(Path::new(root), true),
        [command, root] if command == "measure" => measure(Path::new(root)),
        _ => Err(
            "usage: uste-memory-demo [crash-probe|verify-crash|verify-wrong-key|measure] ROOT"
                .into(),
        ),
    }
}

fn measure(root: &Path) -> Result<(), Box<dyn Error>> {
    if !root.is_dir() || fs::read_dir(root)?.next().is_some() {
        return Err("ROOT must be an existing empty directory".into());
    }
    let source = SourceVersionId {
        source: record(10),
        version: 1,
    };
    let bytes = vec![0x5a; 1024 * 1024];
    let consumer_root = root.join("consumer-authority");
    let engine_root = root.join("uste-derived-index");
    fs::DirBuilder::new().mode(0o700).create(&consumer_root)?;
    fs::DirBuilder::new().mode(0o700).create(&engine_root)?;
    File::open(root)?.sync_all()?;
    let mut authority = DiskAuthority::create(&consumer_root, scope(), source, &bytes)?;
    let config = LocalAdapterConfig {
        root: engine_root,
        database_name: EntryName::new("memory-demo")?,
        scope: scope(),
        filesystem_profile: LinuxFilesystemProfile::Btrfs,
    };
    let (policy, principal) = policy_and_principal(scope())?;
    let mut adapter = LocalMemoryAdapter::create(
        &config,
        password()?,
        policy,
        principal,
        &mut authority,
        operation(1),
    )?;
    let ingest_started = Instant::now();
    adapter.ingest_source(
        &mut authority,
        source,
        SourceEncoding::Opaque,
        None,
        operation(2),
    )?;
    let ingest_elapsed = ingest_started.elapsed();
    adapter.put_fact(
        FactInput {
            id: record(20),
            subject: "pilot".to_owned(),
            predicate: "measurement".to_owned(),
            value: "bounded".to_owned(),
            links: Vec::new(),
            source,
            locator: SourceLocator::ByteRange { start: 0, end: 1 },
            source_event_time: None,
        },
        operation(3),
    )?;
    adapter.complete_rebuild(operation(4))?;
    let query = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(20),
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    };
    for _ in 0..100 {
        let _ = adapter.query(&query, &uste_txn::NeverCancel)?;
    }
    let mut latencies = Vec::with_capacity(1_000);
    for _ in 0..1_000 {
        let started = Instant::now();
        let _ = adapter.query(&query, &uste_txn::NeverCancel)?;
        latencies.push(started.elapsed().as_micros());
    }
    latencies.sort_unstable();
    drop(adapter);

    let cold_started = Instant::now();
    let mut authority = DiskAuthority::open(&consumer_root, source)?;
    let (policy, principal) = policy_and_principal(scope())?;
    let mut adapter =
        LocalMemoryAdapter::open(&config, password()?, policy, principal, &mut authority)?;
    let cold_millis = cold_started.elapsed().as_millis();
    if !matches!(
        adapter.query(&query, &uste_txn::NeverCancel)?,
        MemoryReadOutput::Fact(_)
    ) {
        return Err("measurement recovery query mismatch".into());
    }
    let nanos = ingest_elapsed.as_nanos();
    let ingest_bytes_per_second = (bytes.len() as u128)
        .checked_mul(1_000_000_000)
        .ok_or("throughput overflow")?
        .checked_div(nanos)
        .unwrap_or(u128::MAX);
    println!(
        concat!(
            "M1_MEASURE schema=memory-pilot-measure-v1 source_bytes={} ",
            "ingest_bytes_per_second={} cold_recovery_millis={} ",
            "warm_query_p50_micros={} warm_query_p95_micros={} warm_query_p99_micros={} ",
            "warm_samples=1000"
        ),
        bytes.len(),
        ingest_bytes_per_second,
        cold_millis,
        latencies[499],
        latencies[949],
        latencies[989],
    );
    Ok(())
}

fn crash_probe(root: &Path) -> Result<(), Box<dyn Error>> {
    if !root.is_dir() || fs::read_dir(root)?.next().is_some() {
        return Err("ROOT must be an existing empty directory".into());
    }
    let source = SourceVersionId {
        source: record(10),
        version: 1,
    };
    let bytes = b"project status alpha\nproject status beta\n";
    let consumer_root = root.join("consumer-authority");
    let engine_root = root.join("uste-derived-index");
    fs::DirBuilder::new().mode(0o700).create(&consumer_root)?;
    fs::DirBuilder::new().mode(0o700).create(&engine_root)?;
    File::open(root)?.sync_all()?;
    let mut authority = DiskAuthority::create(&consumer_root, scope(), source, bytes)?;
    let config = LocalAdapterConfig {
        root: engine_root,
        database_name: EntryName::new("memory-demo")?,
        scope: scope(),
        filesystem_profile: LinuxFilesystemProfile::Btrfs,
    };
    let (policy, principal) = policy_and_principal(scope())?;
    let mut adapter = LocalMemoryAdapter::create(
        &config,
        password()?,
        policy,
        principal,
        &mut authority,
        operation(1),
    )?;
    adapter.ingest_source(
        &mut authority,
        source,
        SourceEncoding::ExactUtf8,
        Some(UtcInstant::new(1_700_000_000, 0)?),
        operation(2),
    )?;
    adapter.put_fact(fact(20, source, "alpha", 0, 20, 1), operation(3))?;
    let ready = adapter.complete_rebuild(operation(4))?;
    if adapter
        .resolve_citation(&citation(1, record(20)))?
        .exact_source_bytes
        != bytes
    {
        return Err("crash-probe precondition mismatch".into());
    }
    println!(
        "M1_CRASH_READY schema=memory-pilot-v1 revision={}",
        ready.revision.get()
    );
    std::io::stdout().flush()?;
    loop {
        std::thread::park();
    }
}

fn verify_crash(root: &Path, wrong_key: bool) -> Result<(), Box<dyn Error>> {
    let consumer_root = root.join("consumer-authority");
    let source = SourceVersionId {
        source: record(10),
        version: 1,
    };
    let bytes = fs::read(consumer_root.join(SOURCE))?;
    let mut authority = DiskAuthority::open(&consumer_root, source)?;
    let config = LocalAdapterConfig {
        root: root.join("uste-derived-index"),
        database_name: EntryName::new("memory-demo")?,
        scope: scope(),
        filesystem_profile: LinuxFilesystemProfile::Btrfs,
    };
    let (policy, principal) = policy_and_principal(scope())?;
    let supplied_password = if wrong_key {
        RecoveryPassword::new(b"uste-m1-deliberately-wrong-password".to_vec())?
    } else {
        password()?
    };
    let mut adapter = LocalMemoryAdapter::open(
        &config,
        supplied_password,
        policy,
        principal,
        &mut authority,
    )?;
    let resolved = adapter.resolve_citation(&citation(1, record(20)))?;
    if resolved.exact_source_bytes != bytes
        || resolved.citation.exact_utf8_excerpt.as_deref() != Some("project status alpha")
    {
        return Err("crash recovery citation mismatch".into());
    }
    println!("M1_CRASH_RECOVERED revision=4 citation=exact");
    Ok(())
}

fn run(root: &Path) -> Result<(), Box<dyn Error>> {
    if !root.is_dir() || fs::read_dir(root)?.next().is_some() {
        return Err("ROOT must be an existing empty directory".into());
    }
    let scope = scope();
    let source = SourceVersionId {
        source: record(10),
        version: 1,
    };
    let bytes = b"project status alpha\nproject status beta\n";
    let consumer_root = root.join("consumer-authority");
    let engine_root = root.join("uste-derived-index");
    fs::DirBuilder::new().mode(0o700).create(&consumer_root)?;
    fs::DirBuilder::new().mode(0o700).create(&engine_root)?;
    File::open(root)?.sync_all()?;
    let mut authority = DiskAuthority::create(&consumer_root, scope, source, bytes)?;
    let config = LocalAdapterConfig {
        root: engine_root,
        database_name: EntryName::new("memory-demo")?,
        scope,
        filesystem_profile: LinuxFilesystemProfile::Btrfs,
    };
    let (policy, principal) = policy_and_principal(scope)?;
    let mut adapter = LocalMemoryAdapter::create(
        &config,
        password()?,
        policy,
        principal,
        &mut authority,
        operation(1),
    )?;
    adapter.ingest_source(
        &mut authority,
        source,
        SourceEncoding::ExactUtf8,
        Some(UtcInstant::new(1_700_000_000, 0)?),
        operation(2),
    )?;
    adapter.put_fact(fact(20, source, "alpha", 0, 20, 1), operation(3))?;
    let ready = adapter.complete_rebuild(operation(4))?;
    let resolved = adapter.resolve_citation(&citation(1, record(20)))?;
    if resolved.exact_source_bytes != bytes
        || resolved.citation.exact_utf8_excerpt.as_deref() != Some("project status alpha")
    {
        return Err("initial citation mismatch".into());
    }
    println!(
        "M1_DEMO phase=created revision={} fact=20 citation=exact",
        ready.revision.get()
    );

    let mut competing_authority = DiskAuthority::open(&consumer_root, source)?;
    let (competing_policy, competing_principal) = policy_and_principal(scope)?;
    let competing = LocalMemoryAdapter::open(
        &config,
        password()?,
        competing_policy,
        competing_principal,
        &mut competing_authority,
    );
    if !matches!(competing, Err(LocalAdapterError::Locked)) {
        return Err("second owner did not receive USTE_MEMORY_LOCKED".into());
    }
    println!("M1_DEMO phase=lock-conflict result=USTE_MEMORY_LOCKED");
    drop(adapter);

    let mut authority = DiskAuthority::open(&consumer_root, source)?;
    let (policy, principal) = policy_and_principal(scope)?;
    let mut adapter =
        LocalMemoryAdapter::open(&config, password()?, policy, principal, &mut authority)?;
    let before_correction = ready.revision;
    adapter.correct_fact(
        record(20),
        fact(21, source, "beta", 21, 40, 2),
        operation(5),
    )?;
    adapter.put_fact(fact(30, source, "alpha", 0, 20, 1), operation(6))?;
    let MemoryReadOutput::Search(current) = adapter.query(&search(1), &uste_txn::NeverCancel)?
    else {
        return Err("unexpected search output".into());
    };
    let historical = MemoryReadRequest::GetFact {
        authority_generation: 1,
        fact: record(20),
        knowledge: KnowledgeAt::Revision(before_correction),
        maximum_output_bytes: 4096,
    };
    if !matches!(
        adapter.query(&historical, &uste_txn::NeverCancel)?,
        MemoryReadOutput::Fact(_)
    ) || current.facts.len() != 2
    {
        return Err("correction/contradiction result mismatch".into());
    }
    println!(
        "M1_DEMO phase=reopened current_facts={} historical_fact=20",
        current.facts.len()
    );

    adapter.revoke_source(source, operation(7))?;
    if !matches!(
        adapter.resolve_citation(&citation(1, record(21))),
        Err(LocalAdapterError::NotFound)
    ) {
        return Err("revoked source remained visible".into());
    }
    println!("M1_DEMO phase=revoked result=USTE_MEMORY_NOT_FOUND");

    authority.set_generation(2)?;
    adapter.begin_rebuild(&mut authority, operation(8))?;
    adapter.ingest_source(
        &mut authority,
        source,
        SourceEncoding::ExactUtf8,
        Some(UtcInstant::new(1_700_000_000, 0)?),
        operation(9),
    )?;
    adapter.put_fact(fact(21, source, "beta", 21, 40, 2), operation(10))?;
    let rebuilt = adapter.complete_rebuild(operation(11))?;
    let resolved = adapter.resolve_citation(&citation(2, record(21)))?;
    if resolved.exact_source_bytes != bytes {
        return Err("rebuilt citation mismatch".into());
    }
    println!(
        "M1_DEMO_OK schema=memory-pilot-v1 generation=2 revision={} source_authority=consumer",
        rebuilt.revision.get()
    );
    Ok(())
}

fn scope() -> NamespaceRef {
    NamespaceRef::new(
        DatabaseId::from_bytes([0x41; 16]),
        NamespaceId::from_bytes([0x42; 16]),
    )
}

fn record(value: u8) -> RecordRef {
    RecordRef::new(
        scope().database(),
        scope().namespace(),
        RecordId::from_bytes([value; 16]),
    )
}

fn operation(value: u8) -> OperationIdentity {
    OperationIdentity {
        idempotency_key: IdempotencyKey::from_bytes([value; 16]),
        transaction_id: TransactionId::from_bytes([value.wrapping_add(64); 16]),
    }
}

fn fact(
    id: u8,
    source: SourceVersionId,
    value: &str,
    start: u64,
    end: u64,
    line: u32,
) -> FactInput {
    FactInput {
        id: record(id),
        subject: "project".to_owned(),
        predicate: "status".to_owned(),
        value: value.to_owned(),
        links: Vec::new(),
        source,
        locator: SourceLocator::Utf8Lines {
            start,
            end,
            first_line: line,
            last_line: line,
        },
        source_event_time: Some(UtcInstant::new(1_700_000_000 + i64::from(line), 0).unwrap()),
    }
}

fn citation(generation: u64, fact: RecordRef) -> MemoryReadRequest {
    MemoryReadRequest::ResolveCitation {
        authority_generation: generation,
        fact,
        knowledge: KnowledgeAt::Current,
        maximum_output_bytes: 4096,
    }
}

fn search(generation: u64) -> MemoryReadRequest {
    MemoryReadRequest::Search {
        scope: scope(),
        authority_generation: generation,
        terms: vec!["status".to_owned()],
        knowledge: KnowledgeAt::Current,
        event_time: EventTimeFilter::Any,
        maximum_candidates: 32,
        maximum_results: 8,
        maximum_output_bytes: 4096,
    }
}

fn password() -> Result<RecoveryPassword, Box<dyn Error>> {
    Ok(RecoveryPassword::new(PASSWORD.to_vec())?)
}

fn policy_and_principal(
    scope: NamespaceRef,
) -> Result<(PolicyKernel, uste_policy::AuthenticatedPrincipal), Box<dyn Error>> {
    let limits = QuotaLimits::new(
        128 * 1024,
        2 * 1024 * 1024,
        32 * 1024 * 1024,
        8,
        u32::try_from(BLOB_CHUNK_BYTES)?,
    )
    .map_err(|_| "invalid synthetic quota policy")?;
    let actions = PermissionSet::from_actions([
        Action::ReadRecord,
        Action::ReadHistory,
        Action::ExpandGraph,
        Action::Search,
        Action::ReadBlob,
        Action::Commit,
        Action::StartUpload,
        Action::ResumeUpload,
        Action::WriteUpload,
        Action::FinishUpload,
        Action::AbortUpload,
        Action::ReadOwnOutcome,
        Action::InspectQuota,
        Action::ManageSchema,
        Action::ManageRetention,
    ]);
    let mut namespace = NamespacePolicy::new(
        scope,
        PolicyVersion::new(1).map_err(|_| "invalid policy version")?,
        limits,
    );
    namespace
        .grant(
            PrincipalDigest::from_bytes([0x51; 32]),
            NamespaceGrant::new(actions, limits),
        )
        .map_err(|_| "invalid synthetic namespace grant")?;
    let mut policy = PolicyKernel::new();
    policy
        .install_initial_policy(namespace)
        .map_err(|_| "invalid synthetic initial policy")?;
    let principal = policy
        .authenticate(&mut AuthAdapter, &())
        .map_err(|_| "synthetic authentication failed")?;
    Ok((policy, principal))
}

struct AuthAdapter;

impl TrustedPrincipalAdapter for AuthAdapter {
    type Credential = ();

    fn authenticate(
        &mut self,
        _credential: &Self::Credential,
    ) -> Result<PrincipalDigest, AuthenticationError> {
        Ok(PrincipalDigest::from_bytes([0x51; 32]))
    }
}

struct DiskAuthority {
    root: PathBuf,
    source: SourceVersionId,
    checkpoint: ConsumerCheckpoint,
}

impl DiskAuthority {
    fn create(
        root: &Path,
        scope: NamespaceRef,
        source: SourceVersionId,
        bytes: &[u8],
    ) -> Result<Self, Box<dyn Error>> {
        write_new_synced(&root.join(SOURCE), bytes)?;
        let authority = Self {
            root: root.to_path_buf(),
            source,
            checkpoint: ConsumerCheckpoint {
                schema_version: 1,
                scope,
                authority_generation: 1,
                pending_uploads: Vec::new(),
            },
        };
        authority.persist()?;
        Ok(authority)
    }

    fn open(root: &Path, source: SourceVersionId) -> Result<Self, Box<dyn Error>> {
        let bytes = fs::read(root.join(CHECKPOINT))?;
        Ok(Self {
            root: root.to_path_buf(),
            source,
            checkpoint: decode_checkpoint(&bytes)?,
        })
    }

    fn set_generation(&mut self, generation: u64) -> Result<(), Box<dyn Error>> {
        let previous = self.checkpoint.authority_generation;
        self.checkpoint.authority_generation = generation;
        if let Err(error) = self.persist() {
            self.checkpoint.authority_generation = previous;
            return Err(error);
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), Box<dyn Error>> {
        let encoded = encode_checkpoint(&self.checkpoint)?;
        let pending = self.root.join(CHECKPOINT_TEMP);
        write_new_synced(&pending, &encoded)?;
        fs::rename(&pending, self.root.join(CHECKPOINT))?;
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}

impl ConsumerAuthority for DiskAuthority {
    fn checkpoint(&mut self) -> Result<ConsumerCheckpoint, LocalAdapterError> {
        Ok(self.checkpoint.clone())
    }

    fn source_bytes(&mut self, source: SourceVersionId) -> Result<Vec<u8>, LocalAdapterError> {
        if source != self.source {
            return Err(LocalAdapterError::SourceChanged);
        }
        fs::read(self.root.join(SOURCE)).map_err(|_| LocalAdapterError::SourceChanged)
    }

    fn persist_pending(&mut self, pending: &PendingSourceUpload) -> Result<(), LocalAdapterError> {
        if !self.checkpoint.pending_uploads.is_empty() {
            return Err(LocalAdapterError::InvalidCheckpoint);
        }
        self.checkpoint.pending_uploads.push(pending.clone());
        if self.persist().is_err() {
            self.checkpoint.pending_uploads.clear();
            return Err(LocalAdapterError::RetryableUnavailable);
        }
        Ok(())
    }

    fn clear_pending(&mut self, token: BlobUploadToken) -> Result<(), LocalAdapterError> {
        let previous = self.checkpoint.pending_uploads.clone();
        self.checkpoint
            .pending_uploads
            .retain(|pending| pending.token != token);
        if self.persist().is_err() {
            self.checkpoint.pending_uploads = previous;
            return Err(LocalAdapterError::RetryableUnavailable);
        }
        Ok(())
    }
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn encode_checkpoint(checkpoint: &ConsumerCheckpoint) -> Result<Vec<u8>, Box<dyn Error>> {
    checkpoint.validate()?;
    let mut output = Vec::new();
    output.extend_from_slice(CHECKPOINT_MAGIC);
    output.extend_from_slice(&checkpoint.schema_version.to_be_bytes());
    output.extend_from_slice(&checkpoint.authority_generation.to_be_bytes());
    output.extend_from_slice(checkpoint.scope.database().as_bytes());
    output.extend_from_slice(checkpoint.scope.namespace().as_bytes());
    output.push(u8::try_from(checkpoint.pending_uploads.len())?);
    for pending in &checkpoint.pending_uploads {
        output.extend_from_slice(&pending.token.upload_id());
        output.extend_from_slice(pending.source.source.record().as_bytes());
        output.extend_from_slice(&pending.source.version.to_be_bytes());
        output.push(match pending.encoding {
            SourceEncoding::ExactUtf8 => 1,
            SourceEncoding::Opaque => 2,
        });
        match pending.source_event_time {
            Some(instant) => {
                output.push(1);
                output.extend_from_slice(&instant.seconds().to_be_bytes());
                output.extend_from_slice(&instant.nanoseconds().to_be_bytes());
            }
            None => output.push(0),
        }
        output.extend_from_slice(pending.operation.idempotency_key.as_bytes());
        output.extend_from_slice(pending.operation.transaction_id.as_bytes());
    }
    Ok(output)
}

fn decode_checkpoint(input: &[u8]) -> Result<ConsumerCheckpoint, Box<dyn Error>> {
    let mut cursor = Cursor::new(input);
    if cursor.take(4)? != CHECKPOINT_MAGIC {
        return Err("unsupported consumer checkpoint magic".into());
    }
    let schema_version = u16::from_be_bytes(cursor.array()?);
    let authority_generation = u64::from_be_bytes(cursor.array()?);
    let scope = NamespaceRef::new(
        DatabaseId::from_bytes(cursor.array()?),
        NamespaceId::from_bytes(cursor.array()?),
    );
    let count = usize::from(cursor.byte()?);
    let mut pending_uploads = Vec::with_capacity(count);
    for _ in 0..count {
        let token = BlobUploadToken::from_upload_id(scope, cursor.array()?);
        let source = SourceVersionId {
            source: RecordRef::new(
                scope.database(),
                scope.namespace(),
                RecordId::from_bytes(cursor.array()?),
            ),
            version: u32::from_be_bytes(cursor.array()?),
        };
        let encoding = match cursor.byte()? {
            1 => SourceEncoding::ExactUtf8,
            2 => SourceEncoding::Opaque,
            _ => return Err("unsupported source encoding".into()),
        };
        let source_event_time = match cursor.byte()? {
            0 => None,
            1 => Some(UtcInstant::new(
                i64::from_be_bytes(cursor.array()?),
                u32::from_be_bytes(cursor.array()?),
            )?),
            _ => return Err("invalid event-time tag".into()),
        };
        pending_uploads.push(PendingSourceUpload {
            token,
            source,
            encoding,
            source_event_time,
            operation: OperationIdentity {
                idempotency_key: IdempotencyKey::from_bytes(cursor.array()?),
                transaction_id: TransactionId::from_bytes(cursor.array()?),
            },
        });
    }
    if cursor.remaining() != 0 {
        return Err("trailing consumer checkpoint bytes".into());
    }
    let checkpoint = ConsumerCheckpoint {
        schema_version,
        scope,
        authority_generation,
        pending_uploads,
    };
    checkpoint.validate()?;
    Ok(checkpoint)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    const fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], Box<dyn Error>> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or("checkpoint overflow")?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or("truncated consumer checkpoint")?;
        self.offset = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Box<dyn Error>> {
        Ok(self.take(N)?.try_into()?)
    }

    fn byte(&mut self) -> Result<u8, Box<dyn Error>> {
        Ok(self.array::<1>()?[0])
    }
}
