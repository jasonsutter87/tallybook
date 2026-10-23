import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// A rule made on the Rules screen files matching bank transactions for you:
// the next statement arrives with the rule's category already suggested.

const OFX = `OFXHEADER:100
DATA:OFXSGML
VERSION:102

<OFX>
<BANKMSGSRSV1><STMTTRNRS><STMTRS>
<CURDEF>USD
<BANKACCTFROM><BANKID>121000248<ACCTID>000123456789<ACCTTYPE>CHECKING</BANKACCTFROM>
<BANKTRANLIST>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261017<TRNAMT>-9.99<FITID>S1<NAME>SPOTIFY USA</STMTTRN>
<STMTTRN><TRNTYPE>DEBIT<DTPOSTED>20261018<TRNAMT>-54.10<FITID>S2<NAME>SHELL OIL 5741</STMTTRN>
</BANKTRANLIST>
<LEDGERBAL><BALAMT>3210.55<DTASOF>20261031</LEDGERBAL>
</STMTRS></STMTTRNRS></BANKMSGSRSV1>
</OFX>`;

type InboxItem = { description: string; suggestion: { kind: string; account_id?: number; rename_to?: string | null } | null };

test("a rule made on the Rules screen suggests the category on the next import", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 100000, opening_date: "2026-09-30" });
  const accounts = await api<{ id: number; name: string }[]>(request, "accounts.list");
  const subscriptions = accounts.find((a) => a.name === "Subscriptions & Streaming")!.id;

  await go(page, "/rules");
  await page.getByRole("button", { name: "New rule" }).first().click();
  const dialog = page.getByRole("dialog", { name: "New rule" });
  await dialog.getByLabel("this text").fill("spotify");
  await dialog.getByLabel("Then categorize as").click();
  await dialog.getByLabel("Then categorize as").fill("Subscriptions");
  await page.getByRole("option", { name: /Subscriptions & Streaming/ }).first().click();
  await dialog.getByLabel("Rename to").fill("Spotify");
  await dialog.getByRole("button", { name: "Create rule" }).click();
  await expect(dialog).toBeHidden();
  expect((await api<unknown[]>(request, "rules.list")).length).toBe(1);

  await api(request, "import.ofx", { content: OFX, index: 0, account_id: checking });
  const inbox = await api<InboxItem[]>(request, "inbox.list");
  const spotify = inbox.find((i) => /SPOTIFY/.test(i.description))!;
  expect(spotify.suggestion).toMatchObject({ kind: "rule", account_id: subscriptions, rename_to: "Spotify" });
  const shell = inbox.find((i) => /SHELL/.test(i.description))!;
  expect(shell.suggestion?.kind, "the rule only matches what it says").not.toBe("rule");
});
