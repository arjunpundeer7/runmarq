//! HTTP-level tests: authentication, browser-origin rejection, DNS-rebinding
//! protection, limits, single instance, spool replay, and the event stream.

use chrono::Utc;
use runmarq_collector::server::{start, CollectorOptions};
use runmarq_collector::{spool, Client};
use runmarq_core::api::{Health, HistoryPage, IngestRequest, StreamMessage};
use runmarq_core::event::*;
use runmarq_core::paths::Paths;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn raw(port: u16, request: String) -> u16 {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    head.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

fn ev(session: &str, t: EventType, id: &str) -> EventEnvelope {
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: id.into(),
        source: AgentSource::ClaudeCode,
        source_session_id: session.into(),
        source_turn_id: None,
        timestamp: Utc::now(),
        timestamp_origin: TimestampOrigin::HookInvocation,
        event_type: t,
        payload: EventPayload::default(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn security_and_api() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::at(dir.path());

    // An event spooled while no collector was running is replayed at startup.
    assert_eq!(
        spool::write(
            &paths,
            &[ev("spooled", EventType::PromptSubmitted, "ev_spooled")]
        ),
        spool::SpoolResult::Written
    );

    let running = start(CollectorOptions {
        paths: paths.clone(),
    })
    .await
    .unwrap();
    let port = running.port;
    let token = running.state.token.clone();
    assert!(
        start(CollectorOptions {
            paths: paths.clone()
        })
        .await
        .is_err(),
        "single instance"
    );

    let p = paths.clone();
    let tok = token.clone();
    tokio::task::spawn_blocking(move || {
        let host = format!("127.0.0.1:{port}");
        let ok = format!("GET /v1/health HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nConnection: close\r\n\r\n");
        assert_eq!(raw(port, ok), 200);
        let no_token = format!("GET /v1/health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        assert_eq!(raw(port, no_token), 401);
        let wrong = format!("GET /v1/health HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n", "0".repeat(64));
        assert_eq!(raw(port, wrong), 401);
        let browser = format!(
            "GET /v1/health HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(raw(port, browser), 403, "browser origins are refused even with a token");
        let rebinding = format!("GET /v1/health HTTP/1.1\r\nHost: evil.example:{port}\r\nAuthorization: Bearer {tok}\r\nConnection: close\r\n\r\n");
        assert_eq!(raw(port, rebinding), 403, "DNS rebinding host refused");
        let form = format!(
            "POST /v1/events HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        assert_eq!(raw(port, form), 415, "simple (non-preflighted) content types are refused");
        let huge = format!(
            "POST /v1/events HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {tok}\r\nContent-Type: application/json\r\nContent-Length: 99999999\r\nConnection: close\r\n\r\n"
        );
        assert_eq!(raw(port, huge), 413);

        let client = Client::discover(&p, Duration::from_secs(5)).unwrap();
        let h: Health = client.health().unwrap();
        assert!(h.ok);

        // Stream: the first message is a snapshot that includes the replayed run.
        let c2 = client.clone();
        let streamed = std::thread::spawn(move || {
            let mut got = Vec::new();
            let _ = c2.stream(|m| {
                got.push(m);
                got.len() < 2
            });
            got
        });
        std::thread::sleep(Duration::from_millis(300));
        let resp = client
            .ingest(&IngestRequest {
                events: vec![ev("live", EventType::PromptSubmitted, "ev_live"), ev("live", EventType::PromptSubmitted, "ev_live")],
                hook_latency_ms: Some(12.0),
                replayed: false,
            })
            .unwrap();
        assert_eq!((resp.accepted, resp.duplicates), (1, 1));
        let got = streamed.join().unwrap();
        match &got[0] {
            StreamMessage::Snapshot { runs } => assert!(runs.iter().any(|r| r.state == runmarq_core::run::RunState::Running)),
            other => panic!("expected snapshot, got {other:?}"),
        }
        assert!(matches!(got[1], StreamMessage::RunUpdated { .. }));

        let page: HistoryPage = client.get("/v1/runs?limit=10").unwrap();
        assert_eq!(page.total, 2, "spooled run plus live run");
        let csv = client.get_text("/v1/export.csv").unwrap();
        assert!(csv.starts_with("run_id,agent"));
        assert_eq!(csv.lines().count(), 3);
        let diag: runmarq_core::api::Diagnostics = client.get("/v1/diagnostics").unwrap();
        assert_eq!(diag.spooled_events_replayed, 1);
        assert_eq!(diag.hook_latency.samples, 1);
        assert_eq!(spool::pending(&p).0, 0, "spool drained");
        client.shutdown().unwrap();
    })
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_secs(5), running.join)
        .await
        .unwrap()
        .unwrap();
}
