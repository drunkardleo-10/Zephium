import { Index, onCleanup, onMount } from "solid-js";
import * as layout from "../../state/layout";

// Drag strips over the pane gaps. Moves are window-level: the chrome holds
// native mouse capture while a button is down, so the window sees every
// event no matter what happens to the strip node or pointer capture.
export function Dividers() {
  let frame = 0;
  let active = false;

  const onMove = (e: PointerEvent) => {
    if (!active || frame) return;
    const { clientX: x, clientY: y } = e;
    frame = requestAnimationFrame(() => {
      frame = 0;
      layout.drag(x, y);
    });
  };
  const onUp = (e: PointerEvent) => {
    if (!active) return;
    active = false;
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    // The final coordinate and release are one ordered native command. Two
    // fire-and-forget IPC calls could otherwise be delivered out of order and
    // persist the previous animation frame's ratio.
    layout.release(
      e.type === "pointerup" ? e.clientX : null,
      e.type === "pointerup" ? e.clientY : null,
    );
    document.body.style.cursor = "";
  };
  const onDown = (e: PointerEvent, vertical: boolean) => {
    e.preventDefault();
    active = true;
    layout.grab(e.clientX, e.clientY);
    document.body.style.cursor = vertical ? "col-resize" : "row-resize";
  };

  onMount(() => {
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
    onCleanup(() => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
    });
  });

  return (
    <Index each={layout.dividers()}>
      {(d) => (
        <div
          class="fixed z-40"
          style={{
            left: `${d().x}px`,
            top: `${d().y}px`,
            width: `${d().width}px`,
            height: `${d().height}px`,
            cursor: d().vertical ? "col-resize" : "row-resize",
          }}
          onPointerDown={(e) => onDown(e, d().vertical)}
        />
      )}
    </Index>
  );
}
