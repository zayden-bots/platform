use sqlx::PgPool;
use zayden_app::applications::SealedToken;
use zayden_app::delivery::{DeliveryStore, GuildHook, NewHook};

const APP: i64 = 1000;
const CUSTOM: i64 = 2000;

fn sealed(tag: u8) -> SealedToken {
    SealedToken { key_id: 1, nonce: vec![tag; 12], ciphertext: vec![tag; 20] }
}

fn new_hook(channel_id: i64, webhook_id: i64, tag: u8) -> NewHook {
    NewHook {
        channel_id,
        application_id: APP,
        guild_id: 1,
        webhook_id,
        sealed: sealed(tag),
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn register_apps(pool: &PgPool, name: &str, avatar: Option<&str>) {
    sqlx::query!(
        "INSERT INTO applications (application_id, slot, kind, bot_user_id, name, avatar)
         VALUES ($1, 'zayden', 'default', 1001, $2, $3)",
        APP,
        name,
        avatar,
    )
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn upsert_replaces_and_clears_broken(pool: PgPool) {
    DeliveryStore::upsert(&pool, &new_hook(10, 500, 1)).await.unwrap();
    assert!(DeliveryStore::mark_broken(&pool, 10, APP, 500).await.unwrap());

    DeliveryStore::upsert(&pool, &new_hook(10, 501, 2)).await.unwrap();

    let hook = DeliveryStore::hook(&pool, 10, APP).await.unwrap().unwrap();
    assert_eq!(hook.webhook_id, 501);
    assert_eq!(hook.sealed, sealed(2));
    assert_eq!(
        DeliveryStore::channel_hook(&pool, 10, APP).await.unwrap(),
        Some(GuildHook { channel_id: 10, webhook_id: 501, broken: false })
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn mark_broken_is_fenced_by_webhook_id(pool: PgPool) {
    DeliveryStore::upsert(&pool, &new_hook(10, 500, 1)).await.unwrap();

    assert!(!DeliveryStore::mark_broken(&pool, 10, APP, 499).await.unwrap());
    assert!(DeliveryStore::hook(&pool, 10, APP).await.unwrap().is_some());
    assert!(!DeliveryStore::delete(&pool, 10, APP, 499).await.unwrap());
    assert!(DeliveryStore::delete(&pool, 10, APP, 500).await.unwrap());
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn hook_skips_broken_rows(pool: PgPool) {
    DeliveryStore::upsert(&pool, &new_hook(10, 500, 1)).await.unwrap();
    DeliveryStore::mark_broken(&pool, 10, APP, 500).await.unwrap();

    assert!(DeliveryStore::hook(&pool, 10, APP).await.unwrap().is_none());
    assert_eq!(DeliveryStore::for_guild(&pool, 1, APP).await.unwrap(), vec![
        GuildHook { channel_id: 10, webhook_id: 500, broken: true }
    ]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn hook_builds_sender_from_applications(pool: PgPool) {
    register_apps(&pool, "Zayden", Some("abc")).await;
    DeliveryStore::upsert(&pool, &new_hook(10, 500, 1)).await.unwrap();

    let hook = DeliveryStore::hook(&pool, 10, APP).await.unwrap().unwrap();
    assert_eq!(hook.sender_name.as_deref(), Some("Zayden"));
    assert_eq!(
        hook.sender_avatar_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/1001/abc.png")
    );

    sqlx::query!("UPDATE applications SET name = 'My Discord Bot'")
        .execute(&pool)
        .await
        .unwrap();
    let hook = DeliveryStore::hook(&pool, 10, APP).await.unwrap().unwrap();
    assert_eq!(hook.sender_name, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR", fixtures("delivery_seat"))]
async fn wanted_follows_the_seat(pool: PgPool) {
    assert_eq!(DeliveryStore::wanted(&pool, 1, APP).await.unwrap(), vec![11]);
    assert_eq!(
        DeliveryStore::wanted(&pool, 2, APP).await.unwrap(),
        Vec::<i64>::new()
    );
    assert_eq!(DeliveryStore::wanted(&pool, 2, CUSTOM).await.unwrap(), vec![22]);
}

#[sqlx::test(migrator = "schema::MIGRATOR", fixtures("delivery_seat"))]
async fn wanted_skips_absent_bots(pool: PgPool) {
    sqlx::query!("UPDATE guild_presence SET left_at = now() WHERE guild_id = 1")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        DeliveryStore::wanted(&pool, 1, APP).await.unwrap(),
        Vec::<i64>::new()
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR", fixtures("delivery_seat"))]
async fn wanted_guilds_includes_stale_hooks(pool: PgPool) {
    DeliveryStore::upsert(&pool, &NewHook {
        channel_id: 33,
        application_id: APP,
        guild_id: 3,
        webhook_id: 600,
        sealed: sealed(3),
    })
    .await
    .unwrap();

    assert_eq!(DeliveryStore::wanted_guilds(&pool, APP).await.unwrap(), vec![1, 3]);
    assert_eq!(DeliveryStore::wanted_guilds(&pool, CUSTOM).await.unwrap(), vec![2]);
}
