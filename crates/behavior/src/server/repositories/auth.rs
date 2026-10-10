use sqlx::{Postgres, postgres::PgArguments, query::Query};

pub(crate) fn credential<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("SELECT policy,active_model FROM behavior_apps WHERE id=$1 AND token_hash=$2")
}
