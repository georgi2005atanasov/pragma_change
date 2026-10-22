use sqlx::PgPool;
use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub(crate) struct App {
    pub(crate) db: PgPool,
    pub(crate) artifacts: PathBuf,
    pub(crate) trainer_url: String,
    pub(crate) trainer_token: String,
    pub(crate) secret: [u8; 32],
    pub(crate) http: reqwest::Client,
}
pub(crate) type State = Arc<App>;
