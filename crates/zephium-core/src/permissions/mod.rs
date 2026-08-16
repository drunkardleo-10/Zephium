//! Durable permission authority contracts.
//!
//! Page permissions are deliberately separate from extension API, host,
//! temporary, and scheme grants. A content match pattern is eligibility, not
//! authority, and therefore does not appear in this module.

mod page;

pub use page::{
    PageOrigin, PageOriginError, PagePermissionApplyError, PagePermissionCatalog,
    PagePermissionCatalogError, PagePermissionCatalogRevision, PagePermissionChange,
    PagePermissionChangeResult, PagePermissionGrant, PagePermissionGrantRevision,
    PagePermissionKind, PagePermissionPatch, PagePermissionPatchApplication,
    PagePermissionPatchError, PagePermissionPatchResults, PagePermissionRequest,
    PagePermissionRequestId, PagePermissionRequestKind, PagePermissionRequestSettlement,
    RememberedPagePermission, MAX_PAGE_ORIGIN_BYTES, MAX_PAGE_PERMISSION_CATALOG_RETAINED_BYTES,
    MAX_PAGE_PERMISSION_GRANTS_PER_PROFILE, MAX_PAGE_PERMISSION_PATCH_CHANGES,
};
