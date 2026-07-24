//! Transforms filter rules into content blocking syntax used on iOS and MacOS.

use crate::filters::cosmetic::CosmeticFilter;
use crate::filters::network::{NetworkFilter, NetworkFilterFeaturesMask, NetworkFilterMask};
use crate::lists::ParsedLine;

use memchr::{memchr as find_char, memrchr as find_char_reverse};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use std::collections::HashSet;
use std::convert::{TryFrom, TryInto};

/// By default, ABP rules do not block top-level document requests. There's no way to express that
/// in content blocking format, so instead it's approximated with a rule that applies an exception
/// to any first-party requests that are document types.
///
/// This rule should be added after all other network rules.
pub fn ignore_previous_fp_documents() -> CbRule {
    let mut resource_type = HashSet::new();
    resource_type.insert(CbResourceType::Document);
    CbRule {
        trigger: CbTrigger {
            url_filter: String::from(".*"),
            resource_type: Some(resource_type),
            load_type: vec![CbLoadType::FirstParty],
            ..CbTrigger::default()
        },
        action: CbAction {
            typ: CbType::IgnorePreviousRules,
            selector: None,
        },
    }
}

/// Rust representation of a single content blocking rule.
///
/// This can be deserialized with `serde_json` directly into the correct format.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CbRule {
    pub action: CbAction,
    pub trigger: CbTrigger,
}

impl CbRule {
    /// If this returns false, the rule will not compile and should not be used.
    fn is_ascii(&self) -> bool {
        self.action.selector.iter().all(|s| s.is_ascii())
            && self.trigger.url_filter.is_ascii()
            && self
                .trigger
                .if_domain
                .iter()
                .flatten()
                .all(|d| d.is_ascii())
            && self
                .trigger
                .unless_domain
                .iter()
                .flatten()
                .all(|d| d.is_ascii())
            && self
                .trigger
                .if_top_url
                .iter()
                .flatten()
                .all(|d| d.is_ascii())
            && self
                .trigger
                .unless_top_url
                .iter()
                .flatten()
                .all(|d| d.is_ascii())
    }
}

/// Corresponds to the `action` field of a Safari content blocking rule.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CbAction {
    #[serde(rename = "type")]
    pub typ: CbType,
    /// Specify a string that defines a selector list. This value is required when the action type
    /// is css-display-none. If it's not, the selector field is ignored by Safari. Use CSS
    /// identifiers as the individual selector values, separated by commas. Safari and WebKit
    /// supports all of its CSS selectors for Safari content-blocking rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

/// Corresponds to the `action.type` field of a Safari content blocking rule.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CbType {
    /// Stops loading of the resource. If the resource was cached, the cache is ignored.
    Block,
    /// Strips cookies from the header before sending to the server. Only cookies otherwise
    /// acceptable to Safari's privacy policy can be blocked. Combining with ignore-previous-rules
    /// doesn't override the browser’s privacy settings.
    BlockCookies,
    /// Hides elements of the page based on a CSS selector. A selector field contains the selector
    /// list. Any matching element has its display property set to none, which hides it.
    CssDisplayNone,
    /// Ignores previously triggered actions.
    IgnorePreviousRules,
    /// Changes a URL from http to https. URLs with a specified (nondefault) port and links using
    /// other protocols are unaffected.
    MakeHttps,
}

/// Corresponds to possible entries in the `trigger.load_type` field of a Safari content blocking
/// rule.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CbLoadType {
    FirstParty,
    ThirdParty,
}

/// Corresponds to possible entries in the `trigger.resource_type` field of a Safari content
/// blocking rule.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CbResourceType {
    Document,
    Image,
    StyleSheet,
    Script,
    Font,
    Raw,
    SvgDocument,
    Media,
    Popup,
}

/// Corresponds to the `trigger` field of a Safari content blocking rule.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CbTrigger {
    /// Specifies a pattern to match the URL against.
    pub url_filter: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// A Boolean value. The default value is false.
    pub url_filter_is_case_sensitive: Option<bool>,
    /// An array of strings matched to a URL's domain; limits action to a list of specific domains.
    /// Values must be lowercase ASCII, or punycode for non-ASCII. Add * in front to match domain
    /// and subdomains. Can't be used with unless-domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_domain: Option<Vec<String>>,
    /// An array of strings matched to a URL's domain; acts on any site except domains in a
    /// provided list. Values must be lowercase ASCII, or punycode for non-ASCII. Add * in front to
    /// match domain and subdomains. Can't be used with if-domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unless_domain: Option<Vec<String>>,
    /// An array of strings representing the resource types (how the browser intends to use the
    /// resource) that the rule should match. If not specified, the rule matches all resource
    /// types. Valid values: document, image, style-sheet, script, font, raw (Any untyped load),
    /// svg-document, media, popup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<HashSet<CbResourceType>>,
    /// An array of strings that can include one of two mutually exclusive values. If not
    /// specified, the rule matches all load types. first-party is triggered only if the resource
    /// has the same scheme, domain, and port as the main page resource. third-party is triggered
    /// if the resource is not from the same domain as the main page resource.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_type: Vec<CbLoadType>,
    /// An array of strings matched to the entire main document URL; limits the action to a
    /// specific list of URL patterns. Values must be lowercase ASCII, or punycode for non-ASCII.
    /// Can't be used with unless-top-url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_top_url: Option<Vec<String>>,
    /// An array of strings matched to the entire main document URL; acts on any site except URL
    /// patterns in provided list. Values must be lowercase ASCII, or punycode for non-ASCII. Can't
    /// be used with if-top-url.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unless_top_url: Option<Vec<String>>,
}

/// Possible failure reasons when attempting to convert an adblock rule into content filtering
/// syntax.
#[derive(Debug)]
pub enum CbRuleCreationFailure {
    /// Currently, only filter rules parsed in debug mode can be translated into equivalent content
    /// blocking syntax.
    NeedsDebugMode,
    /// Content blocking rules cannot have if-domain and unless-domain together at the same time.
    UnlessAndIfDomainTogetherUnsupported,
    /// A network filter rule with only the given content type flags was provided, and none of them
    /// are supported. If at least one supported content type is provided, no failure will occur
    /// and unsupported types will be silently dropped.
    NoSupportedNetworkOptions(NetworkFilterMask),
    /// Network rules with redirect options cannot be represented in content blocking syntax.
    NetworkRedirectUnsupported,
    /// Network rules with generichide options cannot be supported in content blocking syntax.
    NetworkGenerichideUnsupported,
    /// Network rules with badfilter options cannot be supported in content blocking syntax.
    NetworkBadFilterUnsupported,
    /// Network rules with csp options cannot be supported in content blocking syntax.
    NetworkCspUnsupported,
    /// Network rules with removeparam options cannot be supported in content blocking syntax.
    NetworkRemoveparamUnsupported,
    /// Content blocking syntax only supports a subset of regex features, namely:
    /// - Matching any character with “.”.
    /// - Matching ranges with the range syntax [a-b].
    /// - Quantifying expressions with “?”, “+” and “*”.
    /// - Groups with parenthesis.
    ///
    /// It may be possible to correctly convert some full-regex rules, but others use unsupported
    /// features (e.g. quantified repetition with {...}) that make conversion to content blocking
    /// syntax impossible.
    FullRegexUnsupported,
    /// `Blocker`-internal `NetworkFilter`s can be represented in optimized form, but these cannot
    /// be currently converted into content blocking syntax.
    OptimizedRulesUnsupported,
    /// Cosmetic rules with entities (e.g. google.*) rather than hostnames cannot be represented in
    /// content blocking syntax.
    CosmeticEntitiesUnsupported,
    /// Cosmetic rules with custom action specification (i.e. `:style(...)`) cannot be represented
    /// in content blocking syntax.
    CosmeticActionRulesNotSupported,
    /// Cosmetic rules with scriptlet injections (i.e. `+js(...)`) cannot be represented in content
    /// blocking syntax.
    ScriptletInjectionsNotSupported,
    /// Valid content blocking rules can only include ASCII characters.
    RuleContainsNonASCII,
    /// A parsed network-domain option could not be normalized to a valid
    /// ASCII IDNA domain for the native content-blocker format.
    InvalidDomain,
    /// `from` as a `domain` alias is not currently supported in content blocking syntax.
    FromNotSupported,
    /// Content blocking rules cannot support procedural cosmetic filter operators.
    ProceduralCosmeticFiltersUnsupported,
}

impl TryFrom<ParsedLine<'_>> for CbRuleEquivalent {
    type Error = CbRuleCreationFailure;

    fn try_from(v: ParsedLine) -> Result<Self, Self::Error> {
        match v {
            ParsedLine::Network(f) => f.try_into(),
            ParsedLine::Cosmetic(f) => Ok(Self::SingleRule(f.try_into()?)),
        }
    }
}

fn non_empty(v: Vec<String>) -> Option<Vec<String>> {
    if !v.is_empty() { Some(v) } else { None }
}

fn normalize_network_domain(domain: &str) -> Result<String, CbRuleCreationFailure> {
    if domain.is_empty() {
        return Err(CbRuleCreationFailure::InvalidDomain);
    }
    let lowercase = domain.to_lowercase();
    let normalized =
        idna::domain_to_ascii(&lowercase).map_err(|_| CbRuleCreationFailure::InvalidDomain)?;
    if normalized.is_empty()
        || normalized.len() > 253
        || normalized.starts_with('.')
        || normalized.ends_with('.')
        || normalized.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(CbRuleCreationFailure::InvalidDomain);
    }
    Ok(normalized)
}

/// Some adblock rules cannot be directly represented by a single content blocking rule. This enum
/// serves as an intermediate conversion step that provides extra context on why one rule turned
/// into multiple rules.
///
/// The contained rules can be accessed using `IntoIterator`.
#[allow(clippy::large_enum_variant)]
pub enum CbRuleEquivalent {
    /// In most successful cases, an ABP rule can be converted into a single content blocking rule.
    SingleRule(CbRule),
    /// If a network rule has more than one specified resource type, one of those types is
    /// `Document`, and no load type is specified, then the rule should be split into two content
    /// blocking rules: the first has all original resource types except `Document`, and the second
    /// only specifies `Document` with a third-party load type.
    SplitDocument(CbRule, CbRule),
}

impl IntoIterator for CbRuleEquivalent {
    type Item = CbRule;
    type IntoIter = CbRuleEquivalentIterator;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Self::SingleRule(r) => CbRuleEquivalentIterator {
                rules: [Some(r), None],
                index: 0,
            },
            Self::SplitDocument(r1, r2) => CbRuleEquivalentIterator {
                rules: [Some(r1), Some(r2)],
                index: 0,
            },
        }
    }
}

/// Returned by [`CbRuleEquivalent`]'s `IntoIterator` implementation.
pub struct CbRuleEquivalentIterator {
    rules: [Option<CbRule>; 2],
    index: usize,
}

impl Iterator for CbRuleEquivalentIterator {
    type Item = CbRule;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.rules.len() {
            return None;
        }
        let result = self.rules[self.index].take();
        self.index += 1;
        result
    }
}

const ABP_SEPARATOR_CLASS: &str = "[^A-Za-z0-9_.%-]";

fn convert_network_pattern(pattern: &str) -> (String, bool) {
    let (pattern, has_trailing_separator) = pattern
        .strip_suffix('^')
        .map_or((pattern, false), |pattern| (pattern, true));
    let mut converted = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        match character {
            '*' => converted.push_str(".*"),
            '^' => converted.push_str(ABP_SEPARATOR_CLASS),
            '.' | '+' | '?' | '$' | '{' | '}' | '(' | ')' | '|' | '[' | ']' | '\\' => {
                converted.push('\\');
                converted.push(character);
            }
            _ => converted.push(character),
        }
    }
    (converted, has_trailing_separator)
}

fn finish_network_pattern(
    converted: &mut String,
    has_trailing_separator: bool,
    is_right_anchor: bool,
) {
    if has_trailing_separator {
        // ABP's final `^` is a separator byte or end-of-URL. WebKit rejects
        // alternation, but explicitly supports groups and `?`/`*`
        // quantifiers. Anchoring an optional suffix therefore represents both
        // branches in one native rule. An ABP right anchor permits at most the
        // separator itself; otherwise the suffix consumes the rest of the URL.
        converted.push('(');
        converted.push_str(ABP_SEPARATOR_CLASS);
        if !is_right_anchor {
            converted.push_str(".*");
        }
        converted.push_str(")?$");
    } else if is_right_anchor {
        converted.push('$');
    }
}

fn raw_pattern_and_anchors(raw_line: &str) -> &str {
    let pattern_and_anchors = find_char_reverse(b'$', raw_line.as_bytes())
        .map_or(raw_line, |options_index| &raw_line[..options_index]);
    pattern_and_anchors
        .strip_prefix("@@")
        .unwrap_or(pattern_and_anchors)
}

fn raw_pattern_ends_with_separator(raw_line: &str) -> bool {
    let pattern_and_anchors = raw_pattern_and_anchors(raw_line);
    pattern_and_anchors
        .strip_suffix('|')
        .unwrap_or(pattern_and_anchors)
        .ends_with('^')
}

fn split_document_rule(rule: CbRule) -> (CbRule, Option<CbRule>) {
    let Some(resource_types) = &rule.trigger.resource_type else {
        return (rule, None);
    };
    if resource_types.len() <= 1
        || !resource_types.contains(&CbResourceType::Document)
        || !rule.trigger.load_type.is_empty()
    {
        return (rule, None);
    }

    let mut non_doc_types = resource_types.clone();
    non_doc_types.remove(&CbResourceType::Document);
    let non_doc_rule = CbRule {
        trigger: CbTrigger {
            resource_type: Some(non_doc_types),
            ..rule.trigger.clone()
        },
        ..rule.clone()
    };
    let mut doc_type = HashSet::new();
    doc_type.insert(CbResourceType::Document);
    let just_doc_rule = CbRule {
        trigger: CbTrigger {
            resource_type: Some(doc_type),
            load_type: vec![CbLoadType::ThirdParty],
            ..rule.trigger
        },
        ..rule
    };
    (non_doc_rule, Some(just_doc_rule))
}

impl TryFrom<NetworkFilter<'_>> for CbRuleEquivalent {
    type Error = CbRuleCreationFailure;

    fn try_from(v: NetworkFilter) -> Result<Self, Self::Error> {
        static SPECIAL_CHARS: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r##"([.+?^${}()|\[\]\\])"##).unwrap());
        if let Some(raw_line) = v.raw_line.as_deref() {
            let has_trailing_separator = raw_pattern_ends_with_separator(raw_line);
            if v.is_redirect() {
                return Err(CbRuleCreationFailure::NetworkRedirectUnsupported);
            }
            if v.is_generic_hide() {
                return Err(CbRuleCreationFailure::NetworkGenerichideUnsupported);
            }
            debug_assert!(
                !v.features_mask
                    .contains(NetworkFilterFeaturesMask::BAD_FILTER),
                "BAD_FILTER should be filtered out"
            );
            if v.is_csp() {
                return Err(CbRuleCreationFailure::NetworkCspUnsupported);
            }
            if v.mask.contains(NetworkFilterMask::IS_COMPLETE_REGEX) {
                return Err(CbRuleCreationFailure::FullRegexUnsupported);
            }
            if v.is_removeparam() {
                return Err(CbRuleCreationFailure::NetworkRemoveparamUnsupported);
            }

            let load_type = if v
                .mask
                .contains(NetworkFilterMask::THIRD_PARTY | NetworkFilterMask::FIRST_PARTY)
            {
                vec![]
            } else if v.mask.contains(NetworkFilterMask::THIRD_PARTY) {
                vec![CbLoadType::ThirdParty]
            } else if v.mask.contains(NetworkFilterMask::FIRST_PARTY) {
                vec![CbLoadType::FirstParty]
            } else {
                vec![]
            };

            // The parser's hostname-right-anchor bit also represents ABP's
            // hostname separator boundary. Only a literal final `|` has the
            // stronger end-of-URL semantics needed by the compact suffix.
            let is_right_anchor = raw_pattern_and_anchors(raw_line).ends_with('|');
            let url_filter = match (v.filter, v.hostname) {
                (crate::filters::network::FilterPart::AnyOf(_), _) => {
                    return Err(CbRuleCreationFailure::OptimizedRulesUnsupported);
                }
                (crate::filters::network::FilterPart::Simple(part), Some(hostname)) => {
                    let (converted_pattern, converted_trailing_separator) =
                        convert_network_pattern(&part);
                    debug_assert_eq!(converted_trailing_separator, has_trailing_separator);
                    let mut prefix = format!(
                        "^[^:]+:(//)?([^/]+\\.)?{}",
                        SPECIAL_CHARS.replace_all(&hostname, r##"\$1"##)
                    );

                    if v.mask.contains(NetworkFilterMask::IS_HOSTNAME_REGEX) {
                        prefix += ".*";
                    }

                    let mut url_filter = format!("{prefix}{converted_pattern}");
                    finish_network_pattern(
                        &mut url_filter,
                        converted_trailing_separator,
                        is_right_anchor,
                    );
                    url_filter
                }
                (crate::filters::network::FilterPart::Simple(part), None) => {
                    let (converted_pattern, converted_trailing_separator) =
                        convert_network_pattern(&part);
                    debug_assert_eq!(converted_trailing_separator, has_trailing_separator);
                    let prefix = if v.mask.contains(NetworkFilterMask::IS_LEFT_ANCHOR) {
                        "^"
                    } else {
                        if v.mask
                            .contains(NetworkFilterMask::FROM_HTTP | NetworkFilterMask::FROM_HTTPS)
                        {
                            ""
                        } else if v.mask.contains(NetworkFilterMask::FROM_HTTP) {
                            "^http://.*"
                        } else if v.mask.contains(NetworkFilterMask::FROM_HTTPS) {
                            "^https://.*"
                        } else if v.mask.contains(NetworkFilterMask::FROM_WEBSOCKET) {
                            "^wss?://.*"
                        } else {
                            unreachable!("Invalid scheme information");
                        }
                    };
                    let mut url_filter = format!("{prefix}{converted_pattern}");
                    finish_network_pattern(
                        &mut url_filter,
                        converted_trailing_separator,
                        is_right_anchor,
                    );
                    url_filter
                }
                (crate::filters::network::FilterPart::Empty, Some(hostname)) => {
                    let escaped_special_chars = SPECIAL_CHARS.replace_all(&hostname, r##"\$1"##);
                    let mut url_filter = format!("^[^:]+:(//)?([^/]+\\.)?{escaped_special_chars}");
                    finish_network_pattern(
                        &mut url_filter,
                        has_trailing_separator,
                        is_right_anchor,
                    );
                    url_filter
                }
                (crate::filters::network::FilterPart::Empty, None) => {
                    let mut url_filter = if v
                        .mask
                        .contains(NetworkFilterMask::FROM_HTTP | NetworkFilterMask::FROM_HTTPS)
                    {
                        "^https?://"
                    } else if v.mask.contains(NetworkFilterMask::FROM_HTTP) {
                        "^http://"
                    } else if v.mask.contains(NetworkFilterMask::FROM_HTTPS) {
                        "^https://"
                    } else if v.mask.contains(NetworkFilterMask::FROM_WEBSOCKET) {
                        "^wss?://"
                    } else {
                        unreachable!("Invalid scheme information");
                    }
                    .to_string();
                    finish_network_pattern(
                        &mut url_filter,
                        has_trailing_separator,
                        is_right_anchor,
                    );
                    url_filter
                }
            };

            let (if_domain, unless_domain) =
                if v.opt_domains.is_some() || v.opt_not_domains.is_some() {
                    let mut if_domain = vec![];
                    let mut unless_domain = vec![];

                    // Network parsing uses the final '$' as the option boundary;
                    // URL patterns may themselves contain '$'. Conversion must
                    // derive domain text from that same boundary rather than
                    // reparsing an earlier pattern fragment as policy.
                    let options_index = find_char_reverse(b'$', raw_line.as_bytes())
                        .ok_or(CbRuleCreationFailure::FromNotSupported)?;
                    let opts = &raw_line[options_index + "$".len()..];
                    let domains = opts
                        .split(',')
                        .find_map(|option| option.strip_prefix("domain="))
                        .ok_or(CbRuleCreationFailure::FromNotSupported)?
                        .split('|');

                    for domain in domains {
                        let (collection, domain) =
                            if let Some(domain_stripped) = domain.strip_prefix('~') {
                                (&mut unless_domain, domain_stripped)
                            } else {
                                (&mut if_domain, domain)
                            };

                        let normalized_domain = normalize_network_domain(domain)?;

                        collection.push(format!("*{normalized_domain}"));
                    }

                    (non_empty(if_domain), non_empty(unless_domain))
                } else {
                    (None, None)
                };

            if if_domain.is_some() && unless_domain.is_some() {
                return Err(CbRuleCreationFailure::UnlessAndIfDomainTogetherUnsupported);
            }

            let blocking_type = if v.mask.contains(NetworkFilterMask::IS_EXCEPTION) {
                CbType::IgnorePreviousRules
            } else {
                CbType::Block
            };

            let resource_type = if v.mask.contains(NetworkFilterMask::FROM_NETWORK_TYPES) {
                None
            } else {
                let mut types = HashSet::new();
                let mut unsupported_flags = NetworkFilterMask::empty();

                macro_rules! push_if_flag {
                    ($flag:ident, $target:ident) => {
                        if v.mask.contains(NetworkFilterMask::$flag) {
                            types.insert(CbResourceType::$target);
                        }
                    };
                    ($flag:ident) => {
                        if v.mask.contains(NetworkFilterMask::$flag) {
                            unsupported_flags |= NetworkFilterMask::$flag;
                        }
                    };
                }
                push_if_flag!(FROM_IMAGE, Image);
                push_if_flag!(FROM_MEDIA, Media);
                push_if_flag!(FROM_OBJECT);
                push_if_flag!(FROM_OTHER);
                push_if_flag!(FROM_PING);
                push_if_flag!(FROM_SCRIPT, Script);
                push_if_flag!(FROM_STYLESHEET, StyleSheet);
                push_if_flag!(FROM_SUBDOCUMENT, Document);
                push_if_flag!(FROM_WEBSOCKET);
                push_if_flag!(FROM_XMLHTTPREQUEST, Raw);
                push_if_flag!(FROM_FONT, Font);
                // TODO - Popup, Document when implemented

                if !unsupported_flags.is_empty() && types.is_empty() {
                    return Err(CbRuleCreationFailure::NoSupportedNetworkOptions(
                        unsupported_flags,
                    ));
                }

                Some(types)
            };

            let url_filter_is_case_sensitive = if v.mask.contains(NetworkFilterMask::MATCH_CASE) {
                Some(true)
            } else {
                None
            };

            let single_rule = CbRule {
                action: CbAction {
                    typ: blocking_type,
                    selector: None,
                },
                trigger: CbTrigger {
                    url_filter,
                    load_type,
                    if_domain,
                    unless_domain,
                    resource_type,
                    url_filter_is_case_sensitive,
                    ..Default::default()
                },
            };
            if !single_rule.is_ascii() {
                return Err(CbRuleCreationFailure::RuleContainsNonASCII);
            }

            let (non_document, document) = split_document_rule(single_rule);
            match document {
                Some(document) => Ok(Self::SplitDocument(non_document, document)),
                None => Ok(Self::SingleRule(non_document)),
            }
        } else {
            Err(CbRuleCreationFailure::NeedsDebugMode)
        }
    }
}

impl TryFrom<CosmeticFilter> for CbRule {
    type Error = CbRuleCreationFailure;

    fn try_from(v: CosmeticFilter) -> Result<Self, Self::Error> {
        use crate::filters::cosmetic::{
            CosmeticFilterLocationType as LocationType, CosmeticFilterMask,
        };

        if v.action.is_some() {
            return Err(CbRuleCreationFailure::CosmeticActionRulesNotSupported);
        }
        if v.mask.contains(CosmeticFilterMask::SCRIPT_INJECT) {
            return Err(CbRuleCreationFailure::ScriptletInjectionsNotSupported);
        }

        if let Some(raw_line) = v.raw_line.as_deref() {
            let mut hostnames_vec = vec![];
            let mut not_hostnames_vec = vec![];

            let mut any_unsupported = false;

            // Unwrap is okay here - cosmetic rules must have a '#' character
            let sharp_index = find_char(b'#', raw_line.as_bytes()).unwrap();
            CosmeticFilter::locations_before_sharp(raw_line, sharp_index).for_each(
                |(location_type, location)| match location_type {
                    LocationType::Entity | LocationType::NotEntity | LocationType::Unsupported => {
                        any_unsupported = true
                    }
                    LocationType::Hostname => {
                        if let Ok(encoded) = idna::domain_to_ascii(location) {
                            hostnames_vec.push(encoded);
                        }
                    }
                    LocationType::NotHostname => {
                        if let Ok(encoded) = idna::domain_to_ascii(location) {
                            not_hostnames_vec.push(encoded);
                        }
                    }
                },
            );

            if any_unsupported && hostnames_vec.is_empty() && not_hostnames_vec.is_empty() {
                return Err(CbRuleCreationFailure::CosmeticEntitiesUnsupported);
            }

            let hostnames_vec = non_empty(hostnames_vec);
            let not_hostnames_vec = non_empty(not_hostnames_vec);

            if hostnames_vec.is_some() && not_hostnames_vec.is_some() {
                return Err(CbRuleCreationFailure::UnlessAndIfDomainTogetherUnsupported);
            }

            let (unless_domain, if_domain) = if v.mask.contains(CosmeticFilterMask::UNHIDE) {
                (hostnames_vec, not_hostnames_vec)
            } else {
                (not_hostnames_vec, hostnames_vec)
            };

            let selector = if let Some(selector) = v.plain_css_selector() {
                selector.to_string()
            } else {
                return Err(CbRuleCreationFailure::ProceduralCosmeticFiltersUnsupported);
            };

            let rule = Self {
                action: CbAction {
                    typ: CbType::CssDisplayNone,
                    selector: Some(selector),
                },
                trigger: CbTrigger {
                    url_filter: ".*".to_string(),
                    if_domain,
                    unless_domain,
                    ..Default::default()
                },
            };

            if !rule.is_ascii() {
                return Err(CbRuleCreationFailure::RuleContainsNonASCII);
            }

            Ok(rule)
        } else {
            Err(CbRuleCreationFailure::NeedsDebugMode)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/content_blocking.rs"]
mod unit_tests;
