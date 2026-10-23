import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// The household's Month view: step back and forward through the months, and
// see each one judged the right way (this month in progress, past ones
// finished, later ones still to come).

const name = (ym: string) => {
  const [y, m] = ym.split("-").map(Number);
  return new Date(y, m - 1, 1).toLocaleDateString("en-US", { month: "long", year: "numeric" });
};
const shift = (ym: string, by: number) => {
  const [y, m] = ym.split("-").map(Number);
  const d = new Date(y, m - 1 + by, 1);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
};

test("month navigation: back to a finished month, forward to one still coming, and home again", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const { today } = await api<{ today: string }>(request, "book.status");
  const now = today.slice(0, 7);
  const title = page.locator(".page-title");
  const subtitle = page.locator(".page-subtitle");

  await go(page, "/month");
  await expect(title).toContainText(name(now));
  await expect(subtitle).toContainText(/Day \d+ of \d+/);

  await page.getByRole("button", { name: "Previous month" }).click();
  await expect(page).toHaveURL(new RegExp(`#/month/${shift(now, -1)}$`));
  await expect(title).toContainText(name(shift(now, -1)));
  await expect(subtitle).toContainText("Finished");
  expect((await api<{ month: string; is_current: boolean }>(request, "budget.month", { month: shift(now, -1) })).is_current).toBe(false);

  await page.getByRole("button", { name: "Previous month" }).click();
  await expect(title).toContainText(name(shift(now, -2)));

  // A month in the future hasn't started.
  await page.getByRole("link", { name: "Back to this month" }).click();
  await expect(page).toHaveURL(/#\/month$/);
  await expect(title).toContainText(name(now));
  await page.getByRole("button", { name: "Next month" }).click();
  await expect(page).toHaveURL(new RegExp(`#/month/${shift(now, 1)}$`));
  await expect(title).toContainText(name(shift(now, 1)));
  await expect(subtitle).toContainText("Coming up");

  // Stepping forward onto the current month returns to the plain /month address.
  await page.getByRole("button", { name: "Previous month" }).click();
  await expect(page).toHaveURL(/#\/month$/);
  await expect(subtitle).toContainText(/Day \d+ of \d+/);
});

test("a month typed into the address that isn't a month falls back to this one", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const { today } = await api<{ today: string }>(request, "book.status");
  await go(page, "/month/not-a-month");
  await expect(page.locator(".page-title")).toContainText(name(today.slice(0, 7)));
});

test("a plan set for one month carries forward to the next, and the later month says so", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const { today } = await api<{ today: string }>(request, "book.status");
  const now = today.slice(0, 7);
  const accounts = await api<{ id: number; name: string }[]>(request, "accounts.list");
  const groceries = accounts.find((a) => a.name === "Groceries")!.id;
  await api(request, "budget.set", { month: now, line: { target: { kind: "account", id: groceries }, amount: 60_000 } });

  await go(page, `/month/${shift(now, 1)}`);
  await expect(page.locator(".page-subtitle")).toContainText(`plan carried forward from ${new Date(`${now}-01T12:00:00`).toLocaleDateString("en-US", { month: "long" })}`);
  await expect(page.getByText("Groceries").first()).toBeVisible();
});
