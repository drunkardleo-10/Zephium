const nonce = new URLSearchParams(location.search).get("nonce") ?? "";
const marker = Symbol.for("zephium.webkit-api-compatibility.v1");
const modeMarker = Symbol.for("zephium.webkit-api-compatibility.mode.v1");
const chromeId = globalThis.chrome?.runtime?.id;
const browserId = globalThis.browser?.runtime?.id;
const mode = globalThis[modeMarker] ?? "missing";
const passed =
  /^[0-9a-f]{32}$/.test(nonce) &&
  globalThis[marker] === true &&
  typeof chromeId === "string" &&
  chromeId.length > 0 &&
  chromeId === browserId &&
  (mode === "native-preserved" || mode === "native-aliased");

parent.postMessage(
  {
    kind: "zephium-web-accessible-extension-page-v1",
    nonce,
    mode,
    passed,
  },
  "*",
);
