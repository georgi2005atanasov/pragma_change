use sqlx::{
    FromRow, Postgres,
    postgres::{PgArguments, PgRow},
    query::{Query, QueryScalar},
};

pub(crate) fn expire_leases<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "UPDATE behavior_training_jobs SET status='failed',error='Training interrupted; create another job' WHERE status='running' AND lease_until<now()",
    )
}

pub(crate) fn claim<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "UPDATE behavior_training_jobs SET status='running',lease_until=now()+interval '5 minutes' WHERE id=(SELECT id FROM behavior_training_jobs WHERE status='queued' ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED) RETURNING id,snapshot_id",
    )
}

pub(crate) fn events<'q, O>() -> QueryScalar<'q, Postgres, O, PgArguments>
where
    (O,): for<'r> FromRow<'r, PgRow>,
{
    sqlx::query_scalar("SELECT events FROM behavior_snapshots WHERE id=$1")
}

pub(crate) fn complete<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "UPDATE behavior_training_jobs SET status='succeeded' WHERE id=$1 AND status='running' AND lease_until>now()",
    )
}

pub(crate) fn save_model<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "INSERT INTO behavior_models(id,application_id,snapshot_id,model) VALUES($1,$2,$3,$4)",
    )
}

pub(crate) fn fail<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query(
        "UPDATE behavior_training_jobs SET status='failed',error=$2 WHERE id=$1 AND status='running'",
    )
}

pub(crate) fn expire_data<'q>(table: &str) -> Query<'q, Postgres, PgArguments> {
    sqlx::query(match table {
        "behavior_events" => {
            "DELETE FROM behavior_events e USING behavior_apps a WHERE e.application_id=a.id AND e.received_at<now()-make_interval(days=>(a.policy->>'retention_days')::int)"
        }
        "behavior_decisions" => {
            "DELETE FROM behavior_decisions e USING behavior_apps a WHERE e.application_id=a.id AND e.created_at<now()-make_interval(days=>(a.policy->>'retention_days')::int)"
        }
        "behavior_state" => {
            "DELETE FROM behavior_state e USING behavior_apps a WHERE e.application_id=a.id AND e.updated_at<now()-make_interval(days=>(a.policy->>'retention_days')::int)"
        }
        _ => unreachable!("internal table selection"),
    })
}
