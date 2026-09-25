import type { commands } from "../ipc/bindings";

type Commands = typeof commands;

/** Explicit overrides only: an unexpected native call must fail the test. */
export function mockBindings(overrides: Partial<Commands> = {}): { commands: Commands } {
  return {
    commands: new Proxy(overrides as Commands, {
      get(target, key, receiver) {
        if (typeof key === "symbol" || key === "then") return undefined;
        if (!Object.hasOwn(target, key)) throw new Error(`Unmocked native command: ${key}`);
        return Reflect.get(target, key, receiver);
      },
    }),
  };
}
