import { onCleanup, onMount } from "solid-js";
import { Shell } from "./Shell";
import * as tabs from "../state/tabs";

export default function App() {
  onMount(() => {
    void tabs.init();
    onCleanup(() => tabs.dispose());
  });
  return <Shell />;
}
