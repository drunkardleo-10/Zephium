const api = globalThis.browser ?? globalThis.chrome;
const marker = Symbol.for("zephium.webkit-api-compatibility.v1");
const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
const token = "zephium-compatibility-round-trip-v1";
const credentialToken = "zephium-credential-selection-v1";
const fixtureCredential = Object.freeze({
  credentialId: "fixture-login-v1",
  username: "fixture.user@zephium.invalid",
  password: "zephium-fixture-password-v1",
});

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
  if (
    message?.kind === "popup-background-lifecycle"
    && message?.token === "zephium-popup-background-lifecycle-v1"
  ) {
    const setting = api.privacy?.services?.passwordSavingEnabled;
    void Promise.resolve()
      .then(async () => {
        if (
          typeof setting?.get !== "function"
          || typeof setting?.set !== "function"
          || typeof setting?.clear !== "function"
        ) {
          throw new Error("privacy-setting-missing");
        }
        const initial = await setting.get({});
        await setting.set({ value: false });
        const controlled = await setting.get({});
        await setting.clear({});
        const cleared = await setting.get({});
        if (
          initial?.value !== false
          || initial?.levelOfControl !== "controllable_by_this_extension"
          || controlled?.value !== false
          || controlled?.levelOfControl !== "controlled_by_this_extension"
          || cleared?.value !== false
          || cleared?.levelOfControl !== "controllable_by_this_extension"
        ) {
          throw new Error("privacy-setting-invalid");
        }
        setTimeout(() => {
          sendResponse({
            kind: "popup-background-ready",
            token: message.token,
            async: true,
            privacy: "disabled-only",
          });
        }, 750);
      })
      .catch(() => sendResponse({ kind: "popup-background-invalid", token: message.token }));
    return true;
  }
  if (message?.kind === "credential-selection" && message?.token === credentialToken) {
    if (!Number.isInteger(sender?.tab?.id) || message.credentialId !== fixtureCredential.credentialId) {
      sendResponse({ kind: "credential-refused", token: credentialToken });
      return undefined;
    }
    sendResponse({
      kind: "credential-response",
      token: credentialToken,
      ...fixtureCredential,
    });
    return undefined;
  }
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
