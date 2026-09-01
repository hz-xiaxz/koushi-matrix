use koushi_protocol::ids::RequestId;

#[cfg(test)]
pub(super) fn fake_rid(seq: u64) -> RequestId {
    RequestId {
        connection_id: koushi_protocol::ids::RuntimeConnectionId(999),
        sequence: seq,
    }
}
