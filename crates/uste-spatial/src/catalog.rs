use std::collections::{BTreeMap, BTreeSet};

use uste_types::{
    CommitRevision, RecordRef,
    spatial::{MAX_FRAME_DEPTH, SpatialVersion, VersionedRecordRef},
};

use crate::FrameDefinition;
use crate::encode_record;
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
    logical_bytes: usize,
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
        let mut candidate = self.clone();
        let outcomes = candidate.apply_batch_prepared(revision, records)?;
        *self = candidate;
        Ok(outcomes)
    }

    /// Apply into state that is already an unpublished prepared copy. Callers must discard `self`
    /// after any error; this avoids cloning the complete catalog twice in durable prepare.
    pub(crate) fn apply_batch_prepared(
        &mut self,
        revision: CommitRevision,
        records: &[SpatialRecord],
    ) -> Result<Vec<ObservationOutcome>, SpatialError> {
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
        let (new_entries, new_logical_bytes) = self.new_entry_upper_bound(records)?;
        let maximum_entries = self
            .entry_count()
            .checked_add(new_entries)
            .ok_or(SpatialError::ResourceLimit)?;
        let maximum_logical_bytes = self
            .logical_bytes
            .checked_add(new_logical_bytes)
            .ok_or(SpatialError::ResourceLimit)?;
        if maximum_entries > MAX_SPATIAL_CATALOG_ENTRIES
            || maximum_logical_bytes > MAX_SPATIAL_CATALOG_LOGICAL_BYTES
        {
            return Err(SpatialError::ResourceLimit);
        }
        let mut outcomes = Vec::new();
        outcomes
            .try_reserve(
                records
                    .iter()
                    .filter(|record| matches!(record, SpatialRecord::Observation(_)))
                    .count(),
            )
            .map_err(|_| SpatialError::ResourceLimit)?;
        for record in records {
            match record {
                SpatialRecord::World(value) => {
                    self.stage_world(revision, value.clone())?;
                }
                SpatialRecord::Frame(value) => {
                    self.stage_frame(revision, value.clone())?;
                }
                SpatialRecord::Geometry(value) => {
                    self.stage_geometry(revision, value.clone())?;
                }
                SpatialRecord::Observation(value) => {
                    let outcome = self.stage_observation(revision, value.as_ref().clone())?;
                    outcomes.push(outcome);
                }
            }
        }
        for record in records {
            self.validate_record(revision, record)?;
        }
        self.last_revision = Some(revision);
        self.logical_bytes = maximum_logical_bytes;
        Ok(outcomes)
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

    fn stage_world(
        &mut self,
        revision: CommitRevision,
        world: WorldDefinition,
    ) -> Result<(), SpatialError> {
        self.ensure_category(world.id(), Category::World)?;
        let history = self.worlds.entry(world.id()).or_default();
        check_next(history, world.version())?;
        if let Some(previous) = history.last()
            && previous.value.root_frame().record != world.root_frame().record
        {
            return Err(SpatialError::InvalidRootFrame);
        }
        push_history(history, revision, world)
    }

    fn stage_frame(
        &mut self,
        revision: CommitRevision,
        frame: FrameDefinition,
    ) -> Result<(), SpatialError> {
        self.ensure_category(frame.id(), Category::Frame)?;
        let history = self.frames.entry(frame.id()).or_default();
        check_next(history, frame.version())?;
        if let Some(previous) = history.last()
            && (previous.value.world() != frame.world() || previous.value.kind() != frame.kind())
        {
            return Err(SpatialError::VersionConflict);
        }
        push_history(history, revision, frame)
    }

    fn stage_geometry(
        &mut self,
        revision: CommitRevision,
        geometry: GeometryVersion,
    ) -> Result<(), SpatialError> {
        self.ensure_category(geometry.id(), Category::Geometry)?;
        let history = self.geometries.entry(geometry.id()).or_default();
        check_next(history, geometry.version())?;
        match (history.last(), geometry.predecessor()) {
            (None, None) => {}
            (Some(previous), Some(reference))
                if reference.record == geometry.id()
                    && reference.version == previous.value.version() => {}
            _ => return Err(SpatialError::InvalidPredecessor),
        }
        push_history(history, revision, geometry)
    }

    fn stage_observation(
        &mut self,
        revision: CommitRevision,
        observation: PositionObservation,
    ) -> Result<ObservationOutcome, SpatialError> {
        self.ensure_category(observation.id(), Category::Observation)?;
        if let Some(existing) = self.observations.get(&observation.id()) {
            return if existing.value == observation {
                Ok(ObservationOutcome::Duplicate {
                    existing: observation.id(),
                })
            } else {
                Err(SpatialError::DuplicateRecord)
            };
        }
        if let Some(existing_id) = self.source_events.get(observation.key()).copied() {
            let existing = &self
                .observations
                .get(&existing_id)
                .ok_or(SpatialError::InvalidObservation)?
                .value;
            return if existing == &observation {
                Ok(ObservationOutcome::Duplicate {
                    existing: existing_id,
                })
            } else {
                Err(SpatialError::SourceEventConflict)
            };
        }
        self.source_events
            .insert(observation.key().clone(), observation.id());
        let id = observation.id();
        self.observations.insert(
            id,
            HistoryEntry {
                recorded_revision: revision,
                value: observation,
            },
        );
        Ok(ObservationOutcome::Inserted { id })
    }

    fn validate_record(
        &self,
        revision: CommitRevision,
        record: &SpatialRecord,
    ) -> Result<(), SpatialError> {
        match record {
            SpatialRecord::World(world) => {
                let root = self.frame_at(world.root_frame(), revision)?;
                if root.world() != world.id() || root.parent().is_some() {
                    return Err(SpatialError::InvalidRootFrame);
                }
                Ok(())
            }
            SpatialRecord::Frame(frame) => self.validate_frame(revision, frame),
            SpatialRecord::Geometry(geometry) => {
                self.world_visible(geometry.world(), revision)?;
                let frame = self.frame_at(geometry.frame(), revision)?;
                if frame.world() != geometry.world()
                    || !accepts_geometry(frame.kind(), geometry.geometry())
                {
                    return Err(SpatialError::IncompatibleFrame);
                }
                Ok(())
            }
            SpatialRecord::Observation(observation) => {
                self.world_visible(observation.world(), revision)?;
                let frame = self.frame_at(observation.frame(), revision)?;
                if frame.world() != observation.world()
                    || !frame.profile().accepts(observation.position())
                {
                    return Err(SpatialError::IncompatibleFrame);
                }
                if let Some(previous_id) = observation.correction_of() {
                    let previous = self
                        .observations
                        .get(&previous_id)
                        .filter(|entry| entry.recorded_revision < revision)
                        .ok_or(SpatialError::InvalidCorrection)?;
                    if previous.value.entity() != observation.entity()
                        || previous.value.world() != observation.world()
                    {
                        return Err(SpatialError::InvalidCorrection);
                    }
                }
                Ok(())
            }
        }
    }

    fn validate_frame(
        &self,
        revision: CommitRevision,
        frame: &FrameDefinition,
    ) -> Result<(), SpatialError> {
        let world = self.world_visible(frame.world(), revision)?;
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
            let next = self.frame_at(parent.frame, revision)?;
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

    fn world_visible(
        &self,
        id: RecordRef,
        revision: CommitRevision,
    ) -> Result<&WorldDefinition, SpatialError> {
        let history = self.worlds.get(&id).ok_or(SpatialError::MissingWorld)?;
        history
            .iter()
            .rev()
            .find(|entry| entry.recorded_revision <= revision)
            .map(|entry| &entry.value)
            .ok_or(SpatialError::MissingWorld)
    }

    fn ensure_category(&self, id: RecordRef, expected: Category) -> Result<(), SpatialError> {
        let actual = if self.worlds.contains_key(&id) {
            Some(Category::World)
        } else if self.frames.contains_key(&id) {
            Some(Category::Frame)
        } else if self.geometries.contains_key(&id) {
            Some(Category::Geometry)
        } else if self.observations.contains_key(&id) {
            Some(Category::Observation)
        } else {
            None
        };
        match actual {
            None => Ok(()),
            Some(actual) if actual == expected => Ok(()),
            Some(_) => Err(SpatialError::DuplicateRecord),
        }
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.worlds
            .values()
            .map(Vec::len)
            .chain(self.frames.values().map(Vec::len))
            .chain(self.geometries.values().map(Vec::len))
            .chain(core::iter::once(self.observations.len()))
            .fold(0, usize::saturating_add)
    }

    fn new_entry_upper_bound(
        &self,
        records: &[SpatialRecord],
    ) -> Result<(usize, usize), SpatialError> {
        let mut count = 0_usize;
        let mut logical_bytes = 0_usize;
        let mut new_observations = BTreeSet::new();
        for record in records {
            let creates_entry = match record {
                SpatialRecord::Observation(observation) => {
                    let exact_id_retry = self
                        .observations
                        .get(&observation.id())
                        .is_some_and(|entry| entry.value == **observation);
                    let exact_key_retry = self
                        .source_events
                        .get(observation.key())
                        .and_then(|id| self.observations.get(id))
                        .is_some_and(|entry| entry.value == **observation);
                    !exact_id_retry && !exact_key_retry && new_observations.insert(observation.id())
                }
                SpatialRecord::World(_) | SpatialRecord::Frame(_) | SpatialRecord::Geometry(_) => {
                    true
                }
            };
            if creates_entry {
                count = count.checked_add(1).ok_or(SpatialError::ResourceLimit)?;
                let encoded_length = encode_record(record)?.len();
                logical_bytes = logical_bytes
                    .checked_add(12)
                    .and_then(|value| encoded_length.checked_add(value))
                    .ok_or(SpatialError::ResourceLimit)?;
            }
        }
        Ok((count, logical_bytes))
    }

    pub(crate) fn stored_effect<'a>(
        &'a self,
        requested: &SpatialRecord,
    ) -> Option<(CommitRevision, SpatialRecordRef<'a>)> {
        match requested {
            SpatialRecord::World(value) => {
                history_entry(self.worlds.get(&value.id()), value.version()).map(|entry| {
                    (
                        entry.recorded_revision,
                        SpatialRecordRef::World(&entry.value),
                    )
                })
            }
            SpatialRecord::Frame(value) => {
                history_entry(self.frames.get(&value.id()), value.version()).map(|entry| {
                    (
                        entry.recorded_revision,
                        SpatialRecordRef::Frame(&entry.value),
                    )
                })
            }
            SpatialRecord::Geometry(value) => {
                history_entry(self.geometries.get(&value.id()), value.version()).map(|entry| {
                    (
                        entry.recorded_revision,
                        SpatialRecordRef::Geometry(&entry.value),
                    )
                })
            }
            SpatialRecord::Observation(value) => self.observations.get(&value.id()).map(|entry| {
                (
                    entry.recorded_revision,
                    SpatialRecordRef::Observation(&entry.value),
                )
            }),
        }
    }

    pub(crate) fn visit_records<E>(
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

fn check_next<T>(history: &[HistoryEntry<T>], version: SpatialVersion) -> Result<(), SpatialError>
where
    T: HasVersion,
{
    match history.last() {
        None if version == SpatialVersion::FIRST => Ok(()),
        Some(previous) if previous.value.version().checked_next()? == version => Ok(()),
        _ => Err(SpatialError::VersionConflict),
    }
}

fn push_history<T>(
    history: &mut Vec<HistoryEntry<T>>,
    revision: CommitRevision,
    value: T,
) -> Result<(), SpatialError> {
    history
        .try_reserve(1)
        .map_err(|_| SpatialError::ResourceLimit)?;
    history.push(HistoryEntry {
        recorded_revision: revision,
        value,
    });
    Ok(())
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
