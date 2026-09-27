use std::mem::size_of;

use zephium_core::extensions::{
    ApiPermissionName, ExtensionGrantBrowsingContext, ExtensionGrantDigest, ExtensionGrantRevision,
    ExtensionNativeGrantDecision, ExtensionNativeGrantRequirement, ExtensionRuntimeGeneration,
    ExtensionRuntimeInstance, MAX_EXTENSION_API_PERMISSIONS, MAX_EXTENSION_HOST_GRANTS,
};
use zephium_core::ids::{ExtensionInstallId, ProfileId};
use zephium_core::injection::MatchPattern;

use super::*;

const REQUIRED: ExtensionNativeGrantRequirement = ExtensionNativeGrantRequirement::Required;
const OPTIONAL: ExtensionNativeGrantRequirement = ExtensionNativeGrantRequirement::Optional;
const GRANTED: ExtensionNativeGrantDecision = ExtensionNativeGrantDecision::Granted;
const DENIED: ExtensionNativeGrantDecision = ExtensionNativeGrantDecision::Denied;

fn identity(seed: u8) -> PlanIdentity {
    PlanIdentity {
        schema: MacosNativeGrantSchema::WkWebExtensionV1,
        apply_mode: MacosNativeGrantApplyMode::ReplaceCompleteGrantedAndDeniedSetsVerifyReadback,
        runtime: ExtensionRuntimeInstance::new(
            ProfileId::from(u128::from(seed)),
            ExtensionInstallId::from(u128::from(seed) + 1),
            ExtensionRuntimeGeneration::new(u64::from(seed) + 1).expect("nonzero generation"),
        ),
        grant_revision: ExtensionGrantRevision::new(u64::from(seed) + 1)
            .expect("nonzero grant revision"),
        grant_digest: ExtensionGrantDigest::from_bytes([seed; 32]),
    }
}

fn brokered_identity(seed: u8) -> PlanIdentity {
    PlanIdentity {
        schema: MacosNativeGrantSchema::WkWebExtensionBrokeredV1,
        ..identity(seed)
    }
}

fn api(
    name: &str,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
) -> ApiGrantInput<'_> {
    ApiGrantInput {
        name,
        requirement,
        decision,
    }
}

fn host(
    pattern: &MatchPattern,
    requirement: ExtensionNativeGrantRequirement,
    decision: ExtensionNativeGrantDecision,
) -> HostGrantInput<'_> {
    HostGrantInput {
        pattern,
        requirement,
        decision,
    }
}

fn compile<'a>(
    context: ExtensionGrantBrowsingContext,
    file_access_granted: bool,
    private_access_granted: bool,
    api_grants: &'a [ApiGrantInput<'a>],
    host_grants: &'a [HostGrantInput<'a>],
) -> Result<MacosNativeGrantPlan, MacosNativeGrantPlanError> {
    compile_projection(CompilerInput {
        identity: identity(7),
        browsing_context: context,
        file_access_granted,
        private_access_granted,
        api_count: api_grants.len(),
        api_grants: api_grants.iter().copied(),
        host_count: host_grants.len(),
        host_grants: host_grants.iter().copied(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    })
}

fn regular(
    api_grants: &[ApiGrantInput<'_>],
    host_grants: &[HostGrantInput<'_>],
) -> Result<MacosNativeGrantPlan, MacosNativeGrantPlanError> {
    compile(
        ExtensionGrantBrowsingContext::Regular,
        false,
        false,
        api_grants,
        host_grants,
    )
}

fn parse(pattern: &str) -> MatchPattern {
    MatchPattern::parse(pattern).expect("valid test match pattern")
}

fn compiled_api_permissions(plan: &MacosNativeGrantPlan) -> Vec<&str> {
    plan.granted_api_permissions()
        .iter()
        .map(|permission| permission.as_str())
        .collect()
}

fn compiled_host_patterns(plan: &MacosNativeGrantPlan) -> Vec<&str> {
    plan.granted_host_patterns().collect()
}

#[test]
fn brokered_schema_keeps_compatibility_only_permissions_out_of_native_sets() {
    let grants = [
        api("bookmarks", REQUIRED, GRANTED),
        api("favicon", REQUIRED, GRANTED),
        api("history", REQUIRED, GRANTED),
        api("nativeMessaging", REQUIRED, GRANTED),
        api("search", REQUIRED, GRANTED),
        api("sessions", REQUIRED, GRANTED),
        api("storage", REQUIRED, GRANTED),
    ];
    let plan = compile_projection(CompilerInput {
        identity: brokered_identity(9),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: grants.len(),
        api_grants: grants,
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    })
    .expect("brokered v1 permission cohort");

    assert_eq!(
        compiled_api_permissions(&plan),
        ["nativeMessaging", "storage"]
    );
    assert_eq!(
        plan.schema(),
        MacosNativeGrantSchema::WkWebExtensionBrokeredV1
    );
    assert_eq!(
        MacosNativeGrantSchema::WkWebExtensionV1.permission_disposition("history"),
        Ok(MacosNativeApiPermissionDisposition::ProductProhibited)
    );
    assert_eq!(
        MacosNativeGrantSchema::WkWebExtensionV1.permission_disposition("nativeMessaging"),
        Ok(MacosNativeApiPermissionDisposition::ProductProhibited)
    );
}

#[test]
fn capability_target_selects_brokered_schema_for_exact_session_grants() {
    let target = zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V1_COMPATIBILITY_TARGET;
    let schema = select_native_grant_schema(target, false).unwrap();
    assert_eq!(schema, MacosNativeGrantSchema::WkWebExtensionBrokeredV1);
    let grants = [
        api("sessions", REQUIRED, GRANTED),
        api("tabs", REQUIRED, GRANTED),
        api("nativeMessaging", REQUIRED, GRANTED),
    ];
    let plan = compile_projection(CompilerInput {
        identity: PlanIdentity {
            schema,
            ..identity(71)
        },
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: grants.len(),
        api_grants: grants,
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    })
    .expect("the new target's exact internal broker and tabs grants compile");
    assert_eq!(
        plan.schema(),
        MacosNativeGrantSchema::WkWebExtensionBrokeredV1
    );
    assert_eq!(compiled_api_permissions(&plan), ["nativeMessaging", "tabs"]);
    assert_eq!(
        select_native_grant_schema(target, true).unwrap(),
        MacosNativeGrantSchema::WkWebExtensionPublisherNativeMessagingV1,
        "publisher-native selection retains its precedence"
    );
    assert_eq!(
        select_native_grant_schema("macos.wkwebextension.v1", false).unwrap(),
        MacosNativeGrantSchema::WkWebExtensionV1
    );
}

#[test]
fn supported_api_table_is_closed_canonical_and_complete() {
    let names = [
        "webRequest",
        "tabs",
        "storage",
        "scripting",
        "notifications",
        "menus",
        "webNavigation",
        "declarativeNetRequestWithHostAccess",
        "declarativeNetRequestFeedback",
        "declarativeNetRequest",
        "cookies",
        "contextMenus",
        "clipboardWrite",
        "alarms",
        "activeTab",
    ];
    let grants: Vec<_> = names
        .iter()
        .map(|name| api(name, REQUIRED, GRANTED))
        .collect();

    let plan = regular(&grants, &[]).expect("supported API grants");
    let actual = compiled_api_permissions(&plan);
    assert_eq!(
        actual,
        [
            "activeTab",
            "alarms",
            "clipboardWrite",
            "contextMenus",
            "cookies",
            "declarativeNetRequest",
            "declarativeNetRequestFeedback",
            "declarativeNetRequestWithHostAccess",
            "menus",
            "notifications",
            "scripting",
            "storage",
            "tabs",
            "webNavigation",
            "webRequest",
        ]
    );
}

#[test]
fn prohibited_unknown_and_manifest_only_api_tokens_remain_distinct() {
    for name in ["history", "nativeMessaging", "unlimitedStorage"] {
        let prohibited = regular(&[api(name, OPTIONAL, GRANTED)], &[])
            .expect_err("effective broker-only capability is product-prohibited");
        assert_eq!(
            prohibited,
            MacosNativeGrantPlanError::ProhibitedApiPermission
        );
        assert!(!format!("{prohibited:?}").contains(name));
        assert!(!prohibited.to_string().contains(name));
        assert!(compiled_api_permissions(
            &regular(&[api(name, OPTIONAL, DENIED)], &[])
                .expect("an unrequested optional prohibited capability is absent")
        )
        .is_empty());
    }

    for name in [
        "clipboardRead",
        "downloads",
        "fontSettings",
        "idle",
        "management",
        "offscreen",
        "privacy",
        "sidePanel",
        "webRequestAuthProvider",
    ] {
        for decision in [GRANTED, DENIED] {
            let plan = regular(&[api(name, OPTIONAL, decision)], &[])
                .expect("WebKit-accepted manifest-only token has no native grant key");
            assert!(compiled_api_permissions(&plan).is_empty());
        }
        assert!(compiled_api_permissions(
            &regular(&[api(name, REQUIRED, GRANTED)], &[])
                .expect("granted required manifest-only capability")
        )
        .is_empty());
        assert_eq!(
            regular(&[api(name, REQUIRED, DENIED)], &[])
                .expect_err("required manifest-only authority remains mandatory"),
            MacosNativeGrantPlanError::RequiredApiGrantDenied
        );
    }

    let name = "futurePermission";
    for decision in [GRANTED, DENIED] {
        let error = regular(&[api(name, OPTIONAL, decision)], &[])
            .expect_err("unknown declaration must fail regardless of status");
        assert_eq!(error, MacosNativeGrantPlanError::UnsupportedApiPermission);
        assert!(!format!("{error:?}").contains(name));
        assert!(!error.to_string().contains(name));
    }
}

#[test]
fn pinned_bitwarden_contract_is_refused_before_unbounded_storage_can_activate() {
    let required = [
        "activeTab",
        "alarms",
        "clipboardRead",
        "clipboardWrite",
        "contextMenus",
        "idle",
        "offscreen",
        "scripting",
        "sidePanel",
        "storage",
        "tabs",
        "unlimitedStorage",
        "webNavigation",
        "webRequest",
        "webRequestAuthProvider",
        "notifications",
    ];
    let optional = ["nativeMessaging", "privacy"];
    let grants = required
        .iter()
        .map(|name| api(name, REQUIRED, GRANTED))
        .chain(optional.iter().map(|name| api(name, OPTIONAL, DENIED)))
        .collect::<Vec<_>>();

    assert_eq!(
        regular(&grants, &[]).expect_err("unbounded storage must be product-prohibited"),
        MacosNativeGrantPlanError::ProhibitedApiPermission
    );
}

#[test]
fn capabilities_v2_offscreen_grants_compile_only_for_the_exact_target() {
    let target = zephium_core::extensions::LOCAL_MACOS_CAPABILITIES_V2_COMPATIBILITY_TARGET;
    let schema = select_native_grant_schema(target, false).unwrap();
    assert_eq!(schema, MacosNativeGrantSchema::WkWebExtensionCapabilitiesV2);
    assert_eq!(
        select_native_grant_schema(target, true),
        Err(MacosNativeGrantPlanError::ProhibitedApiPermission),
        "the v2 offscreen broker cannot borrow publisher-native host authority"
    );
    let required = [
        "activeTab",
        "alarms",
        "clipboardWrite",
        "contextMenus",
        "idle",
        "offscreen",
        "scripting",
        "storage",
        "tabs",
        "webNavigation",
        "webRequest",
        "webRequestAuthProvider",
        "notifications",
        "nativeMessaging",
    ];
    let grants = required
        .iter()
        .map(|name| api(name, REQUIRED, GRANTED))
        .chain([api("privacy", OPTIONAL, DENIED)])
        .collect::<Vec<_>>();
    let plan = compile_projection(CompilerInput {
        identity: PlanIdentity {
            schema,
            ..identity(72)
        },
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: grants.len(),
        api_grants: grants.iter().copied(),
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    })
    .expect("prepared v2 native permission cohort");
    assert_eq!(
        compiled_api_permissions(&plan),
        [
            "activeTab",
            "alarms",
            "clipboardWrite",
            "contextMenus",
            "nativeMessaging",
            "notifications",
            "scripting",
            "storage",
            "tabs",
            "webNavigation",
            "webRequest",
        ]
    );
    assert_eq!(
        regular(&grants, &[]).expect_err("ordinary runtime cannot expose native messaging"),
        MacosNativeGrantPlanError::ProhibitedApiPermission
    );
    assert_eq!(
        schema.permission_disposition("history"),
        Ok(MacosNativeApiPermissionDisposition::ProductProhibited)
    );
}

#[test]
fn never_requested_optional_api_is_omitted_without_native_prompt_suppression() {
    let plan = regular(&[api("storage", OPTIONAL, DENIED)], &[])
        .expect("supported optional absence is representable");
    assert!(compiled_api_permissions(&plan).is_empty());
    assert_eq!(
        plan.apply_mode(),
        MacosNativeGrantApplyMode::ReplaceCompleteGrantedAndDeniedSetsVerifyReadback
    );
}

#[test]
fn every_required_denial_refuses_the_complete_plan() {
    assert_eq!(
        regular(&[api("storage", REQUIRED, DENIED)], &[]).expect_err("required API denial"),
        MacosNativeGrantPlanError::RequiredApiGrantDenied
    );

    let pattern = parse("https://example.com/*");
    assert_eq!(
        regular(&[], &[host(&pattern, REQUIRED, DENIED)]).expect_err("required host denial"),
        MacosNativeGrantPlanError::RequiredHostGrantDenied
    );
}

#[test]
fn rendered_bytes_define_adversarial_sort_and_deduplication_order() {
    let patterns = [
        parse("*://*.example.com/*"),
        parse("http://127.0.0.1/*"),
        parse("http://alpha.example/*"),
        parse("http://*/*"),
    ];
    let grants: Vec<_> = patterns
        .iter()
        .map(|pattern| host(pattern, REQUIRED, GRANTED))
        .collect();

    let plan = regular(&[], &grants).expect("representable web patterns");
    let expected = ["http://*/*", "https://*.example.com/*"];
    assert_eq!(compiled_host_patterns(&plan), expected);
    assert_eq!(
        plan.host_pattern_arena.len(),
        expected.iter().map(|pattern| pattern.len()).sum::<usize>()
    );
    assert_eq!(plan.host_pattern_spans.len(), expected.len());
}

#[test]
fn absent_rows_never_become_native_denials_or_override_effective_grants() {
    let api_plan = regular(
        &[
            api("storage", OPTIONAL, GRANTED),
            api("storage", OPTIONAL, DENIED),
        ],
        &[],
    )
    .expect("effective API grant wins over declaration-local absence");
    assert_eq!(compiled_api_permissions(&api_plan), ["storage"]);

    let all_urls = parse("<all_urls>");
    let explicit_http = parse("http://*/*");
    let same_key_plan = regular(
        &[],
        &[
            host(&all_urls, OPTIONAL, GRANTED),
            host(&explicit_http, OPTIONAL, DENIED),
        ],
    )
    .expect("same-key absence cannot erase the effective broad grant");
    assert_eq!(
        compiled_host_patterns(&same_key_plan),
        ["http://*/*", "https://*/*"]
    );

    let broad = parse("https://*.example.com/*");
    let narrow = parse("https://login.example.com/*");
    let overlap_plan = regular(
        &[],
        &[
            host(&broad, OPTIONAL, GRANTED),
            host(&narrow, OPTIONAL, DENIED),
        ],
    )
    .expect("narrow absence must never become prompt-suppressing native denial");
    assert_eq!(
        compiled_host_patterns(&overlap_plan),
        ["https://*.example.com/*"]
    );
    assert_eq!(
        overlap_plan.apply_mode(),
        MacosNativeGrantApplyMode::ReplaceCompleteGrantedAndDeniedSetsVerifyReadback
    );
}

#[test]
fn all_urls_never_smuggles_file_access_into_the_native_plan() {
    let all_urls = parse("<all_urls>");
    let plan = regular(&[], &[host(&all_urls, REQUIRED, GRANTED)])
        .expect("web subset of all URLs is representable");
    assert_eq!(compiled_host_patterns(&plan), ["http://*/*", "https://*/*"]);
}

#[test]
fn overlapping_effective_host_grants_compile_to_one_native_union() {
    let patterns = [
        "*://*.youtube.com/*",
        "https://www.youtube.com/*",
        "https://m.youtube.com/*",
        "http://www.youtube.com/*",
        "https://youtube.com/*",
        "https://*.nested.youtube.com/*",
    ]
    .map(parse);
    for reverse in [false, true] {
        let mut grants = patterns
            .iter()
            .map(|pattern| host(pattern, REQUIRED, GRANTED))
            .collect::<Vec<_>>();
        if reverse {
            grants.reverse();
        }
        let plan = regular(&[], &grants).unwrap();
        assert_eq!(
            compiled_host_patterns(&plan),
            ["http://*.youtube.com/*", "https://*.youtube.com/*"]
        );
    }
}

#[test]
fn native_host_compaction_preserves_url_access_across_grant_combinations() {
    let inputs = [
        "*://*.example.com/*",
        "https://a.example.com/*",
        "https://*.a.example.com/*",
        "http://*/*",
        "https://127.0.0.1/*",
    ]
    .map(parse);
    let urls = [
        "https://example.com/",
        "http://example.com/",
        "https://a.example.com/account?x=1",
        "http://a.example.com/",
        "https://b.a.example.com/",
        "https://notexample.com/",
        "https://example.com.other.test/",
        "http://other.test/",
        "https://other.test/",
        "https://127.0.0.1/",
        "http://127.0.0.1/",
        "https://example.com:8443/",
        "file:///tmp/example",
    ]
    .map(|url| url::Url::parse(url).unwrap());
    for mask in 0..(1 << inputs.len()) {
        let selected = inputs
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, pattern)| pattern)
            .collect::<Vec<_>>();
        let grants = selected
            .iter()
            .map(|pattern| host(pattern, REQUIRED, GRANTED))
            .collect::<Vec<_>>();
        let plan = regular(&[], &grants).unwrap();
        let native = plan.granted_host_patterns().map(parse).collect::<Vec<_>>();
        for url in &urls {
            assert_eq!(
                selected.iter().any(|pattern| pattern.matches_url(url)),
                native.iter().any(|pattern| pattern.matches_url(url)),
                "access changed for mask {mask} at {url}"
            );
        }
    }
}

#[test]
fn native_host_compaction_preserves_scheme_domain_and_denial_boundaries() {
    let broad = parse("https://*.example.com/*");
    let narrow = parse("https://app.example.com/*");
    let plain_http = parse("http://app.example.com/*");
    let suffix_collision = parse("https://notexample.com/*");
    let prefix_collision = parse("https://example.com.other.test/*");
    let plan = regular(
        &[],
        &[
            host(&broad, OPTIONAL, DENIED),
            host(&narrow, REQUIRED, GRANTED),
        ],
    )
    .unwrap();
    assert_eq!(compiled_host_patterns(&plan), ["https://app.example.com/*"]);
    let plan = regular(
        &[],
        &[
            host(&broad, REQUIRED, GRANTED),
            host(&narrow, REQUIRED, GRANTED),
            host(&plain_http, REQUIRED, GRANTED),
            host(&suffix_collision, REQUIRED, GRANTED),
            host(&prefix_collision, REQUIRED, GRANTED),
        ],
    )
    .unwrap();
    assert_eq!(
        compiled_host_patterns(&plan),
        [
            "http://app.example.com/*",
            "https://*.example.com/*",
            "https://example.com.other.test/*",
            "https://notexample.com/*",
        ]
    );
}

#[test]
fn dynamic_all_urls_response_is_web_only_and_file_only_requests_are_refused() {
    let all_urls = parse("<all_urls>");
    let response = super::compile_runtime_host_permission_response(&[all_urls]).unwrap();
    assert_eq!(&*response, ["http://*/*", "https://*/*"]);

    let file = parse("file:///tmp/*");
    assert_eq!(
        super::compile_runtime_host_permission_response(&[file]),
        Err(MacosNativeGrantPlanError::FileAccessUnproven)
    );
}

#[test]
fn dynamic_api_request_reuses_the_native_product_prohibition_boundary() {
    let permitted = ApiPermissionName::parse_exact("clipboardRead").unwrap();
    assert!(super::validate_runtime_api_permission_request(&[permitted]).is_ok());

    let prohibited = ApiPermissionName::parse_exact("nativeMessaging").unwrap();
    assert_eq!(
        super::validate_runtime_api_permission_request(&[prohibited]),
        Err(MacosNativeGrantPlanError::ProhibitedApiPermission)
    );
}

#[test]
fn file_only_rows_are_omitted_while_the_independent_file_gate_is_false() {
    let file = parse("file:///tmp/*");
    for row in [
        host(&file, OPTIONAL, DENIED),
        host(&file, OPTIONAL, GRANTED),
        host(&file, REQUIRED, GRANTED),
    ] {
        let plan = regular(&[], &[row]).expect("file-only row is gated independently");
        assert_eq!(plan.granted_host_patterns().len(), 0);
    }

    assert_eq!(
        compile(
            ExtensionGrantBrowsingContext::Regular,
            true,
            false,
            &[],
            &[]
        )
        .expect_err("effective file gate remains unproven"),
        MacosNativeGrantPlanError::FileAccessUnproven
    );
}

#[test]
fn private_context_or_effective_private_access_refuses_before_translation() {
    assert_eq!(
        compile(
            ExtensionGrantBrowsingContext::Private,
            false,
            false,
            &[],
            &[]
        )
        .expect_err("private partition"),
        MacosNativeGrantPlanError::PrivateRuntimeUnsupported
    );
    assert_eq!(
        compile(
            ExtensionGrantBrowsingContext::Regular,
            false,
            true,
            &[],
            &[]
        )
        .expect_err("effective private grant"),
        MacosNativeGrantPlanError::PrivateRuntimeUnsupported
    );
}

#[test]
fn exact_ports_non_root_paths_and_ipv6_are_rejected_independently() {
    let exact_port = parse("https://example.com:8443/*");
    for decision in [GRANTED, DENIED] {
        assert_eq!(
            regular(&[], &[host(&exact_port, OPTIONAL, decision)]).expect_err("exact port"),
            MacosNativeGrantPlanError::ExactPortUnsupported
        );
    }
    assert_eq!(
        regular(&[], &[host(&exact_port, REQUIRED, DENIED)])
            .expect_err("required denial still validates native shape first"),
        MacosNativeGrantPlanError::ExactPortUnsupported
    );

    for source in [
        "https://example.com/account/*",
        "https://example.com/*?query=*",
    ] {
        let pattern = parse(source);
        for decision in [GRANTED, DENIED] {
            assert_eq!(
                regular(&[], &[host(&pattern, OPTIONAL, decision)])
                    .expect_err("non-root-wildcard path"),
                MacosNativeGrantPlanError::PathSemanticsUnsupported
            );
        }
    }

    let ipv6 = parse("https://[2001:db8::1]/*");
    for decision in [GRANTED, DENIED] {
        assert_eq!(
            regular(&[], &[host(&ipv6, OPTIONAL, decision)]).expect_err("IPv6 host"),
            MacosNativeGrantPlanError::Ipv6HostUnsupported
        );
    }
}

#[test]
fn all_urls_native_grant_subsumes_narrow_content_routes_without_widening() {
    let all_urls = parse("<all_urls>");
    let path_route = parse("https://example.com/account/*");
    let query_route = parse("https://example.com/*?query=*");
    let exact_port = parse("https://example.com:8443/*");
    let ipv6 = parse("https://[2001:db8::1]/*");
    let plan = regular(
        &[],
        &[
            host(&all_urls, REQUIRED, GRANTED),
            host(&path_route, REQUIRED, GRANTED),
            host(&query_route, REQUIRED, GRANTED),
            host(&exact_port, REQUIRED, GRANTED),
            host(&ipv6, REQUIRED, GRANTED),
        ],
    )
    .expect("the broad effective authority subsumes narrower content routes");
    assert_eq!(compiled_host_patterns(&plan), ["http://*/*", "https://*/*"]);

    assert_eq!(
        regular(
            &[],
            &[
                host(&all_urls, REQUIRED, GRANTED),
                host(&path_route, REQUIRED, DENIED),
            ],
        )
        .expect_err("a denied required route is never hidden by broad authority"),
        MacosNativeGrantPlanError::PathSemanticsUnsupported
    );
}

#[test]
fn ipv4_rendering_preserves_zero_single_double_and_triple_digit_octets() {
    let ipv4 = parse("http://0.9.10.255/*");
    let plan = regular(&[], &[host(&ipv4, OPTIONAL, GRANTED)]).expect("representable IPv4 grant");
    assert_eq!(compiled_host_patterns(&plan), ["http://0.9.10.255/*"]);
}

#[test]
fn declared_counts_and_input_limits_are_checked_before_compaction() {
    let mismatched = compile_projection(CompilerInput {
        identity: identity(1),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 1,
        api_grants: std::iter::empty(),
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        mismatched.expect_err("projection count mismatch"),
        MacosNativeGrantPlanError::DeclaredCountMismatch
    );

    let too_many_api = compile_projection(CompilerInput {
        identity: identity(1),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: MAX_EXTENSION_API_PERMISSIONS + 1,
        api_grants: std::iter::empty(),
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        too_many_api.expect_err("API entry bound"),
        MacosNativeGrantPlanError::ApiEntryLimitExceeded
    );

    let too_many_hosts = compile_projection(CompilerInput {
        identity: identity(1),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::empty(),
        host_count: MAX_EXTENSION_HOST_GRANTS + 1,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        too_many_hosts.expect_err("host entry bound"),
        MacosNativeGrantPlanError::HostEntryLimitExceeded
    );
}

#[test]
fn surplus_and_infinite_iterators_stop_before_exceeding_declared_capacity() {
    let surplus_api = compile_projection(CompilerInput {
        identity: identity(2),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::once(api("futurePermission", OPTIONAL, GRANTED)),
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        surplus_api.expect_err("surplus API row must precede taxonomy and push"),
        MacosNativeGrantPlanError::DeclaredCountMismatch
    );

    let exact_port = parse("https://example.com:8443/*");
    let surplus_host = compile_projection(CompilerInput {
        identity: identity(2),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::empty(),
        host_count: 0,
        host_grants: std::iter::once(host(&exact_port, OPTIONAL, GRANTED)),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        surplus_host.expect_err("surplus host row must precede translation and push"),
        MacosNativeGrantPlanError::DeclaredCountMismatch
    );

    let infinite_api = compile_projection(CompilerInput {
        identity: identity(2),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: MAX_EXTENSION_API_PERMISSIONS,
        api_grants: std::iter::repeat(api("storage", OPTIONAL, GRANTED)),
        host_count: 0,
        host_grants: std::iter::empty(),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        infinite_api.expect_err("infinite API iterator is bounded by admitted capacity"),
        MacosNativeGrantPlanError::ApiEntryLimitExceeded
    );

    let all_urls = parse("<all_urls>");
    let infinite_host = compile_projection(CompilerInput {
        identity: identity(2),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::empty(),
        host_count: MAX_EXTENSION_HOST_GRANTS,
        host_grants: std::iter::repeat(host(&all_urls, OPTIONAL, GRANTED)),
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    });
    assert_eq!(
        infinite_host.expect_err("infinite host iterator is bounded by admitted capacity"),
        MacosNativeGrantPlanError::HostEntryLimitExceeded
    );
}

#[test]
fn compact_retained_accounting_is_exact_for_empty_and_populated_plans() {
    let empty = regular(&[], &[]).expect("empty representable plan");
    assert_eq!(empty.retained_bytes(), size_of::<MacosNativeGrantPlan>());

    let api_grants = [api("storage", REQUIRED, GRANTED)];
    let pattern = parse("https://example.com/*");
    let host_grants = [host(&pattern, REQUIRED, GRANTED)];
    let populated = regular(&api_grants, &host_grants).expect("populated plan");
    let expected = size_of::<MacosNativeGrantPlan>()
        + RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES
        + size_of::<MacosNativeApiPermission>()
        + RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES
        + "https://example.com/*".len()
        + RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES
        + size_of::<PatternSpan>();
    assert_eq!(populated.retained_bytes(), expected);
    assert!(populated.retained_bytes() <= MAX_MACOS_NATIVE_GRANT_PLAN_RETAINED_BYTES);

    assert_eq!(
        retained_boxed_allocation_bytes(usize::MAX).expect_err("allocation overhead overflow"),
        MacosNativeGrantPlanError::RetainedBytesOverflow
    );
    assert_eq!(
        calculate_retained_bytes(0, 0, usize::MAX, 0).expect_err("span multiplication overflow"),
        MacosNativeGrantPlanError::RetainedBytesOverflow
    );
}

#[test]
fn compiler_exposes_a_conservative_logical_transient_heap_ceiling() {
    let exact_ceiling =
        2 * MAX_MACOS_NATIVE_API_PERMISSIONS * size_of::<MacosNativeApiPermission>()
            + MAX_MACOS_NATIVE_HOST_PATTERNS * size_of::<WebPatternKey<'static>>()
            + MAX_MACOS_NATIVE_PATTERN_ARENA_BYTES
            + MAX_MACOS_NATIVE_HOST_PATTERNS * size_of::<PatternSpan>()
            + 7 * RETAINED_HEAP_ALLOCATION_OVERHEAD_BYTES;
    assert_eq!(
        MACOS_NATIVE_GRANT_COMPILER_CONSERVATIVE_TRANSIENT_HEAP_CEILING_BYTES,
        exact_ceiling
    );
}

#[test]
fn profile_site_denials_compile_separately_from_manifest_grants() {
    let broad = parse("https://*/*");
    let denied = parse("https://denied.example/*");
    let plan = compile_projection(CompilerInput {
        identity: identity(43),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::empty(),
        host_count: 1,
        host_grants: [host(&broad, REQUIRED, GRANTED)],
        denied_site_count: 1,
        denied_sites: [&denied],
    })
    .expect("representable exact site denial");

    assert_eq!(
        plan.granted_host_patterns().collect::<Vec<_>>(),
        ["https://*/*"]
    );
    assert_eq!(
        plan.denied_site_patterns().collect::<Vec<_>>(),
        ["https://denied.example/*"]
    );
    assert!(plan.retained_bytes() <= MAX_MACOS_NATIVE_GRANT_PLAN_RETAINED_BYTES);

    let exact = parse("https://exact.example/*");
    let exact_denied = compile_projection(CompilerInput {
        identity: identity(44),
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 0,
        api_grants: std::iter::empty(),
        host_count: 1,
        host_grants: [host(&exact, REQUIRED, GRANTED)],
        denied_site_count: 1,
        denied_sites: [&exact],
    })
    .expect("exact policy denial dominates an identical manifest grant");
    assert!(exact_denied.granted_host_patterns().next().is_none());
    assert_eq!(
        exact_denied.denied_site_patterns().collect::<Vec<_>>(),
        ["https://exact.example/*"]
    );
}

#[test]
fn plan_retains_exact_generation_identity_and_redacts_debug_output() {
    let expected = identity(41);
    let pattern = parse("https://secret.example/*");
    let plan = compile_projection(CompilerInput {
        identity: expected,
        browsing_context: ExtensionGrantBrowsingContext::Regular,
        file_access_granted: false,
        private_access_granted: false,
        api_count: 1,
        api_grants: [api("storage", REQUIRED, GRANTED)],
        host_count: 1,
        host_grants: [host(&pattern, REQUIRED, GRANTED)],
        denied_site_count: 0,
        denied_sites: std::iter::empty(),
    })
    .expect("representable plan");

    assert_eq!(plan.runtime(), expected.runtime);
    assert_eq!(plan.schema(), MacosNativeGrantSchema::WkWebExtensionV1);
    assert_eq!(plan.apply_mode(), expected.apply_mode);
    assert_eq!(size_of::<MacosNativeGrantSchema>(), 1);
    assert_eq!(size_of::<MacosNativeGrantApplyMode>(), 1);
    assert_eq!(plan.grant_revision(), expected.grant_revision);
    assert_eq!(plan.grant_digest(), expected.grant_digest);
    let debug = format!("{plan:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret.example"));
    assert!(!debug.contains("storage"));
    assert!(!debug.contains("2929"));
}

#[test]
fn plan_is_safe_to_transfer_to_the_native_ui_thread() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MacosNativeGrantPlan>();
}

#[test]
fn explicit_granted_origin_covers_its_static_route_without_silently_widening_access() {
    let root = parse("https://example.com/*");
    let route = parse("https://example.com/embed/*");
    assert!(regular(
        &[],
        &[
            host(&root, REQUIRED, GRANTED),
            host(&route, REQUIRED, GRANTED)
        ]
    )
    .is_ok());
    assert!(regular(
        &[],
        &[
            host(&root, OPTIONAL, DENIED),
            host(&route, REQUIRED, GRANTED)
        ]
    )
    .is_err());
    for unrelated in ["http://example.com/*", "https://other.example/*"] {
        let other = parse(unrelated);
        assert!(regular(
            &[],
            &[
                host(&other, REQUIRED, GRANTED),
                host(&route, REQUIRED, GRANTED)
            ]
        )
        .is_err());
    }
    assert!(regular(&[], &[host(&route, REQUIRED, GRANTED)]).is_err());
}

#[test]
fn granted_subdomain_origin_covers_a_static_route_on_an_included_host() {
    let root = parse("https://*.example.com/*");
    for route in [
        "https://app.example.com/add*",
        "https://example.com/add*",
        "https://*.nested.example.com/add*",
    ] {
        let route = parse(route);
        let plan = regular(
            &[],
            &[
                host(&root, REQUIRED, GRANTED),
                host(&route, REQUIRED, GRANTED),
            ],
        )
        .expect("an already granted parent domain covers the content route");
        assert_eq!(compiled_host_patterns(&plan), ["https://*.example.com/*"]);
        assert!(regular(
            &[],
            &[
                host(&root, OPTIONAL, DENIED),
                host(&route, REQUIRED, GRANTED)
            ],
        )
        .is_err());
    }
    for route in [
        "http://app.example.com/add*",
        "https://notexample.com/add*",
        "https://example.com.other.test/add*",
        "https://*.com/add*",
        "https://app.example.com:8443/add*",
    ] {
        let route = parse(route);
        assert!(regular(
            &[],
            &[
                host(&root, REQUIRED, GRANTED),
                host(&route, REQUIRED, GRANTED)
            ],
        )
        .is_err());
    }
}
