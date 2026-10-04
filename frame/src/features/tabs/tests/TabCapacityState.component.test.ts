import { expect, test, vi } from "vitest";
import { render } from "vitest-browser-svelte";
import { tabs } from "$domain/tabs";
import { tabFixture } from "$shared/testing/fixtures";
import TabCapacityState from "../components/TabCapacityState.svelte";

vi.mock("$domain/tabs", () => ({ tabs: { navigate: vi.fn() } }));

test("capacity retry is an explicit navigation to the held intent, while waiting offers no retry", async () => {
  const blocked = tabFixture({
    title: "Committed title",
    url: "https://old.example/",
    availability: { state: "blocked_by_capacity", url: "https://requested.example/path" },
  });
  const screen = await render(TabCapacityState, { tab: blocked });
  await screen.getByRole("button", { name: "Try again" }).click();
  expect(tabs.navigate).toHaveBeenCalledExactlyOnceWith(
    blocked.id,
    "https://requested.example/path",
  );
  expect(blocked.title).toBe("Committed title");
  await screen.rerender({
    tab: {
      ...blocked,
      availability: { state: "waiting_for_capacity", url: "https://requested.example/path" },
    },
  });
  await expect.element(screen.getByRole("button", { name: "Try again" })).not.toBeInTheDocument();
  await expect.element(screen.getByRole("status")).toHaveTextContent("Making room for this page");
});
