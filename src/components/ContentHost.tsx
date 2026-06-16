import { onCleanup, onMount } from "solid-js";
import * as wv from "../ipc/webview";

/// Reserves the page area. Native tab webviews are positioned over this rect;
/// we report it (CSS px) to Rust on every layout change. The active tab follows.
export function ContentHost() {
  let host!: HTMLDivElement;

  const report = () => {
    const r = host.getBoundingClientRect();
    void wv.setContentBounds({ x: r.x, y: r.y, width: r.width, height: r.height });
  };

  let frame = 0;
  const schedule = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      report();
    });
  };

  onMount(() => {
    report();
    const observer = new ResizeObserver(schedule);
    observer.observe(host);
    window.addEventListener("resize", schedule);

    onCleanup(() => {
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      if (frame) cancelAnimationFrame(frame);
    });
  });

  return <div ref={host} class="content-host" />;
}
