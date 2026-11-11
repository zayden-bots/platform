use jiff::Timestamp;
use sqlx::PgPool;
use zayden_app::applications::{ApplicationStore, NewApplication, Slot};
use zayden_app::delivery::{DeliveryKind, DeliveryStore};
use zayden_app::guilds::GuildPresence;

const ZAYDEN: i64 = 1;
const MARIA: i64 = 3;
const GUILD: i64 = 10;
const CHANNEL: i64 = 50;

const KINDS: [DeliveryKind; 5] = [
    DeliveryKind::Youtube,
    DeliveryKind::Patreon,
    DeliveryKind::DeadlockPatch,
    DeliveryKind::DeadlockSummary,
    DeliveryKind::DeadlockSquad,
];

const fn joined() -> Timestamp {
    Timestamp::constant(1_750_000_000, 0)
}

#[expect(
    clippy::unwrap_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn register_default(pool: &PgPool, slot: Slot, application_id: i64) {
    let app = NewApplication {
        application_id,
        slot,
        bot_user_id: application_id,
        name: slot.name(),
        avatar: None,
    };
    ApplicationStore::register_default(pool, &app).await.unwrap();
}

#[expect(
    clippy::unwrap_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn join(pool: &PgPool, application_id: i64) {
    GuildPresence::joined(pool, GUILD, application_id, joined()).await.unwrap();
}

async fn both_present(pool: &PgPool) {
    register_default(pool, Slot::Zayden, ZAYDEN).await;
    register_default(pool, Slot::Maria, MARIA).await;
    join(pool, ZAYDEN).await;
    join(pool, MARIA).await;
}

#[expect(
    clippy::unwrap_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn insert_row(pool: &PgPool, kind: DeliveryKind) {
    match kind {
        DeliveryKind::Youtube => {
            sqlx::query!(
                "INSERT INTO youtube_announce (guild_id, channel_id) VALUES ($1, $2)",
                GUILD,
                CHANNEL,
            )
            .execute(pool)
            .await
            .unwrap();
        },
        DeliveryKind::Patreon => {
            sqlx::query!(
                "INSERT INTO patreon_announce (guild_id, channel_id) VALUES ($1, $2)",
                GUILD,
                CHANNEL,
            )
            .execute(pool)
            .await
            .unwrap();
        },
        DeliveryKind::DeadlockPatch => {
            sqlx::query!(
                "INSERT INTO deadlock_patch_announce (guild_id, channel_id)
                 VALUES ($1, $2)",
                GUILD,
                CHANNEL,
            )
            .execute(pool)
            .await
            .unwrap();
        },
        DeliveryKind::DeadlockSummary => {
            sqlx::query!(
                "INSERT INTO deadlock_summary_channel (guild_id, channel_id)
                 VALUES ($1, $2)",
                GUILD,
                CHANNEL,
            )
            .execute(pool)
            .await
            .unwrap();
        },
        DeliveryKind::DeadlockSquad => {
            sqlx::query!(
                "INSERT INTO deadlock_squad_channel (guild_id, channel_id)
                 VALUES ($1, $2)",
                GUILD,
                CHANNEL,
            )
            .execute(pool)
            .await
            .unwrap();
        },
    }
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn binding_round_trips_for_every_kind(pool: PgPool) {
    both_present(&pool).await;
    for kind in KINDS {
        insert_row(&pool, kind).await;

        let changed =
            DeliveryStore::bind_application(&pool, kind, GUILD, Some(MARIA))
                .await
                .unwrap();
        let bound =
            DeliveryStore::bound_application(&pool, kind, GUILD).await.unwrap();

        assert!(changed, "{kind:?}");
        assert_eq!(bound, Some(MARIA), "{kind:?}");
    }
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn binding_the_same_value_again_changes_nothing(pool: PgPool) {
    both_present(&pool).await;
    insert_row(&pool, DeliveryKind::Youtube).await;
    let kind = DeliveryKind::Youtube;

    DeliveryStore::bind_application(&pool, kind, GUILD, Some(MARIA)).await.unwrap();
    let again = DeliveryStore::bind_application(&pool, kind, GUILD, Some(MARIA))
        .await
        .unwrap();

    assert!(!again);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn binding_none_clears_the_column(pool: PgPool) {
    both_present(&pool).await;
    insert_row(&pool, DeliveryKind::Patreon).await;
    let kind = DeliveryKind::Patreon;

    DeliveryStore::bind_application(&pool, kind, GUILD, Some(MARIA)).await.unwrap();
    let cleared =
        DeliveryStore::bind_application(&pool, kind, GUILD, None).await.unwrap();
    let bound = DeliveryStore::bound_application(&pool, kind, GUILD).await.unwrap();

    assert!(cleared);
    assert_eq!(bound, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn binding_a_guild_without_a_row_does_nothing(pool: PgPool) {
    both_present(&pool).await;
    let kind = DeliveryKind::DeadlockSquad;

    let changed = DeliveryStore::bind_application(&pool, kind, GUILD, Some(MARIA))
        .await
        .unwrap();
    let bound = DeliveryStore::bound_application(&pool, kind, GUILD).await.unwrap();

    assert!(!changed);
    assert_eq!(bound, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn effective_prefers_a_present_bound_application(pool: PgPool) {
    both_present(&pool).await;

    let effective = DeliveryStore::effective_application(&pool, GUILD, Some(MARIA))
        .await
        .unwrap();

    assert_eq!(effective, Some(MARIA));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn effective_falls_back_to_the_zayden_seat(pool: PgPool) {
    both_present(&pool).await;

    let effective =
        DeliveryStore::effective_application(&pool, GUILD, None).await.unwrap();

    assert_eq!(effective, Some(ZAYDEN));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn effective_is_none_when_the_bot_is_absent(pool: PgPool) {
    register_default(&pool, Slot::Zayden, ZAYDEN).await;
    register_default(&pool, Slot::Maria, MARIA).await;
    join(&pool, ZAYDEN).await;

    let effective = DeliveryStore::effective_application(&pool, GUILD, Some(MARIA))
        .await
        .unwrap();

    assert_eq!(effective, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn rebinding_moves_the_channel_between_bots(pool: PgPool) {
    both_present(&pool).await;
    insert_row(&pool, DeliveryKind::Youtube).await;

    let zayden_before = DeliveryStore::wanted(&pool, GUILD, ZAYDEN).await.unwrap();
    let maria_before = DeliveryStore::wanted(&pool, GUILD, MARIA).await.unwrap();
    DeliveryStore::bind_application(
        &pool,
        DeliveryKind::Youtube,
        GUILD,
        Some(MARIA),
    )
    .await
    .unwrap();
    let zayden_after = DeliveryStore::wanted(&pool, GUILD, ZAYDEN).await.unwrap();
    let maria_after = DeliveryStore::wanted(&pool, GUILD, MARIA).await.unwrap();

    assert!(zayden_before.contains(&CHANNEL));
    assert!(!maria_before.contains(&CHANNEL));
    assert!(!zayden_after.contains(&CHANNEL));
    assert!(maria_after.contains(&CHANNEL));
}
