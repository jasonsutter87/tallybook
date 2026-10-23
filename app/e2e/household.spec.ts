import { expect, test, type APIRequestContext } from "@playwright/test";
import { api, go, householdBook, license } from "./helpers";

// The household's standing plans: a bill that comes every month, and a goal
// that money is put toward.

async function household(request: APIRequestContext) {
  license("free");
  await householdBook(request);
  const checking = await api<number>(request, "accounts.create", { name: "Joint Checking", subtype: "bank", opening_balance: 500000, opening_date: "2026-09-30" });
  const savings = await api<number>(request, "accounts.create", { name: "Savings", subtype: "bank", opening_balance: 0, opening_date: "2026-09-30" });
  return { checking, savings };
}

test("a monthly bill is added and shows up with its next due date", async ({ page, request }) => {
  await household(request);
  await go(page, "/due");
  await page.getByRole("button", { name: "Add a bill" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Add a bill" });
  await dialog.getByLabel("Name").fill("Phone plan");
  await dialog.getByLabel("Amount").fill("95");
  // Exact: another field's hint also mentions the category.
  const category = dialog.getByLabel("Category", { exact: true });
  await category.click();
  await category.fill("Phones");
  await page.getByRole("option", { name: /Phones/ }).first().click();
  // Monthly is the default: it asks for the day of the month, not a start date.
  await dialog.getByLabel("Due on day").fill("24");
  await dialog.getByRole("button", { name: "Add bill" }).click();
  await expect(dialog).toBeHidden();

  const bills = await api<{ name: string; amount: number }[]>(request, "recurring.list");
  expect(bills).toEqual([expect.objectContaining({ name: "Phone plan", amount: 9500 })]);
  await expect(page.getByText("Phone plan").first()).toBeVisible();
});

test("a goal is set, and money put toward it moves into its account", async ({ page, request }) => {
  const { checking, savings } = await household(request);
  await go(page, "/goals");
  await page.getByRole("button", { name: /New goal|Set a goal/ }).first().click();
  const goal = page.getByRole("dialog", { name: "New goal" });
  await goal.getByLabel("What are you saving for?").fill("Emergency fund");
  await goal.getByLabel("How much").fill("6000");
  await goal.getByLabel("Kept in").click();
  await goal.getByLabel("Kept in").fill("Savings");
  await page.getByRole("option", { name: /^Savings/ }).first().click();
  await goal.getByRole("button", { name: "Save goal" }).click();
  await expect(goal).toBeHidden();

  await page.getByRole("button", { name: /Put money in/ }).first().click();
  const put = page.getByRole("dialog", { name: /Put money toward Emergency fund/ });
  await put.getByLabel("How much").fill("250");
  await put.getByLabel("From").click();
  await put.getByLabel("From").fill("Joint");
  await page.getByRole("option", { name: /Joint Checking/ }).first().click();
  await put.getByRole("button", { name: "Put it in" }).click();
  await expect(put).toBeHidden();

  const accounts = await api<{ id: number; balance: number }[]>(request, "accounts.list");
  expect(accounts.find((a) => a.id === savings)!.balance).toBe(25000);
  expect(accounts.find((a) => a.id === checking)!.balance).toBe(475000);
});
