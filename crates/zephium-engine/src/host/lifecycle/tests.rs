use super::*;

#[test]
fn renderer_crash_classification_never_exposes_dead_spare() {
    let spare = ItemId::from(1);
    let live = ItemId::from(2);
    let retired = ItemId::from(3);
    assert_eq!(
        renderer_crash_target(Some(spare), false, spare),
        RendererCrashTarget::Spare
    );
    assert_eq!(
        renderer_crash_target(Some(spare), true, live),
        RendererCrashTarget::Live
    );
    assert_eq!(
        renderer_crash_target(Some(spare), false, retired),
        RendererCrashTarget::Retired
    );
}
