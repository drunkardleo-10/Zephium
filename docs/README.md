# Zephium documentation map

Read the documents according to the question being answered:

1. `work-runtime-continuation.md` is the current implementation handoff: what
   is proven, what remains, and the Rust Work runtime now being built.
2. `product-system.md` defines the product, first-release scope, product model,
   and target system architecture.
3. `agentic-browsing.md` defines the implementation and qualification program
   for the browser-execution layer behind Work.
4. `architecture.md` defines the implemented browser structure, domain
   ownership, native lifecycle, and agreed low-level browser target.
5. `security-model.md` defines security guarantees actually enforced by the
   current tree and the gates for product-disabled capabilities.
6. `frontend.md` is the working contract for the current Svelte frame.
7. `adblock.md` defines the native network blocker.
8. `security-maintenance.md` defines recurring security and release evidence.

When documents appear to disagree:

- code and tests are evidence of present behavior;
- `security-model.md` controls claims about present security guarantees;
- `architecture.md` controls the present browser foundation;
- `product-system.md` controls product intent and target architecture;
- `work-runtime-continuation.md` controls the current Work implementation
  checkpoint and delivery direction without weakening the sources above;
- the narrowest accepted subsystem specification controls implementation of a
  not-yet-shipping feature.

Do not rewrite an implemented security or native-layout invariant merely to
make a target product feature easier. Record the evidence, revise the
replaceable mechanism, and use a short ADR only when an expensive-to-reverse
boundary must change.
