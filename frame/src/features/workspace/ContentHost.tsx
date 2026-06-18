import { Show, onCleanup, onMount } from "solid-js";
import * as tabs from "../../state/tabs";
import { NewTab } from "../newtab/NewTab";

export function ContentHost() {
  let host!: HTMLDivElement;
  let frame = 0;

  const report = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const r = host.getBoundingClientRect();
      tabs.setContentBounds({ x: r.x, y: r.y, width: r.width, height: r.height });
    });
  };

  onMount(() => {
    report();
    const observer = new ResizeObserver(report);
    observer.observe(host);
    window.addEventListener("resize", report);
    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("resize", report);
      if (frame) cancelAnimationFrame(frame);
    });
  });

  return (
    <div ref={host} class="relative min-h-0 flex-1 bg-bg">
      <Show when={!tabs.activeTab()?.url}>
        <NewTab />
      </Show>
    </div>
  );
}
