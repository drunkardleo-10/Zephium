const API_PERMISSION_LABELS: Readonly<Record<string, string>> = {
  activeTab: "Access the current tab after you use the extension",
  alarms: "Schedule background tasks",
  clipboardRead: "Read copied content",
  clipboardWrite: "Copy content to the clipboard",
  contextMenus: "Add items to page context menus",
  cookies: "Read and change cookies on allowed sites",
  declarativeNetRequest: "Block or redirect network requests using declared rules",
  declarativeNetRequestFeedback: "Inspect requests matched by blocking rules",
  "declarativeNetRequest.withHostAccess": "Apply blocking rules to requests on allowed sites",
  declarativeNetRequestWithHostAccess: "Apply blocking rules to requests on allowed sites",
  idle: "Detect when the device is idle",
  menus: "Add items to browser and page menus",
  notifications: "Show notifications",
  offscreen: "Run an offscreen extension document",
  privacy: "Read and change supported browser privacy settings",
  sidePanel: "Show content in the browser side panel",
  scripting: "Run extension scripts on allowed sites",
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
