const api = globalThis.browser ?? globalThis.chrome;
const marker = Symbol.for("zephium.webkit-api-compatibility.v1");
const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
const token = "zephium-compatibility-round-trip-v1";

const descriptor = (name) => {
  const value = Object.getOwnPropertyDescriptor(globalThis, name);
  if (!value) return "absent";
  if ("value" in value) {
    return `data-${Number(value.configurable)}${Number(value.writable)}`;
  }
  return `accessor-${Number(value.configurable)}${Number(typeof value.set === "function")}`;
};
const backgroundMode = globalThis[modeMarker] ?? "missing";
const diagnostic = [
  backgroundMode,
  descriptor("chrome"),
  descriptor("browser"),
  `runtime-${Number(Object.isExtensible(api?.runtime))}`,
  `update-${Number(api?.runtime?.onUpdateAvailable != null)}`,
].join(":");
api.action.setTitle({ title: `ZEPHIUM_COMPAT_BACKGROUND:${diagnostic}` });
api.action.onClicked.addListener(() => {
  api.action.setTitle({ title: `ZEPHIUM_COMPAT_CLICKED:${diagnostic}` });
});
api.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message?.kind !== "ping" || message?.token !== token) return undefined;
  const senderKind = Number.isInteger(sender?.tab?.id)
    ? "integer"
    : sender?.tab == null
      ? "absent"
      : typeof sender.tab.id;
  api.action.setTitle({ title: `ZEPHIUM_COMPAT_MESSAGE:${senderKind}:${diagnostic}` });
  if (senderKind !== "integer") return undefined;
  const pong = {
    kind: "pong",
    token: message.token,
    backgroundInstalled: globalThis[marker] === true,
    backgroundMode,
    senderTab: true,
  };
  sendResponse({ ...pong, channel: "runtime-response" });
  void Promise.resolve(
    api.tabs.sendMessage(sender.tab.id, { ...pong, channel: "tabs-message" }),
  ).then(
    () => api.action.setTitle({ title: `ZEPHIUM_COMPAT_TABS:fulfilled:${diagnostic}` }),
    () => api.action.setTitle({ title: `ZEPHIUM_COMPAT_TABS:rejected:${diagnostic}` }),
  );
  return undefined;
});
