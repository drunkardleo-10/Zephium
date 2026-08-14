const api = globalThis.browser ?? globalThis.chrome;
const marker = Symbol.for("zephium.webkit-api-compatibility.v1");
const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
const token = "zephium-compatibility-round-trip-v1";
const attribute = "data-zephium-compatibility-round-trip";
const contentModeAttribute = "data-zephium-compatibility-content-mode";
const backgroundModeAttribute = "data-zephium-compatibility-background-mode";
const root = document.documentElement;
const contentMode = globalThis[modeMarker] ?? "missing";
let settled = false;
let responsePassed = false;
let tabsPassed = false;

function settle(value) {
  if (settled) return;
  settled = true;
  root.setAttribute(attribute, value);
}

root.setAttribute(attribute, "armed");
root.setAttribute(contentModeAttribute, contentMode);
root.setAttribute(backgroundModeAttribute, "pending");
function accept(message, expectedChannel) {
  const supportedMode = (value) =>
    value === "native-preserved" || value === "native-aliased";
  const checks = {
    kind: message?.kind === "pong",
    token: message?.token === token,
    channel: message?.channel === expectedChannel,
    background: message?.backgroundInstalled === true,
    backgroundMode: supportedMode(message?.backgroundMode),
    sender: message?.senderTab === true,
    content: globalThis[marker] === true,
    contentMode: supportedMode(contentMode),
    chrome: Boolean(globalThis.chrome?.runtime?.id),
    browser: Boolean(globalThis.browser?.runtime?.id),
  };
  const failed = Object.entries(checks)
    .filter(([, passed]) => !passed)
    .map(([name]) => name);
  if (failed.length !== 0) {
    settle(`invalid:${expectedChannel}:${failed.join(",")}`);
    return;
  }
  root.setAttribute(backgroundModeAttribute, message.backgroundMode);
  if (expectedChannel === "runtime-response") responsePassed = true;
  if (expectedChannel === "tabs-message") tabsPassed = true;
  if (responsePassed && tabsPassed) settle("passed");
}

api.runtime.onMessage.addListener((message) => {
  if (message?.kind !== "pong" || message?.token !== token) return undefined;
  accept(message, "tabs-message");
  return undefined;
});
let attempts = 0;
function ping() {
  if (settled) return;
  attempts += 1;
  void Promise.resolve(api.runtime.sendMessage({ kind: "ping", token })).then(
    (response) => {
      if (response != null) accept(response, "runtime-response");
    },
    () => {},
  );
  if (attempts < 40) {
    setTimeout(ping, 100);
  } else {
    setTimeout(() => settle("exhausted"), 100);
  }
}
ping();
