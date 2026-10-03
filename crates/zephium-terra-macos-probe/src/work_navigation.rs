//! Standalone adapter shares the actual-app qualifier's exact trusted task.
#[cfg(target_os = "macos")]
pub(super) use zephium_work_composition::navigation_qualification::OBJECTIVE;
pub(super) use zephium_work_composition::navigation_qualification::{task, verify_owned};

#[test]
fn standalone_route_selection_remains_closed() {
    for name in [
        "react-navigation/other",
        "react-navigation?redirect=1",
        "https://react.dev/learn/your-first-component",
    ] {
        assert!(super::work_sites::Site::parse(name).is_err());
    }
}
