use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;

use super::{ApplicationError, Keyring, SealedToken, Slot};

const DEFAULT_SLOT_INDEX: &str = "applications_default_slot_idx";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationStatus {
    Active,
    Revoked,
}

impl ApplicationStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NewApplication<'a> {
    pub application_id: i64,
    pub slot: Slot,
    pub bot_user_id: i64,
    pub name: &'a str,
    pub avatar: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct NewCustomApplication<'a> {
    pub app: NewApplication<'a>,
    pub public_key: &'a str,
    pub registered_by: i64,
}

pub struct ApplicationStore;

impl ApplicationStore {
    pub async fn register_default(
        pool: &PgPool,
        app: &NewApplication<'_>,
    ) -> Result<(), ApplicationError> {
        let written = sqlx::query!(
            "INSERT INTO applications
                 (application_id, slot, kind, bot_user_id, name, avatar)
             VALUES ($1, $2, 'default', $3, $4, $5)
             ON CONFLICT (application_id) DO UPDATE SET
                 bot_user_id = EXCLUDED.bot_user_id,
                 name = EXCLUDED.name,
                 avatar = EXCLUDED.avatar
             WHERE applications.kind = 'default'
               AND applications.slot = EXCLUDED.slot",
            app.application_id,
            app.slot.as_str(),
            app.bot_user_id,
            app.name,
            app.avatar,
        )
        .execute(pool)
        .await
        .map_err(|error| {
            if let sqlx::Error::Database(db) = &error
                && db.constraint() == Some(DEFAULT_SLOT_INDEX)
            {
                return ApplicationError::SlotTaken(app.slot);
            }
            ApplicationError::Sqlx(error)
        })?
        .rows_affected();

        if written == 0 {
            return Err(ApplicationError::NotDefault(app.application_id));
        }

        Ok(())
    }

    pub async fn upsert_custom(
        pool: &PgPool,
        app: &NewCustomApplication<'_>,
        sealed: &SealedToken,
    ) -> Result<(), ApplicationError> {
        let written = sqlx::query!(
            "INSERT INTO applications
                 (application_id, slot, kind, bot_user_id, name, avatar,
                  public_key, token_ct, token_nonce, key_id, registered_by)
             VALUES ($1, $2, 'custom', $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (application_id) DO UPDATE SET
                 bot_user_id = EXCLUDED.bot_user_id,
                 name = EXCLUDED.name,
                 avatar = EXCLUDED.avatar,
                 public_key = EXCLUDED.public_key,
                 token_ct = EXCLUDED.token_ct,
                 token_nonce = EXCLUDED.token_nonce,
                 key_id = EXCLUDED.key_id,
                 status = 'active',
                 last_error = NULL,
                 token_rotated_at = now()
             WHERE applications.kind = 'custom'",
            app.app.application_id,
            app.app.slot.as_str(),
            app.app.bot_user_id,
            app.app.name,
            app.app.avatar,
            app.public_key,
            sealed.ciphertext,
            sealed.nonce,
            sealed.key_id,
            app.registered_by,
        )
        .execute(pool)
        .await?
        .rows_affected();

        if written == 0 {
            return Err(ApplicationError::NotCustom(app.app.application_id));
        }

        Ok(())
    }

    pub async fn attach(
        pool: &PgPool,
        guild_id: i64,
        application_id: i64,
        attached_by: i64,
    ) -> Result<(), ApplicationError> {
        let mut tx = pool.begin().await?;

        sqlx::query!(
            "INSERT INTO guilds (id) VALUES ($1) ON CONFLICT (id) DO NOTHING",
            guild_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "INSERT INTO application_guilds
                 (guild_id, slot, application_id, attached_by)
             SELECT $1, a.slot, a.application_id, $3 FROM applications a
             WHERE a.application_id = $2 AND a.kind = 'custom'
             ON CONFLICT (guild_id, slot) DO NOTHING",
            guild_id,
            application_id,
            attached_by,
        )
        .execute(&mut *tx)
        .await?;

        let attached = sqlx::query!(
            "SELECT g.application_id, g.slot
             FROM application_guilds g
             JOIN applications a ON a.slot = g.slot
             WHERE g.guild_id = $1 AND a.application_id = $2
               AND a.kind = 'custom'",
            guild_id,
            application_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApplicationError::UnknownApplication(application_id))?;

        if attached.application_id != application_id {
            return Err(ApplicationError::GuildTaken {
                guild_id,
                slot: attached.slot.parse()?,
                application_id: attached.application_id,
            });
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn detach(
        pool: &PgPool,
        guild_id: i64,
        slot: Slot,
    ) -> Result<bool, ApplicationError> {
        let detached = sqlx::query!(
            "DELETE FROM application_guilds WHERE guild_id = $1 AND slot = $2",
            guild_id,
            slot.as_str(),
        )
        .execute(pool)
        .await?
        .rows_affected();

        Ok(detached > 0)
    }

    pub async fn guilds(
        pool: &PgPool,
        application_id: i64,
    ) -> Result<Vec<i64>, ApplicationError> {
        let guilds = sqlx::query_scalar!(
            "SELECT guild_id FROM application_guilds
             WHERE application_id = $1 ORDER BY guild_id",
            application_id
        )
        .fetch_all(pool)
        .await?;

        Ok(guilds)
    }

    pub async fn sealed_token(
        pool: &PgPool,
        application_id: i64,
    ) -> Result<Option<SealedToken>, ApplicationError> {
        let sealed = sqlx::query!(
            "SELECT key_id, token_nonce, token_ct FROM applications
             WHERE application_id = $1",
            application_id
        )
        .fetch_optional(pool)
        .await?
        .and_then(|row| {
            Some(SealedToken {
                key_id: row.key_id?,
                nonce: row.token_nonce?,
                ciphertext: row.token_ct?,
            })
        });

        Ok(sealed)
    }

    pub async fn open_token(
        pool: &PgPool,
        keyring: &Keyring,
        application_id: i64,
    ) -> Result<SecretString, ApplicationError> {
        let sealed = Self::sealed_token(pool, application_id)
            .await?
            .ok_or(ApplicationError::NoToken(application_id))?;

        keyring.open(application_id, &sealed)
    }

    pub async fn publish_default_token(
        pool: &PgPool,
        keyring: &Keyring,
        application_id: i64,
        token: &SecretString,
    ) -> Result<bool, ApplicationError> {
        if let Some(sealed) = Self::sealed_token(pool, application_id).await?
            && sealed.key_id == keyring.active_key_id()
            && let Ok(stored) = keyring.open(application_id, &sealed)
            && stored.expose_secret() == token.expose_secret()
        {
            return Ok(false);
        }

        let sealed = keyring.seal(application_id, token)?;

        let written = sqlx::query!(
            "UPDATE applications
             SET key_id = $2, token_nonce = $3, token_ct = $4,
                 token_rotated_at = now()
             WHERE application_id = $1 AND kind = 'default'",
            application_id,
            sealed.key_id,
            sealed.nonce,
            sealed.ciphertext,
        )
        .execute(pool)
        .await?
        .rows_affected();

        if written == 0 {
            return Err(ApplicationError::NotDefault(application_id));
        }

        Ok(true)
    }

    pub async fn bot_user_id(
        pool: &PgPool,
        application_id: i64,
    ) -> Result<Option<i64>, ApplicationError> {
        let bot_user_id = sqlx::query_scalar!(
            "SELECT bot_user_id FROM applications WHERE application_id = $1",
            application_id
        )
        .fetch_optional(pool)
        .await?;

        Ok(bot_user_id)
    }

    pub async fn set_status(
        pool: &PgPool,
        application_id: i64,
        status: ApplicationStatus,
    ) -> Result<(), ApplicationError> {
        sqlx::query!(
            "UPDATE applications SET status = $2 WHERE application_id = $1",
            application_id,
            status.as_str(),
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
