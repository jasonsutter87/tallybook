import { expect, test, type APIRequestContext } from "@playwright/test";
import { api, demoBook, go, householdBook, license } from "./helpers";

// Once the books are closed, nothing dated on or before the lock date changes:
// not an entry's payee (person, goal), and not a reviewed bank match. The UI
// says so in words, and the books stay exactly as they were.

const OFX = `OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<BANKMSGSRSV1><STMTTRNRS><STMTRS>
<CURDEF>USD
<BANKACCTFROM><BANKID>121000248<ACCTID>000123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20260102<TRNAMT>-84.20<FITID>C1<NAME>TRADER JOES</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>915.80<DTASOF>20260131</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>`;

type Entry = { payee_id: number | null; person_id: number | null; goal_id?: number | null };

async function businessEntry(request: APIRequestContext) {
  license("tax");
  await demoBook(request);
  const accounts = await api<{ id: number; name: string; subtype: string; archived: boolean }[]>(request, "accounts.list");
  const bank = accounts.find((a) => a.subtype === "bank" && !a.archived)!.id;
  const expense = accounts.find((a) => a.subtype === "expense" && !a.archived)!.id;
  await api<number>(request, "contacts.create", { kind: "vendor", name: "Lena Ortiz Design" });
  const entry = await api<number>(request, "entries.post", {
    date: "2020-03-10",
    memo: "Logo contractor",
    lines: [
      { account_id: expense, amount: 50000 },
      { account_id: bank, amount: -50000 },
    ],
  });
  return { entry };
}

test("a closed period's entry can't get a payee, and the screen says why", async ({ page, request }) => {
  const { entry } = await businessEntry(request);
  await api(request, "settings.set", { lock_date: "2020-12-31" });

  // The register is where an entry opens; use the account it was paid from.
  const fromLine = (await api<{ lines: { account_id: number; amount: number }[] }>(request, "entries.get", { id: entry })).lines.find((l) => l.amount < 0)!;
  await go(page, `/accounts/${fromLine.account_id}`);

  await page.getByRole("button", { name: "Logo contractor" }).click();
  const dialog = page.getByRole("dialog", { name: "Logo contractor" });
  await dialog.getByRole("button", { name: "Add payee" }).click();
  const choose = dialog.getByPlaceholder("Choose a vendor");
  await choose.click();
  await page.getByRole("option", { name: /Lena Ortiz Design/ }).click();

  const toast = page.locator(".toast.error");
  await expect(toast).toBeVisible();
  await expect(toast).toContainText(/lock date/);
  expect((await api<Entry>(request, "entries.get", { id: entry })).payee_id, "payee unchanged").toBeNull();
});

test("person and goal on a closed-period entry are refused too", async ({ request }) => {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 500000, opening_date: "2019-12-31" });
  const savings = await api<number>(request, "accounts.create", { name: "Savings", subtype: "bank", opening_balance: 0, opening_date: "2019-12-31" });
  const person = await api<number>(request, "people.create", { name: "Sam" });
  const goal = await api<number>(request, "goals.create", { name: "Rainy day", target: 100000, account_id: savings });
  const { entry_id } = await api<{ entry_id: number }>(request, "goals.contribute", { id: goal, from: checking, amount: 10000, date: "2020-02-01" });
  await api(request, "settings.set", { lock_date: "2020-12-31" });

  await expect(api(request, "entries.set_person", { id: entry_id, person_id: person })).rejects.toThrow(/rule/);
  await expect(api(request, "entries.set_goal", { id: entry_id, goal_id: null })).rejects.toThrow(/rule/);
  const e = await api<Entry>(request, "entries.get", { id: entry_id });
  expect(e.person_id).toBeNull();
});

test("a reviewed bank match in a closed period can't be undone", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 100000, opening_date: "2025-12-31" });
  const accounts = await api<{ id: number; name: string }[]>(request, "accounts.list");
  const groceries = accounts.find((a) => a.name === "Groceries")!.id;
  await api(request, "entries.post", {
    date: "2026-01-02",
    memo: "Trader Joe's",
    lines: [
      { account_id: groceries, amount: 8420 },
      { account_id: checking, amount: -8420 },
    ],
  });
  await api(request, "import.ofx", { content: OFX, index: 0, account_id: checking });
  const [txn] = await api<{ id: number }[]>(request, "inbox.list");
  const [candidate] = await api<{ kind: string; line_id: number }[]>(request, "inbox.candidates", { id: txn.id });
  await api(request, "inbox.match", { id: txn.id, line_id: candidate.line_id });
  expect(await api<number>(request, "inbox.count")).toBe(0);
  await api(request, "settings.set", { lock_date: "2026-01-31" });

  await go(page, "/review");
  await page.getByRole("tab", { name: "Reviewed" }).click();
  await expect(page.getByText("TRADER JOES").first()).toBeVisible();
  await page.getByRole("button", { name: "Undo" }).click();

  const toast = page.locator(".toast.error");
  await expect(toast).toBeVisible();
  await expect(toast).toContainText(/lock date/);
  expect(await api<number>(request, "inbox.count"), "still matched, not back in Review").toBe(0);
  const reviewed = await api<{ status: string }[]>(request, "inbox.reviewed");
  expect(reviewed.map((r) => r.status)).toEqual(["matched"]);
});
