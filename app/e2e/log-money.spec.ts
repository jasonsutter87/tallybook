import { expect, test } from "@playwright/test";
import { api, bookPath, go, license } from "./helpers";

// "Log cash" on the business side: a sale, an expense paid to a contractor and
// a move between accounts, each posting a balanced entry the books show.

type Account = { id: number; name: string; subtype: string; balance: number };
type Entry = { memo: string; payee_id: number | null; lines: { account_id: number; amount: number }[] };

async function business(request: Parameters<typeof api>[0]) {
  license("tax");
  await api(request, "book.create", { path: bookPath("logcash"), company_name: "Log Cash Co" });
  const year = new Date().getFullYear();
  await api(request, "accounts.create", { name: "Cash Till", subtype: "bank", opening_balance: 20_000, opening_date: `${year}-01-01` });
  await api(request, "accounts.create", { name: "Business Checking", subtype: "bank", opening_balance: 100_000, opening_date: `${year}-01-01` });
  await api(request, "accounts.create", { name: "Market Sales", subtype: "income" });
  await api(request, "accounts.create", { name: "Booth Supplies", subtype: "expense" });
}
const balance = async (request: Parameters<typeof api>[0], name: string) => (await api<Account[]>(request, "accounts.list")).find((a) => a.name === name)!.balance;
const todaysEntries = async (request: Parameters<typeof api>[0]) => {
  const { today } = await api<{ today: string }>(request, "book.status");
  return api<Entry[]>(request, "entries.list", { from: today, to: today });
};

test("a cash sale is logged from the dashboard and lands in the till", async ({ page, request }) => {
  await business(request);
  await go(page, "/");

  await page.getByRole("button", { name: "Log cash" }).click();
  const dialog = page.getByRole("dialog", { name: "Log cash" });
  await dialog.getByRole("tab", { name: "Sale or money in" }).click();
  // The till is chosen for you.
  await expect(dialog.getByLabel("Went into")).toHaveValue(/Cash Till/);
  await dialog.getByLabel("How much?").fill("86.50");
  await dialog.getByLabel("What was it for?").click();
  await dialog.getByLabel("What was it for?").fill("Market");
  await page.getByRole("option", { name: /Market Sales/ }).first().click();
  await dialog.getByLabel("Note (optional)").fill("Saturday market");
  await dialog.getByRole("button", { name: "Log it" }).click();

  await expect(page.getByText("$86.50 in · logged")).toBeVisible();
  expect(await balance(request, "Cash Till")).toBe(20_000 + 8_650);
  expect(await balance(request, "Market Sales")).toBe(8_650);
  expect((await todaysEntries(request)).map((e) => e.memo)).toContain("Saturday market");
});

test("a cash expense paid to a contractor is tagged to them for the 1099", async ({ page, request }) => {
  await business(request);
  const lena = await api<number>(request, "contacts.create", { kind: "vendor", name: "Lena Ortiz" });
  await go(page, "/");

  await page.getByRole("button", { name: "Log cash" }).click();
  const dialog = page.getByRole("dialog", { name: "Log cash" });
  await dialog.getByRole("tab", { name: "Expense" }).click();
  await dialog.getByLabel("How much?").fill("40");
  await dialog.getByLabel("What was it?").click();
  await dialog.getByLabel("What was it?").fill("Booth");
  await page.getByRole("option", { name: /Booth Supplies/ }).first().click();
  await dialog.getByLabel("Paid to (optional)").click();
  await page.getByRole("option", { name: /Lena Ortiz/ }).first().click();
  await dialog.getByRole("button", { name: "Log it" }).click();

  await expect(page.getByText("$40.00 out · logged")).toBeVisible();
  expect(await balance(request, "Cash Till")).toBe(20_000 - 4_000);
  expect(await balance(request, "Booth Supplies")).toBe(4_000);
  const [entry] = await todaysEntries(request);
  expect(entry).toMatchObject({ memo: "Cash expense", payee_id: lena });
});

test("a deposit moves money between accounts, and can't go from an account to itself", async ({ page, request }) => {
  await business(request);
  await go(page, "/");

  await page.getByRole("button", { name: "Log cash" }).click();
  const dialog = page.getByRole("dialog", { name: "Log cash" });
  await dialog.getByRole("tab", { name: "Deposit or move" }).click();
  await dialog.getByLabel("How much?").fill("150");
  await dialog.getByLabel("From").fill("Cash");
  await page.getByRole("option", { name: /Cash Till/ }).first().click();
  await dialog.getByLabel("To").click();
  // The account it's leaving isn't offered as the place it arrives.
  await expect(page.getByRole("option", { name: /Cash Till/ })).toHaveCount(0);
  await page.getByRole("option", { name: /Business Checking/ }).first().click();
  await dialog.getByLabel("Note (optional)").fill("Bank deposit");
  await dialog.getByRole("button", { name: "Log it" }).click();

  await expect(page.getByText("$150.00 moved · logged")).toBeVisible();
  expect(await balance(request, "Cash Till")).toBe(20_000 - 15_000);
  expect(await balance(request, "Business Checking")).toBe(100_000 + 15_000);
});

test("Log it stays off until the form is complete", async ({ page, request }) => {
  await business(request);
  await go(page, "/");
  await page.getByRole("button", { name: "Log cash" }).click();
  const dialog = page.getByRole("dialog", { name: "Log cash" });
  const submit = dialog.getByRole("button", { name: "Log it" });
  await dialog.getByRole("tab", { name: "Sale or money in" }).click();
  await expect(submit).toBeDisabled();
  await dialog.getByLabel("How much?").fill("12");
  await expect(submit, "an amount alone isn't enough").toBeDisabled();
  await dialog.getByLabel("What was it for?").click();
  await dialog.getByLabel("What was it for?").fill("Market");
  await page.getByRole("option", { name: /Market Sales/ }).first().click();
  await expect(submit).toBeEnabled();
});
