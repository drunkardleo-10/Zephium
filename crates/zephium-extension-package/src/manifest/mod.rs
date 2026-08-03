//! Bounded admission of exact Manifest V3 package declarations.
//!
//! Parsing is deliberately separate from compatibility policy. This module
//! proves byte/tree identity, validates the closed typed subset, preserves all
//! other top-level declarations as blocking unmodeled authority, and asks a
//! product-owned policy to classify every resulting core declaration.

mod csp;
mod execution;
mod locale;
mod metadata;
mod resources;

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::mem::size_of;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zephium_core::extensions::{
    ApiPermissionName, ApiPermissionNameError, ExtensionActionDeclaration,
    ExtensionApiPermissionSet, ExtensionBackgroundDeclaration,
    ExtensionCompatibilityClassification, ExtensionCompatibilityLevel,
    ExtensionCompatibilityTargetId, ExtensionContentScriptDeclaration,
    ExtensionContentSecurityPolicyDeclaration, ExtensionHostPermissionSet,
    ExtensionManifestDeclaration, ExtensionManifestDeclarations, ExtensionManifestDescriptor,
    ExtensionManifestError, ExtensionManifestExecutionSurfaces, ExtensionManifestResourceDigest,
    ExtensionOverrideTarget, ExtensionSandboxDeclaration, ExtensionUnmodeledDeclarationName,
    ExtensionWebAccessibleResourceDeclaration, MAX_EXTENSION_API_PERMISSIONS,
    MAX_EXTENSION_CONTENT_SCRIPT_GLOBS, MAX_EXTENSION_HOST_PERMISSION_PATTERNS,
};
use zephium_core::injection::{MatchOptions, MatchSet};

use crate::{
    parse_bounded_json, BoundedJsonError, BoundedJsonLimits, ChromiumManifestKey,
    ChromiumManifestKeyError, ExtensionReleaseTreeBinding, PortableRelativePath,
    MAX_EXTENSION_CONTENT_SCRIPT_GLOB_BYTES, MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES,
};

use self::csp::parse_effective_csp;
use self::execution::{
    parse_action, parse_background, parse_content_scripts, parse_overrides, parse_sandbox,
    parse_web_accessible, ParsedAction,
};
pub use self::locale::{
    resolve_extension_default_locale, ExtensionDefaultLocaleResolutionError,
    ExtensionResolvedMetadataDigest, ResolvedExtensionManifestMetadata,
    TrustedExtensionDisplayText,
};
use self::metadata::{parse_chromium_key, parse_inert_metadata, validate_required_metadata};
pub use self::metadata::{
    ExtensionLocalizedMessageKey, ExtensionManifestMetadata, ExtensionUnresolvedDisplayText,
};
use self::resources::digest_resources;
pub use self::resources::{
    ExtensionContentScriptResources, ExtensionDeclaredResourcePattern, ExtensionManifestIcon,
    ExtensionManifestResource, ExtensionManifestResourcePlan, ExtensionOverrideResource,
    ExtensionWebAccessibleAudience, ExtensionWebAccessibleResourceGroup,
};

const ADMISSION_DIGEST_DOMAIN: &[u8] = b"zephium:extension-manifest-admission:v1\0";
const MAX_ICON_ENTRIES: usize = 64;

/// Product-owned compatibility decision source for one exact backend target.
///
/// Returning `None` fails admission. The parser never supplies a default and
/// never equates syntactic support with runtime support. Each decision receives
/// both the path-free declaration set and the exact admitted resource plan so
/// policy can inspect nested execution semantics instead of trusting an opaque
/// digest or a broad declaration tag.
pub trait ExtensionManifestCompatibilityPolicy {
    /// Returns the exact versioned compatibility target being assessed.
    fn target(&self) -> &ExtensionCompatibilityTargetId;

    /// Classifies one exact declaration, or returns `None` when policy has no decision.
    fn classify(
        &self,
        subject: ExtensionManifestCompatibilitySubject<'_>,
    ) -> Option<ExtensionCompatibilityLevel>;
}

/// Exact inspectable input for one compatibility-policy decision.
#[derive(Clone, Copy)]
pub struct ExtensionManifestCompatibilitySubject<'a> {
    declaration: &'a ExtensionManifestDeclaration,
    declarations: &'a ExtensionManifestDeclarations,
    resources: &'a ExtensionManifestResourcePlan,
}

impl<'a> ExtensionManifestCompatibilitySubject<'a> {
    /// Returns the canonical declaration key being classified.
    pub const fn declaration(self) -> &'a ExtensionManifestDeclaration {
        self.declaration
    }

    /// Returns the complete typed manifest semantics containing that key.
    pub const fn declarations(self) -> &'a ExtensionManifestDeclarations {
        self.declarations
    }

    /// Returns exact paths, globs, audiences, and effective policies paired
    /// with the path-free declarations.
    pub const fn resources(self) -> &'a ExtensionManifestResourcePlan {
        self.resources
    }
}

/// SHA-256 of exact package, policy result, and admitted resource semantics.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionManifestAdmissionDigest([u8; 32]);

impl ExtensionManifestAdmissionDigest {
    /// Returns exact digest bytes.
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows exact digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for ExtensionManifestAdmissionDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ExtensionManifestAdmissionDigest({:02x}{:02x}{:02x}{:02x}…)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

/// Exact admitted manifest, path plan, native identity evidence, and policy result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedExtensionManifest {
    descriptor: ExtensionManifestDescriptor,
    metadata: ExtensionManifestMetadata,
    resources: ExtensionManifestResourcePlan,
    chromium_key: Option<ChromiumManifestKey>,
    admission_digest: ExtensionManifestAdmissionDigest,
    retained_bytes: usize,
}

impl AdmittedExtensionManifest {
    /// Returns the complete path-free core authority and compatibility descriptor.
    pub const fn descriptor(&self) -> &ExtensionManifestDescriptor {
        &self.descriptor
    }

    /// Returns bounded UI identity and its exact icon/locale bindings.
    pub const fn metadata(&self) -> &ExtensionManifestMetadata {
        &self.metadata
    }

    /// Returns exact authenticated resource paths paired with the descriptor.
    pub const fn resources(&self) -> &ExtensionManifestResourcePlan {
        &self.resources
    }

    /// Returns the verified Chromium key when the release targets Chromium identity.
    pub const fn chromium_key(&self) -> Option<&ChromiumManifestKey> {
        self.chromium_key.as_ref()
    }

    /// Returns the deterministic admission digest.
    pub const fn admission_digest(&self) -> ExtensionManifestAdmissionDigest {
        self.admission_digest
    }

    /// Returns the conservative retained-memory charge.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

/// Stable fail-closed manifest admission reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionManifestAdmissionError {
    /// The shared duplicate-key-safe JSON boundary rejected source bytes.
    Json(BoundedJsonError),
    /// Exact length or SHA-256 differs from the bound tree's root manifest.
    ManifestBindingMismatch,
    /// The JSON root is not an object.
    RootNotObject,
    /// A required top-level or nested field is absent.
    MissingField(Box<str>),
    /// A field has the wrong type, shape, value, or an unknown nested key.
    InvalidField(Box<str>),
    /// An exact declared resource path is invalid or absent from the closed tree.
    InvalidResource(Box<str>),
    /// A Chromium key required by signed release metadata is absent.
    ChromiumKeyMissing,
    /// A manifest key exists although signed release metadata declares no Chromium identity.
    ChromiumKeyUnexpected,
    /// The manifest key is not canonical standard Base64.
    ChromiumKey(ChromiumManifestKeyError),
    /// The decoded manifest key differs from the signed expected Chromium identity.
    ChromiumIdentityMismatch,
    /// An unknown top-level declaration name cannot be represented safely in core.
    InvalidUnmodeledDeclaration(ApiPermissionNameError),
    /// Product policy did not classify one exact declaration.
    UnclassifiedDeclaration(ExtensionManifestDeclaration),
    /// Product policy attempted to call an unmodeled declaration runnable.
    RunnableUnmodeledDeclaration(ExtensionManifestDeclaration),
    /// The core manifest domain rejected the constructed declaration set.
    Core(ExtensionManifestError),
    /// Retained-memory accounting overflowed or exceeded the plan ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionManifestAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "extension manifest JSON is invalid: {error}"),
            Self::ManifestBindingMismatch => formatter
                .write_str("extension manifest bytes do not match the authenticated resource tree"),
            Self::RootNotObject => formatter.write_str("extension manifest root is not an object"),
            Self::MissingField(field) => {
                write!(formatter, "extension manifest field {field} is missing")
            }
            Self::InvalidField(field) => {
                write!(formatter, "extension manifest field {field} is invalid")
            }
            Self::InvalidResource(field) => write!(
                formatter,
                "extension manifest resource {field} is invalid or missing"
            ),
            Self::ChromiumKeyMissing => {
                formatter.write_str("extension manifest has no release-required Chromium key")
            }
            Self::ChromiumKeyUnexpected => {
                formatter.write_str("extension manifest has an unsigned Chromium identity key")
            }
            Self::ChromiumKey(error) => write!(
                formatter,
                "extension manifest Chromium key is invalid: {error}"
            ),
            Self::ChromiumIdentityMismatch => formatter.write_str(
                "extension manifest Chromium key does not match signed release identity",
            ),
            Self::InvalidUnmodeledDeclaration(error) => write!(
                formatter,
                "extension manifest declaration name cannot be retained: {error}"
            ),
            Self::UnclassifiedDeclaration(declaration) => write!(
                formatter,
                "extension manifest declaration {declaration:?} is not classified"
            ),
            Self::RunnableUnmodeledDeclaration(declaration) => write!(
                formatter,
                "unmodeled extension declaration {declaration:?} cannot be classified as runnable"
            ),
            Self::Core(error) => write!(
                formatter,
                "extension manifest declarations are invalid: {error}"
            ),
            Self::RetainedBytesExceeded => {
                formatter.write_str("extension manifest retained-memory budget is exceeded")
            }
        }
    }
}

impl Error for ExtensionManifestAdmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::ChromiumKey(error) => Some(error),
            Self::InvalidUnmodeledDeclaration(error) => Some(error),
            Self::Core(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ExtensionManifestError> for ExtensionManifestAdmissionError {
    fn from(error: ExtensionManifestError) -> Self {
        Self::Core(error)
    }
}

/// Admits one exact root `manifest.json` against a bound release tree and policy.
pub fn admit_extension_manifest(
    binding: ExtensionReleaseTreeBinding<'_>,
    manifest_bytes: &[u8],
    compatibility: &impl ExtensionManifestCompatibilityPolicy,
) -> Result<AdmittedExtensionManifest, ExtensionManifestAdmissionError> {
    verify_manifest_binding(binding, manifest_bytes)?;
    let bounded = parse_bounded_json(manifest_bytes, BoundedJsonLimits::extension_manifest())
        .map_err(ExtensionManifestAdmissionError::Json)?;
    let mut root = match bounded.into_value() {
        Value::Object(root) => root,
        _ => return Err(ExtensionManifestAdmissionError::RootNotObject),
    };

    let manifest_version = take_u32(&mut root, "manifest_version")?;
    if manifest_version != 3 {
        return Err(invalid("manifest_version"));
    }
    let (name, version) = validate_required_metadata(&mut root)?;
    let chromium_key = parse_chromium_key(&mut root, binding)?;
    let csp_value = root.remove("content_security_policy");

    let required_api = parse_api_permissions(root.remove("permissions"), "permissions")?;
    let optional_api =
        parse_api_permissions(root.remove("optional_permissions"), "optional_permissions")?;
    let required_hosts =
        parse_host_permissions(root.remove("host_permissions"), "host_permissions")?;
    let optional_hosts = parse_host_permissions(
        root.remove("optional_host_permissions"),
        "optional_host_permissions",
    )?;

    let (content_scripts, content_resources) =
        parse_content_scripts(root.remove("content_scripts"), binding)?;
    let (background, background_resource) = parse_background(root.remove("background"), binding)?;
    let mut auxiliary_resources = Vec::new();
    let ParsedAction {
        declaration: action,
        popup: action_popup,
        icons: action_icons,
        title: action_title,
    } = parse_action(root.remove("action"), binding)?;
    let (overrides, override_resources) =
        parse_overrides(root.remove("chrome_url_overrides"), binding)?;
    let sandbox_resources = parse_sandbox(root.remove("sandbox"), binding)?;
    let (web_accessible, web_accessible_resources) =
        parse_web_accessible(root.remove("web_accessible_resources"), binding)?;

    let metadata = parse_inert_metadata(&mut root, binding, name, version, action_title)?;
    let unmodeled = preserve_unmodeled(root, binding, &mut auxiliary_resources)?;
    let csp = parse_effective_csp(csp_value, !sandbox_resources.is_empty())
        .map_err(|_| invalid("content_security_policy"))?;

    finish_admission(
        binding,
        manifest_bytes,
        manifest_version,
        chromium_key,
        metadata,
        required_api,
        optional_api,
        required_hosts,
        optional_hosts,
        content_scripts,
        content_resources,
        background,
        background_resource,
        action,
        action_popup,
        action_icons,
        overrides,
        override_resources,
        sandbox_resources,
        web_accessible,
        web_accessible_resources,
        auxiliary_resources,
        unmodeled,
        csp,
        compatibility,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_admission(
    binding: ExtensionReleaseTreeBinding<'_>,
    manifest_bytes: &[u8],
    manifest_version: u32,
    chromium_key: Option<ChromiumManifestKey>,
    metadata: ExtensionManifestMetadata,
    required_api: ExtensionApiPermissionSet,
    optional_api: ExtensionApiPermissionSet,
    required_hosts: Option<ExtensionHostPermissionSet>,
    optional_hosts: Option<ExtensionHostPermissionSet>,
    content_scripts: Vec<ExtensionContentScriptDeclaration>,
    content_resources: Vec<ExtensionContentScriptResources>,
    background: Option<ExtensionBackgroundDeclaration>,
    background_resource: Option<ExtensionManifestResource>,
    action: Option<ExtensionActionDeclaration>,
    action_popup: Option<ExtensionManifestResource>,
    action_icons: Vec<ExtensionManifestIcon>,
    overrides: Vec<ExtensionOverrideTarget>,
    override_resources: Vec<ExtensionOverrideResource>,
    sandbox_resources: Vec<ExtensionManifestResource>,
    web_accessible: Vec<ExtensionWebAccessibleResourceDeclaration>,
    web_accessible_resources: Vec<ExtensionWebAccessibleResourceGroup>,
    auxiliary_resources: Vec<ExtensionManifestResource>,
    unmodeled: Vec<ExtensionUnmodeledDeclarationName>,
    csp: csp::EffectiveCsp,
    compatibility: &impl ExtensionManifestCompatibilityPolicy,
) -> Result<AdmittedExtensionManifest, ExtensionManifestAdmissionError> {
    let extension_pages_csp_digest = csp.extension_pages.digest;
    let extension_pages_csp = csp.extension_pages.canonical;
    let sandbox_csp = csp.sandbox.map(|policy| (policy.digest, policy.canonical));
    let sandbox = match &sandbox_csp {
        None => None,
        Some((effective_csp, _)) => Some(ExtensionSandboxDeclaration::new(
            ExtensionManifestResourceDigest::from_bytes(digest_resources(&sandbox_resources, &[])),
            sandbox_resources.len(),
            *effective_csp,
        )?),
    };
    let execution = ExtensionManifestExecutionSurfaces::new(
        content_scripts,
        ExtensionContentSecurityPolicyDeclaration::new(extension_pages_csp_digest),
        sandbox,
        web_accessible,
    )?;
    let declarations = ExtensionManifestDeclarations::new(
        required_api,
        optional_api,
        required_hosts,
        optional_hosts,
        background,
        action,
        overrides,
        execution,
        unmodeled,
    )?;
    let resources = ExtensionManifestResourcePlan::new(
        content_resources,
        background_resource,
        action_popup,
        action_icons,
        override_resources,
        sandbox_resources,
        web_accessible_resources,
        auxiliary_resources,
        extension_pages_csp,
        sandbox_csp.map(|(_, canonical)| canonical),
    )
    .ok_or(ExtensionManifestAdmissionError::RetainedBytesExceeded)?;
    let classifications = classify_all(&declarations, &resources, compatibility)?;
    let descriptor = ExtensionManifestDescriptor::new(
        binding.package().identity().clone(),
        manifest_version,
        declarations,
        compatibility.target().clone(),
        classifications,
    )?;

    let retained_bytes = descriptor
        .retained_bytes()
        .checked_add(resources.retained_bytes())
        .and_then(|bytes| bytes.checked_add(metadata.retained_bytes()))
        .and_then(|bytes| bytes.checked_add(size_of::<AdmittedExtensionManifest>()))
        .and_then(|bytes| {
            chromium_key.as_ref().map_or(Some(bytes), |key| {
                bytes.checked_add(key.decoded_bytes().len())
            })
        })
        .ok_or(ExtensionManifestAdmissionError::RetainedBytesExceeded)?;
    if resources.retained_bytes() > MAX_EXTENSION_MANIFEST_PLAN_RETAINED_BYTES {
        return Err(ExtensionManifestAdmissionError::RetainedBytesExceeded);
    }
    let admission_digest = digest_admission(
        binding,
        manifest_bytes,
        &descriptor,
        &metadata,
        &resources,
        chromium_key.as_ref(),
    );
    Ok(AdmittedExtensionManifest {
        descriptor,
        metadata,
        resources,
        chromium_key,
        admission_digest,
        retained_bytes,
    })
}

fn verify_manifest_binding(
    binding: ExtensionReleaseTreeBinding<'_>,
    bytes: &[u8],
) -> Result<(), ExtensionManifestAdmissionError> {
    let path = PortableRelativePath::parse("manifest.json")
        .expect("constant root manifest path is portable");
    let file = binding
        .index()
        .file(&path)
        .ok_or(ExtensionManifestAdmissionError::ManifestBindingMismatch)?;
    let length = u64::try_from(bytes.len())
        .map_err(|_| ExtensionManifestAdmissionError::ManifestBindingMismatch)?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if file.length() != length
        || file.sha256() != digest
        || binding.package().identity().manifest_sha256().bytes() != digest
    {
        return Err(ExtensionManifestAdmissionError::ManifestBindingMismatch);
    }
    Ok(())
}

fn parse_api_permissions(
    value: Option<Value>,
    field: &'static str,
) -> Result<ExtensionApiPermissionSet, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return ExtensionApiPermissionSet::new(Vec::new()).map_err(Into::into);
    };
    let values = into_array(value, field)?;
    if values.len() > MAX_EXTENSION_API_PERMISSIONS {
        return Err(invalid(field));
    }
    let mut names = Vec::with_capacity(values.len());
    for value in values {
        let value = value.as_str().ok_or_else(|| invalid(field))?;
        if value == "<all_urls>" || value.contains("://") {
            return Err(invalid(field));
        }
        names.push(ApiPermissionName::parse_exact(value).map_err(|_| invalid(field))?);
    }
    ExtensionApiPermissionSet::new(names).map_err(Into::into)
}

fn parse_host_permissions(
    value: Option<Value>,
    field: &'static str,
) -> Result<Option<ExtensionHostPermissionSet>, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let values = string_array(value, field, MAX_EXTENSION_HOST_PERMISSION_PATTERNS)?;
    if values.is_empty() {
        return Ok(None);
    }
    let matches = MatchSet::parse(&values, std::iter::empty::<&str>(), MatchOptions::default())
        .map_err(|_| invalid(field))?;
    ExtensionHostPermissionSet::new(matches)
        .map(Some)
        .map_err(Into::into)
}

fn preserve_unmodeled(
    root: Map<String, Value>,
    binding: ExtensionReleaseTreeBinding<'_>,
    resources: &mut Vec<ExtensionManifestResource>,
) -> Result<Vec<ExtensionUnmodeledDeclarationName>, ExtensionManifestAdmissionError> {
    let mut declarations = Vec::with_capacity(root.len());
    for (name, value) in root {
        validate_known_unmodeled_resource(&name, &value, binding, resources)?;
        declarations.push(
            ExtensionUnmodeledDeclarationName::parse_exact(&name)
                .map_err(ExtensionManifestAdmissionError::InvalidUnmodeledDeclaration)?,
        );
    }
    Ok(declarations)
}

fn validate_known_unmodeled_resource(
    name: &str,
    value: &Value,
    binding: ExtensionReleaseTreeBinding<'_>,
    resources: &mut Vec<ExtensionManifestResource>,
) -> Result<(), ExtensionManifestAdmissionError> {
    match name {
        "options_page" | "devtools_page" => {
            let source = value.as_str().ok_or_else(|| invalid(name))?;
            resources.push(bind_resource(binding, source, name)?);
        }
        "options_ui" => {
            let object = value.as_object().ok_or_else(|| invalid(name))?;
            if let Some(page) = object.get("page") {
                let source = page.as_str().ok_or_else(|| invalid(name))?;
                resources.push(bind_resource(binding, source, name)?);
            }
        }
        "side_panel" => {
            let object = value.as_object().ok_or_else(|| invalid(name))?;
            if let Some(path) = object.get("default_path") {
                let source = path.as_str().ok_or_else(|| invalid(name))?;
                resources.push(bind_resource(binding, source, name)?);
            }
        }
        _ => {}
    }
    Ok(())
}

fn classify_all(
    declarations: &ExtensionManifestDeclarations,
    resources: &ExtensionManifestResourcePlan,
    policy: &impl ExtensionManifestCompatibilityPolicy,
) -> Result<Vec<ExtensionCompatibilityClassification>, ExtensionManifestAdmissionError> {
    let keys = declarations.declaration_keys();
    let mut classifications = Vec::with_capacity(keys.len());
    for declaration in keys {
        let level = policy
            .classify(ExtensionManifestCompatibilitySubject {
                declaration: &declaration,
                declarations,
                resources,
            })
            .ok_or_else(|| {
                ExtensionManifestAdmissionError::UnclassifiedDeclaration(declaration.clone())
            })?;
        if matches!(
            declaration,
            ExtensionManifestDeclaration::UnmodeledAuthority(_)
        ) && matches!(
            level,
            ExtensionCompatibilityLevel::Compatible | ExtensionCompatibilityLevel::Degraded
        ) {
            return Err(ExtensionManifestAdmissionError::RunnableUnmodeledDeclaration(declaration));
        }
        classifications.push(ExtensionCompatibilityClassification::new(
            declaration,
            level,
        ));
    }
    Ok(classifications)
}

fn digest_admission(
    binding: ExtensionReleaseTreeBinding<'_>,
    manifest_bytes: &[u8],
    descriptor: &ExtensionManifestDescriptor,
    metadata: &ExtensionManifestMetadata,
    resources: &ExtensionManifestResourcePlan,
    chromium_key: Option<&ChromiumManifestKey>,
) -> ExtensionManifestAdmissionDigest {
    let package = binding.package().identity();
    let mut digest = Sha256::new();
    digest.update(ADMISSION_DIGEST_DOMAIN);
    digest.update(package.authority().as_bytes());
    digest.update(package.key().as_bytes());
    digest.update(package.revision().get().to_be_bytes());
    package.payload().update_sha256(&mut digest);
    digest.update(package.manifest_sha256().as_bytes());
    digest.update(package.tree_sha256().as_bytes());
    digest.update(binding.index().index_sha256().as_bytes());
    digest.update((manifest_bytes.len() as u64).to_be_bytes());
    digest.update(Sha256::digest(manifest_bytes));
    digest.update(descriptor.compatibility_digest().as_bytes());
    digest.update(metadata.digest());
    digest.update(resources.digest().as_bytes());
    match chromium_key {
        None => digest.update([0]),
        Some(key) => {
            digest.update([1]);
            digest.update(key.digest().as_bytes());
        }
    }
    ExtensionManifestAdmissionDigest(digest.finalize().into())
}

fn bind_resource(
    binding: ExtensionReleaseTreeBinding<'_>,
    source: &str,
    field: &str,
) -> Result<ExtensionManifestResource, ExtensionManifestAdmissionError> {
    let canonical = source.strip_prefix('/').unwrap_or(source);
    if canonical.starts_with('/') {
        return Err(ExtensionManifestAdmissionError::InvalidResource(
            field.into(),
        ));
    }
    let path = PortableRelativePath::parse(canonical)
        .map_err(|_| ExtensionManifestAdmissionError::InvalidResource(field.into()))?;
    let file = binding
        .index()
        .file(&path)
        .ok_or_else(|| ExtensionManifestAdmissionError::InvalidResource(field.into()))?;
    Ok(ExtensionManifestResource::new(
        path,
        file.length(),
        file.sha256(),
    ))
}

fn parse_resource_array(
    value: Option<Value>,
    field: &str,
    max: usize,
    binding: ExtensionReleaseTreeBinding<'_>,
) -> Result<Vec<ExtensionManifestResource>, ExtensionManifestAdmissionError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = into_array(value, field)?;
    if values.len() > max {
        return Err(invalid(field));
    }
    values
        .into_iter()
        .try_fold(
            (Vec::new(), BTreeSet::new()),
            |(mut resources, mut seen), value| {
                let source = value.as_str().ok_or_else(|| invalid(field))?;
                let resource = bind_resource(binding, source, field)?;
                if !seen.insert(resource.path().collision_key()) {
                    return Err(invalid(field));
                }
                resources.push(resource);
                Ok((resources, seen))
            },
        )
        .map(|(resources, _)| resources)
}

fn parse_icons(
    value: Value,
    field: &str,
    binding: ExtensionReleaseTreeBinding<'_>,
    allow_shorthand: bool,
) -> Result<Vec<ExtensionManifestIcon>, ExtensionManifestAdmissionError> {
    if let Some(source) = value.as_str() {
        if !allow_shorthand {
            return Err(invalid(field));
        }
        let resource = bind_icon_resource(binding, source, field)?;
        return Ok(vec![ExtensionManifestIcon::new(None, resource)]);
    }
    let object = value.as_object().ok_or_else(|| invalid(field))?;
    if object.is_empty() || object.len() > MAX_ICON_ENTRIES {
        return Err(invalid(field));
    }
    let mut icons = Vec::with_capacity(object.len());
    let mut paths = BTreeSet::new();
    for (size, value) in object {
        let parsed = size.parse::<u16>().map_err(|_| invalid(field))?;
        if parsed == 0 || parsed.to_string() != *size {
            return Err(invalid(field));
        }
        let source = value.as_str().ok_or_else(|| invalid(field))?;
        let resource = bind_icon_resource(binding, source, field)?;
        if !paths.insert(resource.path().collision_key()) {
            return Err(invalid(field));
        }
        icons.push(ExtensionManifestIcon::new(Some(parsed), resource));
    }
    icons.sort_unstable_by_key(ExtensionManifestIcon::size);
    Ok(icons)
}

fn bind_icon_resource(
    binding: ExtensionReleaseTreeBinding<'_>,
    source: &str,
    field: &str,
) -> Result<ExtensionManifestResource, ExtensionManifestAdmissionError> {
    let extension = source
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .ok_or_else(|| invalid(field))?;
    if !matches!(
        extension.as_str(),
        "png" | "bmp" | "gif" | "ico" | "jpg" | "jpeg"
    ) {
        return Err(invalid(field));
    }
    bind_resource(binding, source, field)
}

fn optional_globs(
    value: Option<Value>,
    field: &str,
) -> Result<Vec<String>, ExtensionManifestAdmissionError> {
    let values = optional_string_array(value, field, MAX_EXTENSION_CONTENT_SCRIPT_GLOBS)?;
    if values.iter().any(|value| {
        value.is_empty()
            || value.len() > MAX_EXTENSION_CONTENT_SCRIPT_GLOB_BYTES
            || !value.is_ascii()
            || value.bytes().any(|byte| byte.is_ascii_control())
    }) {
        return Err(invalid(field));
    }
    reject_duplicate_strings(&values, field)?;
    Ok(values)
}

fn reject_duplicate_strings(
    values: &[String],
    field: &str,
) -> Result<(), ExtensionManifestAdmissionError> {
    let mut seen = BTreeSet::new();
    if values.iter().any(|value| !seen.insert(value.as_str())) {
        Err(invalid(field))
    } else {
        Ok(())
    }
}

fn required_string_array(
    object: &mut Map<String, Value>,
    key: &str,
    field: &str,
    max: usize,
) -> Result<Vec<String>, ExtensionManifestAdmissionError> {
    let value = object
        .remove(key)
        .ok_or_else(|| missing(&format!("{field}.{key}")))?;
    string_array(value, field, max)
}

fn optional_string_array(
    value: Option<Value>,
    field: &str,
    max: usize,
) -> Result<Vec<String>, ExtensionManifestAdmissionError> {
    value.map_or(Ok(Vec::new()), |value| string_array(value, field, max))
}

fn string_array(
    value: Value,
    field: &str,
    max: usize,
) -> Result<Vec<String>, ExtensionManifestAdmissionError> {
    let values = into_array(value, field)?;
    if values.len() > max {
        return Err(invalid(field));
    }
    values
        .into_iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| invalid(field))
        })
        .collect()
}

fn reject_unknown_nested(
    object: &Map<String, Value>,
    allowed: &[&str],
    field: &str,
) -> Result<(), ExtensionManifestAdmissionError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        Err(invalid(field))
    } else {
        Ok(())
    }
}

fn optional_bool(
    object: &mut Map<String, Value>,
    key: &str,
    default: bool,
    field: &str,
) -> Result<bool, ExtensionManifestAdmissionError> {
    object.remove(key).map_or(Ok(default), |value| {
        value.as_bool().ok_or_else(|| invalid(field))
    })
}

fn optional_string<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
    default: &'static str,
    field: &str,
) -> Result<&'a str, ExtensionManifestAdmissionError> {
    if !object.contains_key(key) {
        object.insert(key.to_owned(), Value::String(default.to_owned()));
    }
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(field))
}

fn take_owned_string(
    root: &mut Map<String, Value>,
    field: &str,
) -> Result<String, ExtensionManifestAdmissionError> {
    root.remove(field)
        .ok_or_else(|| missing(field))?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| invalid(field))
}

fn take_u32(
    root: &mut Map<String, Value>,
    field: &str,
) -> Result<u32, ExtensionManifestAdmissionError> {
    let value = root.remove(field).ok_or_else(|| missing(field))?;
    let value = value.as_u64().ok_or_else(|| invalid(field))?;
    u32::try_from(value).map_err(|_| invalid(field))
}

fn into_array(value: Value, field: &str) -> Result<Vec<Value>, ExtensionManifestAdmissionError> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(invalid(field)),
    }
}

fn into_object(
    value: Value,
    field: &str,
) -> Result<Map<String, Value>, ExtensionManifestAdmissionError> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(invalid(field)),
    }
}

fn missing(field: &str) -> ExtensionManifestAdmissionError {
    ExtensionManifestAdmissionError::MissingField(field.into())
}

fn invalid(field: &str) -> ExtensionManifestAdmissionError {
    ExtensionManifestAdmissionError::InvalidField(field.into())
}

#[cfg(test)]
mod tests;
