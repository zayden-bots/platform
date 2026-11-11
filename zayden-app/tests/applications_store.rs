use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use zayden_app::applications::{
    AppKind,
    ApplicationError,
    ApplicationProfile,
    ApplicationStatus,
    ApplicationStore,
    Keyring,
    NewApplication,
    NewCustomApplication,
    Slot,
};
use zayden_app::guilds::GuildPresence;

const KEY: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const SECOND_KEY: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
const ZAYDEN: i64 = 100;
const OTHER_DEFAULT: i64 = 101;
const LLAMA: i64 = 200;
const OTHER: i64 = 300;
const GUILD: i64 = 10;
const SECOND_GUILD: i64 = 20;
const ADMIN: i64 = 1;
const TOKEN: &str = "not.a-real.token";

const fn default_app(application_id: i64, name: &str) -> NewApplication<'_> {
    NewApplication {
        application_id,
        slot: Slot::Zayden,
        bot_user_id: application_id,
        name,
        avatar: None,
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "a free helper sits outside the #[test] items clippy.toml exempts"
)]
async fn register(pool: &PgPool, application_id: i64) {
    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let sealed =
        keys.seal(application_id, &SecretString::from(TOKEN.to_owned())).unwrap();
    let app = NewCustomApplication {
        app: default_app(application_id, "LlamaBot"),
        public_key: "public",
        registered_by: ADMIN,
    };
    ApplicationStore::upsert_custom(pool, &app, &sealed).await.unwrap();
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_stored_token_round_trips(pool: PgPool) {
    register(&pool, LLAMA).await;

    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let sealed =
        ApplicationStore::sealed_token(&pool, LLAMA).await.unwrap().unwrap();

    assert_eq!(keys.open(LLAMA, &sealed).unwrap().expose_secret(), TOKEN);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn profile_reads_a_registered_default(pool: PgPool) {
    let app = NewApplication {
        bot_user_id: 999,
        avatar: Some("abc123"),
        ..default_app(ZAYDEN, "Zayden")
    };
    ApplicationStore::register_default(&pool, &app).await.unwrap();

    assert_eq!(
        ApplicationStore::profile(&pool, ZAYDEN).await.unwrap(),
        Some(ApplicationProfile {
            application_id: ZAYDEN,
            slot: Slot::Zayden,
            kind: AppKind::Default,
            bot_user_id: 999,
            name: "Zayden".to_owned(),
            avatar: Some("abc123".to_owned()),
        })
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn profile_is_none_for_an_unknown_application(pool: PgPool) {
    assert_eq!(ApplicationStore::profile(&pool, ZAYDEN).await.unwrap(), None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn present_in_guild_lists_only_present_applications(pool: PgPool) {
    let joined = jiff::Timestamp::constant(1_750_000_000, 0);
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    let maria =
        NewApplication { slot: Slot::Maria, ..default_app(OTHER_DEFAULT, "Maria") };
    ApplicationStore::register_default(&pool, &maria).await.unwrap();
    GuildPresence::joined(&pool, GUILD, ZAYDEN, joined).await.unwrap();
    GuildPresence::joined(&pool, GUILD, OTHER_DEFAULT, joined).await.unwrap();
    GuildPresence::left(&pool, GUILD, ZAYDEN).await.unwrap();

    let present = ApplicationStore::present_in_guild(&pool, GUILD).await.unwrap();

    assert_eq!(present.iter().map(|app| app.application_id).collect::<Vec<_>>(), [
        OTHER_DEFAULT
    ]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn the_bot_user_id_is_looked_up_by_application(pool: PgPool) {
    let app = NewApplication { bot_user_id: 999, ..default_app(ZAYDEN, "Zayden") };
    ApplicationStore::register_default(&pool, &app).await.unwrap();

    assert_eq!(
        ApplicationStore::bot_user_id(&pool, ZAYDEN).await.unwrap(),
        Some(999)
    );
    assert_eq!(ApplicationStore::bot_user_id(&pool, OTHER).await.unwrap(), None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_default_application_has_no_stored_token(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();

    assert_eq!(ApplicationStore::sealed_token(&pool, ZAYDEN).await.unwrap(), None);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn one_bot_can_serve_several_guilds(pool: PgPool) {
    register(&pool, LLAMA).await;

    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();
    ApplicationStore::attach(&pool, SECOND_GUILD, LLAMA, ADMIN).await.unwrap();

    assert_eq!(ApplicationStore::guilds(&pool, LLAMA).await.unwrap(), [
        GUILD,
        SECOND_GUILD
    ]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn attaching_the_same_bot_twice_is_idempotent(pool: PgPool) {
    register(&pool, LLAMA).await;

    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();
    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();

    assert_eq!(ApplicationStore::guilds(&pool, LLAMA).await.unwrap(), [GUILD]);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_guild_takes_only_one_custom_bot_per_slot(pool: PgPool) {
    register(&pool, LLAMA).await;
    register(&pool, OTHER).await;
    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();

    let refused = ApplicationStore::attach(&pool, GUILD, OTHER, ADMIN).await;

    assert!(matches!(
        refused,
        Err(ApplicationError::GuildTaken {
            guild_id: GUILD,
            slot: Slot::Zayden,
            application_id: LLAMA,
        })
    ));
    assert_eq!(
        ApplicationStore::guilds(&pool, OTHER).await.unwrap(),
        Vec::<i64>::new()
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn attaching_an_unknown_application_is_refused(pool: PgPool) {
    let refused = ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await;

    assert!(matches!(refused, Err(ApplicationError::UnknownApplication(LLAMA))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn purging_a_guild_detaches_its_bot(pool: PgPool) {
    register(&pool, LLAMA).await;
    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();

    sqlx::query!("DELETE FROM guilds WHERE id = $1", GUILD)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        ApplicationStore::guilds(&pool, LLAMA).await.unwrap(),
        Vec::<i64>::new()
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn detaching_reports_whether_anything_was_attached(pool: PgPool) {
    register(&pool, LLAMA).await;
    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();

    assert!(ApplicationStore::detach(&pool, GUILD, Slot::Zayden).await.unwrap());
    assert!(!ApplicationStore::detach(&pool, GUILD, Slot::Zayden).await.unwrap());
    assert_eq!(
        ApplicationStore::guilds(&pool, LLAMA).await.unwrap(),
        Vec::<i64>::new()
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn registering_a_default_twice_updates_its_name(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Renamed"))
        .await
        .unwrap();

    let name = sqlx::query_scalar!(
        "SELECT name FROM applications WHERE application_id = $1",
        ZAYDEN
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(name, "Renamed");
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_slot_takes_only_one_default(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();

    let refused = ApplicationStore::register_default(
        &pool,
        &default_app(OTHER_DEFAULT, "Other"),
    )
    .await;

    assert!(matches!(refused, Err(ApplicationError::SlotTaken(Slot::Zayden))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_default_cannot_overwrite_a_custom_application(pool: PgPool) {
    register(&pool, LLAMA).await;

    let refused =
        ApplicationStore::register_default(&pool, &default_app(LLAMA, "LlamaBot"))
            .await;

    assert!(matches!(refused, Err(ApplicationError::NotDefault(LLAMA))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_custom_upsert_cannot_overwrite_a_default_application(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();

    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let sealed = keys.seal(ZAYDEN, &SecretString::from(TOKEN.to_owned())).unwrap();
    let app = NewCustomApplication {
        app: default_app(ZAYDEN, "Zayden"),
        public_key: "public",
        registered_by: ADMIN,
    };
    let refused = ApplicationStore::upsert_custom(&pool, &app, &sealed).await;

    assert!(matches!(refused, Err(ApplicationError::NotCustom(ZAYDEN))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_default_application_publishes_its_token_once(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let token = SecretString::from(TOKEN.to_owned());

    assert!(
        ApplicationStore::publish_default_token(&pool, &keys, ZAYDEN, &token)
            .await
            .unwrap()
    );
    assert!(
        !ApplicationStore::publish_default_token(&pool, &keys, ZAYDEN, &token)
            .await
            .unwrap()
    );
    assert_eq!(
        ApplicationStore::open_token(&pool, &keys, ZAYDEN)
            .await
            .unwrap()
            .expose_secret(),
        TOKEN
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_changed_token_is_published_again(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let token = SecretString::from(TOKEN.to_owned());
    let changed = SecretString::from("another.fake.token".to_owned());

    ApplicationStore::publish_default_token(&pool, &keys, ZAYDEN, &token)
        .await
        .unwrap();

    assert!(
        ApplicationStore::publish_default_token(&pool, &keys, ZAYDEN, &changed)
            .await
            .unwrap()
    );
    assert_eq!(
        ApplicationStore::open_token(&pool, &keys, ZAYDEN)
            .await
            .unwrap()
            .expose_secret(),
        "another.fake.token"
    );
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_key_rotation_reseals_the_token(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    let token = SecretString::from(TOKEN.to_owned());
    let first = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let rotated = Keyring::parse(&format!("1:{KEY},2:{SECOND_KEY}"), "2").unwrap();

    ApplicationStore::publish_default_token(&pool, &first, ZAYDEN, &token)
        .await
        .unwrap();

    assert!(
        ApplicationStore::publish_default_token(&pool, &rotated, ZAYDEN, &token)
            .await
            .unwrap()
    );
    let sealed =
        ApplicationStore::sealed_token(&pool, ZAYDEN).await.unwrap().unwrap();
    assert_eq!(sealed.key_id, 2);
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_custom_application_refuses_a_default_token_write(pool: PgPool) {
    register(&pool, LLAMA).await;
    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();
    let token = SecretString::from("another.fake.token".to_owned());

    let refused =
        ApplicationStore::publish_default_token(&pool, &keys, LLAMA, &token).await;

    assert!(matches!(refused, Err(ApplicationError::NotDefault(LLAMA))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn opening_a_default_without_a_token_is_no_token(pool: PgPool) {
    ApplicationStore::register_default(&pool, &default_app(ZAYDEN, "Zayden"))
        .await
        .unwrap();
    let keys = Keyring::parse(&format!("1:{KEY}"), "1").unwrap();

    let refused = ApplicationStore::open_token(&pool, &keys, ZAYDEN).await;

    assert!(matches!(refused, Err(ApplicationError::NoToken(ZAYDEN))));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn change_notifications_carry_ids_and_never_the_token(pool: PgPool) {
    register(&pool, LLAMA).await;
    ApplicationStore::attach(&pool, GUILD, LLAMA, ADMIN).await.unwrap();

    let mut listener = PgListener::connect_with(&pool).await.unwrap();
    listener.listen_all(["custom_bots_changed", "serving_changed"]).await.unwrap();

    ApplicationStore::set_status(&pool, LLAMA, ApplicationStatus::Revoked)
        .await
        .unwrap();

    let mut received = Vec::new();
    for _ in 0..2 {
        let notification = listener.recv().await.unwrap();
        assert!(!notification.payload().contains(TOKEN));
        received.push((
            notification.channel().to_owned(),
            notification.payload().to_owned(),
        ));
    }
    received.sort();

    assert_eq!(received, [
        ("custom_bots_changed".to_owned(), LLAMA.to_string()),
        ("serving_changed".to_owned(), GUILD.to_string()),
    ]);
}
