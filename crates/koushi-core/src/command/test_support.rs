use crate::ids::RequestId;

#[cfg(test)]
pub(super) fn fake_rid(seq: u64) -> RequestId {
    RequestId {
        connection_id: crate::ids::RuntimeConnectionId(999),
        sequence: seq,
    }
}
