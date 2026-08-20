//! Exact AST extraction draft from immutable timeline baseline.

const MEDIA_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
struct PrivateMediaEntry {
    source: MediaSource,
    thumbnail_source: Option<MediaSource>,
    mimetype: Option<String>,
    size: u64,
    width: Option<u64>,
    height: Option<u64>,
}

struct MediaDownloadReady {
    download_state: TimelineMediaDownloadState,
    source_url: String,
    byte_count: u64,
    mimetype: Option<String>,
    width: Option<u64>,
    height: Option<u64>,
}

enum MediaDownloadOutcome {
    Ready(MediaDownloadReady),
    Failed(TimelineFailureKind),
}

fn media_download_selection_token(selection: &MediaDownloadSelection) -> &'static str {
    match selection {
        MediaDownloadSelection::File => "file",
        MediaDownloadSelection::Thumbnail { .. } => "thumbnail",
    }
}

fn media_download_failure_token(kind: TimelineFailureKind) -> &'static str {
    match kind {
        TimelineFailureKind::InvalidDirection => "invalid_direction",
        TimelineFailureKind::InvalidReactionTarget => "invalid_reaction_target",
        TimelineFailureKind::InvalidReactionState => "invalid_reaction_state",
        TimelineFailureKind::InvalidSendTarget => "invalid_send_target",
        TimelineFailureKind::InvalidSendState => "invalid_send_state",
        TimelineFailureKind::SecureBackupRequired => "secure_backup_required",
        TimelineFailureKind::ComposerRevisionExhausted => "composer_revision_exhausted",
        TimelineFailureKind::UnsupportedSlashCommand => "unsupported_slash_command",
        TimelineFailureKind::NotSubscribed => "not_subscribed",
        TimelineFailureKind::Forbidden => "forbidden",
        TimelineFailureKind::Network => "network",
        TimelineFailureKind::Timeout => "timeout",
        TimelineFailureKind::Sdk => "sdk",
        TimelineFailureKind::QueueOverflow => "queue_overflow",
    }
}

fn media_source_token(source: &MediaSource) -> &'static str {
    match source {
        MediaSource::Plain(_) => "plain",
        MediaSource::Encrypted(_) => "encrypted",
    }
}

fn media_format_token(format: &MediaFormat) -> &'static str {
    match format {
        MediaFormat::File => "file",
        MediaFormat::Thumbnail(_) => "thumbnail",
    }
}

fn sdk_media_error_token(error: &matrix_sdk::Error) -> &'static str {
    match error {
        matrix_sdk::Error::Http(_) => "http",
        matrix_sdk::Error::AuthenticationRequired => "authentication_required",
        matrix_sdk::Error::InsufficientData => "insufficient_data",
        matrix_sdk::Error::SerdeJson(_) => "serde_json",
        matrix_sdk::Error::Io(_) => "io",
        matrix_sdk::Error::CrossProcessLockError(_) => "cross_process_lock",
        matrix_sdk::Error::StateStore(_) => "state_store",
        matrix_sdk::Error::EventCacheStore(_) => "event_cache_store",
        matrix_sdk::Error::MediaStore(_) => "media_store",
        matrix_sdk::Error::Identifier(_) => "identifier",
        matrix_sdk::Error::Url(_) => "url",
        matrix_sdk::Error::UserTagName(_) => "user_tag_name",
        matrix_sdk::Error::SlidingSync(_) => "sliding_sync",
        matrix_sdk::Error::WrongRoomState(_) => "wrong_room_state",
        matrix_sdk::Error::MultipleSessionCallbacks => "multiple_session_callbacks",
        matrix_sdk::Error::OAuth(_) => "oauth",
        matrix_sdk::Error::ConcurrentRequestFailed => "concurrent_request_failed",
        matrix_sdk::Error::UnknownError(_) => "unknown",
        matrix_sdk::Error::EventCache(_) => "event_cache",
        matrix_sdk::Error::SendQueueWedgeError(_) => "send_queue_wedge",
        matrix_sdk::Error::BackupNotEnabled => "backup_not_enabled",
        matrix_sdk::Error::CantIgnoreLoggedInUser => "cant_ignore_logged_in_user",
        matrix_sdk::Error::Media(_) => "media",
        matrix_sdk::Error::ReplyError(_) => "reply",
        matrix_sdk::Error::PowerLevels(_) => "power_levels",
        matrix_sdk::Error::Timeout => "timeout",
        _ => "other",
    }
}

fn trace_media_download_request(
    stage: &'static str,
    request_id: RequestId,
    selection: &MediaDownloadSelection,
    entry: Option<&PrivateMediaEntry>,
    outcome: Option<&'static str>,
) {
    let mut event = DiagnosticEvent::new(DiagnosticLevel::Info, "core.media_download", stage)
        .field(DiagnosticField::request_id(
            "request_id",
            request_id.connection_id.0,
            request_id.sequence,
        ))
        .field(DiagnosticField::token(
            "selection",
            media_download_selection_token(selection),
        ));
    if let Some(entry) = entry {
        event = event
            .field(DiagnosticField::token(
                "source",
                media_source_token(&entry.source),
            ))
            .field(DiagnosticField::boolean(
                "source_encrypted",
                matches!(entry.source, MediaSource::Encrypted(_)),
            ))
            .field(DiagnosticField::boolean(
                "thumbnail_source_present",
                entry.thumbnail_source.is_some(),
            ))
            .field(DiagnosticField::count("declared_size", entry.size));
    }
    if let Some(outcome) = outcome {
        event = event.field(DiagnosticField::token("outcome", outcome));
    }
    koushi_diagnostics::record(event);
}

fn trace_media_download_worker(
    stage: &'static str,
    request: &MediaRequestParameters,
    byte_count: Option<u64>,
    failure: Option<&'static str>,
    sdk_error: Option<&'static str>,
) {
    let mut event = DiagnosticEvent::new(DiagnosticLevel::Info, "core.media_download", stage)
        .field(DiagnosticField::token(
            "source",
            media_source_token(&request.source),
        ))
        .field(DiagnosticField::boolean(
            "source_encrypted",
            matches!(request.source, MediaSource::Encrypted(_)),
        ))
        .field(DiagnosticField::token(
            "format",
            media_format_token(&request.format),
        ));
    if let Some(byte_count) = byte_count {
        event = event.field(DiagnosticField::count("byte_count", byte_count));
    }
    if let Some(failure) = failure {
        event = event.field(DiagnosticField::token("failure", failure));
    }
    if let Some(sdk_error) = sdk_error {
        event = event.field(DiagnosticField::token("sdk_error", sdk_error));
    }
    koushi_diagnostics::record(event);
}

fn trace_media_download_file_write_failed(
    request: &MediaRequestParameters,
    byte_count: u64,
    failure: &'static str,
    error: Option<&std::io::Error>,
    data_dir_present: bool,
    target_dir: Option<&Path>,
    target_path: Option<&Path>,
) {
    let mut event = DiagnosticEvent::new(
        DiagnosticLevel::Info,
        "core.media_download",
        "file_write_failed",
    )
    .field(DiagnosticField::token(
        "source",
        media_source_token(&request.source),
    ))
    .field(DiagnosticField::boolean(
        "source_encrypted",
        matches!(request.source, MediaSource::Encrypted(_)),
    ))
    .field(DiagnosticField::token(
        "format",
        media_format_token(&request.format),
    ))
    .field(DiagnosticField::count("byte_count", byte_count))
    .field(DiagnosticField::token("failure", failure))
    .field(DiagnosticField::boolean(
        "data_dir_present",
        data_dir_present,
    ))
    .field(DiagnosticField::boolean(
        "target_dir_exists",
        target_dir.is_some_and(Path::exists),
    ))
    .field(DiagnosticField::boolean(
        "target_path_exists",
        target_path.is_some_and(Path::exists),
    ))
    .field(DiagnosticField::boolean(
        "target_path_is_file",
        target_path.is_some_and(Path::is_file),
    ))
    .field(DiagnosticField::boolean(
        "target_path_is_dir",
        target_path.is_some_and(Path::is_dir),
    ));
    if let Some(error) = error {
        event = event.field(DiagnosticField::token(
            "io_error_kind",
            io_error_kind_token(error.kind()),
        ));
        if let Some(raw_os_error) = error.raw_os_error()
            && let Ok(raw_os_error) = u64::try_from(raw_os_error)
        {
            event = event.field(DiagnosticField::count("raw_os_error", raw_os_error));
        }
    }
    koushi_diagnostics::record(event);
}

fn io_error_kind_token(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "permission_denied",
        std::io::ErrorKind::AlreadyExists => "already_exists",
        std::io::ErrorKind::InvalidInput => "invalid_input",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::TimedOut => "timed_out",
        std::io::ErrorKind::WriteZero => "write_zero",
        std::io::ErrorKind::Interrupted => "interrupted",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        std::io::ErrorKind::OutOfMemory => "out_of_memory",
        std::io::ErrorKind::Other => "other",
        _ => "unknown",
    }
}

impl TimelineActor {
    fn sdk_room_for_key(&self) -> Option<matrix_sdk::Room> {
        let room_id_str = match &self.key.kind {
            TimelineKind::Room { room_id }
            | TimelineKind::Thread { room_id, .. }
            | TimelineKind::Focused { room_id, .. } => room_id,
        };
        let room_id = matrix_sdk::ruma::RoomId::parse(room_id_str).ok()?;
        self.session.client().get_room(&room_id)
    }
    async fn handle_download_media(
        &mut self,
        request_id: RequestId,
        event_id: String,
        selection: MediaDownloadSelection,
    ) {
        trace_media_download_request(
            "request_received",
            request_id,
            &selection,
            self.media_sources.get(&event_id),
            None,
        );
        if self.media_downloads_in_progress.contains(&event_id) {
            trace_media_download_request(
                "request_rejected",
                request_id,
                &selection,
                self.media_sources.get(&event_id),
                Some("already_in_progress"),
            );
            self.emit_media_download_current_state(request_id, &event_id)
                .await;
            return;
        }

        let Some(entry) = self.media_sources.get(&event_id).cloned() else {
            trace_media_download_request(
                "request_rejected",
                request_id,
                &selection,
                None,
                Some("missing_media_source"),
            );
            self.emit_download_failed(request_id, &event_id, TimelineFailureKind::Sdk)
                .await;
            return;
        };

        let Some(request) = media_request_for_download(&entry, &selection) else {
            trace_media_download_request(
                "request_rejected",
                request_id,
                &selection,
                Some(&entry),
                Some("unsupported_request"),
            );
            self.emit_download_failed(request_id, &event_id, TimelineFailureKind::Sdk)
                .await;
            return;
        };

        self.media_downloads_in_progress.insert(event_id.clone());
        self.emit_media_download_current_state(request_id, &event_id)
            .await;

        let session = self.session.clone();
        let room_id = self.key.room_id().to_owned();
        let data_dir = self.data_dir.clone();
        let actor_tx = self.msg_tx.clone();
        let event_id_for_task = event_id.clone();
        let task = executor::spawn(async move {
            let outcome = Self::download_media_for(
                session,
                data_dir,
                room_id,
                event_id_for_task.clone(),
                entry,
                request,
            )
            .await;
            let _ = actor_tx
                .send(TimelineActorMessage::MediaDownloadFinished {
                    request_id,
                    event_id: event_id_for_task,
                    outcome,
                })
                .await;
        });
        self.media_download_tasks.insert(event_id, task);
    }
    async fn download_media_for(
        session: Arc<MatrixClientSession>,
        data_dir: Option<PathBuf>,
        room_id: String,
        event_id: String,
        entry: PrivateMediaEntry,
        request: MediaRequestParameters,
    ) -> MediaDownloadOutcome {
        let Some(data_dir) = data_dir else {
            trace_media_download_file_write_failed(
                &request,
                0,
                "missing_data_dir",
                None,
                false,
                None,
                None,
            );
            return MediaDownloadOutcome::Failed(TimelineFailureKind::Sdk);
        };

        // Matrix IDs contain ':' which is not valid in Windows path components.
        // Use hashed path components so the local path is portable and private.
        let dir_name = sanitize_matrix_id_for_path(&room_id);
        let file_name = format!("{}.bin", sanitize_matrix_id_for_path(&event_id));
        let dir = data_dir.join("media_downloads").join(dir_name);
        let path = dir.join(file_name);
        if let Ok(metadata) = tokio::fs::metadata(&path).await {
            if metadata.is_file() && metadata.len() > 0 {
                let byte_count = metadata.len();
                let source_url = path.to_string_lossy().into_owned();
                trace_media_download_worker("cache_hit", &request, Some(byte_count), None, None);
                return MediaDownloadOutcome::Ready(MediaDownloadReady {
                    download_state: TimelineMediaDownloadState::Ready {
                        source_url: source_url.clone(),
                        width: entry.width,
                        height: entry.height,
                        mime_type: entry.mimetype.clone(),
                    },
                    source_url,
                    byte_count,
                    mimetype: entry.mimetype,
                    width: entry.width,
                    height: entry.height,
                });
            }
        }

        trace_media_download_worker("sdk_fetch_started", &request, None, None, None);
        let bytes = match executor::timeout(
            MEDIA_DOWNLOAD_TIMEOUT,
            session.client().media().get_media_content(&request, true),
        )
        .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                let kind = classify_media_download_error(&error);
                trace_media_download_worker(
                    "sdk_fetch_failed",
                    &request,
                    None,
                    Some(media_download_failure_token(kind)),
                    Some(sdk_media_error_token(&error)),
                );
                return MediaDownloadOutcome::Failed(kind);
            }
            Err(_) => {
                trace_media_download_worker(
                    "sdk_fetch_failed",
                    &request,
                    None,
                    Some(media_download_failure_token(TimelineFailureKind::Timeout)),
                    Some("timeout"),
                );
                return MediaDownloadOutcome::Failed(TimelineFailureKind::Timeout);
            }
        };

        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if let Err(error) = tokio::fs::create_dir_all(&dir).await {
            trace_media_download_file_write_failed(
                &request,
                byte_count,
                "create_dir",
                Some(&error),
                true,
                Some(&dir),
                None,
            );
            return MediaDownloadOutcome::Failed(TimelineFailureKind::Sdk);
        }
        if let Err(error) = tokio::fs::write(&path, &bytes).await {
            trace_media_download_file_write_failed(
                &request,
                byte_count,
                "write_file",
                Some(&error),
                true,
                Some(&dir),
                Some(&path),
            );
            return MediaDownloadOutcome::Failed(TimelineFailureKind::Sdk);
        }

        let source_url = path.to_string_lossy().into_owned();
        trace_media_download_worker("completed", &request, Some(byte_count), None, None);
        MediaDownloadOutcome::Ready(MediaDownloadReady {
            download_state: TimelineMediaDownloadState::Ready {
                source_url: source_url.clone(),
                width: entry.width,
                height: entry.height,
                mime_type: entry.mimetype.clone(),
            },
            source_url,
            byte_count,
            mimetype: entry.mimetype,
            width: entry.width,
            height: entry.height,
        })
    }
    async fn handle_media_download_finished(
        &mut self,
        request_id: RequestId,
        event_id: String,
        outcome: MediaDownloadOutcome,
    ) {
        self.media_downloads_in_progress.remove(&event_id);
        self.media_download_tasks.remove(&event_id);
        match outcome {
            MediaDownloadOutcome::Ready(ready) => {
                self.emit(CoreEvent::Timeline(TimelineEvent::MediaDownloadCompleted {
                    request_id,
                    key: self.key.clone(),
                    event_id: event_id.clone(),
                    source_url: ready.source_url,
                    byte_count: ready.byte_count,
                    mimetype: ready.mimetype,
                    width: ready.width,
                    height: ready.height,
                }));
                self.emit_action_reliable(AppAction::MediaDownloadUpdated {
                    room_id: self.key.room_id().to_owned(),
                    event_id,
                    state: ready.download_state,
                })
                .await;
            }
            MediaDownloadOutcome::Failed(kind) => {
                self.emit_download_failed(request_id, &event_id, kind).await;
            }
        }
    }
    async fn emit_media_download_current_state(&self, request_id: RequestId, event_id: &str) {
        let total = self
            .media_sources
            .get(event_id)
            .map(|entry| entry.size)
            .unwrap_or(0);
        let progress = MediaTransferProgress { current: 0, total };
        self.emit(CoreEvent::Timeline(TimelineEvent::MediaDownloadProgress {
            request_id,
            key: self.key.clone(),
            event_id: event_id.to_owned(),
            progress,
        }));
        self.emit_action_reliable(AppAction::MediaDownloadUpdated {
            room_id: self.key.room_id().to_owned(),
            event_id: event_id.to_owned(),
            state: TimelineMediaDownloadState::Pending {
                progress: Some(progress),
            },
        })
        .await;
    }
    async fn emit_download_failed(
        &self,
        request_id: RequestId,
        event_id: &str,
        kind: TimelineFailureKind,
    ) {
        trace_media_download_request(
            "failed_projected",
            request_id,
            &MediaDownloadSelection::File,
            self.media_sources.get(event_id),
            Some(media_download_failure_token(kind)),
        );
        self.emit(CoreEvent::Timeline(TimelineEvent::MediaDownloadFailed {
            request_id,
            key: self.key.clone(),
            event_id: event_id.to_owned(),
            kind,
        }));
        // Use reliable delivery — a dropped failure action leaves the UI stuck
        // in a pending download state (REPOSITORY_RULES L124-128).
        self.emit_action_reliable(AppAction::MediaDownloadUpdated {
            room_id: self.key.room_id().to_owned(),
            event_id: event_id.to_owned(),
            state: TimelineMediaDownloadState::Failed {
                failure_kind: match kind {
                    TimelineFailureKind::Network => OperationFailureKind::Network,
                    TimelineFailureKind::Sdk => OperationFailureKind::Sdk,
                    TimelineFailureKind::Forbidden => OperationFailureKind::Forbidden,
                    TimelineFailureKind::Timeout => OperationFailureKind::Timeout,
                    _ => OperationFailureKind::Sdk,
                },
            },
        })
        .await;
    }
    async fn emit_media_gallery_if_changed(&mut self) {
        let items = media_gallery_items_from_timeline_items(&self.key, &self.navigation_items);
        if items == self.media_gallery_items {
            return;
        }
        if let Some(action) = media_gallery_updated_action(&self.key, items) {
            if !self.emit_action_reliable(action).await {
                return;
            }
        }
        self.media_gallery_items =
            media_gallery_items_from_timeline_items(&self.key, &self.navigation_items);
    }
    fn apply_sdk_media_cache_diff(
        media_sources: &mut HashMap<String, PrivateMediaEntry>,
        diff: &eyeball_im::VectorDiff<Arc<SdkTimelineItem>>,
    ) {
        use eyeball_im::VectorDiff;

        match diff {
            VectorDiff::PushFront { value }
            | VectorDiff::PushBack { value }
            | VectorDiff::Insert { value, .. }
            | VectorDiff::Set { value, .. } => {
                cache_sdk_item_media_source(media_sources, value);
            }
            VectorDiff::Append { values } => {
                for item in values {
                    cache_sdk_item_media_source(media_sources, item);
                }
            }
            VectorDiff::Reset { values } => {
                media_sources.clear();
                for item in values {
                    cache_sdk_item_media_source(media_sources, item);
                }
            }
            VectorDiff::Clear => {
                media_sources.clear();
            }
            VectorDiff::Remove { .. }
            | VectorDiff::Truncate { .. }
            | VectorDiff::PopFront
            | VectorDiff::PopBack => {}
        }
    }
}

fn media_gallery_updated_action(
    key: &TimelineKey,
    items: Vec<TimelineMediaGalleryItem>,
) -> Option<AppAction> {
    let TimelineKind::Room { room_id } = &key.kind else {
        return None;
    };

    Some(AppAction::MediaGalleryUpdated {
        room_id: room_id.clone(),
        items,
    })
}

struct AuthoritativeMediaGalleryReplacement {
    items: Vec<TimelineMediaGalleryItem>,
    action: AppAction,
}

fn authoritative_media_gallery_replacement(
    key: &TimelineKey,
    current: &[TimelineMediaGalleryItem],
    authoritative_items: &[TimelineItem],
) -> Option<AuthoritativeMediaGalleryReplacement> {
    let items = media_gallery_items_from_timeline_items(key, authoritative_items);
    if items == current {
        return None;
    }
    let action = media_gallery_updated_action(key, items.clone())?;
    Some(AuthoritativeMediaGalleryReplacement { items, action })
}

fn media_gallery_items_from_timeline_items(
    key: &TimelineKey,
    items: &[TimelineItem],
) -> Vec<TimelineMediaGalleryItem> {
    let TimelineKind::Room { room_id } = &key.kind else {
        return Vec::new();
    };

    let mut gallery_items = items
        .iter()
        .filter_map(|item| media_gallery_item_from_timeline_item(room_id, item))
        .collect::<Vec<_>>();
    gallery_items.sort_by(|left, right| {
        right
            .timestamp_ms
            .cmp(&left.timestamp_ms)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    gallery_items
}

fn media_gallery_item_from_timeline_item(
    room_id: &str,
    item: &TimelineItem,
) -> Option<TimelineMediaGalleryItem> {
    if item.is_hidden || item.is_redacted {
        return None;
    }
    let TimelineItemId::Event { event_id } = &item.id else {
        return None;
    };
    let media = item.media.as_ref()?;

    Some(TimelineMediaGalleryItem {
        event_id: event_id.clone(),
        room_id: room_id.to_owned(),
        sender: item.sender.clone(),
        sender_label: item.sender_label.clone(),
        timestamp_ms: item.timestamp_ms.unwrap_or_default(),
        media: TimelineMediaGalleryMedia {
            kind: media_gallery_kind_from_timeline_kind(media.kind),
            filename: media.filename.clone(),
            source: media_gallery_source_from_timeline_source(&media.source),
            mimetype: media.mimetype.clone(),
            size: media.size,
            width: media.width,
            height: media.height,
            thumbnail: media.thumbnail.as_ref().map(media_gallery_thumbnail),
        },
    })
}

fn media_gallery_kind_from_timeline_kind(kind: TimelineMediaKind) -> GalleryTimelineMediaKind {
    match kind {
        TimelineMediaKind::Image => GalleryTimelineMediaKind::Image,
        TimelineMediaKind::File => GalleryTimelineMediaKind::File,
        TimelineMediaKind::Audio => GalleryTimelineMediaKind::Audio,
        TimelineMediaKind::Video => GalleryTimelineMediaKind::Video,
    }
}

fn media_gallery_source_from_timeline_source(
    source: &TimelineMediaSource,
) -> TimelineMediaGallerySource {
    TimelineMediaGallerySource {
        mxc_uri: source.mxc_uri.clone(),
        encrypted: source.encrypted,
        encryption_version: source.encryption_version.clone(),
    }
}

fn media_gallery_thumbnail(thumbnail: &TimelineMediaThumbnail) -> TimelineMediaGalleryThumbnail {
    TimelineMediaGalleryThumbnail {
        source: media_gallery_source_from_timeline_source(&thumbnail.source),
        mimetype: thumbnail.mimetype.clone(),
        size: thumbnail.size,
        width: thumbnail.width,
        height: thumbnail.height,
    }
}

fn classify_media_download_error(error: &matrix_sdk::Error) -> TimelineFailureKind {
    if matches!(error, matrix_sdk::Error::Timeout) {
        return TimelineFailureKind::Timeout;
    }
    if error
        .client_api_error_kind()
        .map(|kind| kind == &matrix_sdk::ruma::api::error::ErrorKind::Forbidden)
        .unwrap_or(false)
    {
        return TimelineFailureKind::Forbidden;
    }
    if matches!(error, matrix_sdk::Error::Http(_)) {
        return TimelineFailureKind::Network;
    }
    TimelineFailureKind::Sdk
}

