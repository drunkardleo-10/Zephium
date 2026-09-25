import type { ExtensionManagementLimitationView } from "$shared/ipc/bindings";

const API_PERMISSION_LABELS: Readonly<Record<string, string>> = {
  activeTab: "Access the current tab after you use the extension",
  alarms: "Schedule background tasks",
  bookmarks: "Read your bookmarks",
  clipboardRead: "Read copied content",
  clipboardWrite: "Copy content to the clipboard",
  contextMenus: "Add items to page context menus",
  cookies: "Read and change cookies on allowed sites",
  declarativeNetRequest: "Block or redirect network requests using declared rules",
  declarativeNetRequestFeedback: "Inspect requests matched by blocking rules",
  "declarativeNetRequest.withHostAccess": "Apply blocking rules to requests on allowed sites",
  declarativeNetRequestWithHostAccess: "Apply blocking rules to requests on allowed sites",
  fontSettings: "Read and change browser font settings",
  favicon: "Display site icons",
  history: "Search recent browsing history",
  idle: "Detect when the device is idle",
  menus: "Add items to browser and page menus",
  nativeMessaging: "Use Zephium's restricted compatibility broker",
  notifications: "Show notifications",
  offscreen: "Run an offscreen extension document",
  privacy: "Read and change supported browser privacy settings",
  sidePanel: "Show content in the browser side panel",
  scripting: "Run extension scripts on allowed sites",
  search: "Search using your default search engine",
  sessions: "Restore the most recently closed tab",
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
    case "api_permission":
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
    case "web_accessible_resources":
      return "Some page-accessible extension resources are limited";
    case "minimum_browser_version":
      return "Some APIs from the requested browser version are limited";
    case "commands":
      return "Some extension keyboard commands are limited";
    case "side_panel":
      return "The extension side panel is limited";
    case "managed_storage":
      return "Administrator-managed extension storage is limited";
    case "options_page":
      return "Some extension settings-page behavior is limited";
    case "declarative_net_request":
      return "Some declarative network rules have platform limitations";
  }
}
