import { describe, expect, it } from "vitest";
import { browserPasskeyStatus } from "../src/domain/credentials/browser-credentials-model";

describe("browser credential capability copy", () => {
  it("distinguishes an unapproved build from a system failure", () => {
    expect(browserPasskeyStatus("entitlement_required")).toBe(
      "Passkeys require an approved macOS browser build.",
    );
    expect(browserPasskeyStatus("unavailable")).toBe(
      "Passkey status is unavailable on this system.",
    );
  });

  it("keeps every remaining native state explicit", () => {
    expect(browserPasskeyStatus("authorized")).toContain("enabled");
    expect(browserPasskeyStatus("denied")).toContain("System Settings");
    expect(browserPasskeyStatus("not_determined")).toContain("Enable passkeys");
    expect(browserPasskeyStatus("unknown")).toContain("unavailable");
    expect(browserPasskeyStatus("unsupported")).toContain("not provided");
    expect(browserPasskeyStatus(null)).toContain("not provided");
  });
});
