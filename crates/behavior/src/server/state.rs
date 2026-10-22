use sqlx::PgPool;
#[derive(Clone)]
pub(super) struct App {
    pub db: PgPool,
    pub admin: String,
    pub secret: Vec<u8>,
}
