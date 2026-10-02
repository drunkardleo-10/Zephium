// Fixtures use US English; the host OS locale must not change their expectations.
for (const name of ["NumberFormat", "DateTimeFormat", "RelativeTimeFormat"] as const) {
  Object.defineProperty(Intl, name, {
    value: new Proxy(Intl[name], {
      construct(target, args) {
        return Reflect.construct(target, [args[0] ?? "en-US", ...args.slice(1)]);
      },
      apply(target, receiver, args) {
        return Reflect.apply(target, receiver, [args[0] ?? "en-US", ...args.slice(1)]);
      },
    }),
  });
}
