import { tabs } from "$domain/tabs";

/**
 * Installs a global listener for middle-clicks (button === 1) on tabs.
 * When any closable tab element in the interface is middle-clicked,
 * it closes that tab.
 */
export function installMiddleClickCloseTab(): () => void {
  const handleAuxClick = (event: MouseEvent) => {
    if (event.button !== 1) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const tabElement = target.closest<HTMLElement>("[data-zephium-tab-id]");
    if (!tabElement) return;
    if (tabElement.dataset.closable === "false") return;
    const id = tabElement.dataset.zephiumTabId;
    if (!id) return;
    event.preventDefault();
    event.stopPropagation();
    tabs.close(id);
  };

  const preventDefaultPointerDown = (event: MouseEvent | PointerEvent) => {
    if (event.button !== 1) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const tabElement = target.closest<HTMLElement>("[data-zephium-tab-id]");
    if (tabElement && tabElement.dataset.closable !== "false") {
      event.preventDefault();
    }
  };

  window.addEventListener("auxclick", handleAuxClick, true);
  window.addEventListener("pointerdown", preventDefaultPointerDown, true);

  return () => {
    window.removeEventListener("auxclick", handleAuxClick, true);
    window.removeEventListener("pointerdown", preventDefaultPointerDown, true);
  };
}
