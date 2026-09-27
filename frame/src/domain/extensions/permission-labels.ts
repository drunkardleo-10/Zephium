import type { ExtensionManagementLimitationView } from "$shared/ipc/bindings";

const API_PERMISSION_LABELS: Readonly<Record<string, string>> = {
  activeTab: "Access the current tab after you use the extension",
  alarms: "Schedule background tasks",
  bookmarks: "Read your bookmarks",
  clipboardRead: "Read copied content",
  clipboardWrite: "Copy content to the clipboard",
  contextMenus: "Add items to page context menus",
  cookies: "Read and change cookies on allowed sites",
  downloads: "Download files through the browser",
  management: "Inspect and manage browser extensions",
  declarativeNetRequest: "Block or redirect network requests using declared rules",
  declarativeNetRequestFeedback: "Inspect requests matched by blocking rules",
  "declarativeNetRequest.withHostAccess": "Apply blocking rules to requests on allowed sites",
  declarativeNetRequestWithHostAccess: "Apply blocking rules to requests on allowed sites",
  fontSettings: "Read and change browser font settings",
  favicon: "Display site icons",
  history: "Search browsing history (read-only)",
  identity: "Sign in to an external service through the extension",
  idle: "Detect when the device is idle",
  menus: "Add items to browser and page menus",
  nativeMessaging: "Communicate with approved desktop apps or Zephium compatibility services",
  notifications: "Show notifications",
  offscreen: "Run an offscreen extension document",
  privacy: "Read and change supported browser privacy settings",
  sidePanel: "Show content in the browser side panel",
  scripting: "Run extension scripts on allowed sites",
  search: "Search using your default search engine",
  sessions: "View and restore recently closed tabs",
  storage: "Store extension settings and data",
  tabs: "Read tab titles and addresses",
  unlimitedStorage: "Store data without the normal extension quota",
  webNavigation: "Observe navigation on allowed sites",
  webRequest: "Observe network requests on allowed sites",
  webRequestAuthProvider: "Provide credentials for supported sign-in requests",
};

/** Browser-owned copy for authenticated manifest permission identifiers. */
export function apiPermissionLabel(permission: string): string {
  return API_PERMISSION_LABELS[permission] ?? `Use the ${permission} browser capability`;
}

/** Browser-owned copy for canonical match patterns retained by the Shell. */
export function hostPermissionLabel(pattern: string): string {
  if (pattern === "<all_urls>") return "Read and change data on all websites";
  if (pattern.startsWith("file://")) return "Read and change data in matching local files";
  return `Read and change data on ${pattern}`;
}

/** Browser-owned copy for authenticated compatibility disclosures. */
export function compatibilityLimitationLabel(
  limitation: ExtensionManagementLimitationView,
): string {
  switch (limitation.type) {
    case "optional_api_unavailable":
      if (limitation.name === "identity") return "Optional extension sign-in is unavailable";
      if (limitation.name === "nativeMessaging")
        return "Optional desktop-app connections are unavailable";
      return `Unavailable optional feature: ${apiPermissionLabel(limitation.name)}`;
    case "optional_host_unavailable":
      return `Optional site access unavailable: ${limitation.pattern}`;
    case "external_messaging_unavailable":
      return "Connections from websites and other extensions are unavailable";
    case "api_permission":
      if (limitation.name === "identity")
        return "Interactive extension sign-in and Chrome account tokens are unavailable";
      if (limitation.name === "unlimitedStorage")
        return "Extension storage remains limited by the browser’s normal quota";
      if (limitation.name === "downloads") return "Extension-managed downloads are unavailable";
      if (limitation.name === "idle") return "Device idle detection is unavailable";
      if (limitation.name === "management") return "Managing other extensions is unavailable";
      if (limitation.name === "webRequestAuthProvider")
        return "HTTP authentication autofill is unavailable";
      if (limitation.name === "privacy")
        return "Only disabling browser autofill is supported for privacy settings";
      if (limitation.name === "notifications")
        return "Some extension notifications may not be shown";
      return `Limited: ${apiPermissionLabel(limitation.name)}`;
    case "host_access":
      return "Some declared site access is limited";
    case "background":
      return "Background tasks have platform limitations";
    case "action":
      return "Some toolbar action behavior is limited";
    case "offscreen":
      return "Offscreen extension documents are limited";
    case "native_messaging":
      return "Arbitrary native app connections are unavailable";
    case "browser_override":
      return "Replacing built-in browser pages is limited";
    case "extension_pages_csp":
      return "Some extension-page security policy behavior is limited";
    case "sandbox":
      return "Sandboxed extension pages are limited";
    case "content_scripts":
      return "Some page scripts have platform limitations";
    case "main_document_content_scripts_only":
      return "Some extension page scripts run only in top-level pages; embedded frames are unsupported";
    case "fragment_url_content_scripts_unavailable":
      return "These page scripts are unavailable when a page URL contains a # fragment";
    case "content_script_fonts_unavailable":
      return "Fonts supplied by these page scripts are unavailable";
    case "side_panel_unavailable":
      return "The extension side panel is unavailable";
    case "offscreen_local_storage_only":
      return "Offscreen documents support local storage only; other offscreen tasks are unavailable";
    case "sandboxed_pages_unavailable":
      return "Sandboxed extension widgets are unavailable";
    case "clipboard_read_unavailable":
      return "Reading copied content is unavailable, including automatic clipboard clearing and SSH key import";
    case "web_accessible_resources":
      return "Some page-accessible extension resources are limited";
    case "minimum_browser_version":
      return "Some APIs from the requested browser version are limited";
    case "commands":
      return "Some extension keyboard commands are limited";
    case "side_panel":
      return "The extension side panel is limited";
    case "managed_storage":
      return "No administrator-managed extension settings are configured in Zephium";
    case "options_page":
      return "Some extension settings-page behavior is limited";
    case "declarative_net_request":
      return "Some declarative network rules have platform limitations";
  }
}
