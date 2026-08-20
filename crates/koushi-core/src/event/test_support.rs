use crate::ids::RequestId;

pub(super) fn fake_rid(sequence: u64) -> RequestId {
    RequestId {
        connection_id: crate::ids::RuntimeConnectionId(7),
        sequence,
    }
}
