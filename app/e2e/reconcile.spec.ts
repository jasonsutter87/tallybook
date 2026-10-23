import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// Reconciling: match the books to a bank statement line by line, and only
// finish once the difference is exactly zero.

test("an account reconciles to its statement, and Finish waits for a zero difference", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", {
    name: "Joint Checking",
    subtype: "bank",
    opening_balance: 100000,
    opening_date: "2026-09-30",
  });
  const accounts = await api<{ id: number; name: string }[]>(request, "accounts.list");
  const id = (name: string) => accounts.find((a) => a.name === name)!.id;
  const post = (date: string, memo: string, account: string, cents: number) =>
    api(request, "entries.post", {
      date,
      memo,
      lines: [
        { account_id: id(account), amount: cents },
        { account_id: checking, amount: -cents },
      ],
    });
  await post("2026-10-02", "Trader Joe's", "Groceries", 8420);
  await post("2026-10-05", "Netflix", "Subscriptions & Streaming", 1549);
  await post("2026-10-15", "Payroll", "Paychecks", -250000);

  // $1,000.00 opening − $84.20 − $15.49 + $2,500.00
  await go(page, `/accounts/${checking}/reconcile`);
  await page.getByLabel("Statement ending date").fill("2026-10-31");
  await page.getByLabel("Statement ending balance").fill("3400.31");
  await page.getByRole("button", { name: "Start reconciling" }).click();

  const finish = page.getByRole("button", { name: "Finish" });
  await expect(finish).toBeDisabled();
  for (const button of await page.getByRole("button", { name: "Check all" }).all()) await button.click();
  await expect(page.getByText(/balanced!/)).toBeVisible();
  await expect(finish).toBeEnabled();
  await finish.click();

  await expect.poll(async () => (await api<unknown[]>(request, "reconcile.history", { account_id: checking })).length).toBe(1);
  expect(await api(request, "reconcile.active", { account_id: checking }), "nothing left in progress").toBeNull();
});
