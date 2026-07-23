use super::*;

#[test]
fn native_resource_ceiling_counts_spare_and_every_cleanup_debt() {
    assert_eq!(owned_native_view_resources(32, false, 0), Some(32));
    assert_eq!(owned_native_view_resources(32, true, 0), Some(33));
    assert_eq!(owned_native_view_resources(32, true, 8), Some(41));
    assert_eq!(owned_native_view_resources(usize::MAX, true, 0), None);
}

#[test]
fn native_construction_reservation_is_bounded_and_released_exactly() {
    let mut reservations = NativeViewReservations::default();
    assert_eq!(
        reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
        Ok(true)
    );
    assert_eq!(reservations.in_construction(), 1);
    // Re-entry/retry observes the first construction reservation and may
    // not allocate the forty-ninth native resource.
    assert_eq!(
        reservations.try_reserve(MAX_NATIVE_VIEW_RESOURCES - 1),
        Ok(false)
    );
    assert_eq!(reservations.in_construction(), 1);
    assert_eq!(reservations.release(), Ok(()));
    assert_eq!(reservations.in_construction(), 0);
    assert_eq!(reservations.release(), Err(()));
}

#[test]
fn explicit_content_policy_brackets_every_first_native_navigation() {
    let file = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/host/construction.rs"
    ));
    let source = file
        .split_once("fn build_view_inner(")
        .expect("construction lost build_view_inner")
        .1
        .split_once("\nfn to_wry(")
        .expect("construction lost build_view_inner's end")
        .0;
    let policy_gate = source
        .find("let Some(content_policy) = self.applied_content_policy")
        .expect("construction lost the explicit profile-policy gate");
    let builder = source
        .find("WebViewBuilder::new")
        .expect("construction lost its native builder");
    assert!(
        policy_gate < builder,
        "policy absence must reject before allocating a native view"
    );

    let last_native_hardening = source
        .rfind("crate::platform::imp::configure(")
        .expect("construction lost platform hardening");
    let policy_install = source
        .find("install_content_policy_on_view(&view, &content_policy)")
        .expect("construction lost native content-policy installation");
    let first_load = source
        .find("view.load_url(url)")
        .expect("construction lost its explicit first navigation");
    assert!(
        last_native_hardening < policy_install && policy_install < first_load,
        "native policy must install after hardening and before first navigation"
    );
}
