import { expect, test, type APIRequestContext } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { api, go, householdBook, license } from "./helpers";
import { DATA_DIR } from "./paths";

// Keeping the books honest: fixing a mistake by voiding it (history stays),
// closing a period so nothing in it can change, and backing up.

async function bookWithSpending(request: APIRequestContext) {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 100000, opening_date: "2026-09-30" });
  const accounts = await api<{ id: number; name: string }[]>(request, "accounts.list");
  const groceries = accounts.find((a) => a.name === "Groceries")!.id;
  const entry = await api<number>(request, "entries.post", {
    date: "2026-10-02",
    memo: "Trader Joe's",
    lines: [
      { account_id: groceries, amount: 8420 },
      { account_id: checking, amount: -8420 },
    ],
  });
  return { checking, groceries, entry };
}

test("an entry opens from the register by keyboard, and voiding it keeps history", async ({ page, request }) => {
  const { checking } = await bookWithSpending(request);
  await go(page, `/accounts/${checking}`);

  const entry = page.getByRole("button", { name: "Trader Joe's" });
  await entry.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Trader Joe's" });
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "Void this transaction" }).click();
  await page.getByRole("dialog", { name: "Void this transaction?" }).getByRole("button", { name: "Void transaction" }).click();
  await expect(page.getByText("Transaction voided")).toBeVisible();

  const accounts = await api<{ id: number; balance: number }[]>(request, "accounts.list");
  expect(accounts.find((a) => a.id === checking)!.balance, "the money is back").toBe(100000);
  const entries = await api<unknown[]>(request, "entries.list", { from: "2026-01-01", to: "2026-12-31" });
  expect(entries.length, "opening balance, the entry, and its reversal: nothing deleted").toBe(3);
});

test("closing the books stops changes on or before the lock date", async ({ page, request }) => {
  const { checking, groceries } = await bookWithSpending(request);
  await go(page, "/settings");
  const card = page.locator("#lock");
  await card.getByLabel("Lock date").fill("2026-10-31");
  await card.getByRole("button", { name: "Lock" }).click();
  await page.getByRole("dialog", { name: /Close the books through/ }).getByRole("button", { name: "Close the books" }).click();
  await expect(card.getByText(/Closed through/)).toBeVisible();

  const settings = await api<{ lock_date: string | null }>(request, "settings.get");
  expect(settings.lock_date).toBe("2026-10-31");
  const inClosedPeriod = request.post("http://127.0.0.1:1441/api/entries.post", {
    data: { date: "2026-10-15", memo: "Too late", lines: [{ account_id: groceries, amount: 100 }, { account_id: checking, amount: -100 }] },
  });
  expect((await inClosedPeriod).ok(), "nothing can be posted into a closed period").toBe(false);
});

test("Back up now writes a full copy beside the book", async ({ page, request }) => {
  await bookWithSpending(request);
  const backups = path.join(DATA_DIR, "Backups");
  const before = fs.existsSync(backups) ? fs.readdirSync(backups).length : 0;

  await go(page, "/settings");
  await page.getByRole("button", { name: "Back up now" }).click();
  await expect(page.getByText(/Backed up to/)).toBeVisible();

  const after = fs.readdirSync(backups).filter((f) => f.endsWith(".tallybook"));
  expect(after.length).toBe(before + 1);
});
