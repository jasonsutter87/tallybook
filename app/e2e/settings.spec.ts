import { expect, test } from "@playwright/test";
import { api, demoBook, go, householdBook, license } from "./helpers";

// Settings: the business profile and invoice defaults save (through the
// allowlisted `settings.set`) and survive a reload, and the theme choice sticks.

type Settings = { prefs: Record<string, string> };

test("the business profile saves and comes back after a reload", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  await go(page, "/settings");

  const card = page.locator("#business");
  await card.getByLabel("Business name").fill("Harbor Lane Studio");
  await card.getByLabel("Email").fill("hello@harborlane.example");
  await card.getByLabel("Phone").fill("(555) 010-0199");
  await card.getByLabel("Address").fill("9 Harbor Lane\nPortland, ME 04101");
  await card.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Business profile saved")).toBeVisible();
  await expect(card.getByText("All changes saved")).toBeVisible();

  const { prefs } = await api<Settings>(request, "settings.get");
  expect(prefs["company.name"]).toBe("Harbor Lane Studio");
  expect(prefs["company.email"]).toBe("hello@harborlane.example");
  expect(prefs["company.phone"]).toBe("(555) 010-0199");
  expect(prefs["company.address"]).toBe("9 Harbor Lane\nPortland, ME 04101");

  await go(page, "/settings");
  await expect(card.getByLabel("Business name")).toHaveValue("Harbor Lane Studio");
  await expect(card.getByLabel("Address")).toHaveValue("9 Harbor Lane\nPortland, ME 04101");
});

test("a business name can't be saved empty", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  const before = (await api<Settings>(request, "settings.get")).prefs["company.name"];
  await go(page, "/settings");

  const card = page.locator("#business");
  await card.getByLabel("Business name").fill("");
  await expect(card.getByText("Your business needs a name.")).toBeVisible();
  await card.getByRole("button", { name: "Save changes" }).click();
  expect((await api<Settings>(request, "settings.get")).prefs["company.name"], "nothing was written").toBe(before);
});

test("invoice defaults save and come back after a reload", async ({ page, request }) => {
  license("tax");
  await demoBook(request);
  await go(page, "/settings");

  const card = page.locator("#invoices");
  await card.getByLabel("Next invoice number").fill("2500");
  await card.getByLabel("Payment terms").selectOption("15");
  await card.getByLabel("Default sales tax").fill("8.25");
  await card.getByLabel("Footer message").fill("Thanks, pay by check or ACH.");
  await card.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Invoice settings saved")).toBeVisible();

  const { prefs } = await api<Settings>(request, "settings.get");
  expect(prefs["invoice.next_number"]).toBe("2500");
  expect(prefs["invoice.default_terms_days"]).toBe("15");
  expect(prefs["invoice.default_tax_bp"]).toBe("825");
  expect(prefs["invoice.footer"]).toBe("Thanks, pay by check or ACH.");

  await go(page, "/settings");
  await expect(card.getByLabel("Next invoice number")).toHaveValue("2500");
  await expect(card.getByLabel("Payment terms")).toHaveValue("15");
  await expect(card.getByLabel("Default sales tax")).toHaveValue("8.25");
  await expect(card.getByLabel("Footer message")).toHaveValue("Thanks, pay by check or ACH.");
});

test("a setting outside the allowlist is refused and writes nothing", async ({ request }) => {
  license("tax");
  await demoBook(request);
  await expect(api(request, "settings.set", { prefs: { "company.name": "Sneaky", "cloud.folder": "/tmp/x" } })).rejects.toThrow(/bad_params/);
  const { prefs } = await api<Settings>(request, "settings.get");
  expect(prefs["company.name"]).not.toBe("Sneaky");
  expect(prefs["cloud.folder"]).toBeUndefined();
});

test("the theme choice applies at once and is still there after a reload", async ({ page, request }) => {
  license("free");
  await householdBook(request);
  await go(page, "/settings");
  const theme = page.locator("#appearance").getByRole("radiogroup", { name: "Theme" });
  const html = page.locator("html");

  await theme.getByRole("radio", { name: "Dark" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  await expect(theme.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");

  await go(page, "/settings");
  await expect(html).toHaveAttribute("data-theme", "dark");
  await expect(theme.getByRole("radio", { name: "Dark" })).toHaveAttribute("aria-checked", "true");

  await theme.getByRole("radio", { name: "Light" }).click();
  await expect(html).toHaveAttribute("data-theme", "light");
  await theme.getByRole("radio", { name: "System" }).click();
  await expect(html).not.toHaveAttribute("data-theme", /.+/);
  await go(page, "/settings");
  await expect(theme.getByRole("radio", { name: "System" })).toHaveAttribute("aria-checked", "true");
});
