import { expect, test, type APIRequestContext } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// Review is where bank transactions become bookkeeping: categorize (by
// keyboard as well as mouse), remember the choice as a rule, split one
// payment across categories, and take a decision back.

const OFX = `OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<BANKMSGSRSV1><STMTTRNRS><STMTRS>
<CURDEF>USD
<BANKACCTFROM><BANKID>121000248<ACCTID>000123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261015<TRNAMT>-42.17<FITID>R1<NAME>PG&amp;E<MEMO>AUTOPAY</STMTTRN>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20261016<TRNAMT>1500.00<FITID>R2<NAME>ACME PAYROLL</STMTTRN>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261017<TRNAMT>-9.99<FITID>R3<NAME>SPOTIFY</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>3210.55<DTASOF>20261031</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>`;

async function bookWithInbox(request: APIRequestContext) {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", {
    name: "Joint Checking",
    subtype: "bank",
    opening_balance: 100000,
    opening_date: "2026-09-30",
  });
  await api(request, "import.ofx", { content: OFX, index: 0, account_id: checking });
  expect(await api<number>(request, "inbox.count")).toBe(3);
}

test("a transaction opens from the keyboard, and categorizing it can remember a rule", async ({ page, request }) => {
  await bookWithInbox(request);
  await go(page, "/review");

  const row = page.getByRole("button", { name: /PG&E/ });
  await row.focus();
  await page.keyboard.press("Enter");
  await expect(row).toHaveAttribute("aria-expanded", "true");

  const where = page.getByLabel("Where did this go?");
  await where.click();
  await where.fill("Gas & Electric");
  await page.getByRole("option", { name: /Gas & Electric/ }).first().click();
  await page.getByRole("checkbox", { name: /Remember for/ }).check();
  await page.getByRole("button", { name: "Accept" }).click();

  await expect.poll(() => api<number>(request, "inbox.count")).toBe(2);
  const rules = await api<unknown[]>(request, "rules.list");
  expect(rules.length, "Remember made a rule").toBe(1);
});

test("one payment splits across two categories", async ({ page, request }) => {
  await bookWithInbox(request);
  await go(page, "/review");

  await page.getByRole("button", { name: /ACME PAYROLL/ }).click();
  await page.getByRole("tab", { name: /Split/ }).click();
  const category = page.getByLabel("Split category");
  const amount = page.getByLabel("Split amount");

  await category.nth(0).click();
  await category.nth(0).fill("Paychecks");
  await page.getByRole("option", { name: /Paychecks/ }).first().click();
  await amount.nth(0).fill("1000");

  if ((await category.count()) < 2) await page.getByRole("button", { name: /Add a line|Add line|Add a split/ }).click();
  await category.nth(1).click();
  await category.nth(1).fill("Side Income");
  await page.getByRole("option", { name: /Side Income/ }).first().click();
  await amount.nth(1).fill("500");

  await page.getByRole("button", { name: "Save split" }).click();
  await expect.poll(() => api<number>(request, "inbox.count")).toBe(2);

  const entries = await api<{ lines: { amount: number }[] }[]>(request, "entries.list", { from: "2026-10-16", to: "2026-10-16" });
  const credits = entries.flatMap((e) => e.lines.map((l) => l.amount)).filter((a) => a < 0).sort((a, b) => a - b);
  expect(credits, "income split $1,000 + $500").toEqual([-100000, -50000]);
});

test("a decision can be taken back from the toast", async ({ page, request }) => {
  await bookWithInbox(request);
  await go(page, "/review");

  await page.getByRole("button", { name: /SPOTIFY/ }).click();
  const where = page.getByLabel("Where did this go?");
  await where.click();
  await where.fill("Subscriptions");
  await page.getByRole("option", { name: /Subscriptions/ }).first().click();
  await page.getByRole("button", { name: "Accept" }).click();
  await expect.poll(() => api<number>(request, "inbox.count")).toBe(2);

  await page.getByRole("button", { name: "Undo" }).click();
  await expect.poll(() => api<number>(request, "inbox.count"), "the transaction is back in Review").toBe(3);
});
