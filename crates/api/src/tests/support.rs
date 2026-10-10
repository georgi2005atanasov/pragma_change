use crate::state::{App, State};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::sync::Arc;
use uuid::Uuid;

pub(super) struct TestContext {
    pub app: State,
    admin: PgPool,
    schema: String,
}

impl TestContext {
    pub async fn new() -> Option<Self> {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("TEST_DATABASE_URL absent: database test skipped");
            return None;
        };
        let admin = PgPoolOptions::new().connect(&url).await.unwrap();
        let schema = format!("refactor_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let options = url
            .parse::<PgConnectOptions>()
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let db = PgPoolOptions::new().connect_with(options).await.unwrap();
        sqlx::migrate!().run(&db).await.unwrap();
        let artifacts = std::env::temp_dir().join(&schema);
        tokio::fs::create_dir_all(&artifacts).await.unwrap();
        Some(Self {
            app: Arc::new(App {
                db,
                artifacts,
                trainer_url: "http://127.0.0.1:1".into(),
                trainer_token: "test".into(),
                secret: [0; 32],
                http: reqwest::Client::new(),
            }),
            admin,
            schema,
        })
    }

    pub async fn cleanup(self) {
        self.app.db.close().await;
        sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
        tokio::fs::remove_dir_all(&self.app.artifacts)
            .await
            .unwrap();
    }
}
