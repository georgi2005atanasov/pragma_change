use sqlx::{
    FromRow, Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryScalar},
};

pub(crate) fn enqueue<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_training_jobs(id,snapshot_id) VALUES($1,$2) ON CONFLICT(id) DO NOTHING",
    )
}

pub(crate) fn job_snapshot<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT snapshot_id FROM behavior_training_jobs WHERE id=$1")
}

pub(crate) fn jobs<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(j)),'[]') FROM (SELECT * FROM behavior_training_jobs ORDER BY created_at DESC LIMIT 200) j",
    )
}

pub(crate) fn list<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(to_jsonb(m)),'[]') FROM (SELECT id,application_id,snapshot_id,created_at,model->'training_events' AS training_events,model->'calibration_events' AS calibration_events,model->'check_surprise' AS check_surprise,model->'test_events' AS test_events,model->'held_out_check_rate' AS held_out_check_rate FROM behavior_models ORDER BY created_at DESC LIMIT 200) m",
    )
}

pub(crate) fn application<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT application_id FROM behavior_models WHERE id=$1")
}

pub(crate) fn lock_application<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,1))")
}

pub(crate) fn activate<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("UPDATE behavior_apps SET active_model=$2 WHERE id=$1")
}

pub(crate) fn clear_history<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("DELETE FROM behavior_state WHERE application_id=$1")
}

pub(crate) fn snapshot_events<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT application_id,events FROM behavior_snapshots WHERE id=$1")
}

pub(crate) fn policy<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT policy FROM behavior_apps WHERE id=$1")
}

pub(crate) fn load<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT model FROM behavior_models WHERE id=$1")
}
