import net from "node:net";
import { expect, test } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// Banking: a SimpleFIN setup token must lead to https on SimpleFIN Bridge,
// and anything else is refused with a clear message before a single network
// connection is made. (Nothing here talks to a real bank or service; a
// listener on this machine stands in as "the network" and must stay unvisited.)

const token = (url: string) => Buffer.from(url).toString("base64");

test("a setup token that isn't SimpleFIN over https is refused before any connection is made", async ({ page, request }) => {
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
    await go(page, "/banking");
    await page.getByRole("button", { name: "Connect a bank" }).first().click();
    const dialog = page.getByRole("dialog", { name: "Connect with SimpleFIN" });
    const field = dialog.getByLabel("Setup token");
    const connect = dialog.getByRole("button", { name: "Connect" });

    for (const [why, value] of [
      ["plain http", token(`http://127.0.0.1:${port}/simplefin/claim/abc`)],
      ["https, but not SimpleFIN Bridge", token(`https://127.0.0.1:${port}/simplefin/claim/abc`)],
      ["not a token at all", "this is not a setup token!"],
    ] as const) {
      await field.fill(value);
      await connect.click();
      await expect(dialog.getByRole("alert"), why).toContainText(/setup token/i);
      await expect(dialog, "the dialog stays open").toBeVisible();
    }
    await page.waitForTimeout(300);
    expect(hits, "nothing connected to the fake bridge").toBe(0);

    // Same answer straight from the backend, with its own error code.
    await expect(api(request, "bank.simplefin_connect", { setup_token: token(`http://127.0.0.1:${port}/x`) })).rejects.toThrow(/bank/);
    expect(hits).toBe(0);
    expect(await api<unknown[]>(request, "bank.connections"), "no connection was recorded").toEqual([]);
  } finally {
    server.close();
  }
});
