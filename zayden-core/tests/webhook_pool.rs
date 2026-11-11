use std::sync::Arc;

use serenity::all::{ApplicationId, ChannelId, MessageId};
use sqlx::PgPool;
use zayden_app::applications::Keyring;
use zayden_core::{Delivered, Edited, Post, PostPayload, WebhookPool};

const KEY: &str = "1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

#[expect(clippy::expect_used, reason = "test helper with a fixed valid key")]
fn pool(db: PgPool) -> WebhookPool {
    let keyring = Keyring::parse(KEY, "1").expect("test key parses");
    WebhookPool::new(db, Arc::new(keyring))
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn send_without_a_hook_queues_a_post(db: PgPool) {
    let post = Post::content("hello");

    let delivered = pool(db.clone())
        .send(ChannelId::new(10), ApplicationId::new(20), &post)
        .await
        .unwrap();

    assert_eq!(delivered, Delivered::Queued);
    let rows = sqlx::query!("SELECT kind, payload FROM scheduled_tasks")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "platform.post");
    let payload: PostPayload =
        serde_json::from_value(rows[0].payload.clone()).unwrap();
    assert_eq!(payload.channel_id, 10);
    assert_eq!(payload.edit_message_id, None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn edit_without_a_hook_queues_an_edit(db: PgPool) {
    let post = Post::content("hello");

    let edited = pool(db.clone())
        .edit(ChannelId::new(10), ApplicationId::new(20), MessageId::new(30), &post)
        .await
        .unwrap();

    assert_eq!(edited, Edited::Queued);
    let rows = sqlx::query!("SELECT kind, payload FROM scheduled_tasks")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "platform.post");
    let payload: PostPayload =
        serde_json::from_value(rows[0].payload.clone()).unwrap();
    assert_eq!(payload.channel_id, 10);
    assert_eq!(payload.edit_message_id, Some(30));
}
