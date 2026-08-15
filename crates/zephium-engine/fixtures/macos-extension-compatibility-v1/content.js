const api = globalThis.browser ?? globalThis.chrome;
const marker = Symbol.for("zephium.webkit-api-compatibility.v1");
const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
const token = "zephium-compatibility-round-trip-v1";
const attribute = "data-zephium-compatibility-round-trip";
const contentModeAttribute = "data-zephium-compatibility-content-mode";
const backgroundModeAttribute = "data-zephium-compatibility-background-mode";
const credentialFillAttribute = "data-zephium-credential-fill";
const credentialSelectionAttribute = "data-zephium-credential-selection";
const credentialToken = "zephium-credential-selection-v1";
const credentialId = "fixture-login-v1";
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

const setCredentialFailure = (reason) => {
  if (root.getAttribute(credentialFillAttribute) === "pending") {
    root.setAttribute(credentialFillAttribute, `invalid:${reason}`);
  }
};

function installCredentialSurface() {
  const usernames = document.querySelectorAll('input[autocomplete="username"]');
  const passwords = document.querySelectorAll('input[autocomplete="current-password"]');
  if (usernames.length !== 1 || passwords.length !== 1) {
    setCredentialFailure("field-cardinality");
    return;
  }
  const username = usernames[0];
  const password = passwords[0];
  if (
    !(username instanceof HTMLInputElement) ||
    !(password instanceof HTMLInputElement) ||
    username.type !== "email" ||
    password.type !== "password"
  ) {
    setCredentialFailure("field-type");
    return;
  }

  const host = document.createElement("span");
  host.setAttribute("data-zephium-credential-host", "v1");
  const shadow = host.attachShadow({ mode: "closed" });
  const sandbox = document.createElement("iframe");
  sandbox.setAttribute("sandbox", "allow-scripts");
  sandbox.setAttribute("title", "Choose a saved login");
  sandbox.style.cssText = "border:0;width:220px;height:44px;display:block";
  shadow.append(sandbox);
  document.body.append(host);

  if (typeof globalThis.crypto?.getRandomValues !== "function") {
    setCredentialFailure("random-source");
    return;
  }
  const random = new Uint32Array(4);
  crypto.getRandomValues(random);
  const nonce = [...random].join("-");
  let consumed = false;
  const onSelection = (event) => {
    if (
      consumed ||
      event.source !== sandbox.contentWindow ||
      event.origin !== "null" ||
      event.data?.kind !== "zephium-credential-inline-selection-v1" ||
      event.data?.nonce !== nonce ||
      event.data?.credentialId !== credentialId ||
      typeof event.data?.trusted !== "boolean"
    ) {
      return;
    }
    consumed = true;
    removeEventListener("message", onSelection);
    root.setAttribute(credentialSelectionAttribute, event.data.trusted ? "trusted" : "simulated");
    void Promise.resolve(
      api.runtime.sendMessage({
        kind: "credential-selection",
        token: credentialToken,
        credentialId,
      }),
    ).then(
      (response) => {
        const valid =
          response?.kind === "credential-response" &&
          response?.token === credentialToken &&
          response?.credentialId === credentialId &&
          typeof response?.username === "string" &&
          response.username.length > 0 &&
          response.username.length <= 256 &&
          typeof response?.password === "string" &&
          response.password.length > 0 &&
          response.password.length <= 1024;
        if (!valid) {
          setCredentialFailure("response");
          return;
        }
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
        if (typeof setter !== "function") {
          setCredentialFailure("input-setter");
          return;
        }
        for (const [field, value] of [
          [username, response.username],
          [password, response.password],
        ]) {
          setter.call(field, value);
          field.dispatchEvent(
            new InputEvent("input", {
              bubbles: true,
              composed: true,
              inputType: "insertReplacementText",
            }),
          );
          field.dispatchEvent(new Event("change", { bubbles: true, composed: true }));
        }
        root.setAttribute(credentialFillAttribute, "passed");
      },
      () => setCredentialFailure("background"),
    );
  };
  addEventListener("message", onSelection);

  void (async () => {
    try {
      const response = await fetch(api.runtime.getURL("credential-inline.payload"));
      if (!response.ok) throw new Error("payload-status");
      const payload = await response.text();
      const placeholder = "__ZEPHIUM_CREDENTIAL_NONCE__";
      if (payload.length === 0 || payload.length > 8192 || payload.split(placeholder).length !== 2) {
        throw new Error("payload-contract");
      }
      const url = URL.createObjectURL(
        new Blob([payload.replace(placeholder, nonce)], { type: "text/html" }),
      );
      sandbox.addEventListener("load", () => URL.revokeObjectURL(url), { once: true });
      sandbox.src = url;
    } catch (error) {
      removeEventListener("message", onSelection);
      setCredentialFailure(String(error?.message ?? error).slice(0, 48));
    }
  })();
}

root.setAttribute(credentialFillAttribute, "pending");
root.setAttribute(credentialSelectionAttribute, "pending");
if (document.readyState === "loading") {
  addEventListener("DOMContentLoaded", installCredentialSurface, { once: true });
} else {
  installCredentialSurface();
}
