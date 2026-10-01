import { expect, test } from "vitest";
import { mediaUrl } from "../media";

const PROFILE = "01M3CTWDG20GFZPACD7905RSRJ";
const DIGEST = "a".repeat(64);

test("a picture is asked for at the width it is shown, and a large view asks for more", () => {
  expect(mediaUrl(PROFILE, DIGEST)).toMatch(/\/01M3CTWDG20GFZPACD7905RSRJ\/a{64}\?w=720$/u);
  expect(mediaUrl(PROFILE, DIGEST, 1600)).toMatch(/\?w=1600$/u);
  expect(mediaUrl(PROFILE, "nope")).toBeNull();
});
