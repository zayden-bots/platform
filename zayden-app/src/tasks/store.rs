use jiff::{SignedDuration, Timestamp};
use jiff_sqlx::ToSqlx;
use sqlx::PgPool;

use super::{CLAIM_TIMEOUT, MAX_ATTEMPTS, NewTask, Task};

pub struct ScheduledTasks;

impl ScheduledTasks {
    pub async fn replace(
        pool: &PgPool,
        application_id: i64,
        key_prefix: &str,
        tasks: &[NewTask],
    ) -> sqlx::Result<()> {
        let mut tx = pool.begin().await?;

        sqlx::query!(
            "DELETE FROM scheduled_tasks
             WHERE application_id = $1 AND starts_with(key, $2)",
            application_id,
            key_prefix
        )
        .execute(&mut *tx)
        .await?;

        for task in tasks {
            #[expect(
                trivial_casts,
                reason = "not a cast: `as T` is sqlx's bind-param type-override syntax, required because TIMESTAMPTZ has no built-in jiff mapping"
            )]
            sqlx::query!(
                "INSERT INTO scheduled_tasks (application_id, kind, key, run_at, payload)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (application_id, kind, key) DO UPDATE
                 SET run_at = EXCLUDED.run_at,
                     payload = EXCLUDED.payload,
                     claimed_at = NULL,
                     attempts = 0",
                application_id,
                task.kind,
                task.key,
                task.run_at.to_sqlx() as jiff_sqlx::Timestamp,
                task.payload
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    pub async fn ensure(
        pool: &PgPool,
        application_id: i64,
        tasks: &[NewTask],
    ) -> sqlx::Result<()> {
        let mut tx = pool.begin().await?;

        for task in tasks {
            #[expect(
                trivial_casts,
                reason = "not a cast: `as T` is sqlx's bind-param type-override syntax, required because TIMESTAMPTZ has no built-in jiff mapping"
            )]
            sqlx::query!(
                "INSERT INTO scheduled_tasks (application_id, kind, key, run_at, payload)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (application_id, kind, key) DO NOTHING",
                application_id,
                task.kind,
                task.key,
                task.run_at.to_sqlx() as jiff_sqlx::Timestamp,
                task.payload
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await
    }

    pub async fn cancel(
        pool: &PgPool,
        application_id: i64,
        key_prefix: &str,
    ) -> sqlx::Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM scheduled_tasks
             WHERE application_id = $1 AND starts_with(key, $2)",
            application_id,
            key_prefix
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn purge_unclaimed(
        pool: &PgPool,
        kinds: &[&str],
        older_than: SignedDuration,
    ) -> sqlx::Result<u64> {
        let kinds: Vec<String> =
            kinds.iter().map(|kind| (*kind).to_owned()).collect();

        let result = sqlx::query!(
            "DELETE FROM scheduled_tasks
             WHERE kind = ANY($1) AND claimed_at IS NULL
               AND run_at < now() - make_interval(secs => $2)",
            &kinds,
            older_than.as_secs_f64()
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    pub async fn claim_due(
        pool: &PgPool,
        application_id: i64,
        kinds: &[&str],
        limit: i64,
    ) -> sqlx::Result<Vec<Task>> {
        let kinds: Vec<String> =
            kinds.iter().map(|kind| (*kind).to_owned()).collect();

        let rows = sqlx::query!(
            r#"UPDATE scheduled_tasks t
               SET claimed_at = now(), attempts = t.attempts + 1
               FROM (
                   SELECT id FROM scheduled_tasks
                   WHERE application_id = $1 AND kind = ANY($2)
                     AND claimed_at IS NULL
                     AND run_at <= now() AND attempts < $4
                   ORDER BY run_at
                   LIMIT $3
                   FOR UPDATE SKIP LOCKED
               ) due
               WHERE t.id = due.id
               RETURNING t.id AS "id!",
                         t.application_id AS "application_id!",
                         t.kind AS "kind!",
                         t.key AS "key!",
                         t.run_at AS "run_at!: jiff_sqlx::Timestamp",
                         t.payload AS "payload!",
                         t.claimed_at AS "claimed_at!: jiff_sqlx::Timestamp",
                         t.attempts AS "attempts!""#,
            application_id,
            &kinds,
            limit,
            MAX_ATTEMPTS
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| Task {
                id: row.id,
                application_id: row.application_id,
                kind: row.kind,
                key: row.key,
                run_at: row.run_at.to_jiff(),
                payload: row.payload,
                claimed_at: row.claimed_at.to_jiff(),
                attempts: row.attempts,
            })
            .collect())
    }

    pub async fn complete(pool: &PgPool, task: &Task) -> sqlx::Result<bool> {
        #[expect(
            trivial_casts,
            reason = "not a cast: `as T` is sqlx's bind-param type-override syntax, required because TIMESTAMPTZ has no built-in jiff mapping"
        )]
        let result = sqlx::query!(
            "DELETE FROM scheduled_tasks WHERE id = $1 AND claimed_at = $2",
            task.id,
            task.claimed_at.to_sqlx() as jiff_sqlx::Timestamp
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected() == 1)
    }

    pub async fn release_stale(
        pool: &PgPool,
        application_id: i64,
    ) -> sqlx::Result<Vec<Task>> {
        let mut tx = pool.begin().await?;
        let timeout = CLAIM_TIMEOUT.as_secs_f64();

        let rows = sqlx::query!(
            r#"DELETE FROM scheduled_tasks
               WHERE application_id = $1
                 AND claimed_at < now() - make_interval(secs => $2)
                 AND attempts >= $3
               RETURNING id AS "id!",
                         application_id AS "application_id!",
                         kind AS "kind!",
                         key AS "key!",
                         run_at AS "run_at!: jiff_sqlx::Timestamp",
                         payload AS "payload!",
                         claimed_at AS "claimed_at!: jiff_sqlx::Timestamp",
                         attempts AS "attempts!""#,
            application_id,
            timeout,
            MAX_ATTEMPTS
        )
        .fetch_all(&mut *tx)
        .await?;

        sqlx::query!(
            "UPDATE scheduled_tasks SET claimed_at = NULL
             WHERE application_id = $1
               AND claimed_at < now() - make_interval(secs => $2)",
            application_id,
            timeout
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(rows
            .into_iter()
            .map(|row| Task {
                id: row.id,
                application_id: row.application_id,
                kind: row.kind,
                key: row.key,
                run_at: row.run_at.to_jiff(),
                payload: row.payload,
                claimed_at: row.claimed_at.to_jiff(),
                attempts: row.attempts,
            })
            .collect())
    }

    pub async fn next_due(
        pool: &PgPool,
        application_id: i64,
        kinds: &[&str],
    ) -> sqlx::Result<Option<Timestamp>> {
        let kinds: Vec<String> =
            kinds.iter().map(|kind| (*kind).to_owned()).collect();

        let row = sqlx::query!(
            r#"SELECT min(run_at) AS "next: jiff_sqlx::Timestamp"
               FROM scheduled_tasks
               WHERE application_id = $1 AND kind = ANY($2)
                 AND claimed_at IS NULL AND attempts < $3"#,
            application_id,
            &kinds,
            MAX_ATTEMPTS
        )
        .fetch_one(pool)
        .await?;

        Ok(row.next.map(jiff_sqlx::Timestamp::to_jiff))
    }
}
