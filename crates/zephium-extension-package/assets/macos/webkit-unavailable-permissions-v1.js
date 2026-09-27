(() => {
  "use strict";

  // WebKit cannot represent optional privacy permission. A well-formed
  // privacy-only readback is false. Mixed queries still ask WebKit to validate
  // the remaining permissions/origins before reporting false; native errors
  // and all queries without privacy retain their original behavior.
  const permissions = globalThis.chrome?.permissions ?? globalThis.browser?.permissions;
  const nativeContains = permissions?.contains;
  if (typeof nativeContains !== "function") {
    throw new Error("Extension permission readback is unavailable");
  }
  const required = globalThis.chrome?.runtime?.getManifest?.()?.permissions ?? [];
  const privacyUnavailable = !Array.isArray(required) || !required.includes("privacy");
  const contains = function (request, callback) {
    const names = request?.permissions;
    const hasPrivacy = privacyUnavailable && Array.isArray(names) &&
      names.every((name) => typeof name === "string") && names.includes("privacy") &&
      (callback === undefined || typeof callback === "function");
    if (hasPrivacy && names.length === 1 && request.origins === undefined) {
      if (typeof callback === "function") {
        queueMicrotask(() => callback(false));
        return undefined;
      }
      return Promise.resolve(false);
    }
    if (hasPrivacy) {
      const remaining = {...request, permissions: names.filter((name) => name !== "privacy")};
      if (typeof callback === "function") {
        return nativeContains.call(permissions, remaining, (granted) => {
          callback(chrome.runtime.lastError ? granted : false);
        });
      }
      return new Promise((resolve, reject) => {
        nativeContains.call(permissions, remaining, () => {
          if (chrome.runtime.lastError) reject(chrome.runtime.lastError);
          else resolve(false);
        });
      });
    }
    return callback === undefined
      ? nativeContains.call(permissions, request)
      : nativeContains.call(permissions, request, callback);
  };
  Object.defineProperty(permissions, "contains", {
    value: contains, writable: false, enumerable: false, configurable: false,
  });
  if (permissions.contains !== contains) {
    throw new Error("Extension permission readback was not installed");
  }
})();
