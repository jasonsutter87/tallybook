import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { api, demoBook, go, householdBook, license, watchErrors } from "./helpers";

// Every screen should render without errors, show no error banner, and pass
// automated accessibility checks for serious problems.

// A sweep loads each screen fresh, so it takes longer than one flow.
test.describe.configure({ timeout: 120_000 });

const BUSINESS = [
  "/",
  "/review",
  "/accounts",
  "/invoices",
  "/invoices/new",
  "/bills",
  "/bills/new",
  "/contacts",
  "/reports",
  "/reports/pnl",
  "/reports/balance-sheet",
  "/rules",
  "/banking",
  "/settings",
  "/taxes",
  "/taxes/schedule-c",
  "/taxes/estimates",
  "/taxes/1099",
  "/taxes/export",
];

const HOUSEHOLD = ["/", "/month", "/due", "/goals", "/accounts", "/reports", "/settings"];

async function sweep(page: import("@playwright/test").Page, routes: string[]) {
  for (const route of routes) {
    await test.step(route, async () => {
      const errors = watchErrors(page);
      await go(page, route);
      await expect(page.locator("main, .page").first()).toBeVisible();
      await expect(page.locator(".banner.error"), `error banner on ${route}`).toHaveCount(0);
      const axe = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa"]).analyze();
      const serious = axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical");
      expect(serious.map((v) => `${v.id}: ${v.help} (${v.nodes.length})`), `accessibility on ${route}`).toEqual([]);
      expect(errors, `console errors on ${route}`).toEqual([]);
      page.removeAllListeners("console");
      page.removeAllListeners("pageerror");
    });
  }
}

test("every business screen renders cleanly", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  const invoices = await api<{ id: number }[]>(request, "invoices.list");
  await sweep(page, [...BUSINESS, `/invoices/${invoices[0].id}`]);
});

test("every household screen renders cleanly", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  await sweep(page, HOUSEHOLD);
});

test("dark mode reads as well as light", async ({ page, request }) => {
  license("tax");
  await page.emulateMedia({ colorScheme: "dark" });
  await demoBook(request);
  await sweep(page, ["/", "/review", "/invoices", "/bills", "/contacts", "/reports/pnl", "/settings", "/taxes", "/taxes/schedule-c"]);
  await householdBook(request);
  await sweep(page, ["/month", "/due", "/goals"]);
});
