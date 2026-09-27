(() => {
  "use strict";

  // Older WebKit releases omit these two well-known symbol names. Define only
  // the names, before publisher modules bind their real disposal methods.
  // This is not a no-op disposer or an implementation of `using` syntax.
  if (location.protocol !== "webkit-extension:") return;
  for (const name of ["dispose", "asyncDispose"]) {
    if (!Object.hasOwn(Symbol, name)) {
      Object.defineProperty(Symbol, name, {
        value: Symbol(`Symbol.${name}`),
        writable: false,
        enumerable: false,
        configurable: false,
      });
    }
  }
})();
