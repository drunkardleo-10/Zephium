import { onCleanup, onMount } from "solid-js";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Shell } from "./Shell";
import { Spotlight } from "../features/spotlight/Spotlight";
import * as tabs from "../state/tabs";

export default function App() {
  if (getCurrentWindow().label === "spotlight") {
    return <Spotlight />;
  }

  onMount(() => {
    void tabs.init();
    onCleanup(() => tabs.dispose());
  });
  return <Shell />;
}
