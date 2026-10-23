import { expect, test } from "@playwright/test";
import { api, demoBook, go, license } from "./helpers";

// Reports: the date-range presets set the right dates, and in a book that
// keeps both sides, A/R and A/P stay on the business while P&L can be flipped
// between business and household.

const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

test("each date-range preset sets its own dates and loads the books for them", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  const { today } = await api<{ today: string }>(request, "book.status");
  const [y, m0] = today.split("-").map((n) => Number(n)) as [number, number];
  const m = m0 - 1;
  const q = Math.floor(m / 3) * 3;
  const presets: [string, string, string][] = [
    ["This month", iso(new Date(y, m, 1)), iso(new Date(y, m + 1, 0))],
    ["Last month", iso(new Date(y, m - 1, 1)), iso(new Date(y, m, 0))],
    ["This quarter", iso(new Date(y, q, 1)), iso(new Date(y, q + 3, 0))],
    ["Year to date", `${y}-01-01`, today],
    ["Last year", `${y - 1}-01-01`, `${y - 1}-12-31`],
  ];

  await go(page, "/reports/pnl");
  for (const [label, from, to] of presets) {
    await page.getByRole("tab", { name: label, exact: true }).click();
    await expect(page.getByLabel("From"), label).toHaveValue(from);
    await expect(page.getByLabel("To"), label).toHaveValue(to);
    await expect(page.getByRole("tab", { name: label, exact: true })).toHaveClass(/active/);

    const pnl = await api<{ net_income: number }>(request, "reports.pnl", { from, to });
    const dollars = (Math.abs(pnl.net_income) / 100).toLocaleString("en-US", { minimumFractionDigits: 2 });
    await expect(page.getByText(dollars).first(), `${label}: net income ${dollars}`).toBeVisible();
  }

  // Typing a date by hand leaves every preset and lands on Custom.
  await page.getByLabel("From").fill(`${y}-02-03`);
  await expect(page.getByText("Custom", { exact: true })).toHaveClass(/active/);
});

test("A/R and A/P aging show the business, even in a book that keeps a household too", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  await api(request, "book.add_side", { scope: "household" });

  const ar = await api<{ rows: { name: string }[]; totals: { current: number; days_31_60: number } }>(request, "reports.ar_aging", {});
  const ap = await api<{ rows: { name: string }[] }>(request, "reports.ap_aging", {});
  expect(ar.rows.length).toBeGreaterThan(0);
  expect(ap.rows.length).toBeGreaterThan(0);

  await go(page, "/reports/ar-aging");
  // Aging has no side switch: invoices and bills only exist on the business side.
  await expect(page.getByRole("tab", { name: "Household" })).toHaveCount(0);
  for (const row of ar.rows) await expect(page.getByRole("cell", { name: row.name, exact: true })).toBeVisible();
  const owed = ar.totals.current + ar.totals.days_31_60;
  await expect(page.getByText(`$${(owed / 100).toLocaleString("en-US", { minimumFractionDigits: 2 })}`).first()).toBeVisible();

  await go(page, "/reports/ap-aging");
  await expect(page.getByRole("tab", { name: "Household" })).toHaveCount(0);
  for (const row of ap.rows) await expect(page.getByRole("cell", { name: row.name, exact: true })).toBeVisible();

  // Asked for the household's side, the backend says nobody owes it anything.
  for (const method of ["reports.ar_aging", "reports.ap_aging"]) {
    const none = await api<{ rows: unknown[]; totals: Record<string, number> }>(request, method, { scope: "household" });
    expect(none.rows, method).toEqual([]);
    expect(Object.values(none.totals).every((v) => v === 0), method).toBe(true);
  }
});

test("a two-sided book's P&L flips between business and household", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  await api(request, "book.add_side", { scope: "household" });
  const year = new Date().getFullYear();

  await go(page, `/reports/pnl?from=${year}-01-01&to=${year}-12-31`);
  await page.getByRole("tab", { name: "Household" }).click();
  await expect(page).toHaveURL(/side=household/);
  const household = await api<{ net_income: number }>(request, "reports.pnl", { from: `${year}-01-01`, to: `${year}-12-31`, scope: "household" });
  const business = await api<{ net_income: number }>(request, "reports.pnl", { from: `${year}-01-01`, to: `${year}-12-31`, scope: "business" });
  expect(household.net_income, "the new household side starts empty").toBe(0);
  expect(business.net_income).not.toBe(0);

  await page.getByRole("tab", { name: "Business" }).click();
  await expect(page).toHaveURL(/side=business/);
  const dollars = (Math.abs(business.net_income) / 100).toLocaleString("en-US", { minimumFractionDigits: 2 });
  await expect(page.getByText(dollars).first()).toBeVisible();
});
