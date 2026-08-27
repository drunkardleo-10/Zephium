import type {
  BlockerStatusChanged,
  BrowserCredentialCapabilityChanged,
  ExtensionActionFailed,
  ExtensionActionShortcut,
  ExtensionActionsChanged,
  ExtensionManagementAvailabilityChanged,
  ExtensionManagementChanged,
  ExtensionDistributionChanged,
  ExtensionRuntimeGrantPromptChanged,
  ItemsChanged,
  LayoutChanged,
  OperationProcessed,
  PagePermissionPromptChanged,
  RuntimeStatusChanged,
  SearchChanged,
  TabChanged,
  UiCommand,
} from "./bindings";

type PayloadEvent<T> = { payload: T };
type Listener<T> = (event: PayloadEvent<T>) => void;
type PresentationTab = { tab: TabChanged; active: string | null };

function scopedEvent<T>(name: string) {
  return {
    listen(listener: Listener<T>): Promise<() => void> {
      const handler = (event: Event) => listener({ payload: (event as CustomEvent<T>).detail });
      window.addEventListener(name, handler);
      return Promise.resolve(() => window.removeEventListener(name, handler));
    },
  };
}

// Rust delivers these directly into the intended WebView. Do not replace this
// with @tauri-apps/api/event: its listen command accepts a caller-selected
// target, which would let the launcher panel subscribe to main-window state.
export const events = {
  browserCredentialCapabilityChanged: scopedEvent<BrowserCredentialCapabilityChanged>(
    "zephium:browser-credential-capability",
  ),
  itemsChanged: scopedEvent<ItemsChanged>("zephium:items"),
  tabChanged: scopedEvent<TabChanged>("zephium:tab"),
  extensionActionsChanged: scopedEvent<ExtensionActionsChanged>("zephium:extension-actions"),
  extensionActionFailed: scopedEvent<ExtensionActionFailed>("zephium:extension-action-failed"),
  extensionActionShortcut: scopedEvent<ExtensionActionShortcut>(
    "zephium:extension-action-shortcut",
  ),
  extensionManagementAvailabilityChanged: scopedEvent<ExtensionManagementAvailabilityChanged>(
    "zephium:extension-management-availability",
  ),
  extensionManagementChanged: scopedEvent<ExtensionManagementChanged>(
    "zephium:extension-management",
  ),
  extensionDistributionChanged: scopedEvent<ExtensionDistributionChanged>(
    "zephium:extension-distribution",
  ),
  extensionRuntimeGrantPromptChanged: scopedEvent<ExtensionRuntimeGrantPromptChanged>(
    "zephium:extension-runtime-grant-prompt",
  ),
  pagePermissionPromptChanged: scopedEvent<PagePermissionPromptChanged>(
    "zephium:page-permission-prompt",
  ),
  presentationTab: scopedEvent<PresentationTab>("zephium:presentation-tab"),
  uiCommand: scopedEvent<UiCommand>("zephium:ui-command"),
  searchChanged: scopedEvent<SearchChanged>("zephium:search"),
  layoutChanged: scopedEvent<LayoutChanged>("zephium:layout"),
  operationProcessed: scopedEvent<OperationProcessed>("zephium:operation-processed"),
  runtimeStatusChanged: scopedEvent<RuntimeStatusChanged>("zephium:runtime-status"),
  blockerStatusChanged: scopedEvent<BlockerStatusChanged>("zephium:blocker-status"),
};
