//! `koushi-export.json`: what an export folder holds, so a later run can
//! resume it.
//!
//! The manifest is rewritten atomically after every room settles. A folder
//! whose manifest names another scope or range is never resumed, so two
//! different exports cannot be mixed in one folder.

use serde::{Deserialize, Serialize};

use koushi_state::{HistoryExportRange, HistoryExportRoomCounts};

pub(crate) const MANIFEST_FILE_NAME: &str = "koushi-export.json";
const FORMAT: &str = "koushi-history-export";
const VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ManifestScope {
    Room { id: String },
    Space { id: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManifestRoomStatus {
    Pending,
    Completed,
    Skipped,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ManifestRoom {
    pub(crate) room_id: String,
    /// Room name when the room was last listed, for the table of contents.
    #[serde(default)]
    pub(crate) display_name: String,
    /// Folder name below `rooms/`, fixed when the room was first listed.
    pub(crate) folder: String,
    pub(crate) status: ManifestRoomStatus,
    pub(crate) counts: HistoryExportRoomCounts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct ExportManifest {
    pub(crate) format: String,
    pub(crate) version: u32,
    pub(crate) scope: ManifestScope,
    pub(crate) range: HistoryExportRange,
    /// The Space or room name when the export was first started.
    pub(crate) title: String,
    pub(crate) rooms: Vec<ManifestRoom>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ManifestMatch {
    /// No manifest: a new export.
    Fresh,
    Resume(ExportManifest),
    /// Another export, a newer format, or an unreadable manifest.
    Mismatch,
}

impl ExportManifest {
    pub(crate) fn new(scope: ManifestScope, range: HistoryExportRange, title: String) -> Self {
        Self {
            format: FORMAT.to_owned(),
            version: VERSION,
            scope,
            range,
            title,
            rooms: Vec::new(),
        }
    }

    pub(crate) fn room(&self, room_id: &str) -> Option<&ManifestRoom> {
        self.rooms.iter().find(|room| room.room_id == room_id)
    }

    /// Update the room in place, or append it.
    pub(crate) fn upsert_room(
        &mut self,
        room_id: &str,
        display_name: &str,
        folder: &str,
        status: ManifestRoomStatus,
        counts: HistoryExportRoomCounts,
    ) {
        match self.rooms.iter_mut().find(|room| room.room_id == room_id) {
            Some(room) => {
                room.display_name = display_name.to_owned();
                room.status = status;
                room.counts = counts;
            }
            None => self.rooms.push(ManifestRoom {
                room_id: room_id.to_owned(),
                display_name: display_name.to_owned(),
                folder: folder.to_owned(),
                status,
                counts,
            }),
        }
    }

    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

pub(crate) fn match_manifest(
    existing: Option<&[u8]>,
    scope: &ManifestScope,
    range: &HistoryExportRange,
) -> ManifestMatch {
    let Some(bytes) = existing else {
        return ManifestMatch::Fresh;
    };
    match serde_json::from_slice::<ExportManifest>(bytes) {
        Ok(manifest)
            if manifest.format == FORMAT
                && manifest.version == VERSION
                && &manifest.scope == scope
                && &manifest.range == range =>
        {
            ManifestMatch::Resume(manifest)
        }
        _ => ManifestMatch::Mismatch,
    }
}
