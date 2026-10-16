use super::*;

fn pin() -> Pin {
    Pin {
        user: "@peer:example.invalid".into(),
        device: "DEVICE".into(),
        curve: "synthetic-curve".into(),
        ed: "synthetic-ed".into(),
        master: "synthetic-master".into(),
    }
}
fn attempt() -> Attempt {
    Attempt {
        pin: pin(),
        nonce: "a".repeat(48),
        expires: 100,
        confirmed: true,
        terminal: false,
        receipt: Receipt::default(),
    }
}

#[test]
fn exact_identity_and_fixed_expiry_bind_replies_including_nonsecret_refusals() {
    let attempt = attempt();
    assert!(attempt.matches(&pin(), &"a".repeat(48), 99));
    assert!(!attempt.matches(&pin(), &"b".repeat(48), 99));
    assert!(!attempt.matches(&pin(), &"a".repeat(48), 100));
    for field in 0..5 {
        let mut changed = pin();
        match field {
            0 => changed.user = "@other:example.invalid".into(),
            1 => changed.device = "OTHER".into(),
            2 => changed.curve.push('x'),
            3 => changed.ed.push('x'),
            _ => changed.master.push('x'),
        }
        assert!(!attempt.matches(&changed, &"a".repeat(48), 99));
    }
}

#[test]
fn unsolicited_stale_and_wrong_peer_chunks_are_rejected_before_identity_queries() {
    let room = "!history:example.invalid";
    let nonce = "a".repeat(48);
    let peer = pin();
    let mut ledger = Ledger::default();
    assert!(!ledger.correlated(room, &nonce, &peer.user, 99));
    ledger.rooms.insert(
        room.into(),
        Pending {
            pins: vec![peer.clone()],
            attempts: BTreeMap::from([(peer.key(), attempt())]),
            ..Default::default()
        },
    );
    assert!(ledger.correlated(room, &nonce, &peer.user, 99));
    assert!(!ledger.correlated(room, &nonce, &peer.user, 100));
    assert!(!ledger.correlated("!other:example.invalid", &nonce, &peer.user, 99));
    assert!(!ledger.correlated(room, &"b".repeat(48), &peer.user, 99));
    assert!(!ledger.correlated(room, &nonce, "@other:example.invalid", 99));
    // Administrator-only retry dispatch does not revoke a manual peer's nonce.
    assert!(ledger.rooms[room].dispatch.is_empty());
    assert!(ledger.correlated(room, &nonce, &peer.user, 99));
    ledger.rooms.get_mut(room).unwrap().pins.clear();
    assert!(!ledger.correlated(room, &nonce, &peer.user, 99));
    ledger.completed.insert(
        nonce_key(&peer, &nonce),
        Completed {
            room: room.into(),
            pin: peer.clone(),
            nonce: nonce.clone(),
            response: "c".repeat(48),
            expires: 100,
        },
    );
    assert!(ledger.correlated(room, &nonce, &peer.user, 99));
    assert!(!ledger.correlated(room, &nonce, &peer.user, 100));
}
