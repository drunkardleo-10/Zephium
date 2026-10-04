import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { tabs } from "$domain/tabs";
import { installMiddleClickCloseTab } from "../lib/middle-click";

class FakeElement {
  dataset: Record<string, string | undefined>;
  parent: FakeElement | null;

  constructor(dataset: Record<string, string | undefined> = {}, parent: FakeElement | null = null) {
    this.dataset = dataset;
    this.parent = parent;
  }

  closest(selector: string): FakeElement | null {
    if (selector === "[data-zephium-tab-id]") {
      if (this.dataset.zephiumTabId !== undefined) {
        return this;
      }
      return this.parent?.closest(selector) ?? null;
    }
    return null;
  }
}

type TestEvent = {
  button?: number;
  target?: FakeElement;
  preventDefault?: () => void;
  stopPropagation?: () => void;
};

describe("installMiddleClickCloseTab", () => {
  let listeners: Map<string, (event: TestEvent) => void>;
  let closeSpy: ReturnType<typeof vi.spyOn>;
  let cleanup: (() => void) | null = null;

  beforeEach(() => {
    listeners = new Map();
    vi.stubGlobal("window", {
      addEventListener: (type: string, listener: (event: TestEvent) => void) => {
        listeners.set(type, listener);
      },
      removeEventListener: (type: string, listener: (event: TestEvent) => void) => {
        if (listeners.get(type) === listener) {
          listeners.delete(type);
        }
      },
    });
    vi.stubGlobal("Element", FakeElement);

    closeSpy = vi.spyOn(tabs, "close").mockImplementation(() => {});
    cleanup = installMiddleClickCloseTab();
  });

  afterEach(() => {
    cleanup?.();
    cleanup = null;
    closeSpy.mockRestore();
    vi.unstubAllGlobals();
  });

  it("closes tab when middle-clicking on a tab element or child", () => {
    const tabEl = new FakeElement({ zephiumTabId: "tab-123" });
    const childEl = new FakeElement({}, tabEl);

    let defaultPrevented = false;
    let propagationStopped = false;

    const auxClick = listeners.get("auxclick");
    expect(auxClick).toBeDefined();

    auxClick?.({
      button: 1,
      target: childEl,
      preventDefault: () => {
        defaultPrevented = true;
      },
      stopPropagation: () => {
        propagationStopped = true;
      },
    });

    expect(closeSpy).toHaveBeenCalledWith("tab-123");
    expect(defaultPrevented).toBe(true);
    expect(propagationStopped).toBe(true);
  });

  it("ignores non-middle clicks (left click or right click)", () => {
    const tabEl = new FakeElement({ zephiumTabId: "tab-123" });
    const auxClick = listeners.get("auxclick");

    auxClick?.({
      button: 0,
      target: tabEl,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    });
    expect(closeSpy).not.toHaveBeenCalled();

    auxClick?.({
      button: 2,
      target: tabEl,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    });
    expect(closeSpy).not.toHaveBeenCalled();
  });

  it("ignores middle click if data-closable is false", () => {
    const tabEl = new FakeElement({
      zephiumTabId: "pinned-1",
      closable: "false",
    });
    const auxClick = listeners.get("auxclick");
    const preventDefault = vi.fn();

    auxClick?.({
      button: 1,
      target: tabEl,
      preventDefault,
      stopPropagation: vi.fn(),
    });

    expect(closeSpy).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
  });

  it("prevents default pointerdown on middle click for closable tab", () => {
    const tabEl = new FakeElement({ zephiumTabId: "tab-123" });
    const pointerdown = listeners.get("pointerdown");
    const preventDefault = vi.fn();

    pointerdown?.({
      button: 1,
      target: tabEl,
      preventDefault,
    });

    expect(preventDefault).toHaveBeenCalled();
  });

  it("does not prevent default pointerdown on unclosable tab", () => {
    const tabEl = new FakeElement({
      zephiumTabId: "pinned-1",
      closable: "false",
    });
    const pointerdown = listeners.get("pointerdown");
    const preventDefault = vi.fn();

    pointerdown?.({
      button: 1,
      target: tabEl,
      preventDefault,
    });

    expect(preventDefault).not.toHaveBeenCalled();
  });

  it("unregisters window listeners on cleanup", () => {
    expect(listeners.has("auxclick")).toBe(true);
    expect(listeners.has("pointerdown")).toBe(true);

    cleanup?.();
    cleanup = null;

    expect(listeners.has("auxclick")).toBe(false);
    expect(listeners.has("pointerdown")).toBe(false);
  });
});
