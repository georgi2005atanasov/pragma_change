use pragma_domain::{Pipeline, Release, RequestEvent};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("validate-release") => {
            let release: Release = serde_json::from_slice(&std::fs::read(
                args.get(2).ok_or("release path required")?,
            )?)?;
            release.validate()?;
            println!(
                "validated release {} ({} models, {} endpoints)",
                release.release_id,
                release.deployments.len(),
                release
                    .deployments
                    .iter()
                    .map(|d| d.pipeline.routes().count())
                    .sum::<usize>()
            );
        }
        Some("schema") => {
            let p: Pipeline = serde_json::from_slice(&std::fs::read(
                args.get(2).ok_or("pipeline path required")?,
            )?)?;
            p.validate()?;
            println!("{}", p.schema_hash());
        }
        Some("replay") => {
            let p: Pipeline = serde_json::from_slice(&std::fs::read(
                args.get(2).ok_or("pipeline path required")?,
            )?)?;
            p.validate()?;
            let contents = std::fs::read_to_string(args.get(3).ok_or("events path required")?)?;
            let mut events = contents
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(serde_json::from_str::<RequestEvent>)
                .collect::<Result<Vec<_>, _>>()?;
            events.sort_by_key(|e| e.timestamp_ms);
            let mut store = pragma_engine::MemoryStore::default();
            let secret =
                std::env::var("PARTITION_SECRET").map_err(|_| "PARTITION_SECRET is required")?;
            let key: [u8; 32] = hex::decode(secret)?
                .try_into()
                .map_err(|_| "secret requires 32 bytes")?;
            for event in events {
                println!(
                    "{}",
                    serde_json::to_string(&pragma_engine::features(&p, &event, &key, &mut store)?)?
                );
            }
        }
        _ => return Err(
            "Usage: pragma-cli validate-release RELEASE | schema PIPELINE | replay PIPELINE EVENTS"
                .into(),
        ),
    }
    Ok(())
}
