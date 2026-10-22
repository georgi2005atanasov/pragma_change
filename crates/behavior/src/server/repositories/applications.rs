use sqlx::{
    FromRow, Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryScalar},
};

pub(crate) fn create<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("INSERT INTO behavior_apps(id,name,token_hash,policy) VALUES($1,$2,$3,$4)")
}

pub(crate) fn list<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(a)),'[]') FROM (SELECT id,name,policy,active_model,created_at,(SELECT count(*) FROM behavior_events e WHERE e.application_id=behavior_apps.id) AS collected_events,(SELECT max(received_at) FROM behavior_events e WHERE e.application_id=behavior_apps.id) AS last_event FROM behavior_apps ORDER BY created_at DESC) a",
    )
}

pub(crate) fn set_policy<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("UPDATE behavior_apps SET policy=$2 WHERE id=$1")
}

pub(crate) fn lock_application<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,1))")
}

pub(crate) fn clear_model<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("UPDATE behavior_apps SET active_model=NULL WHERE id=$1")
}

pub(crate) fn delete_training_jobs<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "DELETE FROM behavior_training_jobs WHERE snapshot_id IN (SELECT id FROM behavior_snapshots WHERE application_id=$1)",
    )
}

pub(crate) fn recent_decisions<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT decision FROM behavior_decisions WHERE application_id=$1 ORDER BY created_at DESC LIMIT 100",
    )
}

pub(crate) fn delete_application_rows<'q>(table: &str) -> Query<'q, Postgres, PgArguments> {
    let sql = match table {
        "behavior_events" => "DELETE FROM behavior_events WHERE application_id=$1",
        "behavior_state" => "DELETE FROM behavior_state WHERE application_id=$1",
        "behavior_decisions" => "DELETE FROM behavior_decisions WHERE application_id=$1",
        "behavior_models" => "DELETE FROM behavior_models WHERE application_id=$1",
        "behavior_snapshots" => "DELETE FROM behavior_snapshots WHERE application_id=$1",
        _ => unreachable!("internal table selection"),
    };
    sqlx::query(sql)
}
