//! Create/drop only a namespaced acceptance-test schema, never reset the user's schema.
use std::os::unix::fs::PermissionsExt;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new("build/remote-lab");
    let metadata = root.join("database.json");
    let connection = std::env::var("DATABASE_URL")?;
    let db = sqlx::PgPool::connect(&connection).await?;
    if std::env::args().nth(1).as_deref() == Some("seed") {
        let metadata: serde_json::Value = serde_json::from_slice(&std::fs::read(&metadata)?)?;
        let lab = sqlx::PgPool::connect(metadata["url"].as_str().ok_or("lab URL missing")?).await?;
        let mut fixture: pragma_domain::Release =
            serde_json::from_slice(&std::fs::read("build/native-test/release.json")?)?;
        let mut ids = vec![];
        for index in 0..2 {
            fixture.release_id = uuid::Uuid::new_v4();
            fixture.validate()?;
            let configuration = serde_json::to_value(&fixture)?;
            use sha2::Digest;
            let digest = hex::encode(sha2::Sha256::digest(serde_json::to_vec(&configuration)?));
            sqlx::query(
                "INSERT INTO releases(id,name,configuration,content_hash) VALUES($1,$2,$3,$4)",
            )
            .bind(fixture.release_id)
            .bind(format!("Native acceptance fixture {index}"))
            .bind(configuration)
            .bind(digest)
            .execute(&lab)
            .await?;
            ids.push(fixture.release_id);
        }
        std::fs::write(root.join("release-ids.json"), serde_json::to_vec(&ids)?)?;
        println!("Seeded validated native acceptance fixtures in the isolated schema");
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("cleanup") {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&metadata)?)?;
        let schema = value["schema"].as_str().ok_or("missing schema")?;
        if !schema.starts_with("remote_lab_")
            || !schema
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err("invalid lab schema".into());
        }
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&db)
            .await?;
        std::fs::remove_file(metadata)?;
        println!("Removed isolated lab schema");
        return Ok(());
    }
    if metadata.exists() {
        return Err("lab database metadata already exists; clean up the previous run".into());
    }
    let schema = format!("remote_lab_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&db)
        .await?;
    let mut url = reqwest::Url::parse(&connection)?;
    url.query_pairs_mut()
        .append_pair("options", &format!("-csearch_path={schema}"));
    let value = serde_json::json!({"schema":schema,"url":url.as_str()});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&metadata)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer(file, &value)?;
    println!("Created isolated lab schema; connection saved privately");
    Ok(())
}
