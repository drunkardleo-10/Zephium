/** A bounded historical excerpt supplied by the trusted host; never a live page capability. */
export type EvidenceView =
  | { state: "loading" }
  | { state: "unavailable"; reason: string }
  | {
      state: "ready";
      title: string;
      origin: string;
      role: string;
      text: string;
      truncated: boolean;
      sourceBytes: string;
      /** Provider attribution only, never a native browser resource. */
      citation?: { provider: string; model: string; url: string; title: string };
    };
