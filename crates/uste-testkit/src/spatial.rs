//! Independent, scan-based frame-history oracle for spatial schema tests.

use std::collections::BTreeSet;

use uste_types::{
    RecordRef,
    spatial::{MAX_FRAME_DEPTH, SpatialVersion, VersionedRecordRef},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceFrame {
    pub id: VersionedRecordRef,
    pub world: RecordRef,
    pub parent: Option<VersionedRecordRef>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceFrameError {
    WrongWorld,
    WrongVersion,
    MissingFrame,
    InvalidRoot,
    Cycle,
    TooDeep,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceFrameHistory {
    world: RecordRef,
    root: VersionedRecordRef,
    frames: Vec<ReferenceFrame>,
}

impl ReferenceFrameHistory {
    #[must_use]
    pub const fn new(world: RecordRef, root: VersionedRecordRef) -> Self {
        Self {
            world,
            root,
            frames: Vec::new(),
        }
    }

    /// Clone, append, scan and publish only a valid final topology.
    pub fn apply(&mut self, batch: &[ReferenceFrame]) -> Result<(), ReferenceFrameError> {
        let mut candidate = self.clone();
        for frame in batch {
            if frame.world != self.world {
                return Err(ReferenceFrameError::WrongWorld);
            }
            let latest = candidate
                .frames
                .iter()
                .filter(|existing| existing.id.record == frame.id.record)
                .map(|existing| existing.id.version)
                .max();
            match latest {
                None if frame.id.version == SpatialVersion::FIRST => {}
                Some(version) if version.checked_next().ok() == Some(frame.id.version) => {}
                _ => return Err(ReferenceFrameError::WrongVersion),
            }
            candidate.frames.push(*frame);
        }
        for frame in batch {
            candidate.validate(*frame)?;
        }
        *self = candidate;
        Ok(())
    }

    #[must_use]
    pub fn contains(&self, reference: VersionedRecordRef) -> bool {
        self.find(reference).is_some()
    }

    fn validate(&self, frame: ReferenceFrame) -> Result<(), ReferenceFrameError> {
        if frame.id == self.root {
            return if frame.parent.is_none() {
                Ok(())
            } else {
                Err(ReferenceFrameError::InvalidRoot)
            };
        }
        let mut parent = frame.parent.ok_or(ReferenceFrameError::MissingFrame)?;
        let mut identities = BTreeSet::new();
        identities.insert(frame.id.record);
        for _ in 0..MAX_FRAME_DEPTH {
            if !identities.insert(parent.record) {
                return Err(ReferenceFrameError::Cycle);
            }
            let resolved = self.find(parent).ok_or(ReferenceFrameError::MissingFrame)?;
            match resolved.parent {
                Some(next) => parent = next,
                None if resolved.id == self.root => return Ok(()),
                None => return Err(ReferenceFrameError::InvalidRoot),
            }
        }
        Err(ReferenceFrameError::TooDeep)
    }

    fn find(&self, reference: VersionedRecordRef) -> Option<&ReferenceFrame> {
        self.frames.iter().find(|frame| frame.id == reference)
    }
}
