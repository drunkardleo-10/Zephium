import { onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TabStrip } from "./components/TabStrip";
import { Toolbar } from "./components/Toolbar";
import { ContentHost } from "./components/ContentHost";
import * as tabs from "./state/tabs";
import * as wv from "./ipc/webview";
import "./styles/app.css";

export default function App() {
  onMount(() => {
    const win = getCurrentWindow();

    const pending = [
      wv.onUrl(({ id, url }) => tabs.applyUrl(id, url)),
      wv.onLoading(({ id, loading }) => tabs.applyLoading(id, loading)),
      wv.onTitle(({ id, title }) => {
        tabs.applyTitle(id, title);
        if (id === tabs.activeId()) void win.setTitle(title || "Browser");
      }),
    ];

    tabs.openTab();

    onCleanup(() => {
      for (const p of pending) void p.then((un) => un());
    });
  });

  const submitAddress = (value: string) => {
    const id = tabs.activeId();
    if (id < 0) return;
    tabs.navigate(id, wv.normalizeUrl(value));
  };

  return (
    <div class="app">
      <TabStrip
        tabs={tabs.tabs()}
        activeId={tabs.activeId()}
        onSelect={tabs.activate}
        onClose={tabs.close}
        onNew={() => tabs.openTab()}
      />
      <Toolbar
        tab={tabs.activeTab()}
        onSubmit={submitAddress}
        onBack={tabs.backActive}
        onForward={tabs.forwardActive}
        onReload={tabs.reloadActive}
      />
      <ContentHost />
    </div>
  );
}
