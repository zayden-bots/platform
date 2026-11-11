use jiff::{SignedDuration, Timestamp};
use serde_json::json;
use sqlx::PgPool;
use zayden_app::tasks::{NewTask, ScheduledTasks, Task};

const APP: i64 = 1001;
const OTHER: i64 = 1002;

fn new_task(kind: &'static str, key: &str, run_at: Timestamp) -> NewTask {
    NewTask { kind, key: key.to_owned(), run_at, payload: json!({ "n": 1 }) }
}

fn ago(hours: i64) -> Timestamp {
    Timestamp::now() - SignedDuration::from_hours(hours)
}

fn ahead(hours: i64) -> Timestamp {
    Timestamp::now() + SignedDuration::from_hours(hours)
}

#[expect(
    clippy::expect_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn keys(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar!("SELECT key FROM scheduled_tasks ORDER BY key")
        .fetch_all(pool)
        .await
        .expect("the key lookup runs")
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn replace_inserts_and_replaces_by_prefix(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "a:2:", &[new_task("k", "a:2:x", ahead(1))])
        .await
        .unwrap();
    ScheduledTasks::replace(&pool, APP, "a:1:", &[
        new_task("k", "a:1:x", ahead(1)),
        new_task("k", "a:1:y", ahead(2)),
    ])
    .await
    .unwrap();
    assert_eq!(keys(&pool).await, ["a:1:x", "a:1:y", "a:2:x"]);

    ScheduledTasks::replace(&pool, APP, "a:1:", &[new_task("k", "a:1:z", ahead(1))])
        .await
        .unwrap();
    assert_eq!(keys(&pool).await, ["a:1:z", "a:2:x"]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn ensure_keeps_existing_tasks(pool: PgPool) {
    let original = ago(1);
    ScheduledTasks::replace(&pool, APP, "e:", &[new_task("k", "e:old", original)])
        .await
        .unwrap();
    let claimed = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    ScheduledTasks::ensure(&pool, APP, &[
        new_task("k", "e:old", ahead(5)),
        new_task("k", "e:new", ahead(1)),
    ])
    .await
    .unwrap();

    assert_eq!(keys(&pool).await, ["e:new", "e:old"]);
    let old = sqlx::query!(
        r#"SELECT run_at AS "run_at!: jiff_sqlx::Timestamp",
                  claimed_at AS "claimed_at: jiff_sqlx::Timestamp",
                  attempts
           FROM scheduled_tasks WHERE key = 'e:old'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old.run_at.to_jiff(), claimed[0].run_at);
    assert_eq!(
        old.claimed_at.map(jiff_sqlx::Timestamp::to_jiff),
        Some(claimed[0].claimed_at)
    );
    assert_eq!(old.attempts, 1);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn claim_due_returns_only_due_tasks_of_the_app_and_kinds(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "due", &[new_task("k", "due", ago(1))])
        .await
        .unwrap();
    ScheduledTasks::replace(&pool, APP, "later", &[new_task(
        "k",
        "later",
        ahead(1),
    )])
    .await
    .unwrap();
    ScheduledTasks::replace(&pool, APP, "kind", &[new_task(
        "other",
        "kind",
        ago(1),
    )])
    .await
    .unwrap();
    ScheduledTasks::replace(&pool, OTHER, "app", &[new_task("k", "app", ago(1))])
        .await
        .unwrap();

    let claimed = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].key, "due");
    assert_eq!(claimed[0].attempts, 1);
    assert_eq!(claimed[0].payload, json!({ "n": 1 }));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_claimed_task_is_not_claimed_twice(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "due", &[new_task("k", "due", ago(1))])
        .await
        .unwrap();

    let first = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();
    let second = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(second, Vec::<Task>::new());
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn complete_deletes_the_claimed_task(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "due", &[new_task("k", "due", ago(1))])
        .await
        .unwrap();
    let claimed = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    assert!(ScheduledTasks::complete(&pool, &claimed[0]).await.unwrap());
    assert_eq!(keys(&pool).await, Vec::<String>::new());
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn complete_keeps_a_task_rescheduled_while_it_ran(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "due", &[new_task("k", "due", ago(1))])
        .await
        .unwrap();
    let claimed = ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    ScheduledTasks::replace(&pool, APP, "due", &[new_task("k", "due", ahead(1))])
        .await
        .unwrap();

    assert!(!ScheduledTasks::complete(&pool, &claimed[0]).await.unwrap());
    let unclaimed = sqlx::query_scalar!(
        r#"SELECT claimed_at IS NULL AS "unclaimed!" FROM scheduled_tasks
           WHERE key = 'due'"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(unclaimed);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn release_stale_releases_old_claims_and_drops_exhausted_ones(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "t:", &[
        new_task("k", "t:keep", ago(1)),
        new_task("k", "t:drop", ago(1)),
    ])
    .await
    .unwrap();
    ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();
    sqlx::query!(
        "UPDATE scheduled_tasks
         SET claimed_at = now() - interval '1 hour',
             attempts = CASE WHEN key = 't:drop' THEN 5 ELSE 1 END"
    )
    .execute(&pool)
    .await
    .unwrap();

    let dropped: Vec<Task> =
        ScheduledTasks::release_stale(&pool, APP).await.unwrap();

    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].key, "t:drop");
    assert_eq!(keys(&pool).await, ["t:keep"]);
    let unclaimed = sqlx::query_scalar!(
        r#"SELECT claimed_at IS NULL AS "unclaimed!" FROM scheduled_tasks"#
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(unclaimed);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn next_due_is_the_earliest_unclaimed(pool: PgPool) {
    assert_eq!(ScheduledTasks::next_due(&pool, APP, &["k"]).await.unwrap(), None);

    let soon = ahead(1);
    ScheduledTasks::replace(&pool, APP, "t:", &[
        new_task("k", "t:soon", soon),
        new_task("k", "t:late", ahead(5)),
    ])
    .await
    .unwrap();
    ScheduledTasks::replace(&pool, APP, "claimed", &[new_task(
        "k",
        "claimed",
        ago(1),
    )])
    .await
    .unwrap();
    ScheduledTasks::claim_due(&pool, APP, &["k"], 10).await.unwrap();

    let next = ScheduledTasks::next_due(&pool, APP, &["k"]).await.unwrap().unwrap();
    assert!(next.as_microsecond().abs_diff(soon.as_microsecond()) < 1_000);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn purge_unclaimed_spares_claimed_fresh_and_other_kinds(pool: PgPool) {
    ScheduledTasks::replace(&pool, APP, "claimed", &[new_task(
        "platform.post",
        "claimed",
        ago(3),
    )])
    .await
    .unwrap();
    ScheduledTasks::claim_due(&pool, APP, &["platform.post"], 10).await.unwrap();
    ScheduledTasks::replace(&pool, APP, "t:", &[
        new_task("platform.post", "t:old", ago(3)),
        new_task("platform.post", "t:fresh", Timestamp::now()),
        new_task("lfg_reminder", "t:lfg", ago(3)),
    ])
    .await
    .unwrap();

    let purged = ScheduledTasks::purge_unclaimed(
        &pool,
        &["platform.post", "platform.dm"],
        SignedDuration::from_hours(1),
    )
    .await
    .unwrap();

    assert_eq!(purged, 1);
    assert_eq!(keys(&pool).await, ["claimed", "t:fresh", "t:lfg"]);
}

#[test]
fn is_stale_after_ten_minutes() {
    let run_at = Timestamp::now();
    let task = Task {
        id: 1,
        application_id: APP,
        kind: "k".to_owned(),
        key: "k".to_owned(),
        run_at,
        payload: json!({}),
        claimed_at: run_at,
        attempts: 1,
    };

    assert!(!task.is_stale(run_at + SignedDuration::from_mins(9)));
    assert!(task.is_stale(run_at + SignedDuration::from_mins(11)));
}
