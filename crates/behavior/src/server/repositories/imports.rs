use sqlx::{Postgres, postgres::PgArguments, query::Query};
pub(crate) fn identity<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "SELECT application_id,source_hash,content_hash,jsonb_array_length(events) AS count FROM behavior_snapshots WHERE id=$1",
    )
}
pub(crate) fn policy<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT policy FROM behavior_apps WHERE id=$1 FOR SHARE")
}
pub(crate) fn insert<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_snapshots(id,application_id,content_hash,events,source_hash,source_manifest) VALUES($1,$2,$3,$4,$5,$6)",
    )
}
