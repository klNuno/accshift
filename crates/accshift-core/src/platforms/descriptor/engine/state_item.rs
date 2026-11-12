//! A descriptor's per-account state walked as one list.

use super::super::plan::PlanTargetKind;
use super::super::schema::{DirItem, FileItem, RegistryItem, State};

/// One item of a descriptor's `state`.
#[derive(Debug, Clone, Copy)]
pub(super) enum StateItem<'a> {
    File(&'a FileItem),
    Value(&'a RegistryItem),
    Dir(&'a DirItem),
}

impl<'a> StateItem<'a> {
    /// Every item: files, then registry values, then directories. Captures,
    /// restores and clears run in this order, and a failed step stops the
    /// ones after it.
    pub(super) fn all(state: &'a State) -> impl Iterator<Item = StateItem<'a>> {
        let files = state.files.iter().map(StateItem::File);
        let values = state.registry_values.iter().map(StateItem::Value);
        let dirs = state.directories.iter().map(StateItem::Dir);
        files.chain(values).chain(dirs)
    }

    /// Its name inside the account's snapshot directory.
    pub(super) fn snapshot(self) -> &'a str {
        match self {
            Self::File(item) => &item.snapshot,
            Self::Value(item) => &item.snapshot,
            Self::Dir(item) => &item.snapshot,
        }
    }

    pub(super) fn snapshot_marker(self) -> bool {
        match self {
            Self::File(item) => item.snapshot_marker,
            Self::Value(item) => item.snapshot_marker,
            Self::Dir(item) => item.snapshot_marker,
        }
    }

    pub(super) fn clear_on_setup(self) -> bool {
        match self {
            Self::File(item) => item.clear_on_setup,
            Self::Value(item) => item.clear_on_setup,
            Self::Dir(item) => item.clear_on_setup,
        }
    }

    pub(super) fn clear_snapshot_when_source_missing(self) -> bool {
        match self {
            Self::File(item) => item.clear_snapshot_when_source_missing,
            Self::Value(item) => item.clear_snapshot_when_source_missing,
            Self::Dir(item) => item.clear_snapshot_when_source_missing,
        }
    }

    /// What a dry-run plan calls this kind of item.
    pub(super) fn plan_kind(self) -> PlanTargetKind {
        match self {
            Self::File(_) => PlanTargetKind::File,
            Self::Value(_) => PlanTargetKind::RegistryValue,
            Self::Dir(_) => PlanTargetKind::Directory,
        }
    }
}
