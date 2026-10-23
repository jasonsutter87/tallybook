import { expect, test, type Page } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// Bringing in a bank statement is how most people start. A CSV and an OFX
// file each land in Review, and importing the same file again adds nothing.

const CSV = `Date,Description,Amount
2026-10-01,TRADER JOES #552,-84.20
2026-10-03,ACME PAYROLL,2500.00
2026-10-05,NETFLIX.COM,-15.49
`;

const OFX = `OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<BANKMSGSRSV1><STMTTRNRS><STMTRS>
<CURDEF>USD
<BANKACCTFROM><BANKID>121000248<ACCTID>000123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261015120000[-8:PST]<TRNAMT>-42.17<FITID>2026101501<NAME>PG&amp;E<MEMO>AUTOPAY</STMTTRN>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20261016<TRNAMT>1500.00<FITID>2026101602<NAME>ACME PAYROLL</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>3210.55<DTASOF>20261031</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>`;

/** Opens the import dialog from Review, loads `file` and imports it into Joint Checking. */
async function importFile(page: Page, name: string, mimeType: string, body: string) {
  await go(page, "/review");
  await page.getByRole("button", { name: "Import file" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.locator('input[type="file"]').setInputFiles({ name, mimeType, buffer: Buffer.from(body) });
  await dialog.getByPlaceholder("Choose a bank or card account").click();
  await page.getByRole("option", { name: /Joint Checking/ }).click();
  await dialog.getByRole("button", { name: /^Import/ }).click();
  await expect(page.getByRole("dialog", { name: "Import complete" })).toBeVisible();
  await page.getByRole("button", { name: "Done" }).click();
}

test.beforeEach(async ({ request }) => {
  license("free");
  await householdBook(request);
  await api(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 100000, opening_date: "2026-09-30" });
});

test("a CSV statement lands in Review, and importing it twice adds nothing", async ({ page, request }) => {
  await importFile(page, "statement.csv", "text/csv", CSV);
  expect(await api<number>(request, "inbox.count")).toBe(3);
  await expect(page.getByText("TRADER JOES #552")).toBeVisible();

  await importFile(page, "statement.csv", "text/csv", CSV);
  expect(await api<number>(request, "inbox.count"), "duplicates are recognized and skipped").toBe(3);
});

test("an OFX statement lands in Review, and importing it twice adds nothing", async ({ page, request }) => {
  await importFile(page, "statement.ofx", "application/x-ofx", OFX);
  expect(await api<number>(request, "inbox.count")).toBe(2);
  await expect(page.getByText("ACME PAYROLL")).toBeVisible();

  await importFile(page, "statement.ofx", "application/x-ofx", OFX);
  expect(await api<number>(request, "inbox.count"), "OFX ids make re-importing safe").toBe(2);
});
