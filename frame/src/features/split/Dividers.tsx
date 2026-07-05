import { For } from "solid-js";
import * as layout from "../../state/layout";

// Invisible drag strips over the pane gaps. The gaps show the chrome
// background, so the chrome owns those pixels; ratio math stays in Rust.
export function Dividers() {
  let frame = 0;
  let active = false;
  const onDown = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    e.preventDefault();
    active = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    layout.grab(e.clientX, e.clientY);
  };
  const onMove = (e: PointerEvent) => {
    if (!active || frame) return;
    const { clientX: x, clientY: y } = e;
    frame = requestAnimationFrame(() => {
      frame = 0;
      layout.drag(x, y);
    });
  };
  const onUp = (e: PointerEvent & { currentTarget: HTMLElement }) => {
    if (!active) return;
    active = false;
    if (frame) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    layout.release();
    e.currentTarget.releasePointerCapture(e.pointerId);
  };
  return (
    <For each={layout.dividers()}>
      {(d) => (
        <div
          class="fixed z-40"
          style={{
            left: `${d.x}px`,
            top: `${d.y}px`,
            width: `${d.width}px`,
            height: `${d.height}px`,
            cursor: d.vertical ? "col-resize" : "row-resize",
          }}
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
        />
      )}
    </For>
  );
}
