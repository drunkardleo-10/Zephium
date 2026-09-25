import { nativeEventNames, type events } from "../ipc/native-events";

type NativeEvents = typeof events;
type EventPayload<K extends keyof NativeEvents> = Parameters<
  Parameters<NativeEvents[K]["listen"]>[0]
>[0]["payload"];

/** Dispatch through the production scoped DOM transport in component tests. */
export function emitNativeEvent<K extends keyof NativeEvents>(
  name: K,
  payload: EventPayload<K>,
): void {
  // The event name mapping lives in the transport, not a duplicate test catalog.
  window.dispatchEvent(new CustomEvent(nativeEventNames[name], { detail: payload }));
}
