import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// The chart of accounts: add an account (with an opening balance), rename it,
// and archive it, but not while it still holds money.

type Account = { id: number; name: string; subtype: string; balance: number; archived: boolean };
const find = async (request: Parameters<typeof api>[0], name: string) => (await api<Account[]>(request, "accounts.list")).find((a) => a.name === name);

test("a bank account is added with an opening balance and opens its register", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  await go(page, "/accounts");

  await page.getByRole("button", { name: "Add account" }).click();
  const dialog = page.getByRole("dialog", { name: "Add an account" });
  await dialog.getByLabel("Name", { exact: true }).fill("Golden Savings");
  await dialog.getByLabel("What kind of account is it?").selectOption("bank");
  await dialog.getByLabel("Opening balance (optional)").fill("1250.50");
  await dialog.getByRole("button", { name: "Add account" }).click();

  await expect(page.getByText("Golden Savings added")).toBeVisible();
  await expect(page).toHaveURL(/#\/accounts\/\d+$/);
  await expect(page.getByRole("heading", { name: "Golden Savings" })).toBeVisible();

  const account = (await find(request, "Golden Savings"))!;
  expect(account).toMatchObject({ subtype: "bank", balance: 125_050, archived: false });
  await expect(page.locator(".hero-amount")).toContainText("1,250");
});

test("a duplicate account code is refused in words and nothing is added", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  await api(request, "accounts.create", { name: "First", subtype: "bank", code: "1020" });
  const before = (await api<Account[]>(request, "accounts.list")).length;
  await go(page, "/accounts");

  await page.getByRole("button", { name: "Add account" }).click();
  const dialog = page.getByRole("dialog", { name: "Add an account" });
  await dialog.getByLabel("Name", { exact: true }).fill("Second");
  await dialog.getByLabel("Account number (optional)").fill("1020");
  await dialog.getByRole("button", { name: "Add account" }).click();

  await expect(dialog.getByRole("alert")).toBeVisible();
  await expect(dialog).toBeVisible();
  expect(await api<Account[]>(request, "accounts.list")).toHaveLength(before);
});

test("an account is renamed from its register, and its history stays", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const id = await api<number>(request, "accounts.create", { name: "Joint Chekcing", subtype: "bank", opening_balance: 50_000, opening_date: "2026-01-01" });
  await go(page, `/accounts/${id}`);

  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  const dialog = page.getByRole("dialog", { name: "Rename account" });
  await dialog.getByLabel("Name").fill("Joint Checking");
  await dialog.getByRole("button", { name: "Save" }).click();

  await expect(page.getByText("Renamed")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Joint Checking" })).toBeVisible();
  expect(await find(request, "Joint Chekcing")).toBeUndefined();
  expect(await find(request, "Joint Checking")).toMatchObject({ id, balance: 50_000 });
});

test("an empty account is archived, hidden, shown on request, and brought back with Unarchive", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const id = await api<number>(request, "accounts.create", { name: "Old Savings", subtype: "bank" });
  await go(page, `/accounts/${id}`);

  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Archive" }).click();
  await expect(page.getByText("Old Savings archived")).toBeVisible();
  expect((await find(request, "Old Savings"))!.archived).toBe(true);

  await go(page, "/accounts");
  await expect(page.getByRole("link", { name: /Old Savings/ })).toHaveCount(0);
  await page.getByLabel(/Show archived/).check();
  await expect(page.getByRole("link", { name: /Old Savings/ })).toBeVisible();

  await page.getByRole("link", { name: /Old Savings/ }).click();
  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Unarchive" }).click();
  await expect(page.getByText("Old Savings is active again")).toBeVisible();
  expect((await find(request, "Old Savings"))!.archived).toBe(false);
});

test("an account that still holds money can't be archived", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const id = await api<number>(request, "accounts.create", { name: "Rainy Day", subtype: "bank", opening_balance: 10_000, opening_date: "2026-01-01" });
  await go(page, `/accounts/${id}`);

  await page.getByRole("button", { name: "More actions" }).click();
  await page.getByRole("menuitem", { name: "Archive" }).click();
  await expect(page.locator(".toast.error")).toBeVisible();
  expect((await find(request, "Rainy Day"))!.archived).toBe(false);
  await expect(page.getByText("Archived", { exact: true })).toHaveCount(0);
});
