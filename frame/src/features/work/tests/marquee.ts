import { expect, vi } from "vitest";

/** Drags a selection box across the canvas pane between two client points. */
export async function marquee(
  container: HTMLElement,
  from: [number, number],
  to: [number, number],
) {
  vi.spyOn(Element.prototype, "setPointerCapture").mockImplementation(() => {});
  vi.spyOn(Element.prototype, "releasePointerCapture").mockImplementation(() => {});
  const pane = container.querySelector<HTMLElement>(".svelte-flow__pane")!;
  await expect.poll(() => pane.classList.contains("selection")).toBe(true);
  const fire = (type: string, [x, y]: [number, number]) =>
    pane.dispatchEvent(
      new PointerEvent(type, {
        bubbles: true,
        cancelable: true,
        clientX: x,
        clientY: y,
        button: 0,
        buttons: type === "pointerup" ? 0 : 1,
        isPrimary: true,
        pointerId: 1,
        pointerType: "mouse",
      }),
    );
  fire("pointerdown", from);
  fire("pointermove", [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2]);
  fire("pointermove", to);
  fire("pointerup", to);
  pane.dispatchEvent(new MouseEvent("click", { bubbles: true, clientX: to[0], clientY: to[1] }));
}
