use std::fmt;
use std::mem::size_of;

use sha2::{Digest, Sha256};
use zephium_core::extensions::{ExtensionManifestResourceDigest, ExtensionOverrideTarget};

use crate::{
    ChromiumExtensionId, PortableRelativePath, MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES,
    MAX_EXTENSION_RESOURCE_PATTERN_BYTES,
};

const RESOURCE_PLAN_DOMAIN: &[u8] = b"zephium:extension-manifest-resource-plan:v1\0";

/// One exact file in the authenticated package tree used by a manifest surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifestResource {
    path: PortableRelativePath,
    length: u64,
    sha256: [u8; 32],
}

impl ExtensionManifestResource {
    pub(crate) const fn new(path: PortableRelativePath, length: u64, sha256: [u8; 32]) -> Self {
        Self {
            path,
            length,
            sha256,
        }
    }

    /// Returns the canonical package-relative path.
    pub const fn path(&self) -> &PortableRelativePath {
        &self.path
    }

    /// Returns the exact indexed file length.
    pub const fn length(&self) -> u64 {
        self.length
    }

    /// Returns SHA-256 of the exact indexed file bytes.
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    pub(crate) fn update_digest(&self, digest: &mut Sha256) {
        update_bytes(digest, self.path.as_str().as_bytes());
        digest.update(self.length.to_be_bytes());
        digest.update(self.sha256);
    }

    fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.path.as_str().len())
    }
}

/// One manifest icon with its exact declared pixel-size selector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifestIcon {
    size: Option<u16>,
    resource: ExtensionManifestResource,
}

impl ExtensionManifestIcon {
    pub(crate) const fn new(size: Option<u16>, resource: ExtensionManifestResource) -> Self {
        Self { size, resource }
    }

    /// Returns the declared pixel size, or `None` for action shorthand syntax.
    pub const fn size(&self) -> Option<u16> {
        self.size
    }

    /// Returns the exact authenticated raster resource.
    pub const fn resource(&self) -> &ExtensionManifestResource {
        &self.resource
    }

    pub(crate) fn update_digest(&self, digest: &mut Sha256) {
        match self.size {
            None => digest.update([0]),
            Some(size) => {
                digest.update([1]);
                digest.update(size.to_be_bytes());
            }
        }
        self.resource.update_digest(digest);
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.resource.path().as_str().len())
    }
}

/// Ordered JS and CSS resources for one static content-script declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionContentScriptResources {
    javascript: Box<[ExtensionManifestResource]>,
    css: Box<[ExtensionManifestResource]>,
    include_globs: Box<[Box<str>]>,
    exclude_globs: Box<[Box<str>]>,
}

impl ExtensionContentScriptResources {
    pub(crate) fn new(
        javascript: Vec<ExtensionManifestResource>,
        css: Vec<ExtensionManifestResource>,
        include_globs: Vec<String>,
        exclude_globs: Vec<String>,
    ) -> Self {
        Self {
            javascript: javascript.into_boxed_slice(),
            css: css.into_boxed_slice(),
            include_globs: include_globs
                .into_iter()
                .map(String::into_boxed_str)
                .collect(),
            exclude_globs: exclude_globs
                .into_iter()
                .map(String::into_boxed_str)
                .collect(),
        }
    }

    /// Returns JS files in manifest injection order.
    pub fn javascript(&self) -> &[ExtensionManifestResource] {
        &self.javascript
    }

    /// Returns CSS files in manifest injection order.
    pub fn css(&self) -> &[ExtensionManifestResource] {
        &self.css
    }

    /// Returns secondary include globs in manifest order.
    pub fn include_globs(&self) -> &[Box<str>] {
        &self.include_globs
    }

    /// Returns secondary exclude globs in manifest order.
    pub fn exclude_globs(&self) -> &[Box<str>] {
        &self.exclude_globs
    }

    pub(crate) fn update_digest(&self, digest: &mut Sha256) {
        update_resources(digest, &self.javascript);
        update_resources(digest, &self.css);
        update_len(digest, self.include_globs.len());
        for glob in &self.include_globs {
            update_bytes(digest, glob.as_bytes());
        }
        update_len(digest, self.exclude_globs.len());
        for glob in &self.exclude_globs {
            update_bytes(digest, glob.as_bytes());
        }
    }

    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.javascript
                    .iter()
                    .map(ExtensionManifestResource::retained_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(
                self.css
                    .iter()
                    .map(ExtensionManifestResource::retained_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(
                self.include_globs
                    .iter()
                    .map(|value| size_of::<Box<str>>() + value.len())
                    .sum::<usize>(),
            )
            .saturating_add(
                self.exclude_globs
                    .iter()
                    .map(|value| size_of::<Box<str>>() + value.len())
                    .sum::<usize>(),
            )
    }
}

/// One extension-page override and its exact authenticated resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionOverrideResource {
    target: ExtensionOverrideTarget,
    resource: ExtensionManifestResource,
}

impl ExtensionOverrideResource {
    pub(crate) const fn new(
        target: ExtensionOverrideTarget,
        resource: ExtensionManifestResource,
    ) -> Self {
        Self { target, resource }
    }

    /// Returns the overridden browser surface.
    pub const fn target(&self) -> ExtensionOverrideTarget {
        self.target
    }

    /// Returns the exact extension page resource.
    pub const fn resource(&self) -> &ExtensionManifestResource {
        &self.resource
    }

    fn update_digest(&self, digest: &mut Sha256) {
        digest.update([match self.target {
            ExtensionOverrideTarget::NewTab => 1,
            ExtensionOverrideTarget::Bookmarks => 2,
            ExtensionOverrideTarget::History => 3,
        }]);
        self.resource.update_digest(digest);
    }
}

/// Canonical web-accessible resource pattern retained exactly as declared.
///
/// A single leading slash has the same extension-root meaning Chromium gives
/// it and is retained in `as_str`; `canonical_pattern` removes that alias for
/// later matching. Only `*` has wildcard meaning. Package admission validates
/// every non-wildcard path component with the portable-path grammar.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionDeclaredResourcePattern {
    declared: Box<str>,
    canonical: Box<str>,
}

impl ExtensionDeclaredResourcePattern {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > MAX_EXTENSION_RESOURCE_PATTERN_BYTES
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return None;
        }
        let canonical = value.strip_prefix('/').unwrap_or(value);
        if canonical.is_empty() || canonical.starts_with('/') {
            return None;
        }
        let portable_probe = canonical.replace('*', "a");
        PortableRelativePath::parse(&portable_probe).ok()?;
        Some(Self {
            declared: value.into(),
            canonical: canonical.into(),
        })
    }

    /// Returns the exact source spelling bound by the manifest.
    pub fn as_str(&self) -> &str {
        &self.declared
    }

    /// Returns the extension-root-relative pattern with its leading-slash alias removed.
    pub fn canonical_pattern(&self) -> &str {
        &self.canonical
    }

    /// Returns whether this declaration contains a wildcard.
    pub fn contains_wildcard(&self) -> bool {
        self.canonical.contains('*')
    }

    fn update_digest(&self, digest: &mut Sha256) {
        update_bytes(digest, self.declared.as_bytes());
        update_bytes(digest, self.canonical.as_bytes());
    }

    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(self.declared.len())
            .saturating_add(self.canonical.len())
    }
}

impl fmt::Debug for ExtensionDeclaredResourcePattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ExtensionDeclaredResourcePattern")
            .field(&self.as_str())
            .finish()
    }
}

/// One explicitly admitted extension audience for web-accessible resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionWebAccessibleAudience {
    /// One canonical Chromium extension identifier.
    ChromiumExtension(ChromiumExtensionId),
}

impl ExtensionWebAccessibleAudience {
    fn update_digest(&self, digest: &mut Sha256) {
        match self {
            Self::ChromiumExtension(id) => {
                digest.update([1]);
                update_bytes(digest, id.as_str().as_bytes());
            }
        }
    }

    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            + match self {
                Self::ChromiumExtension(id) => id.as_str().len(),
            }
    }
}

/// Exact resources and extension audiences for one web-accessible group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionWebAccessibleResourceGroup {
    resources: Box<[ExtensionDeclaredResourcePattern]>,
    extension_ids: Box<[ExtensionWebAccessibleAudience]>,
    use_dynamic_url: bool,
}

impl ExtensionWebAccessibleResourceGroup {
    pub(crate) fn new(
        resources: Vec<ExtensionDeclaredResourcePattern>,
        extension_ids: Vec<ExtensionWebAccessibleAudience>,
        use_dynamic_url: bool,
    ) -> Self {
        Self {
            resources: resources.into_boxed_slice(),
            extension_ids: extension_ids.into_boxed_slice(),
            use_dynamic_url,
        }
    }

    /// Returns resource patterns in manifest order.
    pub fn resources(&self) -> &[ExtensionDeclaredResourcePattern] {
        &self.resources
    }

    /// Returns canonical extension-id audiences in manifest order.
    pub fn extension_ids(&self) -> &[ExtensionWebAccessibleAudience] {
        &self.extension_ids
    }

    /// Returns whether Chromium-style per-session dynamic URLs were requested.
    pub const fn use_dynamic_url(&self) -> bool {
        self.use_dynamic_url
    }

    pub(crate) fn update_digest(&self, digest: &mut Sha256) {
        update_len(digest, self.resources.len());
        for resource in &self.resources {
            resource.update_digest(digest);
        }
        update_len(digest, self.extension_ids.len());
        for extension_id in &self.extension_ids {
            extension_id.update_digest(digest);
        }
        digest.update([u8::from(self.use_dynamic_url)]);
    }

    fn retained_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.resources
                    .iter()
                    .map(ExtensionDeclaredResourcePattern::retained_bytes)
                    .sum::<usize>(),
            )
            .saturating_add(
                self.extension_ids
                    .iter()
                    .map(ExtensionWebAccessibleAudience::retained_bytes)
                    .sum::<usize>(),
            )
    }
}

/// Complete authenticated path plan paired with a path-free core descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionManifestResourcePlan {
    content_scripts: Box<[ExtensionContentScriptResources]>,
    background_worker: Option<ExtensionManifestResource>,
    action_popup: Option<ExtensionManifestResource>,
    action_icons: Box<[ExtensionManifestIcon]>,
    overrides: Box<[ExtensionOverrideResource]>,
    sandbox_pages: Box<[ExtensionManifestResource]>,
    web_accessible_resources: Box<[ExtensionWebAccessibleResourceGroup]>,
    auxiliary_resources: Box<[ExtensionManifestResource]>,
    extension_pages_csp: Box<str>,
    sandbox_csp: Option<Box<str>>,
    digest: ExtensionManifestResourceDigest,
    retained_bytes: usize,
}

impl ExtensionManifestResourcePlan {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        content_scripts: Vec<ExtensionContentScriptResources>,
        background_worker: Option<ExtensionManifestResource>,
        action_popup: Option<ExtensionManifestResource>,
        action_icons: Vec<ExtensionManifestIcon>,
        overrides: Vec<ExtensionOverrideResource>,
        sandbox_pages: Vec<ExtensionManifestResource>,
        web_accessible_resources: Vec<ExtensionWebAccessibleResourceGroup>,
        auxiliary_resources: Vec<ExtensionManifestResource>,
        extension_pages_csp: Box<str>,
        sandbox_csp: Option<Box<str>>,
    ) -> Option<Self> {
        let content_scripts = content_scripts.into_boxed_slice();
        let action_icons = action_icons.into_boxed_slice();
        let overrides = overrides.into_boxed_slice();
        let sandbox_pages = sandbox_pages.into_boxed_slice();
        let web_accessible_resources = web_accessible_resources.into_boxed_slice();
        let auxiliary_resources = auxiliary_resources.into_boxed_slice();

        let mut retained_bytes = size_of::<Self>();
        retained_bytes = retained_bytes.checked_add(
            content_scripts
                .iter()
                .map(ExtensionContentScriptResources::retained_bytes)
                .sum::<usize>(),
        )?;
        retained_bytes = retained_bytes.checked_add(extension_pages_csp.len())?;
        retained_bytes =
            retained_bytes.checked_add(sandbox_csp.as_ref().map_or(0, |policy| policy.len()))?;
        retained_bytes = retained_bytes.checked_add(
            background_worker
                .as_ref()
                .map_or(0, ExtensionManifestResource::retained_bytes),
        )?;
        retained_bytes = retained_bytes.checked_add(
            action_popup
                .as_ref()
                .map_or(0, ExtensionManifestResource::retained_bytes),
        )?;
        retained_bytes = retained_bytes.checked_add(
            action_icons
                .iter()
                .map(ExtensionManifestIcon::retained_bytes)
                .sum::<usize>(),
        )?;
        retained_bytes = retained_bytes.checked_add(
            overrides
                .iter()
                .map(|value| {
                    size_of::<ExtensionOverrideResource>() + value.resource.path.as_str().len()
                })
                .sum::<usize>(),
        )?;
        retained_bytes = retained_bytes.checked_add(
            sandbox_pages
                .iter()
                .map(ExtensionManifestResource::retained_bytes)
                .sum::<usize>(),
        )?;
        retained_bytes = retained_bytes.checked_add(
            web_accessible_resources
                .iter()
                .map(ExtensionWebAccessibleResourceGroup::retained_bytes)
                .sum::<usize>(),
        )?;
        retained_bytes = retained_bytes.checked_add(
            auxiliary_resources
                .iter()
                .map(ExtensionManifestResource::retained_bytes)
                .sum::<usize>(),
        )?;
        if retained_bytes > MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES {
            return None;
        }

        let mut digest = Sha256::new();
        digest.update(RESOURCE_PLAN_DOMAIN);
        update_len(&mut digest, content_scripts.len());
        for resources in &content_scripts {
            resources.update_digest(&mut digest);
        }
        match &background_worker {
            None => digest.update([0]),
            Some(resource) => {
                digest.update([1]);
                resource.update_digest(&mut digest);
            }
        }
        match &action_popup {
            None => digest.update([0]),
            Some(resource) => {
                digest.update([1]);
                resource.update_digest(&mut digest);
            }
        }
        update_len(&mut digest, action_icons.len());
        for icon in &action_icons {
            icon.update_digest(&mut digest);
        }
        update_len(&mut digest, overrides.len());
        for value in &overrides {
            value.update_digest(&mut digest);
        }
        update_resources(&mut digest, &sandbox_pages);
        update_len(&mut digest, web_accessible_resources.len());
        for resources in &web_accessible_resources {
            resources.update_digest(&mut digest);
        }
        update_resources(&mut digest, &auxiliary_resources);
        update_bytes(&mut digest, extension_pages_csp.as_bytes());
        match &sandbox_csp {
            None => digest.update([0]),
            Some(policy) => {
                digest.update([1]);
                update_bytes(&mut digest, policy.as_bytes());
            }
        }

        Some(Self {
            content_scripts,
            background_worker,
            action_popup,
            action_icons,
            overrides,
            sandbox_pages,
            web_accessible_resources,
            auxiliary_resources,
            extension_pages_csp,
            sandbox_csp,
            digest: ExtensionManifestResourceDigest::from_bytes(digest.finalize().into()),
            retained_bytes,
        })
    }

    /// Returns content-script file groups in manifest declaration order.
    pub fn content_scripts(&self) -> &[ExtensionContentScriptResources] {
        &self.content_scripts
    }

    /// Returns the exact background service-worker resource when declared.
    pub const fn background_worker(&self) -> Option<&ExtensionManifestResource> {
        self.background_worker.as_ref()
    }

    /// Returns the exact action popup resource when declared.
    pub const fn action_popup(&self) -> Option<&ExtensionManifestResource> {
        self.action_popup.as_ref()
    }

    /// Returns action icons with their exact manifest size selectors.
    pub fn action_icons(&self) -> &[ExtensionManifestIcon] {
        &self.action_icons
    }

    /// Returns extension-page override resources in canonical target order.
    pub fn overrides(&self) -> &[ExtensionOverrideResource] {
        &self.overrides
    }

    /// Returns exact sandbox page resources in manifest order.
    pub fn sandbox_pages(&self) -> &[ExtensionManifestResource] {
        &self.sandbox_pages
    }

    /// Returns web-accessible groups in manifest declaration order.
    pub fn web_accessible_resources(&self) -> &[ExtensionWebAccessibleResourceGroup] {
        &self.web_accessible_resources
    }

    /// Returns paths retained from blocking, not-yet-modeled UI declarations.
    pub fn auxiliary_resources(&self) -> &[ExtensionManifestResource] {
        &self.auxiliary_resources
    }

    /// Returns the canonical effective extension-pages CSP for runtime enforcement.
    pub fn extension_pages_csp(&self) -> &str {
        &self.extension_pages_csp
    }

    /// Returns the canonical effective sandbox CSP when sandbox pages exist.
    pub fn sandbox_csp(&self) -> Option<&str> {
        self.sandbox_csp.as_deref()
    }

    /// Returns the deterministic digest of the complete resource plan.
    pub const fn digest(&self) -> ExtensionManifestResourceDigest {
        self.digest
    }

    /// Returns the conservative retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

pub(crate) fn digest_resources(
    javascript: &[ExtensionManifestResource],
    css: &[ExtensionManifestResource],
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"zephium:extension-content-script-resources:v1\0");
    update_resources(&mut digest, javascript);
    update_resources(&mut digest, css);
    digest.finalize().into()
}

pub(crate) fn digest_strings(domain: &[u8], values: &[String]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    update_len(&mut digest, values.len());
    for value in values {
        update_bytes(&mut digest, value.as_bytes());
    }
    digest.finalize().into()
}

fn update_resources(digest: &mut Sha256, resources: &[ExtensionManifestResource]) {
    update_len(digest, resources.len());
    for resource in resources {
        resource.update_digest(digest);
    }
}

fn update_len(digest: &mut Sha256, length: usize) {
    digest.update((length as u64).to_be_bytes());
}

fn update_bytes(digest: &mut Sha256, value: &[u8]) {
    update_len(digest, value.len());
    digest.update(value);
}
