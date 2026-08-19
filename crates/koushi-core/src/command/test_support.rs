#[cfg(test)]
pub(super) fn fake_rid(seq: u64) -> crate::ids::RequestId {
    crate::ids::RequestId {
        connection_id: crate::ids::RuntimeConnectionId(999),
        sequence: seq,
    }
}
