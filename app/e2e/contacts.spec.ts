import { expect, test } from "@playwright/test";
import { api, bookPath, go, license } from "./helpers";

// Contacts: add and edit customers and vendors, keep email addresses plain
// (one address, no odd characters), and turn a good one into a mailto link.

type Contact = { id: number; name: string; email: string | null; kind: string; archived: boolean };

async function contacts(request: Parameters<typeof api>[0]) {
  return api<Contact[]>(request, "contacts.list");
}

test("a customer is added, then edited, and the panel shows a plain mailto link", async ({ page, request }) => {
  license("tax");
  await api(request, "book.create", { path: bookPath("contacts"), company_name: "Contact Test Co" });
  await go(page, "/contacts");

  await page.getByRole("button", { name: "Add contact" }).first().click();
  const add = page.getByRole("dialog", { name: "Add a contact" });
  await add.getByLabel("Name", { exact: true }).fill("Bluebird Coffee");
  await add.getByLabel("Company").fill("Bluebird LLC");
  await add.getByLabel("Email").fill("billing@bluebird.example");
  await add.getByLabel("Phone").fill("555-0142");
  await add.getByRole("button", { name: "Add contact" }).click();
  await expect(add).toBeHidden();

  let [bluebird] = await contacts(request);
  expect(bluebird).toMatchObject({ name: "Bluebird Coffee", kind: "customer", email: "billing@bluebird.example" });

  // A new contact's panel opens by itself.
  const panel = page.getByRole("dialog", { name: "Bluebird Coffee" });
  const link = panel.getByRole("link", { name: "billing@bluebird.example" });
  // A plain mailto: address and nothing else (the @ may be percent-encoded).
  const href = (await link.getAttribute("href"))!;
  expect(decodeURIComponent(href)).toBe("mailto:billing@bluebird.example");
  expect(href).not.toMatch(/[?&;,\s<>"]/);

  await panel.getByRole("button", { name: "Edit" }).click();
  const edit = page.getByRole("dialog", { name: "Edit Bluebird Coffee" });
  await edit.getByLabel("Name", { exact: true }).fill("Bluebird Roasters");
  await edit.getByLabel("Email").fill("ap@bluebird.example");
  await edit.getByRole("button", { name: "Save changes" }).click();
  await expect(edit).toBeHidden();

  [bluebird] = await contacts(request);
  expect(bluebird).toMatchObject({ name: "Bluebird Roasters", email: "ap@bluebird.example" });
  await expect(page.getByRole("button", { name: /Bluebird Roasters/ })).toBeVisible();
});

test("an email with several addresses or odd characters is refused, and nothing is saved", async ({ page, request }) => {
  license("tax");
  await api(request, "book.create", { path: bookPath("contacts"), company_name: "Contact Test Co" });
  await go(page, "/contacts");

  await page.getByRole("button", { name: "Add contact" }).first().click();
  const add = page.getByRole("dialog", { name: "Add a contact" });
  await add.getByLabel("Name", { exact: true }).fill("Mallory");

  for (const bad of ["a@example.com, b@example.com", "a@example.com;b@example.com", "a@example.com?bcc=x@evil.example", "<script>@example.com", "not an address"]) {
    await add.getByLabel("Email").fill(bad);
    await add.getByRole("button", { name: "Add contact" }).click();
    await expect(add.getByRole("alert"), bad).toContainText(/email/i);
    expect(await contacts(request), `${bad} was not saved`).toHaveLength(0);
    await expect(add).toBeVisible();
  }

  // The same address, fixed, goes through.
  await add.getByLabel("Email").fill("mallory@example.com");
  await add.getByRole("button", { name: "Add contact" }).click();
  await expect(add).toBeHidden();
  expect((await contacts(request)).map((c) => c.email)).toEqual(["mallory@example.com"]);
});

test("a vendor's contractor details are saved with the contact", async ({ page, request }) => {
  license("tax");
  await api(request, "book.create", { path: bookPath("contacts"), company_name: "Contact Test Co" });
  await go(page, "/contacts");

  await page.getByRole("button", { name: "Add contact" }).first().click();
  const add = page.getByRole("dialog", { name: "Add a contact" });
  await add.getByRole("button", { name: /Vendor/ }).click();
  await add.getByLabel("Name", { exact: true }).fill("Lena Ortiz");
  await add.getByLabel("1099 contractor").check();
  await add.getByLabel(/W-9 on file/).check();
  await add.getByLabel(/Legal name/).fill("Lena M. Ortiz");
  await add.getByLabel(/Tax ID/).fill("6789");
  await add.getByRole("button", { name: "Add contact" }).click();
  await expect(add).toBeHidden();

  const [lena] = await api<{ name: string; is_1099: boolean; w9_on_file: boolean; legal_name: string; tin_last4: string }[]>(request, "contacts.list");
  expect(lena).toMatchObject({ name: "Lena Ortiz", is_1099: true, w9_on_file: true, legal_name: "Lena M. Ortiz", tin_last4: "6789" });
});

test("archiving a contact hides it until 'Show archived' is on", async ({ page, request }) => {
  license("tax");
  await api(request, "book.create", { path: bookPath("contacts"), company_name: "Contact Test Co" });
  await api(request, "contacts.create", { kind: "customer", name: "Old Client" });
  await api(request, "contacts.create", { kind: "customer", name: "Current Client" });
  await go(page, "/contacts");

  await page.getByRole("button", { name: /Old Client/ }).click();
  await page.getByRole("dialog", { name: "Old Client" }).getByRole("button", { name: "Archive" }).click();
  await page.getByRole("dialog", { name: "Old Client" }).getByRole("button", { name: "Close" }).click();

  await expect(page.getByRole("button", { name: /Old Client/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /Current Client/ })).toBeVisible();
  expect((await contacts(request)).find((c) => c.name === "Old Client")!.archived).toBe(true);

  await page.getByLabel(/Show archived/).check();
  await expect(page.getByRole("button", { name: /Old Client/ })).toBeVisible();
});
