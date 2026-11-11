use jiff::Timestamp;
use sqlx::PgPool;

pub struct ApplicationModules;

impl ApplicationModules {
    pub async fn list(
        pool: &PgPool,
        application_id: i64,
    ) -> sqlx::Result<Vec<String>> {
        sqlx::query_scalar!(
            "SELECT module FROM application_modules
             WHERE application_id = $1 ORDER BY module",
            application_id
        )
        .fetch_all(pool)
        .await
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRow {
    pub process: String,
    pub application_id: Option<i64>,
    pub name: Option<String>,
    pub build_sha: String,
    pub started_at: Timestamp,
    pub last_seen: Timestamp,
}

pub struct ProcessStatus;

impl ProcessStatus {
    pub async fn list(pool: &PgPool) -> sqlx::Result<Vec<ProcessRow>> {
        let rows = sqlx::query!(
            r#"SELECT s.process, s.application_id, a.name AS "name?",
                      s.build_sha,
                      s.started_at AS "started_at: jiff_sqlx::Timestamp",
                      s.last_seen AS "last_seen: jiff_sqlx::Timestamp"
               FROM application_status s
               LEFT JOIN applications a ON a.application_id = s.application_id
               ORDER BY s.application_id IS NULL, a.name, s.process"#
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| ProcessRow {
                process: row.process,
                application_id: row.application_id,
                name: row.name,
                build_sha: row.build_sha,
                started_at: row.started_at.to_jiff(),
                last_seen: row.last_seen.to_jiff(),
            })
            .collect())
    }

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

    pub async fn seen(
        pool: &PgPool,
        process: &str,
        application_id: Option<i64>,
        build_sha: &str,
    ) -> sqlx::Result<()> {
        sqlx::query!(
            "INSERT INTO application_status
                 (process, application_id, build_sha, started_at, last_seen)
             VALUES ($1, $2, $3, now(), now())
             ON CONFLICT (process) DO UPDATE SET last_seen = now()",
            process,
            application_id,
            build_sha
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
