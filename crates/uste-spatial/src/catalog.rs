use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use uste_types::{
    CommitRevision, RecordRef,
    spatial::{MAX_FRAME_DEPTH, SpatialVersion, VersionedRecordRef},
};

use crate::FrameDefinition;
use crate::codec::encode_record_ref;
use crate::model::{
    GeometryVersion, MAX_SPATIAL_BATCH_RECORDS, MAX_SPATIAL_CATALOG_ENTRIES,
    MAX_SPATIAL_CATALOG_LOGICAL_BYTES, ObservationKey, PositionObservation, SpatialError,
    SpatialRecord, SpatialRecordRef, WorldDefinition, accepts_geometry,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationOutcome {
    Inserted { id: RecordRef },
    Duplicate { existing: RecordRef },
}

#[derive(Clone, Debug)]
struct HistoryEntry<T> {
    recorded_revision: CommitRevision,
    value: T,
}

/// Bounded in-memory R1 reference history and admission validator.
///
/// This is not a spatial query index. Exact immutable frame versions are retained so later frame
/// versions cannot rewrite geometry or observation ancestry.
#[derive(Clone, Debug, Default)]
pub struct SpatialCatalog {
    worlds: BTreeMap<RecordRef, Vec<HistoryEntry<WorldDefinition>>>,
    frames: BTreeMap<RecordRef, Vec<HistoryEntry<FrameDefinition>>>,
    geometries: BTreeMap<RecordRef, Vec<HistoryEntry<GeometryVersion>>>,
    observations: BTreeMap<RecordRef, HistoryEntry<PositionObservation>>,
    source_events: BTreeMap<ObservationKey, RecordRef>,
    last_revision: Option<CommitRevision>,
    entry_count: usize,
    logical_bytes: usize,
    content_anchor: [u8; 32],
}

#[derive(Debug)]
pub(crate) struct PreparedCatalogBatch {
    base_revision: Option<CommitRevision>,
    base_entry_count: usize,
    base_logical_bytes: usize,
    base_content_anchor: [u8; 32],
    revision: CommitRevision,
    inserted: Vec<SpatialRecord>,
    outcomes: Vec<ObservationOutcome>,
    effect_revisions: Vec<CommitRevision>,
    resulting_entry_count: usize,
    resulting_logical_bytes: usize,
    resulting_content_anchor: [u8; 32],
}

impl PreparedCatalogBatch {
    pub(crate) fn outcomes(&self) -> &[ObservationOutcome] {
        &self.outcomes
    }

    pub(crate) fn effect_revisions(&self) -> &[CommitRevision] {
        &self.effect_revisions
    }

    pub(crate) fn inserted_count(&self) -> usize {
        self.inserted.len()
    }
}

impl SpatialCatalog {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Atomically admit one committed record batch. Forward references within the batch are valid.
    pub fn apply_batch(
        &mut self,
        revision: CommitRevision,
        records: &[SpatialRecord],
    ) -> Result<Vec<ObservationOutcome>, SpatialError> {
        let prepared = self.prepare_batch(revision, records)?;
        Ok(self.publish_prepared(prepared))
    }

    /// Validate a batch against a borrowed base while retaining only request-sized changes.
    pub(crate) fn prepare_batch(
        &self,
        revision: CommitRevision,
        records: &[SpatialRecord],
    ) -> Result<PreparedCatalogBatch, SpatialError> {
        if records.len() > MAX_SPATIAL_BATCH_RECORDS {
            return Err(SpatialError::ResourceLimit);
        }
        if records.is_empty()
            || self
                .last_revision
                .is_some_and(|previous| revision <= previous)
        {
            return Err(SpatialError::VersionConflict);
        }
        let mut preparation = CatalogPreparation::new(self, revision, records.len())?;
        for (index, record) in records.iter().enumerate() {
            preparation.stage(index, record)?;
        }
        for record in records {
            validate_record(&preparation, revision, record)?;
        }
        preparation.finish(records)
    }

    /// Publish a delta previously validated against this exact catalog base.
    pub(crate) fn publish_prepared(
        &mut self,
        prepared: PreparedCatalogBatch,
    ) -> Vec<ObservationOutcome> {
        let PreparedCatalogBatch {
            base_revision,
            base_entry_count,
            base_logical_bytes,
            base_content_anchor,
            revision,
            inserted,
            outcomes,
            effect_revisions: _,
            resulting_entry_count,
            resulting_logical_bytes,
            resulting_content_anchor,
        } = prepared;
        assert_eq!(
            (
                self.last_revision,
                self.entry_count,
                self.logical_bytes,
                self.content_anchor,
            ),
            (
                base_revision,
                base_entry_count,
                base_logical_bytes,
                base_content_anchor,
            ),
            "prepared spatial delta must publish on its exact base state"
        );
        for record in inserted {
            match record {
                SpatialRecord::World(value) => {
                    self.worlds
                        .entry(value.id())
                        .or_default()
                        .push(HistoryEntry {
                            recorded_revision: revision,
                            value,
                        })
                }
                SpatialRecord::Frame(value) => {
                    self.frames
                        .entry(value.id())
                        .or_default()
                        .push(HistoryEntry {
                            recorded_revision: revision,
                            value,
                        })
                }
                SpatialRecord::Geometry(value) => self
                    .geometries
                    .entry(value.id())
                    .or_default()
                    .push(HistoryEntry {
                        recorded_revision: revision,
                        value,
                    }),
                SpatialRecord::Observation(value) => {
                    self.source_events.insert(value.key().clone(), value.id());
                    self.observations.insert(
                        value.id(),
                        HistoryEntry {
                            recorded_revision: revision,
                            value: *value,
                        },
                    );
                }
            }
        }
        self.last_revision = Some(revision);
        self.entry_count = resulting_entry_count;
        self.logical_bytes = resulting_logical_bytes;
        self.content_anchor = resulting_content_anchor;
        outcomes
    }

    pub(crate) fn can_publish(&self, prepared: &PreparedCatalogBatch) -> bool {
        (
            self.last_revision,
            self.entry_count,
            self.logical_bytes,
            self.content_anchor,
        ) == (
            prepared.base_revision,
            prepared.base_entry_count,
            prepared.base_logical_bytes,
            prepared.base_content_anchor,
        )
    }

    #[must_use]
    pub const fn last_revision(&self) -> Option<CommitRevision> {
        self.last_revision
    }

    pub fn world_at(
        &self,
        reference: VersionedRecordRef,
        knowledge_revision: CommitRevision,
    ) -> Result<&WorldDefinition, SpatialError> {
        visible(
            history_entry(self.worlds.get(&reference.record), reference.version),
            knowledge_revision,
            SpatialError::MissingWorld,
        )
    }

    pub fn frame_at(
        &self,
        reference: VersionedRecordRef,
        knowledge_revision: CommitRevision,
    ) -> Result<&FrameDefinition, SpatialError> {
        visible(
            history_entry(self.frames.get(&reference.record), reference.version),
            knowledge_revision,
            SpatialError::MissingFrameVersion,
        )
    }

    pub fn geometry_at(
        &self,
        reference: VersionedRecordRef,
        knowledge_revision: CommitRevision,
    ) -> Result<&GeometryVersion, SpatialError> {
        visible(
            history_entry(self.geometries.get(&reference.record), reference.version),
            knowledge_revision,
            SpatialError::MissingGeometryVersion,
        )
    }

    pub fn observation_at(
        &self,
        id: RecordRef,
        knowledge_revision: CommitRevision,
    ) -> Result<&PositionObservation, SpatialError> {
        visible(
            self.observations.get(&id),
            knowledge_revision,
            SpatialError::MissingObservation,
        )
    }

    #[must_use]
    pub fn frame_history_len(&self, id: RecordRef) -> usize {
        self.frames.get(&id).map_or(0, Vec::len)
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.entry_count
    }

    /// Visit every retained version in deterministic canonical order.
    pub fn visit_records<E>(
        &self,
        mut visitor: impl FnMut(CommitRevision, SpatialRecordRef<'_>) -> Result<(), E>,
    ) -> Result<(), E> {
        for entries in self.worlds.values() {
            for entry in entries {
                visitor(
                    entry.recorded_revision,
                    SpatialRecordRef::World(&entry.value),
                )?;
            }
        }
        for entries in self.frames.values() {
            for entry in entries {
                visitor(
                    entry.recorded_revision,
                    SpatialRecordRef::Frame(&entry.value),
                )?;
            }
        }
        for entries in self.geometries.values() {
            for entry in entries {
                visitor(
                    entry.recorded_revision,
                    SpatialRecordRef::Geometry(&entry.value),
                )?;
            }
        }
        for entry in self.observations.values() {
            visitor(
                entry.recorded_revision,
                SpatialRecordRef::Observation(&entry.value),
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Category {
    World,
    Frame,
    Geometry,
    Observation,
}

struct CatalogPreparation<'a> {
    base: &'a SpatialCatalog,
    revision: CommitRevision,
    worlds: BTreeMap<RecordRef, Vec<&'a WorldDefinition>>,
    frames: BTreeMap<RecordRef, Vec<&'a FrameDefinition>>,
    geometries: BTreeMap<RecordRef, Vec<&'a GeometryVersion>>,
    observations: BTreeMap<RecordRef, &'a PositionObservation>,
    source_events: BTreeMap<ObservationKey, RecordRef>,
    inserted_indices: Vec<usize>,
    outcomes: Vec<ObservationOutcome>,
    effect_revisions: Vec<CommitRevision>,
    new_logical_bytes: usize,
    resulting_content_anchor: [u8; 32],
}

impl<'a> CatalogPreparation<'a> {
    fn new(
        base: &'a SpatialCatalog,
        revision: CommitRevision,
        record_count: usize,
    ) -> Result<Self, SpatialError> {
        let observation_count = record_count;
        let mut inserted_indices = Vec::new();
        let mut outcomes = Vec::new();
        let mut effect_revisions = Vec::new();
        inserted_indices
            .try_reserve_exact(record_count)
            .map_err(|_| SpatialError::ResourceLimit)?;
        outcomes
            .try_reserve_exact(observation_count)
            .map_err(|_| SpatialError::ResourceLimit)?;
        effect_revisions
            .try_reserve_exact(record_count)
            .map_err(|_| SpatialError::ResourceLimit)?;
        Ok(Self {
            base,
            revision,
            worlds: BTreeMap::new(),
            frames: BTreeMap::new(),
            geometries: BTreeMap::new(),
            observations: BTreeMap::new(),
            source_events: BTreeMap::new(),
            inserted_indices,
            outcomes,
            effect_revisions,
            new_logical_bytes: 0,
            resulting_content_anchor: base.content_anchor,
        })
    }

    fn stage(&mut self, index: usize, record: &'a SpatialRecord) -> Result<(), SpatialError> {
        match record {
            SpatialRecord::World(value) => self.stage_world(index, value),
            SpatialRecord::Frame(value) => self.stage_frame(index, value),
            SpatialRecord::Geometry(value) => self.stage_geometry(index, value),
            SpatialRecord::Observation(value) => self.stage_observation(index, value),
        }
    }

    fn stage_world(
        &mut self,
        index: usize,
        world: &'a WorldDefinition,
    ) -> Result<(), SpatialError> {
        self.ensure_category(world.id(), Category::World)?;
        let previous = self.latest_world(world.id());
        check_next_value(previous, world.version())?;
        if previous
            .is_some_and(|previous| previous.root_frame().record != world.root_frame().record)
        {
            return Err(SpatialError::InvalidRootFrame);
        }
        self.charge(index, SpatialRecordRef::World(world))?;
        let history = self.worlds.entry(world.id()).or_default();
        history
            .try_reserve(1)
            .map_err(|_| SpatialError::ResourceLimit)?;
        history.push(world);
        Ok(())
    }

    fn stage_frame(
        &mut self,
        index: usize,
        frame: &'a FrameDefinition,
    ) -> Result<(), SpatialError> {
        self.ensure_category(frame.id(), Category::Frame)?;
        let previous = self.latest_frame(frame.id());
        check_next_value(previous, frame.version())?;
        if previous.is_some_and(|previous| {
            previous.world() != frame.world() || previous.kind() != frame.kind()
        }) {
            return Err(SpatialError::VersionConflict);
        }
        self.charge(index, SpatialRecordRef::Frame(frame))?;
        let history = self.frames.entry(frame.id()).or_default();
        history
            .try_reserve(1)
            .map_err(|_| SpatialError::ResourceLimit)?;
        history.push(frame);
        Ok(())
    }

    fn stage_geometry(
        &mut self,
        index: usize,
        geometry: &'a GeometryVersion,
    ) -> Result<(), SpatialError> {
        self.ensure_category(geometry.id(), Category::Geometry)?;
        let previous = self.latest_geometry(geometry.id());
        check_next_value(previous, geometry.version())?;
        match (previous, geometry.predecessor()) {
            (None, None) => {}
            (Some(previous), Some(reference))
                if reference.record == geometry.id() && reference.version == previous.version() => {
            }
            _ => return Err(SpatialError::InvalidPredecessor),
        }
        self.charge(index, SpatialRecordRef::Geometry(geometry))?;
        let history = self.geometries.entry(geometry.id()).or_default();
        history
            .try_reserve(1)
            .map_err(|_| SpatialError::ResourceLimit)?;
        history.push(geometry);
        Ok(())
    }

    fn stage_observation(
        &mut self,
        index: usize,
        observation: &'a PositionObservation,
    ) -> Result<(), SpatialError> {
        self.ensure_category(observation.id(), Category::Observation)?;
        if let Some((recorded_revision, existing)) = self.observation(observation.id()) {
            if existing != observation {
                return Err(SpatialError::DuplicateRecord);
            }
            self.outcomes.push(ObservationOutcome::Duplicate {
                existing: observation.id(),
            });
            self.effect_revisions.push(recorded_revision);
            return Ok(());
        }
        if let Some(existing_id) = self.source_event(observation.key()) {
            let (recorded_revision, existing) = self
                .observation(existing_id)
                .ok_or(SpatialError::InvalidObservation)?;
            if existing != observation {
                return Err(SpatialError::SourceEventConflict);
            }
            self.outcomes.push(ObservationOutcome::Duplicate {
                existing: existing_id,
            });
            self.effect_revisions.push(recorded_revision);
            return Ok(());
        }
        self.charge(index, SpatialRecordRef::Observation(observation))?;
        self.observations.insert(observation.id(), observation);
        self.source_events
            .insert(observation.key().clone(), observation.id());
        self.outcomes.push(ObservationOutcome::Inserted {
            id: observation.id(),
        });
        Ok(())
    }

    fn charge(&mut self, index: usize, record: SpatialRecordRef<'_>) -> Result<(), SpatialError> {
        let resulting_entry_count = self
            .base
            .entry_count
            .checked_add(self.inserted_indices.len())
            .and_then(|count| count.checked_add(1))
            .ok_or(SpatialError::ResourceLimit)?;
        let encoded = encode_record_ref(record)?;
        let encoded_length = encoded.len();
        let new_logical_bytes = self
            .new_logical_bytes
            .checked_add(12)
            .and_then(|bytes| bytes.checked_add(encoded_length))
            .ok_or(SpatialError::ResourceLimit)?;
        let resulting_logical_bytes = self
            .base
            .logical_bytes
            .checked_add(new_logical_bytes)
            .ok_or(SpatialError::ResourceLimit)?;
        if resulting_entry_count > MAX_SPATIAL_CATALOG_ENTRIES
            || resulting_logical_bytes > MAX_SPATIAL_CATALOG_LOGICAL_BYTES
        {
            return Err(SpatialError::ResourceLimit);
        }
        self.inserted_indices.push(index);
        self.effect_revisions.push(self.revision);
        self.new_logical_bytes = new_logical_bytes;
        xor_anchor(
            &mut self.resulting_content_anchor,
            entry_anchor(self.revision, &encoded),
        );
        Ok(())
    }

    fn finish(self, records: &[SpatialRecord]) -> Result<PreparedCatalogBatch, SpatialError> {
        if self.effect_revisions.len() != records.len() {
            return Err(SpatialError::InvalidEncoding);
        }
        let mut inserted = Vec::new();
        inserted
            .try_reserve_exact(self.inserted_indices.len())
            .map_err(|_| SpatialError::ResourceLimit)?;
        for index in self.inserted_indices {
            inserted.push(
                records
                    .get(index)
                    .ok_or(SpatialError::InvalidEncoding)?
                    .clone(),
            );
        }
        Ok(PreparedCatalogBatch {
            base_revision: self.base.last_revision,
            base_entry_count: self.base.entry_count,
            base_logical_bytes: self.base.logical_bytes,
            base_content_anchor: self.base.content_anchor,
            revision: self.revision,
            resulting_entry_count: self
                .base
                .entry_count
                .checked_add(inserted.len())
                .ok_or(SpatialError::ResourceLimit)?,
            resulting_logical_bytes: self
                .base
                .logical_bytes
                .checked_add(self.new_logical_bytes)
                .ok_or(SpatialError::ResourceLimit)?,
            resulting_content_anchor: self.resulting_content_anchor,
            inserted,
            outcomes: self.outcomes,
            effect_revisions: self.effect_revisions,
        })
    }

    fn category(&self, id: RecordRef) -> Option<Category> {
        if self.worlds.contains_key(&id) || self.base.worlds.contains_key(&id) {
            Some(Category::World)
        } else if self.frames.contains_key(&id) || self.base.frames.contains_key(&id) {
            Some(Category::Frame)
        } else if self.geometries.contains_key(&id) || self.base.geometries.contains_key(&id) {
            Some(Category::Geometry)
        } else if self.observations.contains_key(&id) || self.base.observations.contains_key(&id) {
            Some(Category::Observation)
        } else {
            None
        }
    }

    fn ensure_category(&self, id: RecordRef, expected: Category) -> Result<(), SpatialError> {
        match self.category(id) {
            None => Ok(()),
            Some(actual) if actual == expected => Ok(()),
            Some(_) => Err(SpatialError::DuplicateRecord),
        }
    }

    fn latest_world(&self, id: RecordRef) -> Option<&'a WorldDefinition> {
        self.worlds
            .get(&id)
            .and_then(|history| history.last().copied())
            .or_else(|| self.base.worlds.get(&id)?.last().map(|entry| &entry.value))
    }

    fn latest_frame(&self, id: RecordRef) -> Option<&'a FrameDefinition> {
        self.frames
            .get(&id)
            .and_then(|history| history.last().copied())
            .or_else(|| self.base.frames.get(&id)?.last().map(|entry| &entry.value))
    }

    fn latest_geometry(&self, id: RecordRef) -> Option<&'a GeometryVersion> {
        self.geometries
            .get(&id)
            .and_then(|history| history.last().copied())
            .or_else(|| {
                self.base
                    .geometries
                    .get(&id)?
                    .last()
                    .map(|entry| &entry.value)
            })
    }

    fn world_visible(&self, id: RecordRef) -> Result<&'a WorldDefinition, SpatialError> {
        self.latest_world(id).ok_or(SpatialError::MissingWorld)
    }

    fn frame_at(&self, reference: VersionedRecordRef) -> Result<&'a FrameDefinition, SpatialError> {
        self.frames
            .get(&reference.record)
            .and_then(|history| {
                history
                    .iter()
                    .copied()
                    .find(|frame| frame.version() == reference.version)
            })
            .or_else(|| {
                history_entry(self.base.frames.get(&reference.record), reference.version)
                    .map(|entry| &entry.value)
            })
            .ok_or(SpatialError::MissingFrameVersion)
    }

    fn observation(&self, id: RecordRef) -> Option<(CommitRevision, &'a PositionObservation)> {
        self.observations
            .get(&id)
            .copied()
            .map(|value| (self.revision, value))
            .or_else(|| {
                self.base
                    .observations
                    .get(&id)
                    .map(|entry| (entry.recorded_revision, &entry.value))
            })
    }

    fn source_event(&self, key: &ObservationKey) -> Option<RecordRef> {
        self.source_events
            .get(key)
            .copied()
            .or_else(|| self.base.source_events.get(key).copied())
    }
}

fn entry_anchor(revision: CommitRevision, encoded: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"USTE-SPATIAL-CATALOG-ENTRY-V1\0");
    digest.update(revision.get().to_be_bytes());
    digest.update((encoded.len() as u64).to_be_bytes());
    digest.update(encoded);
    digest.finalize().into()
}

fn xor_anchor(anchor: &mut [u8; 32], entry: [u8; 32]) {
    for (target, value) in anchor.iter_mut().zip(entry) {
        *target ^= value;
    }
}

fn check_next_value<T: HasVersion>(
    previous: Option<&T>,
    version: SpatialVersion,
) -> Result<(), SpatialError> {
    match previous {
        None if version == SpatialVersion::FIRST => Ok(()),
        Some(previous) if previous.version().checked_next()? == version => Ok(()),
        _ => Err(SpatialError::VersionConflict),
    }
}

fn validate_record(
    catalog: &CatalogPreparation<'_>,
    revision: CommitRevision,
    record: &SpatialRecord,
) -> Result<(), SpatialError> {
    match record {
        SpatialRecord::World(world) => {
            let root = catalog.frame_at(world.root_frame())?;
            if root.world() != world.id() || root.parent().is_some() {
                return Err(SpatialError::InvalidRootFrame);
            }
            Ok(())
        }
        SpatialRecord::Frame(frame) => validate_frame(catalog, frame),
        SpatialRecord::Geometry(geometry) => {
            catalog.world_visible(geometry.world())?;
            let frame = catalog.frame_at(geometry.frame())?;
            if frame.world() != geometry.world()
                || !accepts_geometry(frame.kind(), geometry.geometry())
            {
                return Err(SpatialError::IncompatibleFrame);
            }
            Ok(())
        }
        SpatialRecord::Observation(observation) => {
            catalog.world_visible(observation.world())?;
            let frame = catalog.frame_at(observation.frame())?;
            if frame.world() != observation.world()
                || !frame.profile().accepts(observation.position())
            {
                return Err(SpatialError::IncompatibleFrame);
            }
            if let Some(previous_id) = observation.correction_of() {
                let (recorded_revision, previous) = catalog
                    .observation(previous_id)
                    .filter(|(recorded_revision, _)| *recorded_revision < revision)
                    .ok_or(SpatialError::InvalidCorrection)?;
                debug_assert!(recorded_revision < revision);
                if previous.entity() != observation.entity()
                    || previous.world() != observation.world()
                {
                    return Err(SpatialError::InvalidCorrection);
                }
            }
            Ok(())
        }
    }
}

fn validate_frame(
    catalog: &CatalogPreparation<'_>,
    frame: &FrameDefinition,
) -> Result<(), SpatialError> {
    let world = catalog.world_visible(frame.world())?;
    if world.root_frame().record == frame.id() {
        if frame.parent().is_some() {
            return Err(SpatialError::InvalidRootFrame);
        }
        return Ok(());
    }
    let mut parent = frame.parent().ok_or(SpatialError::MissingFrame)?;
    let mut seen = BTreeSet::new();
    seen.insert(frame.id());
    let mut depth = 0_usize;
    loop {
        depth = depth.checked_add(1).ok_or(SpatialError::ResourceLimit)?;
        if depth > MAX_FRAME_DEPTH {
            return Err(SpatialError::ResourceLimit);
        }
        if !seen.insert(parent.frame.record) {
            return Err(SpatialError::FrameCycle);
        }
        let next = catalog.frame_at(parent.frame)?;
        if next.world() != frame.world() {
            return Err(SpatialError::ScopeMismatch);
        }
        match next.parent() {
            Some(next_parent) => parent = next_parent,
            None if next.id() == world.root_frame().record => return Ok(()),
            None => return Err(SpatialError::InvalidRootFrame),
        }
    }
}

trait HasVersion {
    fn version(&self) -> SpatialVersion;
}

impl HasVersion for WorldDefinition {
    fn version(&self) -> SpatialVersion {
        self.version()
    }
}

impl HasVersion for FrameDefinition {
    fn version(&self) -> SpatialVersion {
        self.version()
    }
}

impl HasVersion for GeometryVersion {
    fn version(&self) -> SpatialVersion {
        self.version()
    }
}

fn history_entry<T>(
    history: Option<&Vec<HistoryEntry<T>>>,
    version: SpatialVersion,
) -> Option<&HistoryEntry<T>> {
    let index = usize::try_from(version.get().checked_sub(1)?).ok()?;
    history?.get(index)
}

fn visible<T>(
    entry: Option<&HistoryEntry<T>>,
    revision: CommitRevision,
    missing: SpatialError,
) -> Result<&T, SpatialError> {
    entry
        .filter(|entry| entry.recorded_revision <= revision)
        .map(|entry| &entry.value)
        .ok_or(missing)
}
