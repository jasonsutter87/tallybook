import { expect, test } from "@playwright/test";
import { api, demoBook, go, license } from "./helpers";

// Big numbers stay readable: full dollars and cents in the cards, short
// "$1.3M"-style labels on the chart's axis (never "$1000k").

test("large amounts read sensibly on the dashboard and its chart", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  const accounts = await api<{ id: number; name: string; subtype: string; archived: boolean }[]>(request, "accounts.list");
  const bank = accounts.find((a) => a.name === "Business Checking")!.id;
  const income = accounts.find((a) => a.subtype === "income" && !a.archived)!.id;
  const { today } = await api<{ today: string }>(request, "book.status");
  // $1,250,000.00 in this month: the cash on hand and a bar far above the demo's.
  await api(request, "entries.post", {
    date: today,
    memo: "Building sale",
    lines: [
      { account_id: bank, amount: 125_000_000 },
      { account_id: income, amount: -125_000_000 },
    ],
  });
  const dash = await api<{ cash: number }>(request, "dashboard", {});
  expect(dash.cash).toBeGreaterThan(125_000_000);

  await go(page, "/");
  const dollars = (c: number) => (c / 100).toLocaleString("en-US", { style: "currency", currency: "USD" });
  await expect(page.locator(".hero-amount")).toContainText(dollars(dash.cash).replace(/\.\d\d$/, ""));

  const chart = page.getByRole("img", { name: /Cash flow by month/ });
  await expect(chart.locator(".recharts-bar-rectangle").first()).toBeVisible();
  await expect(chart).toHaveAttribute("aria-label", /\$1,2\d\d,\d{3}\.\d\d in/);

  const labels = await chart.locator(".recharts-cartesian-axis-tick-value").allTextContents();
  const ticks = labels.filter((t) => t.startsWith("$"));
  expect(ticks.length).toBeGreaterThan(2);
  for (const t of ticks) expect(t, "axis label").toMatch(/^\$\d{1,3}(\.\d)?[kM]?$/);
  expect(ticks.join(" ")).toMatch(/\$\d+(\.\d)?M/);
  expect(ticks.join(" ")).not.toMatch(/\d{4}k/);
});
