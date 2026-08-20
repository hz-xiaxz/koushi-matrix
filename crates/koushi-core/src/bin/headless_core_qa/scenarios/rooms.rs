async fn run_invites_dm_stage(
    config: &QaConfig,
    conn_a: &mut CoreConnection,
    conn_b: &mut CoreConnection,
) -> Result<(), String> {
    let user_b_full_id = format!("@{}:{}", config.user_b, config.server_name);
    let user_a_full_id = format!("@{}:{}", config.user_a, config.server_name);

    let accept_room_id = create_room_for_qa(
        conn_a,
        "QA Invite Accept Room",
        false,
        "invites_dm create accept room",
    )
    .await?;
    invite_user_for_qa(
        conn_a,
        &accept_room_id,
        &user_b_full_id,
        "invites_dm invite B to room",
    )
    .await?;
    wait_for_invite_in_snapshot(
        conn_b,
        &accept_room_id,
        Some(false),
        "invites_dm wait for room invite",
    )
    .await?;
    println!("invite_recv=ok");

    accept_invite_for_qa(conn_b, &accept_room_id, "invites_dm accept room invite").await?;
    wait_for_room_in_room_list(
        conn_b,
        &accept_room_id,
        "invites_dm room list after room accept",
    )
    .await?;
    let accept_room_settings =
        load_room_settings_for_qa(conn_b, &accept_room_id, "invites_dm accepted room members")
            .await?;
    assert_room_settings_contains_members(
        &accept_room_settings,
        &[user_a_full_id.as_str(), user_b_full_id.as_str()],
        "invites_dm accepted room members",
    )?;

    let accept_space_id = create_space_for_qa(
        conn_a,
        "QA Invite Accept Space",
        "invites_dm create accept space",
    )
    .await?;
    invite_user_for_qa(
        conn_a,
        &accept_space_id,
        &user_b_full_id,
        "invites_dm invite B to space",
    )
    .await?;
    wait_for_invite_in_snapshot(
        conn_b,
        &accept_space_id,
        Some(false),
        "invites_dm wait for space invite",
    )
    .await?;
    accept_invite_for_qa(conn_b, &accept_space_id, "invites_dm accept space invite").await?;
    wait_for_space_in_space_list(
        conn_b,
        &accept_space_id,
        "invites_dm space list after space accept",
    )
    .await?;
    let accept_space_settings = load_room_settings_for_qa(
        conn_b,
        &accept_space_id,
        "invites_dm accepted space members",
    )
    .await?;
    assert_room_settings_contains_members(
        &accept_space_settings,
        &[user_a_full_id.as_str(), user_b_full_id.as_str()],
        "invites_dm accepted space members",
    )?;
    let accept_space_settings_a = load_room_settings_for_qa(
        conn_a,
        &accept_space_id,
        "invites_dm creator observes accepted space member",
    )
    .await?;
    assert_room_settings_contains_members(
        &accept_space_settings_a,
        &[user_a_full_id.as_str(), user_b_full_id.as_str()],
        "invites_dm creator observes accepted space member",
    )?;
    println!("invite_accept=ok");
    println!("member_list=ok");

    let decline_room_id = create_room_for_qa(
        conn_a,
        "QA Invite Decline Room",
        false,
        "invites_dm create decline room",
    )
    .await?;
    invite_user_for_qa(
        conn_a,
        &decline_room_id,
        &user_b_full_id,
        "invites_dm invite B to decline room",
    )
    .await?;
    wait_for_invite_in_snapshot(
        conn_b,
        &decline_room_id,
        Some(false),
        "invites_dm wait for decline invite",
    )
    .await?;
    decline_invite_for_qa(conn_b, &decline_room_id, "invites_dm decline room invite").await?;
    wait_for_invite_absent(
        conn_b,
        &decline_room_id,
        "invites_dm wait for declined invite removal",
    )
    .await?;
    println!("invite_decline=ok");

    let dm_room_id =
        start_direct_message_for_qa(conn_a, &user_b_full_id, "invites_dm start direct message")
            .await?;
    wait_for_dm_room_in_room_list(conn_a, &dm_room_id, "invites_dm A room list after DM start")
        .await?;
    wait_for_invite_in_snapshot(
        conn_b,
        &dm_room_id,
        Some(true),
        "invites_dm wait for DM invite",
    )
    .await?;
    println!("dm_start=ok");

    let user_c_full_id = config.dm_scope_control_user_id()?;
    let control_dm_room_id = start_direct_message_for_qa(
        conn_a,
        &user_c_full_id,
        "invites_dm start control direct message",
    )
    .await?;
    wait_for_dm_room_in_room_list(
        conn_a,
        &control_dm_room_id,
        "invites_dm A room list after control DM start",
    )
    .await?;
    assert_dm_space_scope_for_qa(conn_a, &accept_space_id, &dm_room_id, &control_dm_room_id)
        .await?;
    println!("dm_space_scope=ok");

    Ok(())
}

async fn run_directory_stage(
    config: &QaConfig,
    conn_a: &mut CoreConnection,
    conn_b: &mut CoreConnection,
) -> Result<(), String> {
    let directory_room_name = "Koushi Directory QA";
    let alias_localpart = format!("koushi-desktop-directory-qa-{}", std::process::id());
    let expected_alias = format!("#{alias_localpart}:{}", config.server_name);
    let public_room_id = create_public_directory_room_for_qa(
        conn_a,
        directory_room_name,
        &alias_localpart,
        "directory create public room",
    )
    .await?;

    let query = DirectoryQuery {
        term: Some(directory_room_name.to_owned()),
        server_name: Some(config.server_name.clone()),
        limit: Some(10),
        since: None,
    };
    let rooms = query_directory_until_room_visible(
        conn_a,
        query,
        &public_room_id,
        &expected_alias,
        "directory query public room",
    )
    .await?;
    if rooms.is_empty() {
        return Err("directory query unexpectedly returned no rooms".to_owned());
    }
    println!("directory_query=ok");

    join_directory_room_for_qa(
        conn_b,
        &expected_alias,
        &config.server_name,
        &public_room_id,
        "directory B joins public room",
    )
    .await?;
    println!("directory_join=ok");

    Ok(())
}

async fn join_directory_room_for_qa(
    conn_b: &mut CoreConnection,
    expected_alias: &str,
    via_server: &str,
    public_room_id: &str,
    label: &str,
) -> Result<(), String> {
    let join_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Room(RoomCommand::JoinDirectoryRoom {
            request_id: join_id,
            room_id_or_alias: expected_alias.to_owned(),
            via_servers: vec![via_server.to_owned()],
        }))
        .await
        .map_err(|e| format!("{label}: submit join by alias failed: {e}"))?;
    wait_for_room_joined(conn_b, join_id, public_room_id, label).await
}

async fn run_room_people_projection_stage(
    config: &QaConfig,
    conn_a: &mut CoreConnection,
    conn_b: &mut CoreConnection,
    account_key_a: &AccountKey,
    account_key_b: &AccountKey,
    room_id: &str,
) -> Result<(), String> {
    let user_a_id = format!("@{}:{}", config.user_a, config.server_name);
    let user_b_id = format!("@{}:{}", config.user_b, config.server_name);
    let user_c_id = config.dm_scope_control_user_id()?;

    let profile_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Account(AccountCommand::SetDisplayName {
            request_id: profile_id,
            display_name: Some("Room People Known".to_owned()),
        }))
        .await
        .map_err(|e| format!("room people: submit display-name update failed: {e}"))?;
    wait_for_profile_updated(conn_a, profile_id, "room people display-name update").await?;

    let main_target = query_mention_candidates(
        conn_a,
        account_key_a,
        room_id,
        MentionSurface::Main,
        "",
        "room people main candidates",
    )
    .await?;
    assert_joined_candidate_scope(&main_target, [&user_a_id, &user_b_id], &user_c_id)?;
    if main_target.room_mention_allowed != RoomMentionPermission::Allowed {
        return Err(
            "room people: room mention permission was not allowed for the room creator".to_owned(),
        );
    }
    if !main_target.candidates.iter().any(|candidate| {
        candidate.user_id == user_a_id
            && candidate.display_label.as_deref() == Some("Room People Known")
    }) {
        return Err("room people: known room display label was not projected".to_owned());
    }
    if !main_target
        .candidates
        .iter()
        .any(|candidate| candidate.user_id == user_b_id)
    {
        return Err(
            "room people: joined member with an optional label was not projected".to_owned(),
        );
    }
    println!("room_people_joined_scope=ok");

    let alias_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Account(AccountCommand::SetLocalUserAlias {
            request_id: alias_id,
            user_id: user_b_id.clone(),
            alias: Some("Room People Personal Alias".to_owned()),
        }))
        .await
        .map_err(|e| format!("room people: submit local alias update failed: {e}"))?;
    wait_for_local_alias(
        conn_a,
        alias_id,
        &user_b_id,
        "Room People Personal Alias",
        "room people local alias update",
    )
    .await?;
    let aliased_target = query_mention_candidates(
        conn_a,
        account_key_a,
        room_id,
        MentionSurface::Main,
        "personal alias",
        "room people alias candidates",
    )
    .await?;
    if aliased_target.candidates.len() != 1
        || aliased_target.candidates[0].user_id != user_b_id
        || aliased_target.candidates[0].display_label.as_deref()
            != Some("Room People Personal Alias")
    {
        return Err(
            "room people: local alias did not drive candidate matching and label".to_owned(),
        );
    }
    println!("room_people_alias_search=ok");

    let _main_target = query_mention_candidates(
        conn_a,
        account_key_a,
        room_id,
        MentionSurface::Main,
        "",
        "room people restored main candidates",
    )
    .await?;
    let thread_target = query_mention_candidates(
        conn_a,
        account_key_a,
        room_id,
        MentionSurface::Thread,
        &config.user_b,
        "room people thread candidates",
    )
    .await?;
    if thread_target.candidates.len() != 1 || thread_target.candidates[0].user_id != user_b_id {
        return Err("room people: thread target did not retain its independent query".to_owned());
    }
    let retained_main = conn_a
        .snapshot()
        .mention_candidates
        .target(room_id, MentionSurface::Main)
        .cloned()
        .ok_or_else(|| "room people: main target disappeared after thread query".to_owned())?;
    assert_joined_candidate_scope(&retained_main, [&user_a_id, &user_b_id], &user_c_id)?;
    println!("room_people_surface_isolation=ok");

    let leave_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Room(RoomCommand::LeaveRoom {
            request_id: leave_id,
            room_id: room_id.to_owned(),
        }))
        .await
        .map_err(|e| format!("room people: submit leave failed: {e}"))?;
    wait_for_room_left(conn_b, leave_id, room_id, "room people member leave").await?;
    wait_for_mention_candidate_ids(
        conn_a,
        room_id,
        MentionSurface::Main,
        [&user_a_id],
        "room people candidates after leave",
    )
    .await?;

    let invite_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Room(RoomCommand::InviteUser {
            request_id: invite_id,
            room_id: room_id.to_owned(),
            user_id: user_b_id.clone(),
        }))
        .await
        .map_err(|e| format!("room people: submit reinvite failed: {e}"))?;
    wait_for_user_invited(
        conn_a,
        invite_id,
        room_id,
        &user_b_id,
        "room people reinvite",
    )
    .await?;
    let rejoin_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Room(RoomCommand::JoinRoom {
            request_id: rejoin_id,
            room_id: room_id.to_owned(),
        }))
        .await
        .map_err(|e| format!("room people: submit rejoin failed: {e}"))?;
    wait_for_room_joined(conn_b, rejoin_id, room_id, "room people member rejoin").await?;
    wait_for_mention_candidate_ids(
        conn_a,
        room_id,
        MentionSurface::Main,
        [&user_a_id, &user_b_id],
        "room people candidates after rejoin",
    )
    .await?;
    println!("room_people_membership_refresh=ok");

    let key = TimelineKey::room(account_key_a.clone(), room_id.to_owned());
    let subscribe_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Timeline(TimelineCommand::Subscribe {
            request_id: subscribe_id,
            key: key.clone(),
        }))
        .await
        .map_err(|e| format!("room people: submit timeline subscribe failed: {e}"))?;
    wait_for_initial_items(conn_a, &key, subscribe_id, "room people timeline subscribe").await?;
    let key_b = TimelineKey::room(account_key_b.clone(), room_id.to_owned());
    let subscribe_b_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Timeline(TimelineCommand::Subscribe {
            request_id: subscribe_b_id,
            key: key_b.clone(),
        }))
        .await
        .map_err(|e| format!("room people: submit receiver timeline subscribe failed: {e}"))?;
    wait_for_initial_items(
        conn_b,
        &key_b,
        subscribe_b_id,
        "room people receiver timeline subscribe",
    )
    .await?;

    let display_label = thread_target.candidates[0]
        .display_label
        .clone()
        .unwrap_or_else(|| "Unknown user".to_owned());
    let document = koushi_state::ComposerDocument::new(vec![
        koushi_state::ComposerInline::Text {
            text: "Room people structured mention QA ".to_owned(),
        },
        koushi_state::ComposerInline::Mention {
            target: MentionTarget::User {
                user_id: user_b_id.clone(),
                display_label: display_label.clone(),
            },
            display_label,
        },
    ]);
    let body = document.plain_body();
    let transaction_id = "qa-room-people-mention";
    let send_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Timeline(TimelineCommand::SendText {
            request_id: send_id,
            key: key.clone(),
            transaction_id: transaction_id.to_owned(),
            document,
        }))
        .await
        .map_err(|e| format!("room people: submit structured mention failed: {e}"))?;
    wait_for_send_flow_completion(
        conn_a,
        send_id,
        &key,
        transaction_id,
        &body,
        "room people structured mention",
    )
    .await?;
    let received = wait_for_item_with_body(
        conn_b,
        &key_b,
        &body,
        "room people receiver structured mention",
    )
    .await?;
    let received_event_id = match received.id {
        TimelineItemId::Event { event_id } => event_id,
        TimelineItemId::Transaction { .. } | TimelineItemId::Synthetic { .. } => {
            return Err("room people: receiver mention did not have a remote event id".to_owned());
        }
    };
    let source_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Timeline(TimelineCommand::LoadMessageSource {
            request_id: source_id,
            key: key_b.clone(),
            event_id: received_event_id,
        }))
        .await
        .map_err(|e| format!("room people: submit message-source load failed: {e}"))?;
    wait_for_structured_mention_source(
        conn_b,
        source_id,
        &user_b_id,
        "room people structured mention source",
    )
    .await?;
    let unsubscribe_id = conn_a.next_request_id();
    conn_a
        .command(CoreCommand::Timeline(TimelineCommand::Unsubscribe {
            request_id: unsubscribe_id,
            key,
        }))
        .await
        .map_err(|e| format!("room people: submit timeline unsubscribe failed: {e}"))?;
    let unsubscribe_b_id = conn_b.next_request_id();
    conn_b
        .command(CoreCommand::Timeline(TimelineCommand::Unsubscribe {
            request_id: unsubscribe_b_id,
            key: key_b,
        }))
        .await
        .map_err(|e| format!("room people: submit receiver timeline unsubscribe failed: {e}"))?;
    println!("room_people_mentions_content=ok");
    println!("room_people_projection=ok");
    Ok(())
}

fn assert_joined_candidate_scope<const N: usize>(
    target: &MentionCandidatesTarget,
    expected_user_ids: [&String; N],
    excluded_user_id: &str,
) -> Result<(), String> {
    if target.completeness != MentionCandidatesCompleteness::Complete {
        return Err("room people: candidate target was not complete".to_owned());
    }
    let actual = target
        .candidates
        .iter()
        .map(|candidate| candidate.user_id.as_str())
        .collect::<BTreeSet<_>>();
    let expected = expected_user_ids
        .into_iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual != expected || actual.contains(excluded_user_id) {
        return Err("room people: candidate target did not match joined-room scope".to_owned());
    }
    Ok(())
}

async fn query_mention_candidates(
    conn: &mut CoreConnection,
    account_key: &AccountKey,
    room_id: &str,
    surface: MentionSurface,
    query: &str,
    label: &str,
) -> Result<MentionCandidatesTarget, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::QueryMentionCandidates {
        request_id,
        account_key: account_key.clone(),
        room_id: room_id.to_owned(),
        surface,
        query: query.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit failed: {e}"))?;
    wait_for_mention_target(conn, room_id, surface, Some(request_id), label, |target| {
        target.completeness == MentionCandidatesCompleteness::Complete
    })
    .await
}

async fn wait_for_mention_candidate_ids<const N: usize>(
    conn: &mut CoreConnection,
    room_id: &str,
    surface: MentionSurface,
    expected_user_ids: [&String; N],
    label: &str,
) -> Result<MentionCandidatesTarget, String> {
    let expected = expected_user_ids
        .into_iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    wait_for_mention_target(conn, room_id, surface, None, label, |target| {
        target.completeness == MentionCandidatesCompleteness::Complete
            && target
                .candidates
                .iter()
                .map(|candidate| candidate.user_id.as_str())
                .collect::<BTreeSet<_>>()
                == expected
    })
    .await
}

async fn wait_for_mention_target(
    conn: &mut CoreConnection,
    room_id: &str,
    surface: MentionSurface,
    request_id: Option<RequestId>,
    label: &str,
    predicate: impl Fn(&MentionCandidatesTarget) -> bool,
) -> Result<MentionCandidatesTarget, String> {
    let deadline = QaEventDeadline::after(ROOM_LIST_EVENT_TIMEOUT);
    loop {
        if let Some(target) = conn
            .snapshot()
            .mention_candidates
            .target(room_id, surface)
            .filter(|target| {
                request_id.is_none_or(|request_id| target.request_id == request_id.sequence)
                    && predicate(target)
            })
            .cloned()
        {
            return Ok(target);
        }
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for mention candidate projection"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        if let CoreEvent::OperationFailed {
            request_id: event_request_id,
            failure,
        } = event
            && request_id == Some(event_request_id)
        {
            return Err(format!(
                "{label}: mention candidate command failed: {failure:?}"
            ));
        }
    }
}

async fn wait_for_profile_updated(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for profile update"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        match event {
            CoreEvent::Account(AccountEvent::ProfileUpdated {
                request_id: event_request_id,
                ..
            }) if event_request_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            } if event_request_id == request_id => {
                return Err(format!("{label}: profile update failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

async fn wait_for_local_alias(
    conn: &mut CoreConnection,
    request_id: RequestId,
    user_id: &str,
    expected_alias: &str,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        if conn
            .snapshot()
            .profile
            .local_aliases
            .get(user_id)
            .is_some_and(|alias| alias == expected_alias)
        {
            return Ok(());
        }
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for local alias projection"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        if let CoreEvent::OperationFailed {
            request_id: event_request_id,
            failure,
        } = event
            && event_request_id == request_id
        {
            return Err(format!("{label}: local alias update failed: {failure:?}"));
        }
    }
}

async fn wait_for_room_left(
    conn: &mut CoreConnection,
    request_id: RequestId,
    room_id: &str,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for room leave"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        match event {
            CoreEvent::Room(RoomEvent::RoomLeft {
                request_id: event_request_id,
                room_id: event_room_id,
            }) if event_request_id == request_id => {
                if event_room_id != room_id {
                    return Err(format!("{label}: room leave target mismatch"));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            } if event_request_id == request_id => {
                return Err(format!("{label}: room leave failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

async fn wait_for_structured_mention_source(
    conn: &mut CoreConnection,
    request_id: RequestId,
    mentioned_user_id: &str,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for message source"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        match event {
            CoreEvent::Timeline(TimelineEvent::MessageSourceLoaded {
                request_id: event_request_id,
                source,
                ..
            }) if event_request_id == request_id => {
                let user_ids = source
                    .original_json
                    .as_ref()
                    .and_then(|json| json.pointer("/content/m.mentions/user_ids"))
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| format!("{label}: source lacked structured mention user ids"))?;
                if user_ids.len() != 1 || user_ids[0].as_str() != Some(mentioned_user_id) {
                    return Err(format!("{label}: structured mention user ids mismatch"));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: event_request_id,
                failure,
            } if event_request_id == request_id => {
                return Err(format!("{label}: message-source load failed: {failure:?}"));
            }
            _ => {}
        }
    }
}

async fn create_room_for_qa(
    conn: &mut CoreConnection,
    name: &str,
    encrypted: bool,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::CreateRoom {
        request_id,
        options: private_room_options(name, encrypted),
    }))
    .await
    .map_err(|e| format!("{label}: submit room create failed: {e}"))?;
    wait_for_room_created(conn, request_id, label).await
}

async fn create_public_directory_room_for_qa(
    conn: &mut CoreConnection,
    name: &str,
    alias_localpart: &str,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::CreatePublicDirectoryRoom {
        request_id,
        name: name.to_owned(),
        alias_localpart: alias_localpart.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit public directory room create failed: {e}"))?;
    wait_for_room_created(conn, request_id, label).await
}

async fn create_space_for_qa(
    conn: &mut CoreConnection,
    name: &str,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::CreateSpace {
        request_id,
        name: name.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit space create failed: {e}"))?;
    wait_for_space_created(conn, request_id, label).await
}

async fn invite_user_for_qa(
    conn: &mut CoreConnection,
    room_id: &str,
    user_id: &str,
    label: &str,
) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::InviteUser {
        request_id,
        room_id: room_id.to_owned(),
        user_id: user_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit invite failed: {e}"))?;
    wait_for_user_invited_ack(conn, request_id, label).await
}

async fn load_room_settings_for_qa(
    conn: &mut CoreConnection,
    room_id: &str,
    label: &str,
) -> Result<RoomSettingsSnapshot, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::LoadRoomSettings {
        request_id,
        room_id: room_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit load settings failed: {e}"))?;
    wait_for_room_settings_loaded(conn, request_id, label).await
}

fn assert_room_settings_contains_members(
    settings: &RoomSettingsSnapshot,
    expected_user_ids: &[&str],
    label: &str,
) -> Result<(), String> {
    let observed_user_ids = settings
        .members
        .iter()
        .map(|member| member.user_id.as_str())
        .collect::<BTreeSet<_>>();
    let missing_count = expected_user_ids
        .iter()
        .filter(|user_id| !observed_user_ids.contains(**user_id))
        .count();
    if missing_count > 0 {
        return Err(format!(
            "{label}: member list missing expected users \
             (expected={}, observed={}, missing={missing_count})",
            expected_user_ids.len(),
            observed_user_ids.len()
        ));
    }
    Ok(())
}

async fn accept_invite_for_qa(
    conn: &mut CoreConnection,
    room_id: &str,
    label: &str,
) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::AcceptInvite {
        request_id,
        room_id: room_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit accept invite failed: {e}"))?;
    wait_for_invite_accepted(conn, request_id, room_id, label).await
}

async fn decline_invite_for_qa(
    conn: &mut CoreConnection,
    room_id: &str,
    label: &str,
) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::DeclineInvite {
        request_id,
        room_id: room_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit decline invite failed: {e}"))?;
    wait_for_invite_declined(conn, request_id, room_id, label).await
}

async fn start_direct_message_for_qa(
    conn: &mut CoreConnection,
    user_id: &str,
    label: &str,
) -> Result<String, String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::StartDirectMessage {
        request_id,
        user_id: user_id.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit start DM failed: {e}"))?;
    wait_for_direct_message_started(conn, request_id, label).await
}

async fn set_space_child_for_qa(
    conn: &mut CoreConnection,
    space_id: &str,
    child_room_id: &str,
    via_server: &str,
    label: &str,
) -> Result<(), String> {
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::SetSpaceChild {
        request_id,
        space_id: space_id.to_owned(),
        child_room_id: child_room_id.to_owned(),
        via_server: via_server.to_owned(),
    }))
    .await
    .map_err(|e| format!("{label}: submit set space child failed: {e}"))?;
    wait_for_space_child_set(conn, request_id, space_id, child_room_id, label).await
}

/// Wait for `RoomEvent::RoomCreated` with the given request_id. Returns room_id.
async fn wait_for_room_created(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<String, String> {
    let mut seen_total = 0usize;
    let mut seen_state_changed = 0usize;
    let mut seen_room_created_other = 0usize;
    let mut seen_operation_failed_other = 0usize;
    let mut last_event_kind = "none";
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for RoomEvent::RoomCreated request_id={}/{} seen_total={seen_total} seen_state_changed={seen_state_changed} seen_room_created_other={seen_room_created_other} seen_operation_failed_other={seen_operation_failed_other} last_event={last_event_kind}",
                    request_id.connection_id.0,
                    request_id.sequence,
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        seen_total += 1;
        last_event_kind = core_event_kind(&event);

        match event {
            CoreEvent::Room(RoomEvent::RoomCreated {
                request_id: ev_id,
                room_id,
            }) if ev_id == request_id => {
                return Ok(room_id);
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            CoreEvent::Room(RoomEvent::RoomCreated { .. }) => {
                seen_room_created_other += 1;
            }
            CoreEvent::OperationFailed { .. } => {
                seen_operation_failed_other += 1;
            }
            CoreEvent::StateChanged(_) => {
                seen_state_changed += 1;
            }
            _ => continue,
        }
    }
}

fn core_event_kind(event: &CoreEvent) -> &'static str {
    match event {
        CoreEvent::StateDelta(_) => "StateDelta",
        CoreEvent::StateChanged(_) => "StateChanged",
        CoreEvent::Account(_) => "Account",
        CoreEvent::Sync(_) => "Sync",
        CoreEvent::Room(room_event) => match room_event {
            RoomEvent::RoomCreated { .. } => "RoomCreated",
            RoomEvent::SpaceCreated { .. } => "SpaceCreated",
            RoomEvent::SpaceChildSet { .. } => "SpaceChildSet",
            RoomEvent::UserInvited { .. } => "UserInvited",
            RoomEvent::InviteAccepted { .. } => "InviteAccepted",
            RoomEvent::InviteDeclined { .. } => "InviteDeclined",
            RoomEvent::RoomJoined { .. } => "RoomJoined",
            RoomEvent::RoomListUpdated => "RoomListUpdated",
            _ => "Room",
        },
        CoreEvent::Timeline(_) => "Timeline",
        CoreEvent::LiveSignals(_) => "LiveSignals",
        CoreEvent::Search(_) => "Search",
        CoreEvent::E2eeTrust(_) => "E2eeTrust",
        CoreEvent::Activity(_) => "Activity",
        CoreEvent::LocalEncryption(_) => "LocalEncryption",
        CoreEvent::NativeAttention(_) => "NativeAttention",
        CoreEvent::CjkTextPolicy(_) => "CjkTextPolicy",
        CoreEvent::ThreadsList(_) => "ThreadsList",
        CoreEvent::IntentLifecycle { .. } => "IntentLifecycle",
        CoreEvent::OperationFailed { .. } => "OperationFailed",
    }
}

async fn query_directory_until_room_visible(
    conn: &mut CoreConnection,
    query: DirectoryQuery,
    room_id: &str,
    alias: &str,
    label: &str,
) -> Result<Vec<DirectoryRoomSummary>, String> {
    for attempt in 1..=6 {
        let request_id = conn.next_request_id();
        conn.command(CoreCommand::Room(RoomCommand::QueryDirectory {
            request_id,
            query: query.clone(),
        }))
        .await
        .map_err(|e| format!("{label}: submit directory query failed: {e}"))?;
        let rooms = wait_for_directory_query_completed(conn, request_id, label).await?;
        if rooms
            .iter()
            .any(|room| room.room_id == room_id || room.canonical_alias.as_deref() == Some(alias))
        {
            return Ok(rooms);
        }
        if attempt < 6 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    Err(format!(
        "{label}: public directory did not return the created room after bounded retries"
    ))
}

async fn wait_for_directory_query_completed(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<Vec<DirectoryRoomSummary>, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for DirectoryQueryCompleted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::DirectoryQueryCompleted {
                request_id: ev_id,
                rooms,
                ..
            }) if ev_id == request_id => {
                return Ok(rooms);
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_room_settings_loaded(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<RoomSettingsSnapshot, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomSettingsLoaded"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomSettingsLoaded {
                request_id: ev_id,
                settings,
            }) if ev_id == request_id => return Ok(settings),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => return Err(format!("{label} failed: {failure:?}")),
            _ => continue,
        }
    }
}

async fn wait_for_room_setting_updated(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<RoomSettingsSnapshot, String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomSettingUpdated"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomSettingUpdated {
                request_id: ev_id,
                settings,
            }) if ev_id == request_id => return Ok(settings),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => return Err(format!("{label} failed: {failure:?}")),
            _ => continue,
        }
    }
}

async fn wait_for_room_member_moderated(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomMemberModerated"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomMemberModerated {
                request_id: ev_id, ..
            }) if ev_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => return Err(format!("{label} failed: {failure:?}")),
            _ => continue,
        }
    }
}

async fn wait_for_room_management_forbidden(
    conn: &mut CoreConnection,
    request_id: RequestId,
    label: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    let mut saw_forbidden_failure = false;

    loop {
        if saw_forbidden_failure && room_management_forbidden_recorded(&conn.snapshot(), request_id)
        {
            return Ok(());
        }

        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for forbidden room-management state"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure:
                    CoreFailure::RoomOperationFailed {
                        kind: RoomFailureKind::Forbidden,
                    },
            } if ev_id == request_id => {
                saw_forbidden_failure = true;
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!(
                    "{label}: expected forbidden room-management failure, got {failure:?}"
                ));
            }
            CoreEvent::StateChanged(snapshot)
                if room_management_forbidden_recorded(&snapshot, request_id) =>
            {
                if saw_forbidden_failure {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn room_management_forbidden_recorded(snapshot: &AppState, request_id: RequestId) -> bool {
    matches!(
        &snapshot.room_management.operation,
        RoomManagementOperationState::Failed {
            request_id: state_request_id,
            operation,
            kind,
            ..
        } if *state_request_id == request_id.sequence
            && *operation == RoomManagementOperationKind::Moderation
            && *kind == OperationFailureKind::Forbidden
    )
}

/// Wait for `RoomEvent::SpaceCreated` with the given request_id. Returns space_id.
async fn wait_for_space_created(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<String, String> {
    let mut seen_total = 0usize;
    let mut seen_state_changed = 0usize;
    let mut seen_space_created_other = 0usize;
    let mut seen_room_created = 0usize;
    let mut seen_operation_failed_other = 0usize;
    let mut last_event_kind = "none";
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| {
                format!(
                    "{label}: timed out waiting for RoomEvent::SpaceCreated request_id={}/{} seen_total={seen_total} seen_state_changed={seen_state_changed} seen_space_created_other={seen_space_created_other} seen_room_created={seen_room_created} seen_operation_failed_other={seen_operation_failed_other} last_event={last_event_kind}",
                    request_id.connection_id.0,
                    request_id.sequence,
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;
        seen_total += 1;
        last_event_kind = core_event_kind(&event);

        match event {
            CoreEvent::Room(RoomEvent::SpaceCreated {
                request_id: ev_id,
                space_id,
            }) if ev_id == request_id => {
                return Ok(space_id);
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            CoreEvent::Room(RoomEvent::SpaceCreated { .. }) => {
                seen_space_created_other += 1;
            }
            CoreEvent::Room(RoomEvent::RoomCreated { .. }) => {
                seen_room_created += 1;
            }
            CoreEvent::OperationFailed { .. } => {
                seen_operation_failed_other += 1;
            }
            CoreEvent::StateChanged(_) => {
                seen_state_changed += 1;
            }
            _ => continue,
        }
    }
}

/// Wait for `RoomEvent::SpaceChildSet` with the given request_id.
async fn wait_for_space_child_set(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    space_id: &str,
    child_room_id: &str,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::SpaceChildSet"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::SpaceChildSet {
                request_id: ev_id,
                space_id: ev_space,
                child_room_id: ev_child,
            }) if ev_id == request_id => {
                if ev_space != space_id || ev_child != child_room_id {
                    return Err(format!(
                        "{label}: SpaceChildSet IDs mismatch: space={ev_space} child={ev_child}"
                    ));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

/// Wait for `RoomEvent::UserInvited` with the given request_id.
async fn wait_for_user_invited(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    room_id: &str,
    user_id: &str,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::UserInvited"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::UserInvited {
                request_id: ev_id,
                room_id: ev_room,
                user_id: ev_user,
            }) if ev_id == request_id => {
                if ev_room != room_id || ev_user != user_id {
                    return Err(format!(
                        "{label}: UserInvited IDs mismatch: room={ev_room} user={ev_user}"
                    ));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

/// Wait for `RoomEvent::UserInvited` by request_id without exposing IDs in
/// failure text. Used by private-data-free invite QA.
async fn wait_for_user_invited_ack(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::UserInvited"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::UserInvited {
                request_id: ev_id, ..
            }) if ev_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_invite_accepted(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    expected_room_id: &str,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::InviteAccepted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::InviteAccepted {
                request_id: ev_id,
                room_id,
            }) if ev_id == request_id => {
                if room_id != expected_room_id {
                    return Err(format!("{label}: accepted invite room mismatch"));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_invite_declined(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    expected_room_id: &str,
    label: &str,
) -> Result<(), String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::InviteDeclined"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::InviteDeclined {
                request_id: ev_id,
                room_id,
            }) if ev_id == request_id => {
                if room_id != expected_room_id {
                    return Err(format!("{label}: declined invite room mismatch"));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_direct_message_started(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<String, String> {
    let deadline = QaEventDeadline::after(EVENT_TIMEOUT);
    loop {
        let event = deadline
            .recv(conn)
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::DirectMessageStarted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::DirectMessageStarted {
                request_id: ev_id,
                room_id,
            }) if ev_id == request_id => return Ok(room_id),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

/// Wait for `RoomEvent::RoomJoined` with the given request_id.
async fn wait_for_room_joined(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    room_id: &str,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::RoomJoined"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomJoined {
                request_id: ev_id,
                room_id: ev_room,
            }) if ev_id == request_id => {
                if ev_room != room_id {
                    return Err(format!(
                        "{label}: RoomJoined room_id mismatch: got {ev_room}, expected {room_id}"
                    ));
                }
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_pin_event_completed(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::PinEventCompleted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::PinEventCompleted {
                request_id: ev_id, ..
            }) if ev_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_unpin_event_completed(
    conn: &mut CoreConnection,
    request_id: koushi_core::ids::RequestId,
    label: &str,
) -> Result<(), String> {
    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for RoomEvent::UnpinEventCompleted"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::UnpinEventCompleted {
                request_id: ev_id, ..
            }) if ev_id == request_id => return Ok(()),
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label} failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn wait_for_pinned_state(
    conn: &mut CoreConnection,
    room_id: &str,
    event_id: &str,
    expected_present: bool,
    label: &str,
) -> Result<(), String> {
    if snapshot_has_pinned_event(&conn.snapshot(), room_id, event_id) == expected_present {
        return Ok(());
    }

    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for pinned state"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::StateChanged(snapshot) => {
                if snapshot_has_pinned_event(&snapshot, room_id, event_id) == expected_present {
                    return Ok(());
                }
            }
            CoreEvent::Room(RoomEvent::PinnedEventsUpdated {
                room_id: ev_room_id,
                pinned,
            }) if ev_room_id == room_id => {
                let has_event = pinned.iter().any(|event| event.event_id == event_id);
                if has_event == expected_present {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
}

fn snapshot_has_pinned_event(snapshot: &AppState, room_id: &str, event_id: &str) -> bool {
    snapshot
        .room_interactions
        .get(room_id)
        .map(|state| {
            state
                .pinned_events
                .iter()
                .any(|event| event.event_id == event_id)
        })
        .unwrap_or(false)
}

/// Wait (event-driven on `RoomListUpdated`/`StateChanged`, bounded by
/// `ROOM_LIST_EVENT_TIMEOUT`) until the snapshot's room list contains the
/// expected room in `rooms` AND the expected space in `spaces`. Returns the matching
/// snapshot. Waiting for "any non-empty list" is not enough: spaces only
/// classify as spaces after the create reaches the client via sync, so the
/// list can be momentarily rooms-only.
async fn wait_for_room_list_containing(
    conn: &mut CoreConnection,
    expected_room_id: &str,
    expected_space_id: &str,
    label: &str,
) -> Result<AppState, String> {
    let contains_expected = |snapshot: &AppState| {
        snapshot.rooms.iter().any(|r| r.room_id == expected_room_id)
            && snapshot
                .spaces
                .iter()
                .any(|s| s.space_id == expected_space_id)
    };

    // Check the latest snapshot first in case it already has the data.
    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                format!(
                    "{label}: timed out waiting for room list to contain room \
                     {expected_room_id} and space {expected_space_id} \
                     (have {} rooms, {} spaces)",
                    snapshot.rooms.len(),
                    snapshot.spaces.len()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                // The discrete event may arrive before the reducer projected
                // the matching snapshot; check the latest snapshot and keep
                // waiting otherwise — a StateChanged will follow.
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            _ => continue,
        }
    }
}

async fn wait_for_room_in_room_list(
    conn: &mut CoreConnection,
    expected_room_id: &str,
    label: &str,
) -> Result<AppState, String> {
    let contains_expected =
        |snapshot: &AppState| snapshot.rooms.iter().any(|r| r.room_id == expected_room_id);

    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                format!(
                    "{label}: timed out waiting for room list to include the expected room \
                     (have {} rooms)",
                    snapshot.rooms.len()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            _ => continue,
        }
    }
}

async fn wait_for_encrypted_room_projection_for_qa(
    conn: &mut CoreConnection,
    expected_room_id: &str,
    label: &str,
) -> Result<AppState, String> {
    let contains_expected = |snapshot: &AppState| {
        snapshot
            .rooms
            .iter()
            .any(|room| room.room_id == expected_room_id && room.is_encrypted)
    };

    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                let encrypted_rooms = snapshot
                    .rooms
                    .iter()
                    .filter(|room| room.is_encrypted)
                    .count();
                format!(
                    "{label}: timed out waiting for encrypted room projection \
                     (rooms={}, encrypted_rooms={encrypted_rooms})",
                    snapshot.rooms.len(),
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) if contains_expected(&snapshot) => {
                return Ok(snapshot);
            }
            _ => {}
        }
    }
}

async fn wait_for_space_in_space_list(
    conn: &mut CoreConnection,
    expected_space_id: &str,
    label: &str,
) -> Result<AppState, String> {
    let contains_expected = |snapshot: &AppState| {
        snapshot
            .spaces
            .iter()
            .any(|s| s.space_id == expected_space_id)
    };

    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                let observer_diagnostics =
                    invite_observer_diagnostic_summary(&koushi_diagnostics::snapshot());
                let sync_diagnostics = sync_diagnostic_summary(&koushi_diagnostics::snapshot());
                format!(
                    "{label}: timed out waiting for space list to include the expected space \
                     (have {} spaces; {observer_diagnostics}; {sync_diagnostics})",
                    snapshot.spaces.len(),
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            _ => continue,
        }
    }
}

async fn wait_for_space_child_projection(
    conn: &mut CoreConnection,
    space_id: &str,
    expected_child_room_ids: &[String],
    label: &str,
) -> Result<AppState, String> {
    let contains_expected = |snapshot: &AppState| {
        space_has_expected_children(snapshot, space_id, expected_child_room_ids)
    };

    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                let observed_child_count = snapshot
                    .spaces
                    .iter()
                    .find(|space| space.space_id == space_id)
                    .map(|space| space.child_room_ids.len())
                    .unwrap_or_default();
                format!(
                    "{label}: timed out waiting for space child projection \
                     (expected_children={}, observed_children={}, spaces={})",
                    expected_child_room_ids.len(),
                    observed_child_count,
                    snapshot.spaces.len()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            _ => continue,
        }
    }
}

async fn select_space_and_wait_for_room_scope(
    conn: &mut CoreConnection,
    space_id: &str,
    expected_room_ids: &[String],
    label: &str,
) -> Result<AppState, String> {
    select_room_list_filter_for_qa(conn, RoomListFilter::Rooms, label).await?;
    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::SelectSpace {
        request_id,
        space_id: Some(space_id.to_owned()),
    }))
    .await
    .map_err(|e| format!("{label}: submit select space failed: {e}"))?;

    let matches_scope = |snapshot: &AppState| {
        room_list_matches_selected_space(snapshot, space_id, expected_room_ids)
    };
    let snapshot = conn.snapshot();
    if matches_scope(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                format!(
                    "{label}: timed out waiting for selected-space room scope \
                     (expected_rooms={}, projected_items={}, total_rooms={}, active_space={})",
                    expected_room_ids.len(),
                    snapshot.room_list.items.len(),
                    snapshot.rooms.len(),
                    snapshot.navigation.active_space_id.is_some()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if matches_scope(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if matches_scope(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label}: select space failed: {failure:?}"));
            }
            _ => continue,
        }
    }
}

async fn select_room_list_filter_for_qa(
    conn: &mut CoreConnection,
    filter: RoomListFilter,
    label: &str,
) -> Result<(), String> {
    if conn.snapshot().room_list.active_filter == filter {
        return Ok(());
    }

    let request_id = conn.next_request_id();
    conn.command(CoreCommand::App(AppCommand::SelectRoomListFilter {
        request_id,
        filter,
    }))
    .await
    .map_err(|e| format!("{label}: submit room-list filter failed: {e}"))?;

    loop {
        let event = tokio::time::timeout(EVENT_TIMEOUT, conn.recv_event())
            .await
            .map_err(|_| format!("{label}: timed out waiting for room-list filter"))?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::StateChanged(snapshot) if snapshot.room_list.active_filter == filter => {
                return Ok(());
            }
            CoreEvent::Room(RoomEvent::RoomListUpdated)
                if conn.snapshot().room_list.active_filter == filter =>
            {
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label}: room-list filter failed: {failure:?}"));
            }
            _ if conn.snapshot().room_list.active_filter == filter => return Ok(()),
            _ => continue,
        }
    }
}

fn space_has_expected_children(
    snapshot: &AppState,
    space_id: &str,
    expected_child_room_ids: &[String],
) -> bool {
    let Some(space) = snapshot
        .spaces
        .iter()
        .find(|space| space.space_id == space_id)
    else {
        return false;
    };
    let child_room_ids = space.child_room_ids.iter().collect::<BTreeSet<_>>();
    expected_child_room_ids
        .iter()
        .all(|room_id| child_room_ids.contains(room_id))
}

fn room_list_matches_selected_space(
    snapshot: &AppState,
    space_id: &str,
    expected_room_ids: &[String],
) -> bool {
    if snapshot.navigation.active_space_id.as_deref() != Some(space_id)
        || snapshot.room_list.active_filter != RoomListFilter::Rooms
        || !space_has_expected_children(snapshot, space_id, expected_room_ids)
    {
        return false;
    }
    let expected = expected_room_ids.iter().collect::<BTreeSet<_>>();
    let projected = snapshot
        .room_list
        .items
        .iter()
        .filter(|item| matches!(item.kind, koushi_state::RoomListEntryKind::Room))
        .map(|item| &item.room_id)
        .collect::<BTreeSet<_>>();
    projected == expected
}

async fn wait_for_dm_room_in_room_list(
    conn: &mut CoreConnection,
    expected_room_id: &str,
    label: &str,
) -> Result<AppState, String> {
    let contains_expected = |snapshot: &AppState| {
        snapshot
            .rooms
            .iter()
            .any(|room| room.room_id == expected_room_id && room.is_dm)
    };

    let snapshot = conn.snapshot();
    if contains_expected(&snapshot) {
        return Ok(snapshot);
    }

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                format!(
                    "{label}: timed out waiting for DM room in room list \
                     (have {} rooms)",
                    snapshot.rooms.len()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::Room(RoomEvent::RoomListUpdated) => {
                let snapshot = conn.snapshot();
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            CoreEvent::StateChanged(snapshot) => {
                if contains_expected(&snapshot) {
                    return Ok(snapshot);
                }
            }
            _ => continue,
        }
    }
}

async fn assert_dm_space_scope_for_qa(
    conn: &mut CoreConnection,
    member_space_id: &str,
    member_dm_room_id: &str,
    control_dm_room_id: &str,
) -> Result<(), String> {
    select_space_scope_for_qa(conn, None, "invites_dm select Home for DM scope").await?;
    wait_for_sidebar_dm_room_ids(
        conn,
        &[member_dm_room_id, control_dm_room_id],
        "invites_dm Home DM scope",
    )
    .await?;

    select_space_scope_for_qa(
        conn,
        Some(member_space_id),
        "invites_dm select member Space for DM scope",
    )
    .await?;
    wait_for_sidebar_dm_room_ids(conn, &[member_dm_room_id], "invites_dm Space DM scope").await
}

async fn select_space_scope_for_qa(
    conn: &mut CoreConnection,
    space_id: Option<&str>,
    label: &str,
) -> Result<(), String> {
    let matches_scope =
        |snapshot: &AppState| snapshot.navigation.active_space_id.as_deref() == space_id;
    if matches_scope(&conn.snapshot()) {
        return Ok(());
    }

    let request_id = conn.next_request_id();
    conn.command(CoreCommand::Room(RoomCommand::SelectSpace {
        request_id,
        space_id: space_id.map(str::to_owned),
    }))
    .await
    .map_err(|e| format!("{label}: submit select space failed: {e}"))?;

    let deadline = tokio::time::Instant::now() + ROOM_LIST_EVENT_TIMEOUT;
    loop {
        let event = tokio::time::timeout_at(deadline, conn.recv_event())
            .await
            .map_err(|_| {
                let snapshot = conn.snapshot();
                format!(
                    "{label}: timed out waiting for space selection \
                     (expected_active={}, observed_active={})",
                    space_id.is_some(),
                    snapshot.navigation.active_space_id.is_some()
                )
            })?
            .map_err(|lag| format!("{label}: event stream lagged (skipped={})", lag.skipped))?;

        match event {
            CoreEvent::StateChanged(snapshot) if matches_scope(&snapshot) => return Ok(()),
            CoreEvent::Room(RoomEvent::RoomListUpdated) if matches_scope(&conn.snapshot()) => {
                return Ok(());
            }
            CoreEvent::OperationFailed {
                request_id: ev_id,
                failure,
            } if ev_id == request_id => {
                return Err(format!("{label}: select space failed: {failure:?}"));
            }
            _ if matches_scope(&conn.snapshot()) => return Ok(()),
            _ => continue,
        }
    }
}


#[cfg(test)]
mod tests {

    #[test]
    fn room_management_scenario_runs_after_room_space_and_reports_private_tokens() {
        assert!(QaScenario::RoomManagement.should_run_stage(QaStage::Safety));
        assert!(QaScenario::RoomManagement.should_run_stage(QaStage::LoginSync));
        assert!(QaScenario::RoomManagement.should_run_stage(QaStage::RoomSpace));
        assert!(QaScenario::RoomManagement.should_run_stage(QaStage::RoomManagement));
        assert!(!QaScenario::RoomManagement.should_run_stage(QaStage::Timeline));
        assert!(QaScenario::RoomManagement.suppress_matrix_identifiers());

        assert_eq!(
            final_tokens_for_scenario(QaScenario::RoomManagement),
            [
                "safety=ok",
                "login_sync=ok",
                "room_space=ok",
                "room_settings=ok",
                "moderation=ok",
                "permission_guard=ok",
                "restore_cleanup=ok",
            ]
        );
    }


    #[test]
    fn room_management_forbidden_predicate_requires_matching_failed_moderation_state() {
        let request_id = RequestId {
            connection_id: koushi_core::ids::RuntimeConnectionId(1),
            sequence: 42,
        };
        let mut state = AppState::default();

        assert!(!room_management_forbidden_recorded(&state, request_id));

        state.room_management.operation = RoomManagementOperationState::Failed {
            request_id: 41,
            room_id: "!redacted:example.invalid".to_owned(),
            operation: RoomManagementOperationKind::Moderation,
            kind: OperationFailureKind::Forbidden,
        };
        assert!(!room_management_forbidden_recorded(&state, request_id));

        state.room_management.operation = RoomManagementOperationState::Failed {
            request_id: 42,
            room_id: "!redacted:example.invalid".to_owned(),
            operation: RoomManagementOperationKind::Moderation,
            kind: OperationFailureKind::Forbidden,
        };
        assert!(room_management_forbidden_recorded(&state, request_id));
    }
}
