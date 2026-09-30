//! Bounded static CSS policy. No scriptlets, replacement code or DOM scanning.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use adblock::filters::cosmetic::{CosmeticFilter, CosmeticFilterMask};
use adblock::{Engine, FilterSet};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_RULES: usize = 50_000;
const MAX_SELECTOR_BYTES: usize = 4 * 1024;
const MAX_SCOPE_DOMAINS: usize = 1_024;
const MAX_SCOPE_PREDICATES: usize = 100_000;
const MAX_POLICY_BYTES: usize = 16 * 1024 * 1024;
const MAX_STYLESHEET_BYTES: usize = 1024 * 1024;
const MAX_CONTROLS: usize = 1_024;

/// Static cosmetic admission statistics, separate from network rule coverage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CosmeticReport {
    /// Accepted static hide and exception input lines.
    pub accepted: usize,
    /// Unsupported, invalid or overlong cosmetic input lines.
    pub rejected: usize,
    /// Generic-hide controls admitted into the document matcher.
    pub generic_controls: usize,
}

/// A cosmetic artifact could not be admitted or queried within its limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CosmeticError {
    /// Aggregate input, rule count or emitted CSS exceeds its fixed budget.
    #[error("cosmetic policy exceeds its resource budget")]
    ResourceLimit,
    /// Encoded policy, selector, domain or control is not valid.
    #[error("cosmetic policy is invalid")]
    InvalidPolicy,
    /// The document is not a bounded HTTP(S) URL with a hostname.
    #[error("cosmetic document URL is invalid")]
    InvalidDocument,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    selector: String,
    include: Vec<String>,
    exclude: Vec<String>,
    exception: bool,
}

impl Rule {
    fn matches(&self, host: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|domain| within(host, domain)))
            && !self.exclude.iter().any(|domain| within(host, domain))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    version: u32,
    rules: Vec<Rule>,
    controls: Vec<String>,
    report: CosmeticReport,
}

/// Immutable selector index shared by profiles and documents. Queries only
/// inspect generic rules and rules indexed under the document's domain labels.
pub struct CosmeticPolicy {
    data: Data,
    generic: Vec<usize>,
    domains: BTreeMap<String, Vec<usize>>,
    controls: Engine,
    fingerprint: [u8; 32],
    default_css: Arc<str>,
}

impl std::fmt::Debug for CosmeticPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CosmeticPolicy")
            .field("report", &self.data.report)
            .finish_non_exhaustive()
    }
}

impl CosmeticPolicy {
    /// Compiles static selectors and generic-hide controls from bounded sources.
    /// Unsupported cosmetic syntax is reported and omitted, never executed.
    pub fn compile<'a>(
        sources: impl IntoIterator<Item = &'a str>,
    ) -> Result<Arc<Self>, CosmeticError> {
        let mut data = Data {
            version: 1,
            rules: Vec::new(),
            controls: Vec::new(),
            report: CosmeticReport::default(),
        };
        let mut bytes = 0usize;
        let mut lines = 0usize;
        let mut predicates = 0usize;
        for (source_index, source) in sources.into_iter().enumerate() {
            if source_index >= 32 {
                return Err(CosmeticError::ResourceLimit);
            }
            bytes = bytes
                .checked_add(source.len())
                .ok_or(CosmeticError::ResourceLimit)?;
            if bytes > 32 * 1024 * 1024 {
                return Err(CosmeticError::ResourceLimit);
            }
            for line in source.lines().map(str::trim) {
                lines += 1;
                if lines > 500_000 || line.len() > 64 * 1024 {
                    return Err(CosmeticError::ResourceLimit);
                }
                if line.starts_with('!') || line.starts_with('[') || line.is_empty() {
                    continue;
                }
                if let Some(control) = generic_control(line)? {
                    data.controls.push(control);
                    if data.controls.len() > MAX_CONTROLS {
                        return Err(CosmeticError::ResourceLimit);
                    }
                    data.report.generic_controls += 1;
                    continue;
                }
                if !crate::compiler::looks_like_cosmetic_rule(line) {
                    continue;
                }
                match parse_rule(line) {
                    Ok(rule) => {
                        predicates += rule.include.len() + rule.exclude.len();
                        if predicates > MAX_SCOPE_PREDICATES {
                            return Err(CosmeticError::ResourceLimit);
                        }
                        data.rules.push(rule);
                        data.report.accepted += 1;
                        if data.rules.len() > MAX_RULES {
                            return Err(CosmeticError::ResourceLimit);
                        }
                    }
                    Err(_) => data.report.rejected += 1,
                }
            }
        }
        data.rules.sort();
        data.rules.dedup();
        data.controls.sort();
        data.controls.dedup();
        Self::from_data(data)
    }

    /// Serializes portable policy data; no browsing decisions or document URLs
    /// are retained in this representation.
    pub fn encode(&self) -> Result<Vec<u8>, CosmeticError> {
        let bytes = serde_json::to_vec(&self.data).map_err(|_| CosmeticError::InvalidPolicy)?;
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(CosmeticError::ResourceLimit);
        }
        Ok(bytes)
    }

    /// Revalidates persisted data, including selectors and domain boundaries.
    pub fn decode(bytes: &[u8]) -> Result<Arc<Self>, CosmeticError> {
        if bytes.len() > MAX_POLICY_BYTES {
            return Err(CosmeticError::ResourceLimit);
        }
        let data: Data = serde_json::from_slice(bytes).map_err(|_| CosmeticError::InvalidPolicy)?;
        if serde_json::to_vec(&data).map_err(|_| CosmeticError::InvalidPolicy)? != bytes {
            return Err(CosmeticError::InvalidPolicy);
        }
        Self::from_data(data)
    }

    /// Returns static filtering admission counts.
    pub fn report(&self) -> CosmeticReport {
        self.data.report
    }

    /// Emits a separate WebKit cosmetic list for top-level documents.
    /// WebKit evaluates frame conditions using the initiating frame when a
    /// child document loads. Child cosmetics must therefore use `stylesheet`
    /// with native frame attribution; do not apply this list to child loads.
    /// Network exceptions must remain in their own list so a cosmetic control
    /// cannot cancel network blocking. An empty policy returns no native list.
    #[cfg(feature = "webkit")]
    pub fn webkit_top_document_rules(&self) -> Result<Option<Arc<str>>, CosmeticError> {
        use adblock::content_blocking::CbRuleEquivalent;
        use adblock::filters::network::{NetworkFilter, NetworkFilterMask};
        use serde_json::json;

        let mut generic = Vec::new();
        let mut specific = Vec::new();
        let mut scope_budget = 8_000_000usize;
        let mut selectors: BTreeMap<&str, Vec<&Rule>> = BTreeMap::new();
        for rule in &self.data.rules {
            selectors.entry(&rule.selector).or_default().push(rule);
        }
        for (selector, rules) in selectors {
            for is_generic in [true, false] {
                let hides: Vec<_> = rules
                    .iter()
                    .copied()
                    .filter(|r| !r.exception && r.include.is_empty() == is_generic)
                    .collect();
                if hides.is_empty() {
                    continue;
                }
                let exceptions: Vec<_> = rules.iter().copied().filter(|r| r.exception).collect();
                for (include, exclude) in scope_regions(&hides, &exceptions, &mut scope_budget)? {
                    let mut trigger = json!({
                        "url-filter": include.as_deref().map(domain_url_pattern).unwrap_or_else(|| "^https?://".into()),
                        "resource-type": ["top-document"]
                    });
                    if !exclude.is_empty() {
                        trigger["unless-top-url"] = json!(exclude
                            .iter()
                            .map(|s| domain_url_pattern(s))
                            .collect::<Vec<_>>());
                    }
                    let rule = json!({"action":{"type":"css-display-none","selector":selector},"trigger":trigger});
                    if is_generic {
                        generic.push(rule);
                    } else {
                        specific.push(rule);
                    }
                }
            }
        }

        if generic.is_empty() && specific.is_empty() {
            return Ok(None);
        }
        let mut generic = group_native_styles(generic)?;
        let specific = group_native_styles(specific)?;
        // Controls only erase preceding generic CSS. Site-specific CSS follows
        // them, and the native network list is independent.
        for control in &self.data.controls {
            let mut filter = NetworkFilter::parse(control, true, Default::default())
                .map_err(|_| CosmeticError::InvalidPolicy)?;
            filter.mask.remove(NetworkFilterMask::FROM_ALL_TYPES);
            filter.mask.insert(NetworkFilterMask::FROM_IMAGE);
            let equivalent =
                CbRuleEquivalent::try_from(filter).map_err(|_| CosmeticError::InvalidPolicy)?;
            for native in equivalent {
                let mut trigger = json!({"url-filter":native.trigger.url_filter,"resource-type":["top-document"]});
                if let Some(domains) = native.trigger.if_domain {
                    trigger["if-top-url"] = json!(domains
                        .iter()
                        .map(|d| domain_url_pattern(d.trim_start_matches('*')))
                        .collect::<Vec<_>>());
                }
                if let Some(domains) = native.trigger.unless_domain {
                    trigger["unless-top-url"] = json!(domains
                        .iter()
                        .map(|d| domain_url_pattern(d.trim_start_matches('*')))
                        .collect::<Vec<_>>());
                }
                if let Some(case_sensitive) = native.trigger.url_filter_is_case_sensitive {
                    trigger["url-filter-is-case-sensitive"] = json!(case_sensitive);
                }
                generic.push(json!({"action":{"type":"ignore-previous-rules"},"trigger":trigger}));
            }
        }
        generic.extend(specific);
        if generic.is_empty() {
            return Ok(None);
        }
        if generic.len() > 140_000 {
            return Err(CosmeticError::ResourceLimit);
        }
        let encoded = serde_json::to_string(&generic).map_err(|_| CosmeticError::InvalidPolicy)?;
        if encoded.len() > 32 * 1024 * 1024 {
            return Err(CosmeticError::ResourceLimit);
        }
        Ok(Some(encoded.into()))
    }

    /// Resolves CSS for one document, honoring matching exceptions and
    /// generic-hide controls. This is navigation work, never request-callback
    /// work. A matcher availability failure suppresses generic hiding.
    pub fn stylesheet(&self, document: &str) -> Result<Arc<str>, CosmeticError> {
        if document.len() > 32 * 1024 {
            return Err(CosmeticError::InvalidDocument);
        }
        let url = url::Url::parse(document).map_err(|_| CosmeticError::InvalidDocument)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(CosmeticError::InvalidDocument);
        }
        let host = url
            .host_str()
            .ok_or(CosmeticError::InvalidDocument)?
            .trim_end_matches('.');
        let request = adblock::request::Request::preparsed(
            url.as_str(),
            host,
            host,
            "document",
            false,
            "get",
        );
        let suppress_generic = self
            .controls
            .try_check_prepared_network_request(&request)
            .map_or(true, |result| result.should_block());
        if !suppress_generic {
            let mut suffix = host;
            let mut scoped = false;
            loop {
                if self.domains.contains_key(suffix) {
                    scoped = true;
                    break;
                }
                let Some((_, tail)) = suffix.split_once('.') else {
                    break;
                };
                suffix = tail;
            }
            if !scoped {
                return Ok(self.default_css.clone());
            }
        }
        let mut hidden = BTreeSet::new();
        let mut exceptions = BTreeSet::new();
        let mut visit = |index: usize| {
            let rule = &self.data.rules[index];
            if !rule.matches(host) {
                return;
            }
            if rule.exception {
                exceptions.insert(rule.selector.as_str());
            } else if !suppress_generic || !rule.include.is_empty() {
                hidden.insert(rule.selector.as_str());
            }
        };
        if !suppress_generic {
            for &index in &self.generic {
                visit(index);
            }
        }
        let mut suffix = host;
        loop {
            if let Some(indices) = self.domains.get(suffix) {
                for &index in indices {
                    visit(index);
                }
            }
            let Some((_, tail)) = suffix.split_once('.') else {
                break;
            };
            suffix = tail;
        }
        let mut css = String::new();
        // Separate rules keep an unsupported browser selector from invalidating
        // every other selector in a comma-separated list.
        for selector in hidden.difference(&exceptions) {
            if css.len() + selector.len() + 26 > MAX_STYLESHEET_BYTES {
                return Err(CosmeticError::ResourceLimit);
            }
            css.push_str(selector);
            css.push_str("{display:none!important}\n");
        }
        Ok(css.into())
    }

    fn from_data(data: Data) -> Result<Arc<Self>, CosmeticError> {
        if data.version != 1
            || data.rules.len() > MAX_RULES
            || data.controls.len() > MAX_CONTROLS
            || data.rules.len() > data.report.accepted
            || data.report.accepted > MAX_RULES
            || data.report.rejected > 500_000
            || data.controls.len() > data.report.generic_controls
            || data.report.generic_controls > MAX_CONTROLS
            || data.rules.windows(2).any(|pair| pair[0] >= pair[1])
            || data.controls.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(CosmeticError::InvalidPolicy);
        }
        let mut generic = Vec::new();
        let mut domains: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut predicates = 0usize;
        for (index, rule) in data.rules.iter().enumerate() {
            validate_selector(&rule.selector)?;
            if rule.include.len() + rule.exclude.len() > MAX_SCOPE_DOMAINS {
                return Err(CosmeticError::InvalidPolicy);
            }
            predicates += rule.include.len() + rule.exclude.len();
            if predicates > MAX_SCOPE_PREDICATES {
                return Err(CosmeticError::ResourceLimit);
            }
            for list in [&rule.include, &rule.exclude] {
                if list.len() > MAX_SCOPE_DOMAINS
                    || list.windows(2).any(|p| p[0] >= p[1])
                    || list.iter().any(|d| normalize_domain(d).as_ref() != Some(d))
                {
                    return Err(CosmeticError::InvalidPolicy);
                }
            }
            if rule.exception && (rule.include.is_empty() || !rule.exclude.is_empty()) {
                return Err(CosmeticError::InvalidPolicy);
            }
            if rule.include.is_empty() {
                generic.push(index);
            }
            // Negative predicates also make a hostname differ from the common
            // generic stylesheet. Index them for the fast-path admission test.
            for domain in rule.include.iter().chain(&rule.exclude) {
                domains.entry(domain.clone()).or_default().push(index);
            }
        }
        let mut filters = FilterSet::new(false);
        for control in &data.controls {
            // Reconstruct the only admitted syntax; persisted controls cannot
            // introduce redirect, CSP, removeparam, or executable resources.
            let (pattern, modifiers) = control
                .rsplit_once('$')
                .ok_or(CosmeticError::InvalidPolicy)?;
            let modifiers = modifiers
                .strip_prefix("document")
                .ok_or(CosmeticError::InvalidPolicy)?;
            let original = format!("@@{pattern}$generichide{modifiers}");
            if generic_control(&original)?.as_ref() != Some(control) {
                return Err(CosmeticError::InvalidPolicy);
            }
            adblock::filters::network::NetworkFilter::parse(control, false, Default::default())
                .map_err(|_| CosmeticError::InvalidPolicy)?;
        }
        filters.add_filter_list(data.controls.join("\n"), Default::default());
        let controls = Engine::new_with_filter_set(filters);
        controls
            .prepare_and_freeze_network_matcher(adblock::blocker::NetworkMatcherPreparationLimits {
                max_regexes: 256,
                max_pattern_bytes: 256 * 1024,
                max_patterns_per_regex: 256,
                max_pattern_bytes_per_regex: 128 * 1024,
                max_regex_size_bytes: 96 * 1024,
                max_regex_dfa_size_bytes: 16 * 1024,
                max_filter_checks_per_request: 256,
            })
            .map_err(|_| CosmeticError::ResourceLimit)?;
        let selectors: BTreeSet<_> = generic
            .iter()
            .map(|&index| data.rules[index].selector.as_str())
            .collect();
        let mut default_css = String::new();
        for selector in selectors {
            if default_css.len() + selector.len() + 26 > MAX_STYLESHEET_BYTES {
                return Err(CosmeticError::ResourceLimit);
            }
            default_css.push_str(selector);
            default_css.push_str("{display:none!important}\n");
        }
        let mut policy = Arc::new(Self {
            data,
            generic,
            domains,
            controls,
            fingerprint: [0; 32],
            default_css: default_css.into(),
        });
        let encoded = policy.encode()?;
        Arc::get_mut(&mut policy)
            .expect("unpublished policy has one owner")
            .fingerprint = Sha256::digest(&encoded).into();
        Ok(policy)
    }
}

impl zephium_core::blocker::DocumentStyleProvider for CosmeticPolicy {
    fn fingerprint(&self) -> zephium_core::blocker::ContentRuleDigest {
        zephium_core::blocker::ContentRuleDigest::from_bytes(self.fingerprint)
    }

    fn stylesheet(
        &self,
        url: &str,
    ) -> Result<Arc<str>, zephium_core::blocker::DocumentStyleFailure> {
        CosmeticPolicy::stylesheet(self, url).map_err(|error| match error {
            CosmeticError::ResourceLimit => {
                zephium_core::blocker::DocumentStyleFailure::ResourceLimit
            }
            CosmeticError::InvalidDocument => {
                zephium_core::blocker::DocumentStyleFailure::InvalidDocument
            }
            CosmeticError::InvalidPolicy => {
                zephium_core::blocker::DocumentStyleFailure::Unavailable
            }
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCosmetics {
    pub(crate) policy: Arc<CosmeticPolicy>,
    pub(crate) native: Option<zephium_core::blocker::DeclarativeStyleRules>,
}

impl PreparedCosmetics {
    pub(crate) fn new(
        policy: Arc<CosmeticPolicy>,
        target: crate::CompileTarget,
    ) -> Result<Self, CosmeticError> {
        #[cfg(feature = "webkit")]
        let native = if target == crate::CompileTarget::WebKit {
            policy
                .webkit_top_document_rules()?
                .map(|encoded| {
                    zephium_core::blocker::DeclarativeStyleRules::new(encoded)
                        .ok_or(CosmeticError::ResourceLimit)
                })
                .transpose()?
        } else {
            None
        };
        #[cfg(not(feature = "webkit"))]
        let native = {
            let _ = target;
            None
        };
        Ok(Self { policy, native })
    }
}

fn within(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(feature = "webkit")]
fn domain_url_pattern(domain: &str) -> String {
    // Domain strings have already passed IDNA/DNS validation. Anchor the
    // authority and port boundary: a hostname inside userinfo is not a match.
    format!(
        "^https?://([^/@]*@)?([^/@]+\\.)?{}(:[0-9]+)?([/?#].*)?$",
        domain.replace('.', "\\.")
    )
}

/// CSS rules with the exact same scope commute. Group within the generic and
/// specific stages separately, never across an ignore-previous-rules control.
/// Small groups bound native selector parsing while removing repeated trigger
/// JSON and declarations from the large generic stylesheet.
#[cfg(feature = "webkit")]
fn group_native_styles(
    rules: Vec<serde_json::Value>,
) -> Result<Vec<serde_json::Value>, CosmeticError> {
    use serde_json::json;
    let mut groups: BTreeMap<String, (serde_json::Value, Vec<String>)> = BTreeMap::new();
    for mut rule in rules {
        let trigger = rule["trigger"].take();
        let key = serde_json::to_string(&trigger).map_err(|_| CosmeticError::InvalidPolicy)?;
        let selector = rule["action"]["selector"]
            .as_str()
            .ok_or(CosmeticError::InvalidPolicy)?
            .to_owned();
        groups
            .entry(key)
            .or_insert_with(|| (trigger, Vec::new()))
            .1
            .push(selector);
    }
    let mut output = Vec::new();
    for (_, (trigger, selectors)) in groups {
        let mut group = String::new();
        let mut count = 0usize;
        for selector in selectors {
            if !group.is_empty() && (group.len() + selector.len() + 1 > 16 * 1024 || count == 128) {
                output.push(json!({"action":{"type":"css-display-none","selector":std::mem::take(&mut group)},"trigger":trigger}));
                count = 0;
            }
            if !group.is_empty() {
                group.push(',');
            }
            group.push_str(&selector);
            count += 1;
        }
        if !group.is_empty() {
            output.push(
                json!({"action":{"type":"css-display-none","selector":group},"trigger":trigger}),
            );
        }
    }
    Ok(output)
}

/// Partition a selector's hide-minus-exception set into positive domain
/// regions and their nearest negative descendants. A more specific region
/// can re-enable a selector excluded by its parent's scope.
#[cfg(feature = "webkit")]
fn scope_regions(
    hides: &[&Rule],
    exceptions: &[&Rule],
    budget: &mut usize,
) -> Result<BTreeMap<Option<String>, Vec<String>>, CosmeticError> {
    let root = hides.iter().any(|r| r.include.is_empty());
    let domains: BTreeSet<_> = hides
        .iter()
        .chain(exceptions)
        .flat_map(|rule| rule.include.iter().chain(&rule.exclude))
        .collect();
    let predicates: usize = hides
        .iter()
        .chain(exceptions)
        .map(|r| 1 + r.include.len() + r.exclude.len())
        .sum();
    let checks = domains
        .len()
        .checked_mul(predicates)
        .ok_or(CosmeticError::ResourceLimit)?;
    *budget = budget
        .checked_sub(checks)
        .ok_or(CosmeticError::ResourceLimit)?;
    let mut domains: Vec<_> = domains.into_iter().collect();
    domains.sort_by_key(|domain| {
        (
            domain.bytes().filter(|b| *b == b'.').count(),
            domain.as_str(),
        )
    });
    let mut boundaries: BTreeMap<&str, bool> = BTreeMap::new();
    let mut regions = BTreeMap::new();
    if root {
        regions.insert(None, Vec::new());
    }
    for domain in domains {
        let wanted = hides.iter().any(|r| r.matches(domain))
            && !exceptions.iter().any(|r| r.matches(domain));
        let mut ancestor = domain.as_str();
        let mut parent = (root, None);
        while let Some((_, suffix)) = ancestor.split_once('.') {
            if let Some(&enabled) = boundaries.get(suffix) {
                parent = (enabled, Some(suffix.to_owned()));
                break;
            }
            ancestor = suffix;
        }
        if wanted == parent.0 {
            continue;
        }
        if wanted {
            regions.insert(Some(domain.clone()), Vec::new());
        } else if let Some(excluded) = regions.get_mut(&parent.1) {
            excluded.push(domain.clone());
        }
        boundaries.insert(domain, wanted);
    }
    Ok(regions)
}

fn normalize_domain(domain: &str) -> Option<String> {
    let host = idna::domain_to_ascii(domain).ok()?.to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return None;
    }
    Some(host)
}

fn validate_selector(selector: &str) -> Result<(), CosmeticError> {
    if selector.is_empty() || selector.len() > MAX_SELECTOR_BYTES {
        return Err(CosmeticError::InvalidPolicy);
    }
    selector_preflight(selector)?;
    let parsed = CosmeticFilter::parse(
        &format!("example.invalid##{selector}"),
        false,
        Default::default(),
    )
    .map_err(|_| CosmeticError::InvalidPolicy)?;
    if parsed.action.is_some()
        || parsed.mask.contains(CosmeticFilterMask::SCRIPT_INJECT)
        || parsed.plain_css_selector() != Some(selector)
    {
        return Err(CosmeticError::InvalidPolicy);
    }
    Ok(())
}

fn selector_preflight(selector: &str) -> Result<(), CosmeticError> {
    if selector.is_empty() || selector.len() > MAX_SELECTOR_BYTES {
        return Err(CosmeticError::InvalidPolicy);
    }
    // Bound parser recursion before entering the upstream CSS grammar. This
    // is an admission ceiling, not a replacement for its syntax validation.
    let mut depth = 0u8;
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut bytes = selector.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if comment {
            if byte == b'*' && bytes.peek() == Some(&b'/') {
                bytes.next();
                comment = false;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'/' if bytes.peek() == Some(&b'*') => {
                bytes.next();
                comment = true;
            }
            b'(' | b'[' => {
                depth += 1;
                if depth > 32 {
                    return Err(CosmeticError::ResourceLimit);
                }
            }
            b')' | b']' => depth = depth.saturating_sub(1),
            b'{' | b'}' | b';' => return Err(CosmeticError::InvalidPolicy),
            _ => {}
        }
    }
    Ok(())
}

/// Validates and canonicalizes a personal selector without admitting scriptlets,
/// procedural actions, declarations or additional filter lines.
pub fn validate_personal_selector(selector: &str) -> Option<String> {
    if selector.chars().any(char::is_control) {
        return None;
    }
    selector_preflight(selector).ok()?;
    let parsed = CosmeticFilter::parse(
        &format!("example.invalid##{selector}"),
        false,
        Default::default(),
    )
    .ok()?;
    if parsed.action.is_some() || parsed.mask.contains(CosmeticFilterMask::SCRIPT_INJECT) {
        return None;
    }
    let canonical = parsed.plain_css_selector()?;
    validate_selector(canonical).ok()?;
    Some(canonical.to_owned())
}

/// Validates all personal rules, including disabled entries, before publishing
/// a complete derived snapshot. An invalid persisted row never becomes a
/// silently filtered subset. This operation performs no I/O.
pub fn prepare_site_preferences(
    preferences: &zephium_core::blocker::BlockerSitePreferences,
) -> Option<Arc<zephium_core::blocker::PreparedBlockerSites>> {
    let mut entries: BTreeMap<zephium_core::blocker::BlockerSite, (bool, BTreeSet<String>)> =
        BTreeMap::new();
    for site in preferences.paused_sites() {
        entries.entry(site.clone()).or_default().0 = true;
    }
    for hide in preferences.hides() {
        let selector = validate_personal_selector(&hide.selector)?;
        if hide.enabled {
            entries
                .entry(hide.site.clone())
                .or_default()
                .1
                .insert(selector);
        }
    }
    let entries = entries.into_iter().map(|(site, (paused, selectors))| {
        let mut css = String::new();
        for selector in selectors {
            css.push_str(&selector);
            css.push_str("{display:none!important}\n");
        }
        (site, paused, Arc::from(css))
    });
    zephium_core::blocker::PreparedBlockerSites::new(preferences.revision(), entries)
}

fn parse_rule(line: &str) -> Result<Rule, CosmeticError> {
    let first = line.find('#').ok_or(CosmeticError::InvalidPolicy)?;
    let second = line[first + 1..]
        .find('#')
        .ok_or(CosmeticError::InvalidPolicy)?
        + first
        + 1;
    selector_preflight(&line[second + 1..])?;
    let parsed = CosmeticFilter::parse(line, false, Default::default())
        .map_err(|_| CosmeticError::InvalidPolicy)?;
    if parsed.action.is_some() || parsed.mask.contains(CosmeticFilterMask::SCRIPT_INJECT) {
        return Err(CosmeticError::InvalidPolicy);
    }
    let selector = parsed
        .plain_css_selector()
        .ok_or(CosmeticError::InvalidPolicy)?
        .to_owned();
    validate_selector(&selector)?;
    let (scope, _) = line.split_once('#').ok_or(CosmeticError::InvalidPolicy)?;
    let mut include = BTreeSet::new();
    let mut exclude = BTreeSet::new();
    if !scope.is_empty() {
        for domain in scope.split(',') {
            let (domain, negative) = domain
                .strip_prefix('~')
                .map_or((domain, false), |d| (d, true));
            let domain = normalize_domain(domain).ok_or(CosmeticError::InvalidPolicy)?;
            if negative {
                exclude.insert(domain);
            } else {
                include.insert(domain);
            }
            if include.len() + exclude.len() > MAX_SCOPE_DOMAINS {
                return Err(CosmeticError::ResourceLimit);
            }
        }
    }
    if include.len() + exclude.len() > MAX_SCOPE_DOMAINS {
        return Err(CosmeticError::InvalidPolicy);
    }
    Ok(Rule {
        selector,
        include: include.into_iter().collect(),
        exclude: exclude.into_iter().collect(),
        exception: parsed.mask.contains(CosmeticFilterMask::UNHIDE),
    })
}

fn generic_control(line: &str) -> Result<Option<String>, CosmeticError> {
    let Some((pattern, options)) = line.rsplit_once('$') else {
        return Ok(None);
    };
    if !options
        .split(',')
        .any(|o| matches!(o, "generichide" | "ghide"))
    {
        return Ok(None);
    }
    let pattern = pattern
        .strip_prefix("@@")
        .ok_or(CosmeticError::InvalidPolicy)?;
    let mut output = vec!["document".to_owned()];
    for option in options.split(',') {
        if matches!(option, "generichide" | "ghide") {
            continue;
        }
        if option == "match-case" {
            output.push(option.into());
            continue;
        }
        let value = option
            .strip_prefix("domain=")
            .ok_or(CosmeticError::InvalidPolicy)?;
        let domains: Option<Vec<_>> = value
            .split('|')
            .map(|domain| {
                let (domain, negative) = domain
                    .strip_prefix('~')
                    .map_or((domain, false), |d| (d, true));
                normalize_domain(domain).map(|d| if negative { format!("~{d}") } else { d })
            })
            .collect();
        let domains = domains.ok_or(CosmeticError::InvalidPolicy)?;
        if domains.len() > MAX_SCOPE_DOMAINS {
            return Err(CosmeticError::ResourceLimit);
        }
        output.push(format!("domain={}", domains.join("|")));
    }
    Ok(Some(format!("{pattern}${}", output.join(","))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_exceptions_and_generic_controls_compose_across_sources() {
        let policy = CosmeticPolicy::compile([
            "##.ad\nexample.com##.specific\n~safe.example.com##.banner\n",
            "example.com#@#.ad\n@@||quiet.example.com^$generichide\n",
        ])
        .unwrap();
        let css = policy.stylesheet("https://www.example.com/").unwrap();
        assert!(!css.contains(".ad{"));
        assert!(css.contains(".specific{"));
        assert!(css.contains(".banner{"));
        assert!(!policy
            .stylesheet("https://safe.example.com/")
            .unwrap()
            .contains(".banner{"));
        let quiet = policy.stylesheet("https://quiet.example.com/").unwrap();
        assert_eq!(quiet.as_ref(), ".specific{display:none!important}\n");
        assert!(policy
            .stylesheet("https://notexample.com/")
            .unwrap()
            .contains(".ad{"));
    }

    #[test]
    fn unsupported_code_and_selector_injection_are_never_css() {
        let policy = CosmeticPolicy::compile([
            "example.com##+js(foo)\nexample.com##.ad:remove()\nexample.com##.ad:has-text(foo)\nexample.com##.ad{}body{color:red}\nexample.*##.ad\n##.good",
        ]).unwrap();
        assert_eq!(policy.report().accepted, 1);
        assert_eq!(policy.report().rejected, 5);
        assert_eq!(
            policy.stylesheet("https://example.com/").unwrap().as_ref(),
            ".good{display:none!important}\n"
        );
        assert!(CosmeticPolicy::compile(["@@||example.com^$generichide,third-party"]).is_err());
    }

    #[test]
    fn cache_round_trip_preserves_policy_and_revalidates_selectors() {
        let policy = CosmeticPolicy::compile(["##.ad\nexample.com#@#.ad"]).unwrap();
        let bytes = policy.encode().unwrap();
        let restored = CosmeticPolicy::decode(&bytes).unwrap();
        assert_eq!(restored.encode().unwrap(), bytes);
        assert_eq!(
            restored
                .stylesheet("https://example.com/")
                .unwrap()
                .as_ref(),
            ""
        );
        let mut data: Data = serde_json::from_slice(&bytes).unwrap();
        data.rules[0].selector = ".ad{}body{color:red}".into();
        assert!(CosmeticPolicy::decode(&serde_json::to_vec(&data).unwrap()).is_err());
    }

    #[test]
    fn unrelated_documents_share_the_generic_stylesheet_allocation() {
        let policy = CosmeticPolicy::compile(["##.ad\nexample.com#@#.ad"]).unwrap();
        let first = policy.stylesheet("https://first.invalid/").unwrap();
        let second = policy.stylesheet("https://second.invalid/").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(policy
            .stylesheet("https://example.com/")
            .unwrap()
            .is_empty());
        let deeply_nested = format!("{}a{}", ":is(".repeat(33), ")".repeat(33));
        assert!(validate_personal_selector(&deeply_nested).is_none());
        assert!(validate_personal_selector(".ad\n##body").is_none());
    }

    #[cfg(feature = "webkit")]
    #[test]
    fn native_top_document_scopes_reenable_explicit_scopes() {
        let policy = CosmeticPolicy::compile([
            "~safe.example,~safe.other##.ad\nsafe.example#@#.ad\nforce.safe.other##.ad\n@@||quiet.example^$generichide\nquiet.example##.specific",
        ]).unwrap();
        let encoded = policy.webkit_top_document_rules().unwrap().unwrap();
        let rules: Vec<serde_json::Value> = serde_json::from_str(&encoded).unwrap();
        assert_eq!(rules.len(), 4);
        assert!(rules[0]["trigger"]["unless-top-url"].is_array());
        assert!(rules[2]["trigger"]["url-filter"]
            .as_str()
            .unwrap()
            .contains("force\\.safe\\.other"));
        assert_eq!(rules[1]["action"]["type"], "ignore-previous-rules");
        assert_eq!(rules[3]["action"]["selector"], ".specific");
        assert!(!encoded.contains("\"if-domain\""));
        assert!(!encoded.contains("\"unless-domain\""));
    }
}
