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
