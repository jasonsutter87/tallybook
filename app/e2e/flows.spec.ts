import { expect, test } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { api, demoBook, go, householdBook, license } from "./helpers";
import { DATA_DIR } from "./paths";

// The jobs people come to Tallybook for, done the way a person does them:
// through the screens, checked against the books afterwards.

test.describe("starting out", () => {
  test("a free copy makes a household book, and the business option shows its price", async ({ page, request }) => {
    license("free");
    await api(request, "book.close");
    await go(page, "/");

    await page.getByRole("button", { name: "Create a new book" }).click();
    const dialog = page.getByRole("dialog");
    // Free copies start on the free choice.
    await expect(dialog.getByRole("radio", { name: /A household/ })).toHaveAttribute("aria-checked", "true");

    // Business is offered at its price, and can't be created without a license.
    await dialog.getByRole("radio", { name: /A business/ }).click();
    await expect(dialog.getByText("$19.99").first()).toBeVisible();
    await dialog.getByLabel("Business name").fill("Should Not Exist");
    await expect(dialog.getByRole("button", { name: "Create book" })).toBeDisabled();

    // The household book goes through.
    await dialog.getByRole("radio", { name: /A household/ }).click();
    const name = `Rivera household ${Date.now()}`;
    await dialog.getByLabel("Household name").fill(name);
    await dialog.getByRole("button", { name: "Create book" }).click();

    await expect(page.locator(".sidebar").getByText(name)).toBeVisible();
    await expect(page.getByRole("link", { name: /Bills & due dates/ })).toBeVisible();
    const status = await api<{ sides: string[] }>(request, "book.status");
    expect(status.sides).toEqual(["household"]);
  });
});

test.describe("licensing", () => {
  test("tax prep waits behind Business + Tax, then opens", async ({ page, request }) => {
    license("business");
    await demoBook(request);

    await go(page, "/taxes");
    await expect(page.getByText("Tallybook Business + Tax")).toBeVisible();
    await expect(page.getByRole("button", { name: /Buy for \$49\.99/ })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Schedule C" })).toHaveCount(0);

    license("tax");
    await go(page, "/taxes");
    await expect(page.getByRole("tab", { name: "Schedule C" })).toBeVisible();
    await expect(page.getByText("Tallybook Business + Tax")).toHaveCount(0);
  });

  test("a license for an earlier season offers the renewal, not the new year", async ({ page, request }) => {
    const year = new Date().getFullYear();
    license("tax", [year - 2, year - 1]);
    await demoBook(request);

    await go(page, `/taxes?year=${year}`);
    await expect(page.getByText(`Tax prep for ${year} is a new tax season.`)).toBeVisible();
    await expect(page.getByRole("button", { name: /Buy for \$29\.99/ })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Schedule C" })).toHaveCount(0);

    await go(page, "/settings");
    await expect(page.locator("#license").getByText(`Tax years ${year - 2}–${year - 1}`)).toBeVisible();
  });

  test("Settings shows the license and can remove it from this computer", async ({ page, request }) => {
    license("tax");
    await demoBook(request);
    await go(page, "/settings");

    const card = page.locator("#license");
    await expect(card.getByText("Business + Tax", { exact: true })).toBeVisible();
    await expect(card.getByText("Key ending 0000")).toBeVisible();

    await card.getByRole("button", { name: "Remove from this computer" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Remove it" }).click();
    await expect(card.getByText("Household (free)")).toBeVisible();
    await expect(card.getByPlaceholder("Paste your license key")).toBeVisible();
  });
});

test.describe("the business", () => {
  test("review: accepting Tallybook's suggestions files them", async ({ page, request }) => {
    license("tax");
    await demoBook(request);
    const before = await api<number>(request, "inbox.count");
    expect(before).toBeGreaterThan(0);

    await go(page, "/review");
    await page.getByRole("button", { name: /Accept all suggested/ }).click();

    await expect.poll(() => api<number>(request, "inbox.count")).toBeLessThan(before);
    const reviewed = await api<unknown[]>(request, "inbox.reviewed");
    expect(reviewed.length).toBeGreaterThan(0);
  });

  test("invoice: write it, issue it, get paid", async ({ page, request }) => {
    license("tax");
    await demoBook(request);

    await go(page, "/invoices/new");
    await page.getByPlaceholder("Choose a customer").click();
    await page.getByRole("option").first().click();
    await page.getByLabel("Description").first().fill("Brand workshop");
    await page.getByLabel("Quantity").first().fill("2");
    await page.getByLabel("Price").first().fill("450");
    await expect(page.getByText("$900.00").first()).toBeVisible();

    await page.getByRole("button", { name: /Save & issue/ }).first().click();
    await expect(page).toHaveURL(/#\/invoices\/\d+$/);
    const id = Number(page.url().split("/").pop());
    const issued = await api<{ summary?: { status: string; total: number }; status?: string; total?: number }>(request, "invoices.get", { id });
    const summary = issued.summary ?? issued;
    expect(summary.total).toBe(90000);

    await page.getByRole("button", { name: "Record payment" }).first().click();
    const dialog = page.getByRole("dialog");
    // Everything a payment needs is filled in from the invoice.
    await expect(dialog.getByLabel("Deposit to")).toHaveAttribute("placeholder", /Business Checking/);
    await dialog.getByRole("button", { name: "Record $900.00" }).click();
    await expect(dialog).toBeHidden();

    await expect.poll(async () => {
      const inv = await api<{ summary?: { status: string }; status?: string }>(request, "invoices.get", { id });
      return (inv.summary ?? inv).status;
    }).toBe("paid");
  });

  test("taxes: the accountant package lands on disk, PDFs included", async ({ page, request }) => {
    license("tax");
    await demoBook(request);
    await go(page, "/taxes/export");

    await page.getByRole("button", { name: /Create tax package/ }).click();
    await expect(page.getByText(/tax package is ready/)).toBeVisible();
    const folder = (await page.locator(".tx-path").textContent())!.trim();
    expect(folder.startsWith(DATA_DIR)).toBe(true);
    for (const f of ["Tax summary.pdf", "General ledger.pdf", "Schedule C worksheet.csv", "README.txt"]) {
      expect(fs.existsSync(path.join(folder, f)), f).toBe(true);
    }
    expect(fs.readFileSync(path.join(folder, "Tax summary.pdf")).subarray(0, 5).toString()).toBe("%PDF-");
  });

  test("the P&L on screen matches the books", async ({ page, request }) => {
    license("tax");
    await demoBook(request);
    const year = new Date().getFullYear();
    const pnl = await api<{ net_income: number }>(request, "reports.pnl", { from: `${year}-01-01`, to: `${year}-12-31` });
    const dollars = (pnl.net_income / 100).toLocaleString("en-US", { minimumFractionDigits: 2 });

    await go(page, `/reports/pnl?from=${year}-01-01&to=${year}-12-31`);
    await expect(page.getByText(dollars).first()).toBeVisible();
  });
});

test.describe("the household", () => {
  test("plan the month, then log spending against it", async ({ page, request }) => {
    license("free");
    await householdBook(request);
    await api(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 300000, opening_date: "2026-01-01" });

    await go(page, "/month");
    await page.getByRole("button", { name: /Make a plan|Edit plan/ }).first().click();
    const plan = page.getByRole("dialog");
    await plan.getByPlaceholder(/Add a category/).click();
    await plan.getByPlaceholder(/Add a category/).fill("Groceries");
    await page.getByRole("option", { name: /Groceries/ }).first().click();
    await plan.getByLabel("Amount for Groceries").fill("600");
    await plan.getByRole("button", { name: "Save plan" }).click();
    await expect(plan).toBeHidden();
    await expect(page.getByText("Groceries").first()).toBeVisible();

    await page.getByRole("button", { name: /Log money/ }).first().click();
    const log = page.getByRole("dialog");
    await log.getByLabel("How much?").fill("84.20");
    await log.getByLabel("What was it?").click();
    await log.getByLabel("What was it?").fill("Groceries");
    await page.getByRole("option", { name: /Groceries/ }).first().click();
    await log.getByLabel("Paid from").click();
    await page.getByRole("option", { name: /Joint Checking/ }).first().click();
    await log.getByRole("button", { name: "Log it" }).click();
    await expect(log).toBeHidden();

    const month = await api<{ lines: { name: string; amount: number; progress: number }[] }>(request, "budget.month");
    const groceries = month.lines.find((l) => l.name === "Groceries")!;
    expect(groceries.amount).toBe(60000);
    expect(groceries.progress).toBe(8420);
  });
});
