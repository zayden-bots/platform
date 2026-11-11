use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use jiff::{SignedDuration, Timestamp};
use sqlx::PgPool;
use zayden_app::AppError;
use zayden_app::guilds::{GuildPresence, JoinKind};
use zayden_app::modules::{
    Backing,
    MODULES,
    ModuleDef,
    ModuleStore,
    command_modules,
    validate,
};

const GUILD: i64 = 1_234_567_890;
const APP: i64 = 100;
const OTHER: i64 = 200;

const fn joined() -> Timestamp {
    Timestamp::constant(1_750_000_000, 0)
}

const fn module(introduced: Option<Timestamp>) -> ModuleDef {
    ModuleDef {
        id: "test",
        label: "Test",
        description: "",
        backing: Backing::Commands,
        introduced,
    }
}

fn tagged_catalogue() -> BTreeMap<&'static str, Vec<Cow<'static, str>>> {
    command_modules().map(|m| (m.id, vec![Cow::Borrowed(m.id)])).collect()
}

fn full_catalogue() -> Vec<&'static str> {
    MODULES.iter().map(|m| m.id).collect()
}

#[test]
fn baseline_module_is_enabled_for_every_guild() {
    assert!(module(None).default_enabled(joined()));
}

#[test]
fn module_introduced_after_the_bot_joined_is_disabled() {
    let introduced = joined() + SignedDuration::from_hours(1);
    assert!(!module(Some(introduced)).default_enabled(joined()));
}

#[test]
fn module_introduced_before_the_bot_joined_is_enabled() {
    let introduced = joined() - SignedDuration::from_hours(1);
    assert!(module(Some(introduced)).default_enabled(joined()));
}

#[test]
fn module_introduced_as_the_bot_joined_is_enabled() {
    assert!(module(Some(joined())).default_enabled(joined()));
}

#[test]
fn validate_accepts_tags_matching_the_catalogue() {
    assert!(validate(&full_catalogue(), &tagged_catalogue()).is_ok());
}

#[test]
fn validate_rejects_an_unknown_tag() {
    let mut tags = tagged_catalogue();
    tags.insert("nonexistent", vec![Cow::Borrowed("cmd")]);

    assert!(matches!(
        validate(&full_catalogue(), &tags),
        Err(AppError::UnknownModule { .. })
    ));
}

#[test]
fn validate_rejects_a_tag_on_a_settings_module() {
    let mut tags = tagged_catalogue();
    tags.insert("ai", vec![Cow::Borrowed("cmd")]);

    assert!(matches!(
        validate(&full_catalogue(), &tags),
        Err(AppError::UnknownModule { .. })
    ));
}

#[test]
fn validate_rejects_a_command_module_without_commands() {
    let mut tags = tagged_catalogue();
    tags.remove("music");

    assert!(matches!(
        validate(&full_catalogue(), &tags),
        Err(AppError::ModuleWithoutCommands("music"))
    ));
}

#[test]
fn a_bot_linking_a_subset_validates() {
    let none = BTreeMap::new();

    assert!(validate(&["ai"], &none).is_ok());
    assert!(matches!(
        validate(&["music"], &none),
        Err(AppError::ModuleWithoutCommands("music"))
    ));
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn first_seed_applies_module_defaults(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let states = ModuleStore::new(pool)
        .seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new())
        .await?;

    assert_eq!(states.len(), command_modules().count());
    for m in command_modules() {
        assert_eq!(states.get(m.id), Some(&m.default_enabled(joined())), "{}", m.id);
    }

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn first_seed_applies_imported_overrides(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let states = ModuleStore::new(pool)
        .seed(GUILD, APP, joined(), JoinKind::First, &HashSet::from(["music"]))
        .await?;

    assert_eq!(states.get("music"), Some(&false));
    assert_eq!(states.get("gambling"), Some(&true));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn guild_needs_import_until_seeded(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool);

    assert!(store.needs_import(GUILD, APP).await?);
    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    assert!(!store.needs_import(GUILD, APP).await?);

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn reseed_keeps_toggles(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool);

    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    store.set(GUILD, APP, "music", false).await?;
    let states = store
        .seed(GUILD, APP, joined(), JoinKind::Reconnect, &HashSet::new())
        .await?;

    assert_eq!(states.get("music"), Some(&false));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn reseed_ignores_imported_overrides(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool);

    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    let states = store
        .seed(GUILD, APP, joined(), JoinKind::Reconnect, &HashSet::from(["music"]))
        .await?;

    assert_eq!(states.get("music"), Some(&true));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn reinvite_resets_toggles(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool.clone());

    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    store.set(GUILD, APP, "music", false).await?;
    let rejoined = joined() + SignedDuration::from_hours(24);
    GuildPresence::left(&pool, GUILD, APP).await?;
    let kind = GuildPresence::joined(&pool, GUILD, APP, rejoined).await?;
    assert_eq!(kind, JoinKind::Rejoin);
    let states =
        store.seed(GUILD, APP, rejoined, kind, &HashSet::from(["gambling"])).await?;

    assert_eq!(states.get("music"), Some(&true));
    assert_eq!(states.get("gambling"), Some(&true));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn set_is_visible_through_the_cache(pool: PgPool) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool);

    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    store.set(GUILD, APP, "music", false).await?;
    assert_eq!(store.states(GUILD, APP).await?.get("music"), Some(&false));

    store.set(GUILD, APP, "music", true).await?;
    assert_eq!(store.states(GUILD, APP).await?.get("music"), Some(&true));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn two_applications_in_one_guild_keep_separate_switches(
    pool: PgPool,
) -> sqlx::Result<()> {
    for app in [APP, OTHER] {
        GuildPresence::joined(&pool, GUILD, app, joined()).await?;
    }
    let store = ModuleStore::new(pool);

    for app in [APP, OTHER] {
        store.seed(GUILD, app, joined(), JoinKind::First, &HashSet::new()).await?;
    }
    store.set(GUILD, APP, "music", false).await?;

    assert_eq!(store.states(GUILD, APP).await?.get("music"), Some(&false));
    assert_eq!(store.states(GUILD, OTHER).await?.get("music"), Some(&true));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn set_for_one_application_leaves_the_other_alone(
    pool: PgPool,
) -> sqlx::Result<()> {
    for app in [APP, OTHER] {
        GuildPresence::joined(&pool, GUILD, app, joined()).await?;
    }
    let store = ModuleStore::new(pool);
    for app in [APP, OTHER] {
        store.seed(GUILD, app, joined(), JoinKind::First, &HashSet::new()).await?;
        store.states(GUILD, app).await?;
    }

    let before = store.states(GUILD, OTHER).await?;

    store.set(GUILD, APP, "music", false).await?;

    assert_eq!(store.states(GUILD, OTHER).await?, before);
    assert_eq!(store.refresh(GUILD, OTHER).await?, before);

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_rejoin_resets_only_that_applications_switches(
    pool: PgPool,
) -> sqlx::Result<()> {
    for app in [APP, OTHER] {
        GuildPresence::joined(&pool, GUILD, app, joined()).await?;
    }
    let store = ModuleStore::new(pool.clone());
    for app in [APP, OTHER] {
        store.seed(GUILD, app, joined(), JoinKind::First, &HashSet::new()).await?;
        store.set(GUILD, app, "music", false).await?;
    }

    let rejoined = joined() + SignedDuration::from_hours(24);
    GuildPresence::left(&pool, GUILD, APP).await?;
    let kind = GuildPresence::joined(&pool, GUILD, APP, rejoined).await?;
    assert_eq!(kind, JoinKind::Rejoin);
    let states = store.seed(GUILD, APP, rejoined, kind, &HashSet::new()).await?;

    assert_eq!(states.get("music"), Some(&true));
    assert_eq!(store.refresh(GUILD, OTHER).await?.get("music"), Some(&false));

    Ok(())
}

#[sqlx::test(migrator = "schema::MIGRATOR")]
async fn a_reconnect_with_a_new_join_time_keeps_toggles(
    pool: PgPool,
) -> sqlx::Result<()> {
    GuildPresence::joined(&pool, GUILD, APP, joined()).await?;
    let store = ModuleStore::new(pool);

    store.seed(GUILD, APP, joined(), JoinKind::First, &HashSet::new()).await?;
    store.set(GUILD, APP, "music", false).await?;
    let other = joined() + SignedDuration::from_hours(24);
    let states =
        store.seed(GUILD, APP, other, JoinKind::Reconnect, &HashSet::new()).await?;

    assert_eq!(states.get("music"), Some(&false));

    Ok(())
}
