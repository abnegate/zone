//! A recurring reminder fires at the person's own wall-clock time.
//!
//! Every clause of a rule, and the time of its first firing, is read on the
//! clock of the reminder's IANA zone, and only the firing that answers is
//! turned into UTC. Two wall-clock times have no single instant, and each is
//! settled the same way every time:
//!
//! - A time the clocks skip, in the hour a spring-forward removes, moves
//!   forward to the first time they do show, the instant they change.
//! - A time the clocks show twice, in the hour a fall-back repeats, is the
//!   first of the two, and the second is not fired as well.
//!
//! An hourly rule counts elapsed hours, which no clock change moves.
use crate::common::context::Harness;
use crate::common::{TestClient, test_email, test_password};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{DateTime, Duration, FixedOffset, NaiveTime, Timelike, Utc};
use serde_json::{Value, json};
use uuid::Uuid;
use zone_core::context;
use zone_server::db::reminders;
use zone_server::db::reminders::Delivered;
use zone_server::db::reminders::Reminder;
use zone_server::db::reminders::Sweep;
use zone_server::db::{chats, users};
use zone_server::services::chat::session::{self, Mode};
use zone_server::services::route::Route;
use zone_server::services::schedule;
use zone_server::services::time_zone::TimeZone;

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("the fixture is a timestamp")
        .with_timezone(&Utc)
}

fn zone(name: &str) -> TimeZone {
    TimeZone::parse(name).expect("the fixture is an IANA zone")
}

/// Each firing after `anchor`, in order, as `rule` reads in `zone`.
fn firings(rule: &str, anchor: &str, zone_name: &str, count: u32) -> Vec<DateTime<Utc>> {
    let rule = schedule::parse(rule).expect("the rule is accepted");
    let anchor = at(anchor);
    let mut fired = Vec::new();
    let mut after = anchor;
    for index in 1..=count {
        let next = rule
            .next_after(anchor, after, index, zone(zone_name))
            .expect("the rule keeps firing");
        fired.push(next);
        after = next;
    }
    fired
}

#[test]
fn a_daily_nine_written_at_plus_one_repeats_at_nine_there_and_not_an_hour_later() {
    assert_eq!(
        firings(
            "FREQ=DAILY;BYHOUR=9",
            "2026-11-10T09:00:00+01:00",
            "Europe/Berlin",
            2
        ),
        [
            at("2026-11-11T09:00:00+01:00"),
            at("2026-11-12T09:00:00+01:00")
        ],
        "read in UTC, BYHOUR=9 fires again at 10:00 the same morning and every day after"
    );
}

#[test]
fn a_monday_eight_at_plus_twelve_repeats_on_monday_morning_there() {
    for rule in ["FREQ=WEEKLY;BYDAY=MO;BYHOUR=8", "FREQ=WEEKLY;BYDAY=MO"] {
        assert_eq!(
            firings(rule, "2027-07-05T08:00:00+12:00", "Pacific/Auckland", 2),
            [
                at("2027-07-12T08:00:00+12:00"),
                at("2027-07-19T08:00:00+12:00")
            ],
            "{rule}: read in UTC this fires on Monday evening or Tuesday morning instead"
        );
    }
}

#[test]
fn a_time_with_no_hour_keeps_its_wall_clock_across_daylight_saving() {
    assert_eq!(
        firings(
            "FREQ=DAILY",
            "2027-03-27T09:00:00+00:00",
            "Europe/London",
            2
        ),
        [
            at("2027-03-28T09:00:00+01:00"),
            at("2027-03-29T09:00:00+01:00")
        ],
        "London springs forward on 28 March, and nine stays nine"
    );
    assert_eq!(
        firings(
            "FREQ=DAILY",
            "2027-10-30T09:00:00+01:00",
            "Europe/London",
            2
        ),
        [
            at("2027-10-31T09:00:00+00:00"),
            at("2027-11-01T09:00:00+00:00")
        ],
        "London falls back on 31 October"
    );
    assert_eq!(
        firings(
            "FREQ=WEEKLY",
            "2027-09-20T09:00:00+12:00",
            "Pacific/Auckland",
            1
        ),
        [at("2027-09-27T09:00:00+13:00")],
        "Auckland springs forward on 26 September"
    );
}

#[test]
fn a_time_london_skips_fires_when_the_clocks_change_and_a_repeated_one_fires_once() {
    assert_eq!(
        firings(
            "FREQ=DAILY;BYHOUR=1;BYMINUTE=30",
            "2027-03-27T01:30:00+00:00",
            "Europe/London",
            2
        ),
        [
            at("2027-03-28T02:00:00+01:00"),
            at("2027-03-29T01:30:00+01:00")
        ],
        "01:30 does not exist on 28 March, so it fires at 02:00 BST, the instant the clocks change"
    );
    assert_eq!(
        firings(
            "FREQ=DAILY;BYHOUR=1;BYMINUTE=30",
            "2027-10-30T01:30:00+01:00",
            "Europe/London",
            2
        ),
        [
            at("2027-10-31T01:30:00+01:00"),
            at("2027-11-01T01:30:00+00:00")
        ],
        "01:30 happens twice on 31 October, and only the first, in BST, fires"
    );
}

#[test]
fn a_time_auckland_skips_fires_when_the_clocks_change_and_a_repeated_one_fires_once() {
    assert_eq!(
        firings(
            "FREQ=DAILY;BYHOUR=2;BYMINUTE=30",
            "2027-09-25T02:30:00+12:00",
            "Pacific/Auckland",
            2
        ),
        [
            at("2027-09-26T03:00:00+13:00"),
            at("2027-09-27T02:30:00+13:00")
        ],
        "02:30 does not exist on 26 September, so it fires at 03:00 NZDT"
    );
    assert_eq!(
        firings(
            "FREQ=DAILY;BYHOUR=2;BYMINUTE=30",
            "2027-04-03T02:30:00+13:00",
            "Pacific/Auckland",
            2
        ),
        [
            at("2027-04-04T02:30:00+13:00"),
            at("2027-04-05T02:30:00+12:00")
        ],
        "02:30 happens twice on 4 April, and only the first, in NZDT, fires"
    );
}

#[test]
fn an_hourly_rule_counts_elapsed_hours_through_a_repeated_hour() {
    assert_eq!(
        firings(
            "FREQ=HOURLY",
            "2027-10-31T00:00:00+01:00",
            "Europe/London",
            3
        ),
        [
            at("2027-10-31T01:00:00+01:00"),
            at("2027-10-31T01:00:00+00:00"),
            at("2027-10-31T02:00:00+00:00"),
        ]
    );
}

#[test]
fn a_monthly_first_at_plus_twelve_stays_on_the_first_there() {
    assert_eq!(
        firings(
            "FREQ=MONTHLY;BYMONTHDAY=1;BYHOUR=8",
            "2027-07-01T08:00:00+12:00",
            "Pacific/Auckland",
            2
        ),
        [
            at("2027-08-01T08:00:00+12:00"),
            at("2027-09-01T08:00:00+12:00")
        ],
        "read in UTC, the 1st at 08:00 +12:00 is the last day of the month before"
    );
}

#[test]
fn until_is_an_instant_however_the_zone_reads_it() {
    let rule = schedule::parse("FREQ=DAILY;BYHOUR=9;UNTIL=20270706T210000Z").unwrap();
    let anchor = at("2027-07-05T09:00:00+12:00");
    let auckland = zone("Pacific/Auckland");
    let second = rule.next_after(anchor, anchor, 1, auckland);
    assert_eq!(second, Some(at("2027-07-06T09:00:00+12:00")));
    let third = rule.next_after(anchor, second.unwrap(), 2, auckland);
    assert_eq!(
        third,
        Some(at("2027-07-07T09:00:00+12:00")),
        "UNTIL includes the instant it names"
    );
    assert_eq!(rule.next_after(anchor, third.unwrap(), 3, auckland), None);
}

/// The member a harness chat belongs to, who is the one a reminder runs as.
async fn owner(harness: &Harness) -> Uuid {
    sqlx::query_scalar(
        "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND role = 'owner' \
         AND is_active LIMIT 1",
    )
    .bind(harness.workspace)
    .fetch_one(&harness.pool)
    .await
    .expect("the harness workspace has an owner")
}

fn daily(due_at: DateTime<FixedOffset>, time_zone: Option<&str>) -> Reminder {
    Reminder {
        content: "Stand up".into(),
        due_at,
        rrule: Some("FREQ=DAILY;BYHOUR=9".into()),
        prompt: None,
        timing_mode: None,
        time_zone: time_zone.map(str::to_string),
    }
}

/// Nine o'clock on `zone`'s clock two days from now, in the offset it shows then.
fn nine_in_two_days(zone: TimeZone) -> DateTime<FixedOffset> {
    let date = zone.local(Utc::now()).date() + Duration::days(2);
    let instant = zone
        .instant(date.and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap()))
        .expect("nine o'clock exists");
    instant.with_timezone(&zone.offset(instant))
}

fn refusal(error: sqlx::Error) -> String {
    match error {
        sqlx::Error::Protocol(message) => message,
        other => panic!("expected a validation refusal, got {other}"),
    }
}

/// Fires one due reminder and returns where it was moved to.
async fn fire(harness: &Harness, reminder: Uuid) -> DateTime<Utc> {
    sqlx::query("UPDATE reminders SET due_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(reminder)
        .execute(&harness.pool)
        .await
        .expect("the reminder is due");
    assert_eq!(
        reminders::deliver_next(&harness.pool, Sweep::Workspace(harness.workspace))
            .await
            .unwrap(),
        Delivered::Settled,
        "a due reminder is waiting to be claimed"
    );
    let (fired, due_at): (i32, DateTime<Utc>) =
        sqlx::query_as("SELECT fired_count, due_at FROM reminders WHERE id = $1")
            .bind(reminder)
            .fetch_one(&harness.pool)
            .await
            .expect("the reminder is readable");
    assert_eq!(fired, 1, "the workspace's only due reminder fired once");
    due_at
}

fn unjittered(reminder: Uuid, rule: &str, next: DateTime<Utc>) -> DateTime<Utc> {
    next - schedule::jitter(reminder, schedule::parse(rule).unwrap().period())
}

#[tokio::test]
async fn a_stored_repeat_fires_next_at_nine_on_its_own_clock() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let berlin = zone("Europe/Berlin");

    let created = reminders::create(
        &harness.pool,
        harness.workspace,
        owner(&harness).await,
        harness.chat,
        daily(nine_in_two_days(berlin), Some("Europe/Berlin")),
    )
    .await
    .expect("a repeat in its own zone is storable");
    assert_eq!(created["time_zone"], "Europe/Berlin");
    let reminder: Uuid = serde_json::from_value(created["id"].clone()).unwrap();

    let next = unjittered(
        reminder,
        "FREQ=DAILY;BYHOUR=9",
        fire(&harness, reminder).await,
    );
    assert!(next > Utc::now(), "{next}");
    assert_eq!(
        (berlin.local(next).hour(), berlin.local(next).minute()),
        (9, 0),
        "{next} is not nine o'clock in Berlin"
    );
}

#[tokio::test]
async fn a_repeat_without_a_zone_takes_the_one_the_person_s_console_reported() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let user = owner(&harness).await;
    let auckland = zone("Pacific/Auckland");
    users::set_time_zone(&harness.pool, user, auckland)
        .await
        .unwrap();

    let created = reminders::create(
        &harness.pool,
        harness.workspace,
        user,
        harness.chat,
        daily(nine_in_two_days(auckland), None),
    )
    .await
    .expect("a repeat written in the person's own offset is storable");
    assert_eq!(created["time_zone"], "Pacific/Auckland");

    let written_in_utc = nine_in_two_days(auckland)
        .with_timezone(&Utc)
        .fixed_offset();
    let refused = refusal(
        reminders::create(
            &harness.pool,
            harness.workspace,
            user,
            harness.chat,
            daily(written_in_utc, None),
        )
        .await
        .expect_err("a due_at in another zone's offset is refused"),
    );
    assert!(refused.contains("Pacific/Auckland"), "{refused}");
    assert!(refused.contains("time_zone"), "{refused}");
}

#[tokio::test]
async fn a_repeat_with_no_zone_known_is_read_in_utc_and_says_so() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let user = owner(&harness).await;

    let refused = refusal(
        reminders::create(
            &harness.pool,
            harness.workspace,
            user,
            harness.chat,
            daily(nine_in_two_days(zone("Europe/Berlin")), None),
        )
        .await
        .expect_err("a +01:00 repeat with no zone known is refused rather than read in UTC"),
    );
    assert!(refused.contains("No time zone is known"), "{refused}");
    assert!(
        !refused.contains("write due_at in"),
        "a UTC rewrite would drift at every clock change, so it is not offered: {refused}"
    );
    assert!(refused.contains("UTC"), "{refused}");

    let eight_utc = nine_in_two_days(TimeZone::UTC) - Duration::hours(1);
    let refused = refusal(
        reminders::create(
            &harness.pool,
            harness.workspace,
            user,
            harness.chat,
            daily(eight_utc, None),
        )
        .await
        .expect_err("a first firing the rule does not select is refused"),
    );
    assert!(refused.contains("does not fire at"), "{refused}");
    assert!(refused.contains("time_zone"), "{refused}");

    let created = reminders::create(
        &harness.pool,
        harness.workspace,
        user,
        harness.chat,
        daily(nine_in_two_days(TimeZone::UTC), None),
    )
    .await
    .expect("a repeat written in UTC is storable");
    assert_eq!(created["time_zone"], "UTC");
}

#[tokio::test]
async fn a_reminder_stored_before_zones_keeps_firing_in_utc() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let user = owner(&harness).await;
    let rule = "FREQ=DAILY;BYHOUR=9";

    let reminder: Uuid = sqlx::query_scalar(
        "INSERT INTO reminders (workspace_id, created_by, chat_id, content, due_at, rrule, \
         anchor_at, expires_at) VALUES ($1, $2, $3, 'Stand up', NOW() + INTERVAL '1 hour', $4, \
         $5, NOW() + INTERVAL '6 days') RETURNING id",
    )
    .bind(harness.workspace)
    .bind(user)
    .bind(harness.chat)
    .bind(rule)
    .bind(at("2026-09-14T09:00:00Z"))
    .fetch_one(&harness.pool)
    .await
    .expect("a row written the way one was before reminders had a zone");

    let listed = reminders::list(&harness.pool, harness.workspace, user)
        .await
        .unwrap();
    let row = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == reminder.to_string())
        .expect("the reminder is listed");
    assert_eq!(row["time_zone"], "UTC");

    let next = unjittered(reminder, rule, fire(&harness, reminder).await);
    assert_eq!(
        (next.hour(), next.minute()),
        (9, 0),
        "{next}: a reminder from before zones fires at the hour it always did"
    );
}

async fn post_auth(client: &TestClient, uri: &str, body: &Value, zone: Option<&str>) -> Value {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(zone) = zone {
        request = request.header("x-zone-time-zone", zone);
    }
    let response = client
        .send_request(request.body(Body::from(body.to_string())).unwrap())
        .await;
    assert!(
        response.status == StatusCode::OK || response.status == StatusCode::CREATED,
        "{uri}: {}",
        response.text()
    );
    response.json_value()
}

async fn reported(client: &TestClient, user: Uuid) -> Option<&'static str> {
    users::time_zone(client.state().db(), user)
        .await
        .unwrap()
        .map(TimeZone::name)
}

#[tokio::test]
async fn the_console_s_zone_is_kept_on_sign_in_and_refresh() {
    let client = TestClient::with_db().await;
    let (email, password) = (test_email(), test_password());
    let registered = post_auth(
        &client,
        "/api/auth/register",
        &json!({"email": email, "password": password, "display_name": "Ari"}),
        Some("Pacific/Auckland"),
    )
    .await;
    let user = Uuid::parse_str(registered["user"]["id"].as_str().unwrap()).unwrap();
    assert_eq!(reported(&client, user).await, Some("Pacific/Auckland"));

    let signed_in = post_auth(
        &client,
        "/api/auth/login",
        &json!({"email": email, "password": password}),
        Some("America/New_York"),
    )
    .await;
    assert_eq!(reported(&client, user).await, Some("America/New_York"));

    let refreshed = post_auth(
        &client,
        "/api/auth/refresh",
        &json!({"refresh_token": signed_in["refresh_token"]}),
        Some("Europe/London"),
    )
    .await;
    assert_eq!(
        reported(&client, user).await,
        Some("Europe/London"),
        "a person who travels is followed"
    );

    let unnamed = post_auth(
        &client,
        "/api/auth/refresh",
        &json!({"refresh_token": refreshed["refresh_token"]}),
        Some("Middle/Earth"),
    )
    .await;
    post_auth(
        &client,
        "/api/auth/refresh",
        &json!({"refresh_token": unnamed["refresh_token"]}),
        None,
    )
    .await;
    assert_eq!(
        reported(&client, user).await,
        Some("Europe/London"),
        "a name that is not a zone, or no name at all, leaves the last real one in place"
    );
}

#[tokio::test]
async fn the_session_context_states_the_person_s_own_zone() {
    let harness = Harness::new(Some(32_768), false, Vec::new()).await;
    let user = owner(&harness).await;
    users::set_time_zone(&harness.pool, user, zone("Europe/London"))
        .await
        .unwrap();
    let state = crate::common::create_test_state(harness.config.clone(), harness.pool.clone());
    let chat = chats::get_chat(&harness.pool, harness.chat)
        .await
        .unwrap()
        .unwrap();
    let route = Route::for_workspace(&state, harness.workspace).await;

    let preview = session::build(
        &state,
        &chat,
        user,
        Some(("When is it?", None)),
        Mode::Preview,
        route.endpoint().expect("a usable route").clone(),
        None,
    )
    .await
    .expect("the chat previews");
    let prompt: String = context::project(&preview.context.entries, None)
        .unwrap()
        .iter()
        .filter_map(|message| message.content.clone())
        .collect();
    let now = prompt
        .lines()
        .find(|line| line.starts_with("- Now: "))
        .expect("the session context states the time");
    assert!(now.contains("(Europe/London)"), "{now}");
}
