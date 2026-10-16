use super::*;

#[test]
fn automatic_targets_admins_even_in_small_rooms_and_full_population_decides_default() {
    let mut joined = (0..30)
        .map(|n| format!("@member{n}:example.invalid"))
        .collect::<BTreeSet<_>>();
    let own = "@member0:example.invalid";
    let admins = BTreeSet::from(["@member1:example.invalid".to_owned()]);
    assert_eq!(
        targets(&joined, &admins, own, false, false).unwrap().len(),
        29
    );
    assert_eq!(targets(&joined, &admins, own, true, true).unwrap().len(), 1);
    joined.insert("@member30:example.invalid".to_owned());
    assert_eq!(
        targets(&joined, &admins, own, false, false).unwrap().len(),
        1
    );
    assert_eq!(
        targets(&joined, &admins, own, true, false).unwrap().len(),
        30
    );
    joined.remove(own);
    assert_eq!(
        targets(&joined, &admins, own, true, false),
        Err(Failure::MembershipChanged)
    );
}

#[test]
fn chunks_reject_replay_conflicts_metadata_changes_and_bounds() {
    let mut receipt = Receipt::default();
    let id = "a".repeat(48);
    assert_eq!(reserve_chunk(&mut receipt, &id, 2, 0, b"first"), Ok(true));
    // A durable reservation interrupted before import must retry idempotently.
    assert_eq!(reserve_chunk(&mut receipt, &id, 2, 0, b"first"), Ok(true));
    receipt.completed.insert(0);
    assert_eq!(reserve_chunk(&mut receipt, &id, 2, 0, b"first"), Ok(false));
    assert_eq!(
        reserve_chunk(&mut receipt, &id, 2, 0, b"conflict"),
        Err(Failure::InvalidResponse)
    );
    assert_eq!(
        reserve_chunk(&mut receipt, &id, 3, 1, b"second"),
        Err(Failure::InvalidResponse)
    );
    assert_eq!(
        reserve_chunk(&mut receipt, &"b".repeat(48), 2, 1, b"second"),
        Err(Failure::InvalidResponse)
    );
    assert_eq!(
        reserve_chunk(&mut receipt, &id, 2, 2, b"bad"),
        Err(Failure::TooLarge)
    );
    assert_eq!(
        reserve_chunk(&mut receipt, &id, 257, 1, b"bad"),
        Err(Failure::TooLarge)
    );
    assert_eq!(
        reserve_chunk(&mut receipt, &id, 2, 1, &vec![0; MESSAGE_LIMIT + 1]),
        Err(Failure::TooLarge)
    );
    assert_eq!(reserve_chunk(&mut receipt, &id, 2, 1, b"second"), Ok(true));
}

#[test]
fn restart_cooldown_fails_closed_for_clock_rollback() {
    assert!(!automatic_due(100, 99));
    assert!(!automatic_due(100, 100 + AUTO_COOLDOWN - 1));
    assert!(automatic_due(100, 100 + AUTO_COOLDOWN));
}
