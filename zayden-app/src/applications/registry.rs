use sqlx::PgPool;

pub struct ApplicationModules;

impl ApplicationModules {
    pub async fn replace(
        pool: &PgPool,
        application_id: i64,
        modules: &[String],
    ) -> sqlx::Result<()> {
        let mut tx = pool.begin().await?;

        sqlx::query!(
            "DELETE FROM application_modules
             WHERE application_id = $1 AND module <> ALL($2::text[])",
            application_id,
            modules
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "INSERT INTO application_modules (application_id, module)
             SELECT $1, m FROM UNNEST($2::text[]) AS t (m)
             ON CONFLICT DO NOTHING",
            application_id,
            modules
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await
    }
}

pub struct ProcessStatus;

impl ProcessStatus {
    pub async fn started(
        pool: &PgPool,
        process: &str,
        application_id: Option<i64>,
        build_sha: &str,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO application_status
                 (process, application_id, build_sha, started_at, last_seen)
             VALUES ($1, $2, $3, now(), now())
             ON CONFLICT (process) DO UPDATE
             SET application_id = EXCLUDED.application_id,
                 build_sha = EXCLUDED.build_sha,
                 started_at = now(),
                 last_seen = now()",
            process,
            application_id,
            build_sha
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn seen(pool: &PgPool, process: &str) -> sqlx::Result<()> {
        sqlx::query!(
            "UPDATE application_status SET last_seen = now() WHERE process = $1",
            process
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
