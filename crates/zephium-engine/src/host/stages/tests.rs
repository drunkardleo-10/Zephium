use super::should_seed_stage_readiness;

#[test]
fn unchanged_layout_does_not_reseed_retained_stage_readiness() {
    assert!(should_seed_stage_readiness(true, true, true));
    assert!(!should_seed_stage_readiness(false, true, true));
    assert!(!should_seed_stage_readiness(true, false, true));
    assert!(!should_seed_stage_readiness(true, true, false));

    let source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/host/stages.rs"));
    assert!(
        source
            .matches("should_seed_stage_readiness(\n                inserted,")
            .count()
            >= 2
    );
}
