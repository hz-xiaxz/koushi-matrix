struct QaTcpProxy {
    listen_addr: SocketAddr,
    enabled: Arc<AtomicBool>,
    room_send_forwarded: Arc<AtomicUsize>,
    room_send_responses_completed: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
    active_streams: Arc<Mutex<Vec<TcpStream>>>,
    messages_control: Arc<Mutex<QaMessagesProxyControl>>,
    accept_thread: Option<JoinHandle<()>>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QaProxyRequestKind {
    RoomSend,
    RoomMessages,
    Other,
}
#[derive(Clone, Debug, Eq, PartialEq)]
enum QaProxyRequestAction {
    Forward,
    FailClosed,
    ServeCannedMessages(Vec<u8>),
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct QaMessagesProxyObservation {
    room_messages_request_count: u32,
    first_request_was_exact_tokenless_limit: bool,
    first_request_had_from: bool,
    freshness_page_served: bool,
    expected_end_token_was_used: bool,
    expected_end_token_request_count: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
enum QaMessagesProxyExpectation {
    TokenlessLiveTail,
    BackwardFrom { token: String },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QaMessagesProxyPhase {
    Open,
    Armed,
    Served,
    Rejected,
}
struct QaMessagesProxyState {
    phase: QaMessagesProxyPhase,
    expectation: Option<QaMessagesProxyExpectation>,
    tracked_end_token: Option<String>,
    observation: QaMessagesProxyObservation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QaMessagesProxyDecision {
    Forward,
    FailClosed,
    ServeCannedPage,
}
impl QaMessagesProxyState {
    fn arm_page(
        &mut self,
        expectation: QaMessagesProxyExpectation,
        tracked_end_token: Option<String>,
    ) {
        self.phase = QaMessagesProxyPhase::Armed;
        self.expectation = Some(expectation);
        self.tracked_end_token = tracked_end_token;
        self.observation = QaMessagesProxyObservation::default();
    }

    fn observe_room_messages_request(
        &mut self,
        metadata: &QaRoomMessagesRequestMetadata,
    ) -> QaMessagesProxyDecision {
        self.observation.room_messages_request_count = self
            .observation
            .room_messages_request_count
            .saturating_add(1);
        if metadata.direction_is_backward
            && self
                .tracked_end_token
                .as_deref()
                .is_some_and(|token| metadata.from_token.as_deref() == Some(token))
        {
            self.observation.expected_end_token_request_count = self
                .observation
                .expected_end_token_request_count
                .saturating_add(1);
        }
        if self.phase != QaMessagesProxyPhase::Armed {
            return QaMessagesProxyDecision::Forward;
        }

        self.observation.first_request_was_exact_tokenless_limit =
            metadata.query_is_exact_tokenless_limit;
        self.observation.first_request_had_from = metadata.has_from;
        let expected_request_matched = match self.expectation.as_ref() {
            Some(QaMessagesProxyExpectation::TokenlessLiveTail) => {
                metadata.query_is_exact_tokenless_limit && !metadata.has_from
            }
            Some(QaMessagesProxyExpectation::BackwardFrom { token }) => {
                let matched = metadata.direction_is_backward
                    && metadata.from_token.as_deref() == Some(token.as_str());
                self.observation.expected_end_token_was_used = matched;
                matched
            }
            None => false,
        };
        if expected_request_matched {
            self.phase = QaMessagesProxyPhase::Served;
            self.observation.freshness_page_served = true;
            QaMessagesProxyDecision::ServeCannedPage
        } else {
            self.phase = QaMessagesProxyPhase::Rejected;
            QaMessagesProxyDecision::FailClosed
        }
    }
}
struct QaCannedTimelineEvent {
    event_id: String,
    sender: String,
    body: String,
    origin_server_ts: u64,
}
struct QaCannedMessagesPage {
    events: Vec<QaCannedTimelineEvent>,
    end: Option<String>,
}
impl QaCannedMessagesPage {
    fn anchored_silent_gap(
        newest_known_event_id: String,
        newest_known_body: String,
        missing_event_id: String,
        missing_body: String,
        older_anchor_event_id: String,
        sender: String,
        older_anchor_body: String,
    ) -> Self {
        Self {
            events: vec![
                QaCannedTimelineEvent {
                    event_id: newest_known_event_id,
                    sender: sender.clone(),
                    body: newest_known_body,
                    origin_server_ts: 1_900_000_000_002,
                },
                QaCannedTimelineEvent {
                    event_id: missing_event_id,
                    sender: sender.clone(),
                    body: missing_body,
                    origin_server_ts: 1_900_000_000_001,
                },
                QaCannedTimelineEvent {
                    event_id: older_anchor_event_id,
                    sender,
                    body: older_anchor_body,
                    origin_server_ts: 1,
                },
            ],
            end: None,
        }
    }

    fn response_body(&self) -> io::Result<Vec<u8>> {
        let chunk = self
            .events
            .iter()
            .map(|event| {
                serde_json::json!({
                    "type": "m.room.message",
                    "event_id": event.event_id,
                    "sender": event.sender,
                    "origin_server_ts": event.origin_server_ts,
                    "content": {
                        "msgtype": "m.text",
                        "body": event.body,
                    },
                })
            })
            .collect::<Vec<_>>();
        let mut response = serde_json::json!({
            "start": "qa-live-tail-start",
            "chunk": chunk,
            "state": [],
        });
        if let Some(end) = &self.end {
            response["end"] = serde_json::Value::String(end.clone());
        }
        serde_json::to_vec(&response)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}
#[derive(Default)]
struct QaMessagesProxyControl {
    state: QaMessagesProxyState,
    canned_page: Option<QaCannedMessagesPage>,
}
impl QaTcpProxy {
    fn start(target_homeserver: &str) -> Result<Self, String> {
        let target = parse_http_homeserver_addr(target_homeserver)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("send_queue proxy bind failed: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("send_queue proxy nonblocking setup failed: {e}"))?;
        let listen_addr = listener
            .local_addr()
            .map_err(|e| format!("send_queue proxy local_addr failed: {e}"))?;
        let enabled = Arc::new(AtomicBool::new(true));
        let room_send_forwarded = Arc::new(AtomicUsize::new(0));
        let room_send_responses_completed = Arc::new(AtomicUsize::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let active_streams = Arc::new(Mutex::new(Vec::new()));
        let messages_control = Arc::new(Mutex::new(QaMessagesProxyControl::default()));

        let thread_enabled = enabled.clone();
        let thread_room_send_forwarded = room_send_forwarded.clone();
        let thread_room_send_responses_completed = room_send_responses_completed.clone();
        let thread_running = running.clone();
        let thread_streams = active_streams.clone();
        let thread_messages_control = messages_control.clone();
        let accept_thread = thread::spawn(move || {
            while thread_running.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((client, _)) => {
                        if !thread_enabled.load(Ordering::SeqCst) {
                            let _ = client.shutdown(Shutdown::Both);
                            continue;
                        }
                        spawn_proxy_pair(
                            client,
                            target,
                            thread_streams.clone(),
                            thread_messages_control.clone(),
                            thread_room_send_forwarded.clone(),
                            thread_room_send_responses_completed.clone(),
                        );
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => {
                        if thread_running.load(Ordering::SeqCst) {
                            thread::sleep(Duration::from_millis(20));
                        }
                    }
                }
            }
        });

        Ok(Self {
            listen_addr,
            enabled,
            room_send_forwarded,
            room_send_responses_completed,
            running,
            active_streams,
            messages_control,
            accept_thread: Some(accept_thread),
        })
    }

    fn homeserver_url(&self) -> String {
        format!("http://{}", self.listen_addr)
    }

    fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
        shutdown_active_streams(&self.active_streams);
    }

    fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    fn room_send_forwarded_count(&self) -> usize {
        self.room_send_forwarded.load(Ordering::SeqCst)
    }

    fn room_send_responses_completed_count(&self) -> usize {
        self.room_send_responses_completed.load(Ordering::SeqCst)
    }

    fn arm_first_live_tail_messages_page(
        &self,
        newest_known_event_id: String,
        newest_known_body: String,
        missing_event_id: String,
        missing_body: String,
        older_anchor_event_id: String,
        sender: String,
        older_anchor_body: String,
    ) -> Result<(), String> {
        self.arm_messages_page(
            QaMessagesProxyExpectation::TokenlessLiveTail,
            QaCannedMessagesPage::anchored_silent_gap(
                newest_known_event_id,
                newest_known_body,
                missing_event_id,
                missing_body,
                older_anchor_event_id,
                sender,
                older_anchor_body,
            ),
            None,
        )
    }

    fn arm_detached_live_tail_messages_page(
        &self,
        events: Vec<QaCannedTimelineEvent>,
        end_token: String,
    ) -> Result<(), String> {
        let tracked_end_token = end_token.clone();
        self.arm_messages_page(
            QaMessagesProxyExpectation::TokenlessLiveTail,
            QaCannedMessagesPage {
                events,
                end: Some(end_token),
            },
            Some(tracked_end_token),
        )
    }

    fn arm_historical_continuation_messages_page(
        &self,
        end_token: String,
        events: Vec<QaCannedTimelineEvent>,
    ) -> Result<(), String> {
        let tracked_end_token = end_token.clone();
        self.arm_messages_page(
            QaMessagesProxyExpectation::BackwardFrom { token: end_token },
            QaCannedMessagesPage { events, end: None },
            Some(tracked_end_token),
        )
    }

    fn arm_messages_page(
        &self,
        expectation: QaMessagesProxyExpectation,
        page: QaCannedMessagesPage,
        tracked_end_token: Option<String>,
    ) -> Result<(), String> {
        let mut control = self
            .messages_control
            .lock()
            .map_err(|_| "timeline messages proxy state lock was poisoned".to_owned())?;
        control.state.arm_page(expectation, tracked_end_token);
        control.canned_page = Some(page);
        Ok(())
    }

    fn live_tail_messages_observation(&self) -> Result<QaMessagesProxyObservation, String> {
        self.messages_control
            .lock()
            .map(|control| control.state.observation)
            .map_err(|_| "timeline messages proxy state lock was poisoned".to_owned())
    }
}
fn parse_http_homeserver_addr(homeserver: &str) -> Result<SocketAddr, String> {
    let without_scheme = homeserver.strip_prefix("http://").ok_or_else(|| {
        format!("send_queue proxy requires a local http:// homeserver, got {homeserver}")
    })?;
    let authority = without_scheme
        .split_once('/')
        .map(|(authority, _)| authority)
        .unwrap_or(without_scheme);
    authority
        .to_socket_addrs()
        .map_err(|e| format!("send_queue proxy could not resolve {authority}: {e}"))?
        .next()
        .ok_or_else(|| format!("send_queue proxy could not resolve {authority}"))
}
fn spawn_proxy_pair(
    mut client: TcpStream,
    target: SocketAddr,
    active_streams: Arc<Mutex<Vec<TcpStream>>>,
    messages_control: Arc<Mutex<QaMessagesProxyControl>>,
    room_send_forwarded: Arc<AtomicUsize>,
    room_send_responses_completed: Arc<AtomicUsize>,
) {
    thread::spawn(move || {
        let _ = proxy_single_http_request(
            &mut client,
            target,
            active_streams,
            messages_control,
            room_send_forwarded,
            room_send_responses_completed,
        );
        let _ = client.shutdown(Shutdown::Both);
    });
}
fn proxy_single_http_request(
    client: &mut TcpStream,
    target: SocketAddr,
    active_streams: Arc<Mutex<Vec<TcpStream>>>,
    messages_control: Arc<Mutex<QaMessagesProxyControl>>,
    room_send_forwarded: Arc<AtomicUsize>,
    room_send_responses_completed: Arc<AtomicUsize>,
) -> io::Result<()> {
    let mut request_head = Vec::new();
    {
        let reader_stream = client.try_clone()?;
        let mut reader = io::BufReader::new(reader_stream);
        loop {
            let mut line = Vec::new();
            let bytes = io::BufRead::read_until(&mut reader, b'\n', &mut line)?;
            if bytes == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "client closed before HTTP headers",
                ));
            }
            request_head.extend_from_slice(&line);
            if request_head.ends_with(b"\r\n\r\n") || request_head.ends_with(b"\n\n") {
                break;
            }
            if request_head.len() > 64 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP headers exceeded QA proxy limit",
                ));
            }
        }

        let content_length = http_content_length(&request_head)?;
        if content_length > 0 {
            let mut body = vec![0u8; content_length];
            io::Read::read_exact(&mut reader, &mut body)?;
            request_head.extend_from_slice(&body);
        }
    }

    let request_kind = qa_proxy_request_kind(&request_head)?;
    let action = qa_messages_proxy_action(&messages_control, request_kind, &request_head)?
        .unwrap_or(QaProxyRequestAction::Forward);
    let count_forwarded_room_send =
        request_kind == QaProxyRequestKind::RoomSend && action == QaProxyRequestAction::Forward;
    match action {
        QaProxyRequestAction::Forward => {}
        QaProxyRequestAction::FailClosed => {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "QA proxy closed a selected sync request",
            ));
        }
        QaProxyRequestAction::ServeCannedMessages(body) => {
            write_qa_json_response(client, &body)?;
            return Ok(());
        }
    }

    let mut server = TcpStream::connect_timeout(&target, Duration::from_secs(2))?;
    if let Ok(mut streams) = active_streams.lock() {
        if let Ok(stream) = client.try_clone() {
            streams.push(stream);
        }
        if let Ok(stream) = server.try_clone() {
            streams.push(stream);
        }
    }

    let request = rewrite_http_request_connection_close(&request_head)?;
    if count_forwarded_room_send {
        room_send_forwarded.fetch_add(1, Ordering::SeqCst);
    }
    io::Write::write_all(&mut server, &request)?;
    io::copy(&mut server, client)?;
    if count_forwarded_room_send {
        room_send_responses_completed.fetch_add(1, Ordering::SeqCst);
    }
    Ok(())
}
fn qa_proxy_request_kind(request: &[u8]) -> io::Result<QaProxyRequestKind> {
    let header_end = find_http_header_end(request)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP headers"))?;
    let head = String::from_utf8_lossy(&request[..header_end]);
    let line = head
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP request line"))?;
    let mut fields = line.split_ascii_whitespace();
    let method = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP method"))?;
    let target = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP target"))?;
    let version = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP version"))?;
    if fields.next().is_some() || !version.starts_with("HTTP/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP request line",
        ));
    }
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    Ok(match (method, path) {
        ("PUT", path)
            if path.starts_with("/_matrix/client/")
                && path.contains("/rooms/")
                && path.contains("/send/") =>
        {
            QaProxyRequestKind::RoomSend
        }
        (_, path)
            if path.starts_with("/_matrix/client/")
                && path.contains("/rooms/")
                && path.ends_with("/messages") =>
        {
            QaProxyRequestKind::RoomMessages
        }
        _ => QaProxyRequestKind::Other,
    })
}
fn qa_messages_proxy_action(
    control: &Arc<Mutex<QaMessagesProxyControl>>,
    request_kind: QaProxyRequestKind,
    request: &[u8],
) -> io::Result<Option<QaProxyRequestAction>> {
    if request_kind != QaProxyRequestKind::RoomMessages {
        return Ok(None);
    }
    let metadata = qa_room_messages_request_metadata(request)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "room messages proxy received a non-room-messages request",
        )
    })?;
    let mut control = control
        .lock()
        .map_err(|_| io::Error::other("QA messages proxy state lock was poisoned"))?;
    match control.state.observe_room_messages_request(&metadata) {
        QaMessagesProxyDecision::Forward => Ok(None),
        QaMessagesProxyDecision::FailClosed => Ok(Some(QaProxyRequestAction::FailClosed)),
        QaMessagesProxyDecision::ServeCannedPage => {
            let page = control.canned_page.take().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "QA messages proxy armed without a canned messages page",
                )
            })?;
            Ok(Some(QaProxyRequestAction::ServeCannedMessages(
                page.response_body()?,
            )))
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct QaRoomMessagesRequestMetadata {
    query_is_exact_tokenless_limit: bool,
    has_from: bool,
    direction_is_backward: bool,
    from_token: Option<String>,
}
fn qa_room_messages_request_metadata(
    request: &[u8],
) -> io::Result<Option<QaRoomMessagesRequestMetadata>> {
    let header_end = find_http_header_end(request)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP headers"))?;
    let head = String::from_utf8_lossy(&request[..header_end]);
    let line = head
        .lines()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP request line"))?;
    let mut fields = line.split_ascii_whitespace();
    let _method = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP method"))?;
    let target = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP target"))?;
    let version = fields
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing HTTP version"))?;
    if fields.next().is_some() || !version.starts_with("HTTP/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP request line",
        ));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if !path.starts_with("/_matrix/client/")
        || !path.contains("/rooms/")
        || !path.ends_with("/messages")
    {
        return Ok(None);
    }
    let mut direction_is_backward = false;
    let mut from_token = None;
    for field in query.split('&') {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        match name {
            "dir" => direction_is_backward = value == "b",
            "from" => from_token = Some(value.to_owned()),
            _ => {}
        }
    }
    Ok(Some(QaRoomMessagesRequestMetadata {
        query_is_exact_tokenless_limit: query == "dir=b&limit=128",
        has_from: from_token.is_some(),
        direction_is_backward,
        from_token,
    }))
}
fn write_qa_json_response(client: &mut TcpStream, body: &[u8]) -> io::Result<()> {
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    io::Write::write_all(client, headers.as_bytes())?;
    io::Write::write_all(client, body)
}
fn http_content_length(request_head: &[u8]) -> io::Result<usize> {
    let head = String::from_utf8_lossy(request_head);
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            return value.trim().parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP content-length")
            });
        }
    }
    Ok(0)
}
fn rewrite_http_request_connection_close(request: &[u8]) -> io::Result<Vec<u8>> {
    let Some(header_end) = find_http_header_end(request) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing HTTP header terminator",
        ));
    };
    let (head, body) = request.split_at(header_end);
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing HTTP request line",
        ));
    };

    let mut rewritten = Vec::with_capacity(request.len() + 32);
    rewritten.extend_from_slice(request_line.as_bytes());
    rewritten.extend_from_slice(b"\r\n");
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let lower = line
            .split_once(':')
            .map(|(name, _)| name.trim().to_ascii_lowercase());
        if matches!(lower.as_deref(), Some("connection" | "proxy-connection")) {
            continue;
        }
        rewritten.extend_from_slice(line.as_bytes());
        rewritten.extend_from_slice(b"\r\n");
    }
    rewritten.extend_from_slice(b"Connection: close\r\n\r\n");
    rewritten.extend_from_slice(body);
    Ok(rewritten)
}
fn find_http_header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .or_else(|| {
            request
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| position + 2)
        })
}
fn shutdown_active_streams(active_streams: &Arc<Mutex<Vec<TcpStream>>>) {
    if let Ok(mut streams) = active_streams.lock() {
        for stream in streams.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

#[test]
fn trust_admission_timeout_summary_is_allowlisted_and_private_safe() {
    use koushi_diagnostics::{
        DiagnosticEvent, DiagnosticField, DiagnosticLevel, DiagnosticRecord, DiagnosticSnapshot,
    };

    let record = |event| DiagnosticRecord {
        timestamp_ms: 0,
        event,
    };
    let snapshot = DiagnosticSnapshot {
        records: vec![
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Info,
                    "core.verification_admission",
                    "trust_recheck_requested",
                )
                .field(DiagnosticField::token(
                    "ignored_private_field",
                    "@private:example.invalid",
                )),
            ),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Info,
                "other.source",
                "trust_recheck_started",
            )),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Info,
                "core.verification_admission",
                "unallowlisted-private-stage",
            )),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Info,
                "core.verification_admission",
                "trust_recheck_started",
            )),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Info,
                "core.verification_admission",
                "trust_recheck_finished_verified",
            )),
        ],
        dropped_records: 0,
    };

    let summary = trust_admission_diagnostic_summary(&snapshot);
    assert_eq!(
        summary,
        "trust_recheck_requested>trust_recheck_started>trust_recheck_finished_verified"
    );
    assert!(!summary.contains("private"));
}
#[test]
fn invite_timeout_diagnostic_summary_is_allowlisted_and_private_safe() {
    use koushi_diagnostics::{
        DiagnosticEvent, DiagnosticField, DiagnosticLevel, DiagnosticRecord, DiagnosticSnapshot,
    };

    let record = |event| DiagnosticRecord {
        timestamp_ms: 0,
        event,
    };
    let snapshot = DiagnosticSnapshot {
        records: vec![
            record(DiagnosticEvent::new(
                DiagnosticLevel::Debug,
                "core.room",
                "live_observer_started",
            )),
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Debug,
                    "core.room",
                    "live_observer_wake_milestone",
                )
                .field(DiagnosticField::token("source", "rls_diff"))
                .field(DiagnosticField::count("wake_count", 4))
                .field(DiagnosticField::token(
                    "ignored_private_field",
                    "!private-room:example.invalid",
                )),
            ),
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Debug,
                    "core.room",
                    "live_observer_wake_milestone",
                )
                .field(DiagnosticField::token("source", "base_room_updates"))
                .field(DiagnosticField::count("wake_count", 8))
                .field(DiagnosticField::boolean("invite_update_observed", true))
                .field(DiagnosticField::boolean("invite_membership_changed", false))
                .field(DiagnosticField::boolean("projection_required", true)),
            ),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Debug,
                "core.room",
                "live_observer_invite_projection",
            )),
            record(
                DiagnosticEvent::new(
                    DiagnosticLevel::Debug,
                    "core.room",
                    "live_observer_invite_projection_completed",
                )
                .field(DiagnosticField::boolean("action_delivered", true)),
            ),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Warn,
                "core.room",
                "live_observer_base_lagged",
            )),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Warn,
                "core.room",
                "live_observer_auxiliary_closed",
            )),
            record(DiagnosticEvent::new(
                DiagnosticLevel::Error,
                "core.room",
                "live_observer_exit",
            )),
        ],
        dropped_records: 2,
    };

    let summary = invite_observer_diagnostic_summary(&snapshot);
    assert_eq!(
        summary,
        "observer_diag_started=1 observer_diag_rls_wake_max=4 \
             observer_diag_base_wake_max=8 observer_diag_base_invite_update_seen=true \
             observer_diag_base_membership_change_seen=false \
             observer_diag_base_projection_required_seen=true \
             observer_diag_invite_projection=1 observer_diag_invite_projection_delivered=1 \
             observer_diag_invite_projection_undelivered=0 observer_diag_last_projection_rooms=0 \
             observer_diag_last_projection_spaces=0 observer_diag_last_projection_invites=0 \
             observer_diag_last_refresh_entries=0 observer_diag_last_refresh_invites=0 \
             observer_diag_last_refresh_authoritative=false \
             observer_diag_last_refresh_room_present=false \
             observer_diag_lagged=1 \
             observer_diag_closed=1 observer_diag_exit=1 observer_diag_last_exit_reason=unknown \
             observer_diag_dropped=2"
    );
    assert!(!summary.contains("private-room"));
    assert!(!summary.contains("room_id"));
}
#[test]
fn send_queue_proxy_forces_connection_close_per_request() {
    let request = b"POST /_matrix/client/v3/login HTTP/1.1\r\nHost: example.test\r\nConnection: keep-alive\r\nProxy-Connection: keep-alive\r\nContent-Length: 2\r\n\r\n{}";
    let rewritten = rewrite_http_request_connection_close(request).unwrap();
    let rewritten = String::from_utf8(rewritten).unwrap();
    let (head, body) = rewritten.split_once("\r\n\r\n").unwrap();

    assert!(
            head.contains("\r\nConnection: close"),
            "send queue proxy must force one HTTP request per connection so response copying can read to EOF"
        );
    assert!(
        !head.to_ascii_lowercase().contains("proxy-connection"),
        "send queue proxy must drop proxy keep-alive headers before forwarding"
    );
    assert_eq!(body, "{}");
}
#[test]
fn live_tail_proxy_enforces_tokenless_refresh_and_exact_continuation_requests() {
    let metadata = qa_room_messages_request_metadata(
            b"GET /_matrix/client/v3/rooms/%21room%3Aexample.invalid/messages?dir=b&limit=128 HTTP/1.1\r\nHost: example.invalid\r\n\r\n",
        )
        .expect("valid request")
        .expect("room messages metadata");
    assert_eq!(
        metadata,
        QaRoomMessagesRequestMetadata {
            query_is_exact_tokenless_limit: true,
            has_from: false,
            direction_is_backward: true,
            from_token: None,
        }
    );

    let mut state = QaMessagesProxyState::default();
    state.arm_page(QaMessagesProxyExpectation::TokenlessLiveTail, None);
    assert_eq!(
        state.observe_room_messages_request(&metadata),
        QaMessagesProxyDecision::ServeCannedPage
    );

    let continuation = qa_room_messages_request_metadata(
            b"GET /_matrix/client/v3/rooms/%21room%3Aexample.invalid/messages?dir=b&from=continuation&limit=128 HTTP/1.1\r\nHost: example.invalid\r\n\r\n",
        )
        .expect("valid continuation request")
        .expect("room messages continuation metadata");
    state.arm_page(
        QaMessagesProxyExpectation::BackwardFrom {
            token: "continuation".to_owned(),
        },
        Some("continuation".to_owned()),
    );
    assert_eq!(
        state.observe_room_messages_request(&continuation),
        QaMessagesProxyDecision::ServeCannedPage
    );
    assert!(state.observation.expected_end_token_was_used);
    assert_eq!(state.observation.expected_end_token_request_count, 1);
}
