import type {
  PanelState,
  FaviconsView,
  NoteOpenRequested,
  NoteChanges,
  ResourceChanged,
  DownloadsChanged,
  BlockerStatusChanged,
  BrowserCredentialCapabilityChanged,
  ExtensionActionFailed,
  ExtensionActionShortcut,
  ExtensionActionsChanged,
  ItemsChanged,
  LayoutChanged,
  OperationProcessed,
  PagePermissionPromptChanged,
  RuntimeStatusChanged,
  SearchChanged,
  TabChanged,
  UiCommand,
  WebExtensionAccessRequested,
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
export const nativeEventNames = {
  panelState: "zephium:panel-state",
  favicons: "zephium:favicons",
  noteOpenRequested: "zephium:note-open-requested",
  resourceChanged: "zephium:resource-changed",
  notesChanged: "zephium:notes-changed",
  downloadsChanged: "zephium:downloads-changed",
  resourceClose: "zephium:resource-close",
  resourceCloseCancelled: "zephium:resource-close-cancelled",
  browserCredentialCapabilityChanged: "zephium:browser-credential-capability",
  itemsChanged: "zephium:items",
  tabChanged: "zephium:tab",
  extensionActionsChanged: "zephium:extension-actions",
  extensionActionFailed: "zephium:extension-action-failed",
  extensionActionShortcut: "zephium:extension-action-shortcut",
  pagePermissionPromptChanged: "zephium:page-permission-prompt",
  webExtensionAccessRequested: "zephium:web-extension-access",
  browserReturn: "zephium:browser-return",
  presentationTab: "zephium:presentation-tab",
  uiCommand: "zephium:ui-command",
  searchChanged: "zephium:search",
  layoutChanged: "zephium:layout",
  operationProcessed: "zephium:operation-processed",
  runtimeStatusChanged: "zephium:runtime-status",
  blockerStatusChanged: "zephium:blocker-status",
} as const;

export const events = {
  favicons: scopedEvent<FaviconsView>(nativeEventNames.favicons),
  noteOpenRequested: scopedEvent<NoteOpenRequested>(nativeEventNames.noteOpenRequested),
  resourceClose: scopedEvent<string>(nativeEventNames.resourceClose),
  resourceCloseCancelled: scopedEvent<string>(nativeEventNames.resourceCloseCancelled),
  resourceChanged: scopedEvent<ResourceChanged>(nativeEventNames.resourceChanged),
  notesChanged: scopedEvent<NoteChanges>(nativeEventNames.notesChanged),
  downloadsChanged: scopedEvent<DownloadsChanged>(nativeEventNames.downloadsChanged),
  panelState: scopedEvent<PanelState>(nativeEventNames.panelState),
  browserCredentialCapabilityChanged: scopedEvent<BrowserCredentialCapabilityChanged>(
    nativeEventNames.browserCredentialCapabilityChanged,
  ),
  itemsChanged: scopedEvent<ItemsChanged>(nativeEventNames.itemsChanged),
  tabChanged: scopedEvent<TabChanged>(nativeEventNames.tabChanged),
  extensionActionsChanged: scopedEvent<ExtensionActionsChanged>(
    nativeEventNames.extensionActionsChanged,
  ),
  extensionActionFailed: scopedEvent<ExtensionActionFailed>(nativeEventNames.extensionActionFailed),
  extensionActionShortcut: scopedEvent<ExtensionActionShortcut>(
    nativeEventNames.extensionActionShortcut,
  ),
  pagePermissionPromptChanged: scopedEvent<PagePermissionPromptChanged>(
    nativeEventNames.pagePermissionPromptChanged,
  ),
  webExtensionAccessRequested: scopedEvent<WebExtensionAccessRequested>(
    nativeEventNames.webExtensionAccessRequested,
  ),
  browserReturn: scopedEvent<ItemsChanged>(nativeEventNames.browserReturn),
  presentationTab: scopedEvent<PresentationTab>(nativeEventNames.presentationTab),
  uiCommand: scopedEvent<UiCommand>(nativeEventNames.uiCommand),
  searchChanged: scopedEvent<SearchChanged>(nativeEventNames.searchChanged),
  layoutChanged: scopedEvent<LayoutChanged>(nativeEventNames.layoutChanged),
  operationProcessed: scopedEvent<OperationProcessed>(nativeEventNames.operationProcessed),
  runtimeStatusChanged: scopedEvent<RuntimeStatusChanged>(nativeEventNames.runtimeStatusChanged),
  blockerStatusChanged: scopedEvent<BlockerStatusChanged>(nativeEventNames.blockerStatusChanged),
};
