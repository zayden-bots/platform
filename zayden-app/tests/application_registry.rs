use sqlx::PgPool;
use zayden_app::applications::{
    ApplicationModules,
    ApplicationStore,
    NewApplication,
    ProcessStatus,
    Slot,
};

const APP: i64 = 100;
const OTHER: i64 = 200;

fn names(modules: &[&str]) -> Vec<String> {
    modules.iter().map(|m| (*m).to_owned()).collect()
}

#[expect(
    clippy::expect_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn modules_of(pool: &PgPool, application_id: i64) -> Vec<String> {
    sqlx::query_scalar!(
        "SELECT module FROM application_modules WHERE application_id = $1
         ORDER BY module",
        application_id
    )
    .fetch_all(pool)
    .await
    .expect("the module lookup runs")
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn replace_adds_the_listed_modules(pool: PgPool) {
    ApplicationModules::replace(&pool, APP, &names(&["music", "ticket"]))
        .await
        .unwrap();

    assert_eq!(modules_of(&pool, APP).await, ["music", "ticket"]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn replace_removes_unlisted_modules(pool: PgPool) {
    ApplicationModules::replace(&pool, APP, &names(&["music", "ticket"]))
        .await
        .unwrap();
    ApplicationModules::replace(&pool, APP, &names(&["music"])).await.unwrap();

    assert_eq!(modules_of(&pool, APP).await, ["music"]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn replace_is_idempotent(pool: PgPool) {
    let modules = names(&["music", "ticket"]);
    ApplicationModules::replace(&pool, APP, &modules).await.unwrap();
    ApplicationModules::replace(&pool, APP, &modules).await.unwrap();

    assert_eq!(modules_of(&pool, APP).await, ["music", "ticket"]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn replace_leaves_other_applications_alone(pool: PgPool) {
    ApplicationModules::replace(&pool, APP, &names(&["music"])).await.unwrap();
    ApplicationModules::replace(&pool, OTHER, &names(&["ticket"])).await.unwrap();
    ApplicationModules::replace(&pool, APP, &names(&[])).await.unwrap();

    assert_eq!(modules_of(&pool, APP).await, Vec::<String>::new());
    assert_eq!(modules_of(&pool, OTHER).await, ["ticket"]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn list_returns_offered_modules_sorted(pool: PgPool) {
    ApplicationModules::replace(&pool, APP, &names(&["ticket", "music"]))
        .await
        .unwrap();
    ApplicationModules::replace(&pool, OTHER, &names(&["ai"])).await.unwrap();

    assert_eq!(ApplicationModules::list(&pool, APP).await.unwrap(), [
        "music", "ticket"
    ]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn process_list_names_bots_and_keeps_the_worker(pool: PgPool) {
    let app = NewApplication {
        application_id: APP,
        slot: Slot::Zayden,
        bot_user_id: APP,
        name: "Zayden",
        avatar: None,
    };
    ApplicationStore::register_default(&pool, &app).await.unwrap();
    ProcessStatus::started(&pool, "worker", None, "abc123").await.unwrap();
    ProcessStatus::started(&pool, "100", Some(APP), "def456").await.unwrap();

    let rows = ProcessStatus::list(&pool).await.unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].process, "100");
    assert_eq!(rows[0].application_id, Some(APP));
    assert_eq!(rows[0].name.as_deref(), Some("Zayden"));
    assert_eq!(rows[0].build_sha, "def456");
    assert_eq!(rows[1].process, "worker");
    assert_eq!(rows[1].name, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn started_then_seen_moves_last_seen(pool: PgPool) {
    ProcessStatus::started(&pool, "zayden", Some(APP), "abc123").await.unwrap();
    ProcessStatus::seen(&pool, "zayden", Some(APP), "abc123").await.unwrap();

    let row = sqlx::query!(
        r#"SELECT application_id, build_sha, (last_seen >= started_at) AS "ordered!"
           FROM application_status WHERE process = $1"#,
        "zayden"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.application_id, Some(APP));
    assert_eq!(row.build_sha, "abc123");
    assert!(row.ordered);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn seen_recreates_a_missing_row(pool: PgPool) {
    ProcessStatus::seen(&pool, "ghost", Some(APP), "abc123").await.unwrap();

    let row = sqlx::query!(
        "SELECT application_id, build_sha FROM application_status
         WHERE process = $1",
        "ghost"
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(row.application_id, Some(APP));
    assert_eq!(row.build_sha, "abc123");
}
