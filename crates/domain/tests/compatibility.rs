use pragma_domain::Pipeline;

#[test]
fn example_pipeline_preserves_serialization_and_schema_hash() {
    let pipeline: Pipeline =
        serde_json::from_str(include_str!("../../../examples/pipeline.json")).unwrap();
    pipeline.validate().unwrap();
    // Captured from the original implementation before the module split.
    assert_eq!(
        pipeline.schema_hash(),
        "91ce684d10b524cb374d18ef43355c48414ac8336249774b05c507e4a3be809b"
    );
    assert_eq!(
        serde_json::to_string(&pipeline).unwrap(),
        include_str!("fixtures/pipeline.json").trim_end()
    );
}
