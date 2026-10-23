use super::*;
use chrono::Duration;
use runmarq_core::api::Preferences;

fn t(s: i64) -> DateTime<Utc> {
    from_ms(1_767_225_600_000 + s * 1000)
}

fn session(store: &Store) -> SessionRow {
    let s = SessionRow {
        id: "s1".into(),
        agent: AgentSource::ClaudeCode,
        source_session_id: "vendor-1".into(),
        repository_id: None,
        started_at: t(0),
        last_seen_at: t(0),
        status: "active".into(),
    };
    store.insert_session(&s).unwrap();
    s
}

fn run(id: &str, status: RunState, start: i64, end: Option<i64>) -> RunRow {
    RunRow {
        id: id.into(),
        session_id: "s1".into(),
        source_turn_id: None,
        agent: AgentSource::ClaudeCode,
        repository_id: None,
        task_category: TaskCategory::BugFix,
        prompt_features: None,
        title: Some("Fix login".into()),
        model: None,
        started_at: t(start),
        ended_at: end.map(t),
        last_event_at: t(end.unwrap_or(start)),
        wall_duration_ms: end.map(|e| ((e - start) * 1000) as u64),
        active_duration_ms: end.map(|e| ((e - start) * 1000) as u64),
        waiting_duration_ms: Some(0),
        status,
        exit_reason: None,
        current_activity: Some(ActivityCategory::RunningTests),
        current_tool: Some("Bash".into()),
        tool_calls: 3,
        tool_failures: 1,
        test_runs: 1,
        plan_total: None,
        plan_completed: None,
        initial_p10_ms: Some(1000),
        initial_p50_ms: Some(2000),
        initial_p90_ms: Some(3000),
        effort: Some("high".into()),
        permission_mode: Some("default".into()),
        signals: Some(RunSignals {
            turns: 4,
            output_tokens: 1200,
            ..Default::default()
        }),
    }
}

#[test]
fn migrations_apply_and_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), MIGRATIONS.last().unwrap().0);
    drop(s);
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), MIGRATIONS.last().unwrap().0);
    let mode: String = s
        .conn()
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn refuses_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 999).unwrap();
    }
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::SchemaTooNew { .. })
    ));
}

#[test]
fn run_roundtrip() {
    let s = Store::open_in_memory().unwrap();
    session(&s);
    let r = run("r1", RunState::Running, 0, None);
    s.upsert_run(&r).unwrap();
    assert_eq!(s.get_run("r1").unwrap().unwrap(), r);
    let mut done = r.clone();
    done.status = RunState::Completed;
    done.ended_at = Some(t(90));
    done.wall_duration_ms = Some(90_000);
    s.upsert_run(&done).unwrap();
    assert_eq!(s.get_run("r1").unwrap().unwrap(), done);
    assert!(s.open_runs().unwrap().is_empty());
}

#[test]
fn duplicate_events_are_ignored() {
    let s = Store::open_in_memory().unwrap();
    session(&s);
    s.upsert_run(&run("r1", RunState::Running, 0, None))
        .unwrap();
    let e = EventRow {
        id: "ev_1".into(),
        run_id: Some("r1".into()),
        session_id: Some("s1".into()),
        event_type: EventType::ToolStarted,
        source_timestamp: t(1),
        received_timestamp: t(1),
        activity: Some(ActivityCategory::RunningTests),
        tool: Some("Bash".into()),
        duration_ms: None,
        status: None,
        metadata: serde_json::json!({}),
    };
    assert!(s.insert_event(&e).unwrap());
    assert!(!s.insert_event(&e).unwrap());
    assert_eq!(s.events_for_run("r1").unwrap().len(), 1);
}

#[test]
fn history_filters_and_search_are_parameterized() {
    let s = Store::open_in_memory().unwrap();
    session(&s);
    s.upsert_run(&run("r1", RunState::Completed, 0, Some(60)))
        .unwrap();
    let mut other = run("r2", RunState::Failed, 100, Some(130));
    other.title = Some("100% done_ish".into());
    s.upsert_run(&other).unwrap();

    let (all, total) = s.list_runs(&HistoryQuery::default()).unwrap();
    assert_eq!(total, 2);
    assert_eq!(all[0].id, "r2", "newest first");

    let q = HistoryQuery {
        state: Some(RunState::Failed),
        ..Default::default()
    };
    assert_eq!(s.list_runs(&q).unwrap().1, 1);

    let q = HistoryQuery {
        search: Some("%".into()),
        ..Default::default()
    };
    let (found, _) = s.list_runs(&q).unwrap();
    assert_eq!(found.len(), 1, "% is matched literally");

    let q = HistoryQuery {
        search: Some("'; DROP TABLE runs; --".into()),
        ..Default::default()
    };
    assert_eq!(s.list_runs(&q).unwrap().1, 0);
    assert_eq!(s.count_runs().unwrap(), 2);

    let q = HistoryQuery {
        from: Some(t(50)),
        ..Default::default()
    };
    assert_eq!(s.list_runs(&q).unwrap().1, 1);
}

#[test]
fn forecasts_roundtrip() {
    let s = Store::open_in_memory().unwrap();
    session(&s);
    s.upsert_run(&run("r1", RunState::Running, 0, None))
        .unwrap();
    let f = EtaForecast {
        run_id: "r1".into(),
        generated_at: t(10),
        remaining_p10_seconds: Some(30),
        remaining_p50_seconds: Some(60),
        remaining_p90_seconds: Some(120),
        expected_finish_at: Some(t(70)),
        confidence: Confidence::Medium,
        status: ForecastStatus::Available,
        reason_codes: vec!["history_agent".into()],
        model_version: "m1".into(),
        basis: ForecastBasis::History,
        sample_size: 12.5,
        active_elapsed_seconds: 10,
        raw_p10_seconds: Some(20),
        raw_p50_seconds: Some(60),
        raw_p90_seconds: Some(100),
        calibrated: true,
        components: vec!["history".into(), "phase".into()],
    };
    s.insert_forecast(&f).unwrap();
    assert_eq!(s.forecasts_for_run("r1").unwrap(), vec![f]);
}

#[test]
fn preferences_default_and_persist() {
    let s = Store::open_in_memory().unwrap();
    assert_eq!(s.load_preferences().unwrap(), Preferences::default());
    let p = Preferences {
        store_task_titles: true,
        retention_days: Some(30),
        ..Default::default()
    };
    s.save_preferences(&p).unwrap();
    assert_eq!(s.load_preferences().unwrap(), p);
    s.conn()
        .execute(
            "INSERT INTO user_preferences (key, value, updated_at) VALUES ('bogus', 'not json', 0)",
            [],
        )
        .unwrap();
    assert_eq!(s.load_preferences().unwrap(), p, "junk keys are ignored");
}

#[test]
fn notifications_dedupe() {
    let s = Store::open_in_memory().unwrap();
    assert!(s.record_notification("r1", "run_finished", t(0)).unwrap());
    assert!(!s.record_notification("r1", "run_finished", t(1)).unwrap());
    assert!(s.record_notification("r1", "run_failed", t(1)).unwrap());
}

#[test]
fn delete_history_cascades() {
    let mut s = Store::open_in_memory().unwrap();
    session(&s);
    s.upsert_run(&run("old", RunState::Completed, 0, Some(10)))
        .unwrap();
    s.upsert_run(&run("live", RunState::Running, 5, None))
        .unwrap();
    s.insert_forecast(&EtaForecast {
        run_id: "old".into(),
        generated_at: t(1),
        remaining_p10_seconds: None,
        remaining_p50_seconds: None,
        remaining_p90_seconds: None,
        expected_finish_at: None,
        confidence: Confidence::Low,
        status: ForecastStatus::InsufficientData,
        reason_codes: vec![],
        model_version: "m".into(),
        basis: ForecastBasis::ColdStart,
        sample_size: 0.0,
        active_elapsed_seconds: 0,
        raw_p10_seconds: None,
        raw_p50_seconds: None,
        raw_p90_seconds: None,
        calibrated: false,
        components: vec![],
    })
    .unwrap();
    let n = s.delete_history(Some(t(0) + Duration::days(1))).unwrap();
    assert_eq!(n, 1, "open runs are never deleted");
    assert!(s.get_run("old").unwrap().is_none());
    assert!(s.forecasts_for_run("old").unwrap().is_empty());
    assert!(s.get_run("live").unwrap().is_some());
}

#[test]
fn version_one_databases_upgrade_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(MIGRATIONS[0].1).unwrap();
        c.pragma_update(None, "user_version", 1).unwrap();
        c.execute_batch(
            "INSERT INTO sessions (id, agent_id, source_session_id, started_at, last_seen_at) VALUES ('s1','claude_code','v',0,0);
             INSERT INTO runs (id, session_id, agent_id, started_at, last_event_at, status, created_at, updated_at)
             VALUES ('old','s1','claude_code',0,0,'completed',0,0);",
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), MIGRATIONS.last().unwrap().0);
    let r = s.get_run("old").unwrap().unwrap();
    assert_eq!(r.effort, None);
    assert_eq!(r.signals, None);
}

#[test]
fn bulk_events_and_signals() {
    let s = Store::open_in_memory().unwrap();
    session(&s);
    s.upsert_run(&run("a", RunState::Completed, 0, Some(10)))
        .unwrap();
    s.upsert_run(&run("b", RunState::Completed, 20, Some(30)))
        .unwrap();
    for (i, r) in ["a", "a", "b"].iter().enumerate() {
        s.insert_event(&EventRow {
            id: format!("e{i}"),
            run_id: Some(r.to_string()),
            session_id: Some("s1".into()),
            event_type: EventType::ToolStarted,
            source_timestamp: t(i as i64),
            received_timestamp: t(i as i64),
            activity: None,
            tool: None,
            duration_ms: None,
            status: None,
            metadata: serde_json::json!({"commandKey": "cmd_x"}),
        })
        .unwrap();
    }
    let ev = s
        .events_for_runs(&["a".into(), "b".into(), "missing".into()])
        .unwrap();
    assert_eq!(ev["a"].len(), 2);
    assert_eq!(ev["b"].len(), 1);
    assert_eq!(ev["a"][0].metadata["commandKey"], "cmd_x");

    let sig = RunSignals {
        turns: 2,
        output_tokens: 500,
        ..Default::default()
    };
    s.insert_run_signals("a", 5_000, &sig).unwrap();
    s.insert_run_signals(
        "a",
        9_000,
        &RunSignals {
            turns: 3,
            ..sig.clone()
        },
    )
    .unwrap();
    let got = s.signals_for_runs(&["a".into()]).unwrap();
    assert_eq!(got["a"].len(), 2);
    assert_eq!(got["a"][1].signals.turns, 3);
}

#[test]
fn command_stats_roundtrip_and_sketch_clearing() {
    let s = Store::open_in_memory().unwrap();
    let c = CommandStatRow {
        repository_id: "repo_1".into(),
        command_key: "cmd_abc".into(),
        samples: 3,
        log_mean: 3.7,
        log_m2: 0.2,
        failures: 1,
        last_seen_at: t(5),
    };
    s.upsert_command_stat(&c).unwrap();
    s.upsert_command_stat(&CommandStatRow {
        samples: 4,
        ..c.clone()
    })
    .unwrap();
    assert_eq!(
        s.command_stats().unwrap(),
        vec![CommandStatRow { samples: 4, ..c }]
    );

    session(&s);
    let mut r = run("sk", RunState::Completed, 0, Some(5));
    r.prompt_features = Some(PromptFeatures {
        category: TaskCategory::BugFix,
        complexity: ComplexityBucket::Small,
        complexity_score: 0.3,
        word_count: 5,
        list_items: 0,
        file_mentions: 0,
        mentions_tests: false,
        fingerprint: None,
        sketch: Some(vec![1, -2, 3]),
    });
    s.upsert_run(&r).unwrap();
    assert_eq!(s.clear_prompt_sketches().unwrap(), 1);
    let back = s.get_run("sk").unwrap().unwrap();
    assert_eq!(back.prompt_features.unwrap().sketch, None);
}
