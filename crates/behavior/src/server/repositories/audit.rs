use sqlx::{Postgres, postgres::PgArguments, query::Query};

pub(crate) fn append<'q>() -> Query<'q, Postgres, PgArguments> {
    sqlx::query("INSERT INTO behavior_audit(application_id,action,details) VALUES($1,$2,$3)")
}
