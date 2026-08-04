use koushi_sdk::{MatrixCachedDirectAccountData, MatrixDirectTargetsByRoom};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectAccountDataSource {
    #[default]
    Unavailable,
    LocalStore,
    SlidingSyncEvent,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DirectClassificationState {
    targets_by_room: MatrixDirectTargetsByRoom,
    source: DirectAccountDataSource,
    invalid_entry_count: u64,
    event_wake_count: u64,
    applied_update_count: u64,
}

impl DirectClassificationState {
    pub(crate) fn from_cached(value: MatrixCachedDirectAccountData) -> Self {
        match value {
            MatrixCachedDirectAccountData::Present(targets_by_room) => {
                Self::from_targets(targets_by_room, DirectAccountDataSource::LocalStore)
            }
            MatrixCachedDirectAccountData::Invalid => Self {
                invalid_entry_count: 1,
                ..Self::default()
            },
            MatrixCachedDirectAccountData::Missing
            | MatrixCachedDirectAccountData::StoreError => Self::default(),
        }
    }

    pub(crate) fn from_targets(
        targets_by_room: MatrixDirectTargetsByRoom,
        source: DirectAccountDataSource,
    ) -> Self {
        Self {
            targets_by_room,
            source,
            ..Self::default()
        }
    }

    pub(crate) fn targets_by_room(&self) -> &MatrixDirectTargetsByRoom {
        &self.targets_by_room
    }

    pub(crate) fn authoritative_targets(&self) -> Option<&MatrixDirectTargetsByRoom> {
        (self.source != DirectAccountDataSource::Unavailable).then_some(&self.targets_by_room)
    }

    pub(crate) fn source(&self) -> DirectAccountDataSource {
        self.source
    }

    pub(crate) fn invalid_entry_count(&self) -> u64 {
        self.invalid_entry_count
    }

    pub(crate) fn replace_targets(&mut self, next: MatrixDirectTargetsByRoom) -> bool {
        self.event_wake_count = self.event_wake_count.saturating_add(1);
        let changed = self.source == DirectAccountDataSource::Unavailable
            || self.targets_by_room != next;
        self.source = DirectAccountDataSource::SlidingSyncEvent;
        self.invalid_entry_count = 0;
        if !changed {
            return false;
        }
        self.targets_by_room = next;
        self.applied_update_count = self.applied_update_count.saturating_add(1);
        true
    }

    pub(crate) fn event_wake_count(&self) -> u64 {
        self.event_wake_count
    }

    pub(crate) fn applied_update_count(&self) -> u64 {
        self.applied_update_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equivalent_event_map_does_not_request_reprojection() {
        let map = MatrixDirectTargetsByRoom::from([(
            "!dm:example.invalid".to_owned(),
            vec!["@alice:example.invalid".to_owned()],
        )]);
        let mut state = DirectClassificationState::from_targets(
            map.clone(),
            DirectAccountDataSource::LocalStore,
        );

        assert!(!state.replace_targets(map));
        assert_eq!(state.event_wake_count(), 1);
        assert_eq!(state.applied_update_count(), 0);
    }

    #[test]
    fn changed_or_removed_mapping_requests_reprojection() {
        let mut state = DirectClassificationState::default();
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::from([(
            "!dm:example.invalid".to_owned(),
            vec!["@alice:example.invalid".to_owned()],
        )])));
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::new()));
        assert_eq!(state.event_wake_count(), 2);
        assert_eq!(state.applied_update_count(), 2);
    }

    #[test]
    fn empty_event_map_after_unavailable_is_authoritative_and_requests_reprojection() {
        let mut state = DirectClassificationState::default();

        assert!(state.authoritative_targets().is_none());
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::new()));
        assert_eq!(
            state.authoritative_targets(),
            Some(&MatrixDirectTargetsByRoom::new())
        );
        assert_eq!(state.source(), DirectAccountDataSource::SlidingSyncEvent);
        assert_eq!(state.event_wake_count(), 1);
        assert_eq!(state.applied_update_count(), 1);
    }

    #[test]
    fn valid_event_clears_invalid_cached_entry_count() {
        let mut state =
            DirectClassificationState::from_cached(MatrixCachedDirectAccountData::Invalid);

        assert_eq!(state.invalid_entry_count(), 1);
        assert!(state.replace_targets(MatrixDirectTargetsByRoom::new()));
        assert_eq!(state.invalid_entry_count(), 0);
    }
}
