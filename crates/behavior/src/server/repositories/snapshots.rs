use sqlx::{
    FromRow, Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryScalar},
};

pub(crate) fn identity<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "SELECT application_id,content_hash,jsonb_array_length(events) AS count FROM behavior_snapshots WHERE id=$1",
    )
}

pub(crate) fn retained_events<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT event FROM behavior_events WHERE application_id=$1 AND NOT quarantined ORDER BY timestamp_ms,(event->>'sequence_number')::numeric NULLS LAST,id LIMIT 100001",
    )
}

pub(crate) fn insert<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_snapshots(id,application_id,content_hash,events) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO NOTHING",
    )
}

pub(crate) fn list<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(s)),'[]') FROM (SELECT id,application_id,content_hash,jsonb_array_length(events) AS event_count,created_at FROM behavior_snapshots ORDER BY created_at DESC LIMIT 200) s",
    )
}
