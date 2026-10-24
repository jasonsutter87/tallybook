import net from "node:net";
import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// Banking: connecting a bank takes a plan with bank sync. Without one, the
// Personal plan is offered next to free file import; an ended plan asks to
// renew; a plan reaches for Tallybook Connect, which this test build doesn't
// have, and says so. A SimpleFIN setup token must still lead to https on
// SimpleFIN Bridge before a single network connection is made. (Nothing here
// talks to a real bank or service.)

const token = (url: string) => Buffer.from(url).toString("base64");

test("a free copy is offered the Personal plan, and file import instead", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  await go(page, "/banking");

  await page.getByRole("button", { name: "Connect a bank" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Bank sync comes with a plan" });
  await expect(dialog.getByText("Tallybook Personal")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Buy for $20 a year" })).toBeVisible();

  await dialog.getByRole("button", { name: "Import a file instead" }).click();
  await expect(page.getByRole("dialog", { name: /Import/ })).toBeVisible();

  // The backend refuses too, in words.
  await expect(api(request, "bank.plaid_start")).rejects.toThrow(/\$20 a year/);
});

test("an ended plan asks to renew before connecting another bank", async ({ page, request }) => {
  license("personal", undefined, { active: false });
  await householdBook(request);
  await go(page, "/banking");

  await page.getByRole("button", { name: "Connect a bank" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Bank sync comes with a plan" });
  await expect(dialog.getByText("Your plan has ended, so bank sync is paused.", { exact: false })).toBeVisible();
  await expect(dialog.getByRole("button", { name: /Renew my plan/ })).toBeVisible();
  await expect(api(request, "bank.plaid_start")).rejects.toThrow(/ended/);
});

test("a plan goes to the bank sign-in, and says plainly when Connect isn't available", async ({ page, request }) => {
  license("personal");
  await householdBook(request);
  await go(page, "/banking");

  await page.getByRole("button", { name: "Connect a bank" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Connect a bank" });
  await expect(dialog.getByText(/Tallybook never sees your bank password/)).toBeVisible();
  await dialog.getByRole("button", { name: "Continue to my bank" }).click();
  await expect(dialog.getByRole("alert")).toContainText("isn't switched on in this version yet");
  expect(await api<unknown[]>(request, "bank.connections"), "no connection was recorded").toEqual([]);
});

test("a setup token that isn't SimpleFIN over https is refused before any connection is made", async ({ request }) => {
  license("free");
  await householdBook(request);

  let hits = 0;
  const server = net.createServer((socket) => {
    hits += 1;
    socket.destroy();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as net.AddressInfo).port;

  try {
    for (const [why, value] of [
      ["plain http", token(`http://127.0.0.1:${port}/simplefin/claim/abc`)],
      ["https, but not SimpleFIN Bridge", token(`https://127.0.0.1:${port}/simplefin/claim/abc`)],
      ["not a token at all", "this is not a setup token!"],
    ] as const) {
      await expect(api(request, "bank.simplefin_connect", { setup_token: value }), why).rejects.toThrow(/setup token/i);
    }
    await new Promise((r) => setTimeout(r, 300));
    expect(hits, "nothing connected to the fake bridge").toBe(0);
    expect(await api<unknown[]>(request, "bank.connections"), "no connection was recorded").toEqual([]);
  } finally {
    server.close();
  }
});
