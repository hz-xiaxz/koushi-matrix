use matrix_sdk::ruma::{event_id, room_id, user_id};
use matrix_sdk::test_utils::mocks::{MatrixMockServer, RoomContextResponseTemplate};
use matrix_sdk_test::event_factory::EventFactory;
use serde_json::json;
use wiremock::matchers::{method, path_regex, query_param};
use wiremock::{Mock, ResponseTemplate};

use crate::account_work::AccountWorkScheduler;

use super::driver::HistoryPageSource;
use super::element::ExportSourceEvent;
use super::sdk_source::MatrixRoomHistorySource;

const TIMESTAMP_TO_EVENT: &str = r"^/_matrix/client/v1/rooms/.*/timestamp_to_event$";

async fn source(server: &MatrixMockServer) -> MatrixRoomHistorySource {
    let client = server.client_builder().build().await;
    let room = server
        .sync_joined_room(&client, room_id!("!history:example.invalid"))
        .await;
    MatrixRoomHistorySource::new(room, AccountWorkScheduler::default())
}

async fn mount_timestamp_to_event(server: &MatrixMockServer, response: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path_regex(TIMESTAMP_TO_EVENT))
        .and(query_param("ts", "1000"))
        .and(query_param("dir", "f"))
        .respond_with(response)
        .expect(1)
        .mount(server.server())
        .await;
}

fn event_ids(events: &[ExportSourceEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            ExportSourceEvent::Plain(json) | ExportSourceEvent::Decrypted(json) => {
                json["event_id"].as_str().unwrap().to_owned()
            }
            ExportSourceEvent::Undecryptable { wire, .. } => {
                wire["event_id"].as_str().unwrap().to_owned()
            }
        })
        .collect()
}

#[tokio::test]
async fn seek_emits_the_found_event_in_order_and_continues_after_it() {
    let server = MatrixMockServer::new().await;
    let mut source = source(&server).await;
    mount_timestamp_to_event(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({
            "event_id": "$found:example.invalid",
            "origin_server_ts": 1000
        })),
    )
    .await;
    let factory = EventFactory::new()
        .room(room_id!("!history:example.invalid"))
        .sender(user_id!("@member-1:example.invalid"));
    server
        .mock_room_event_context()
        .ok(RoomContextResponseTemplate::new(
            factory
                .text_msg("Synthetic found")
                .event_id(event_id!("$found:example.invalid"))
                .server_ts(1000)
                .into_event(),
        )
        .events_before(vec![
            factory
                .text_msg("Synthetic before")
                .event_id(event_id!("$before:example.invalid"))
                .server_ts(999)
                .into_event(),
        ])
        .events_after(vec![
            factory
                .text_msg("Synthetic after")
                .event_id(event_id!("$after:example.invalid"))
                .server_ts(1001)
                .into_event(),
        ])
        .start("before-token")
        .end("after-token"))
        .expect(1)
        .mount()
        .await;

    let page = source.seek(1000).await.unwrap().expect("a seek page");
    assert_eq!(
        event_ids(&page.events),
        vec![
            "$before:example.invalid",
            "$found:example.invalid",
            "$after:example.invalid"
        ]
    );
    assert_eq!(page.end.as_deref(), Some("after-token"));
}

#[tokio::test]
async fn an_unsupported_or_empty_seek_falls_back() {
    for (status, errcode) in [
        (404, "M_UNRECOGNIZED"),
        (404, "M_NOT_FOUND"),
        (400, "M_UNRECOGNIZED"),
    ] {
        let server = MatrixMockServer::new().await;
        let mut source = source(&server).await;
        mount_timestamp_to_event(
            &server,
            ResponseTemplate::new(status)
                .set_body_json(json!({ "errcode": errcode, "error": "Synthetic" })),
        )
        .await;
        assert!(
            source.seek(1000).await.unwrap().is_none(),
            "{status} {errcode} must fall back"
        );
    }
}

#[tokio::test]
async fn a_context_without_a_forward_token_falls_back() {
    let server = MatrixMockServer::new().await;
    let mut source = source(&server).await;
    mount_timestamp_to_event(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({
            "event_id": "$found:example.invalid",
            "origin_server_ts": 1000
        })),
    )
    .await;
    server
        .mock_room_event_context()
        .ok(RoomContextResponseTemplate::new(
            EventFactory::new()
                .room(room_id!("!history:example.invalid"))
                .sender(user_id!("@member-1:example.invalid"))
                .text_msg("Synthetic found")
                .event_id(event_id!("$found:example.invalid"))
                .server_ts(1000)
                .into_event(),
        ))
        .expect(1)
        .mount()
        .await;
    assert!(source.seek(1000).await.unwrap().is_none());
}
