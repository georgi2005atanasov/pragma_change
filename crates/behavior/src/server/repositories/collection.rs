use sqlx::{
    FromRow, Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryScalar},
};

pub(crate) fn lock_application_shared<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT pg_advisory_xact_lock_shared(hashtextextended($1,1))")
}

pub(crate) fn lock_partition<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
}

pub(crate) fn event_hash<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT content_hash FROM behavior_events WHERE application_id=$1 AND id=$2")
}

pub(crate) fn attempt_context<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT context FROM behavior_decisions WHERE application_id=$1 AND event_id=$2",
    )
}

pub(crate) fn history_for_update<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT history FROM behavior_state WHERE application_id=$1 AND partition_key=$2 FOR UPDATE",
    )
}

pub(crate) fn save_history<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_state(application_id,partition_key,history) VALUES($1,$2,$3) ON CONFLICT(application_id,partition_key) DO UPDATE SET history=$3,updated_at=now()",
    )
}

pub(crate) fn insert_event<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_events(application_id,id,partition_key,content_hash,event,timestamp_ms) VALUES($1,$2,$3,$4,$5,$6)",
    )
}

pub(crate) fn decision<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "SELECT content_hash,decision FROM behavior_decisions WHERE application_id=$1 AND event_id=$2",
    )
}

pub(crate) fn history<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT history FROM behavior_state WHERE application_id=$1 AND partition_key=$2",
    )
}

pub(crate) fn insert_decision<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_decisions(application_id,event_id,content_hash,decision,context) VALUES($1,$2,$3,$4,$5)",
    )
}

pub(crate) fn quarantine<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("UPDATE behavior_events SET quarantined=true WHERE application_id=$1 AND id=$2")
}
