import { expect, test } from "@playwright/test";
import { api, bookPath, demoBook, go, license } from "./helpers";

// The tax center beyond the export: the quarterly-estimates plan and recording
// (then voiding) a payment, and the 1099 tab's W-9 and contractor details.

type Estimate = {
  plan_annual: number;
  quarters: { quarter: number; planned: number; paid_federal: number; status: string }[];
  payments: { id: number; amount: number; quarter: number; jurisdiction: string; voided: boolean }[];
};

test("estimates: answering the setup questions makes a plan, and a payment is recorded against it", async ({ page, request }) => {
  license("tax");
  await api(request, "book.create", { path: bookPath("estimates"), company_name: "Estimates Test Co" });
  await api(request, "accounts.create", { name: "Operating", subtype: "bank", opening_balance: 5_000_000, opening_date: `${new Date().getFullYear()}-01-01` });
  await go(page, "/taxes/estimates");

  // First visit: it asks about last year before it will suggest anything.
  await expect(page.getByText("Let's set up your estimate")).toBeVisible();
  await page.getByLabel(/Last year's total tax/).fill("20000");
  await page.getByRole("button", { name: "Show my plan" }).click();
  await expect(page.getByText("Saved — your estimate is updated")).toBeVisible();

  const profile = await api<{ method: string; last_year_tax: number }>(request, "tax.profile");
  expect(profile).toMatchObject({ method: "safe_harbor", last_year_tax: 2_000_000 });
  let est = await api<Estimate>(request, "tax.estimate", {});
  expect(est.plan_annual, "safe harbor: last year's tax").toBe(2_000_000);
  const quarter = est.quarters[0].planned;
  expect(quarter * 4).toBe(2_000_000);
  await expect(page.locator(".hero")).toContainText("Pay about");
  await expect(page.locator(".hero")).toContainText("5,000");
  await expect(page.getByText("No payments yet")).toBeVisible();

  // Record this quarter's payment; its amount is filled in from the plan.
  await page.getByRole("button", { name: "Record a payment" }).click();
  const dialog = page.getByRole("dialog", { name: "Record an estimated tax payment" });
  await expect(dialog.getByLabel("Amount")).toHaveValue("5000.00");
  await dialog.getByLabel("Quarter").selectOption("1");
  await dialog.getByRole("button", { name: "Record payment" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByText(/Recorded \$5,000\.00 for Q1/)).toBeVisible();

  est = await api<Estimate>(request, "tax.estimate", {});
  expect(est.payments).toHaveLength(1);
  expect(est.payments[0]).toMatchObject({ amount: 500_000, quarter: 1, jurisdiction: "federal", voided: false });
  await expect(page.getByRole("button", { name: "Void this payment" })).toBeVisible();

  // Taking it back leaves it in the history, voided.
  await page.getByRole("button", { name: "Void this payment" }).click();
  await page.getByRole("dialog", { name: "Void this payment?" }).getByRole("button", { name: "Void payment" }).click();
  await expect(page.getByText("Payment voided")).toBeVisible();
  est = await api<Estimate>(request, "tax.estimate", {});
  expect(est.payments.map((p) => p.voided)).toEqual([true]);
});

test("estimates: a payment with no amount is refused in words, and nothing is recorded", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  await api(request, "tax.set_profile", {
    filing_status: "single",
    method: "current_year",
    other_income: 0,
    w2_wages: 0,
    withholding: 0,
    home_office_sqft: 0,
    state_rate_bp: 0,
  });
  const before = (await api<Estimate>(request, "tax.estimate", {})).payments.length;
  await go(page, "/taxes/estimates");

  await page.getByRole("button", { name: "Record a payment" }).click();
  const dialog = page.getByRole("dialog", { name: "Record an estimated tax payment" });
  await dialog.getByLabel("Amount").fill("");
  await dialog.getByRole("button", { name: "Record payment" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Enter how much you paid.");
  expect((await api<Estimate>(request, "tax.estimate", {})).payments, "nothing was recorded").toHaveLength(before);
});

test("1099s: a contractor gets a W-9, a legal name and the last four of their tax ID", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  const accounts = await api<{ id: number; name: string; subtype: string; archived: boolean }[]>(request, "accounts.list");
  const bank = accounts.find((a) => a.name === "Business Checking")!.id;
  const expense = accounts.find((a) => a.subtype === "expense" && !a.archived)!.id;
  const vendor = await api<number>(request, "contacts.create", { kind: "vendor", name: "Lena Ortiz Design" });
  const year = new Date().getFullYear();
  const entry = await api<number>(request, "entries.post", {
    date: `${year}-02-10`,
    memo: "Logo project",
    payee_id: vendor,
    lines: [
      { account_id: expense, amount: 900_000 },
      { account_id: bank, amount: -900_000 },
    ],
  });
  expect(entry).toBeGreaterThan(0);

  await go(page, "/taxes/1099");
  const row = page.getByRole("row", { name: /Lena Ortiz Design/ });
  await expect(row).toContainText("$9,000.00");
  await expect(row.getByText("Missing")).toBeVisible();
  await expect(row).toContainText("Not marked as contractor");

  await row.getByRole("button", { name: "Mark as contractor" }).click();
  await expect(page.getByText("Lena Ortiz Design marked as a 1099 contractor")).toBeVisible();
  await expect(row).toContainText("Needs 1099");

  await row.getByRole("button", { name: "Edit tax details for Lena Ortiz Design" }).click();
  const dialog = page.getByRole("dialog", { name: /Tax details · Lena Ortiz Design/ });
  await dialog.getByLabel(/W-9 on file/).check();
  await dialog.getByLabel(/Legal name/).fill("Lena M. Ortiz");
  await dialog.getByLabel(/Tax ID/).fill("4321");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(dialog).toBeHidden();

  await expect(row.getByText("On file")).toBeVisible();
  await expect(row).toContainText("Lena M. Ortiz");
  await expect(row).toContainText("···4321");
  const report = await api<{ rows: { name: string; is_1099: boolean; w9_on_file: boolean; legal_name: string; tin_last4: string; over_threshold: boolean }[] }>(request, "tax.1099", { year });
  expect(report.rows.find((r) => r.name === "Lena Ortiz Design")).toMatchObject({ is_1099: true, w9_on_file: true, legal_name: "Lena M. Ortiz", tin_last4: "4321", over_threshold: true });
  await expect(page.getByText("Every form has a W-9 on file")).toBeVisible();
});
