import { expect, test } from "@playwright/test";
import { api, demoBook, go, license } from "./helpers";

// Money owed both ways: a vendor's bill entered and paid, and an invoice
// drafted, changed, then issued.

test.beforeEach(async ({ request }) => {
  license("business");
  await demoBook(request);
});

test("a bill is entered from the screen and paid in full", async ({ page, request }) => {
  await go(page, "/bills/new");
  await page.getByPlaceholder("Choose a vendor").click();
  await page.getByRole("option").first().click();
  await page.getByLabel("Description").first().fill("October hosting");
  await page.getByLabel("Category").first().click();
  await page.getByLabel("Category").first().fill("Software");
  await page.getByRole("option", { name: /Software/ }).first().click();
  await page.getByLabel("Amount").first().fill("312.50");
  await page.getByRole("button", { name: "Save bill" }).click();

  await expect(page).toHaveURL(/#\/bills\/\d+$/);
  const id = Number(page.url().split("/").pop());
  const saved = await api<{ summary: { total: number; status: string } }>(request, "bills.get", { id });
  expect(saved.summary.total).toBe(31250);

  await page.getByRole("button", { name: "Pay bill" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Pay bills" });
  await dialog.getByRole("button", { name: "Pay $312.50" }).click();
  await expect(dialog).toBeHidden();

  await expect.poll(async () => (await api<{ summary: { status: string } }>(request, "bills.get", { id })).summary.status).toBe("paid");
});

test("an invoice draft is edited before it's issued", async ({ page, request }) => {
  await go(page, "/invoices/new");
  await page.getByPlaceholder("Choose a customer").click();
  await page.getByRole("option").first().click();
  await page.getByLabel("Description").first().fill("Logo concepts");
  await page.getByLabel("Quantity").first().fill("1");
  await page.getByLabel("Price").first().fill("800");
  await page.getByRole("button", { name: /Save draft/ }).first().click();

  await expect(page).toHaveURL(/#\/invoices\/\d+$/);
  const id = Number(page.url().split("/").pop());
  // invoices.get flattens the summary: status and total sit at the top level.
  const draft = await api<{ status: string; total: number }>(request, "invoices.get", { id });
  expect(draft).toMatchObject({ status: "draft", total: 80000 });

  await go(page, `/invoices/${id}/edit`);
  await page.getByLabel("Quantity").first().fill("3");
  await expect(page.getByText("$2,400.00").first()).toBeVisible();
  await page.getByRole("button", { name: /Save & issue/ }).first().click();

  await expect(page).toHaveURL(new RegExp(`#/invoices/${id}$`));
  const issued = await api<{ status: string; total: number }>(request, "invoices.get", { id });
  expect(issued.total).toBe(240000);
  expect(issued.status).not.toBe("draft");
});
