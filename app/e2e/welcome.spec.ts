import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { api, bookPath, go, license } from "./helpers";

// The Welcome screen's "Open a book". In the browser preview it asks for the
// file's path with window.prompt; these tests answer that prompt.

async function answerPrompt(page: Page, answer: string | null) {
  page.once("dialog", async (dialog) => {
    expect(dialog.type()).toBe("prompt");
    expect(dialog.message()).toBe("Path to a .tallybook file");
    if (answer === null) await dialog.dismiss();
    else await dialog.accept(answer);
  });
}

test.beforeEach(async ({ request }) => {
  license("free");
  await api(request, "book.close");
});

test("opening a path with no book at it says so, and stays on Welcome", async ({ page, request }) => {
  const missing = bookPath("missing");
  await go(page, "/");
  await answerPrompt(page, missing);
  await page.getByRole("button", { name: "Open a book" }).click();

  const toast = page.locator(".toast.error");
  await expect(toast).toBeVisible();
  await expect(toast).toContainText(/not found|doesn't exist|no such|couldn't find/i);
  await expect(page.getByRole("button", { name: "Create a new book" })).toBeVisible();
  expect((await api<{ open: boolean }>(request, "book.status")).open).toBe(false);
  expect(fs.existsSync(missing), "opening never creates the file").toBe(false);
});

test("another program's SQLite database is refused and left as it was", async ({ page, request }) => {
  const other = bookPath("other-program");
  execFileSync("sqlite3", [other, "create table notes (id integer primary key, body text); insert into notes (body) values ('not a book');"]);
  const before = fs.readFileSync(other);
  await go(page, "/");
  await answerPrompt(page, other);
  await page.getByRole("button", { name: "Open a book" }).click();

  await expect(page.locator(".toast.error")).toContainText(/Tallybook/);
  await expect(page.getByRole("button", { name: "Create a new book" })).toBeVisible();
  expect((await api<{ open: boolean }>(request, "book.status")).open).toBe(false);
  expect(fs.readFileSync(other).equals(before), "nothing was written into it").toBe(true);
});

test("a file with no readable header is treated as an encrypted book and asks for its key", async ({ page, request }) => {
  // Encrypted books look like noise on disk, so anything that does is offered the unlock prompt.
  // The test only cancels it: entering a key would involve the real keychain.
  const noise = bookPath("noise");
  fs.writeFileSync(noise, "this is just some text, not a database");
  await go(page, "/");
  await answerPrompt(page, noise);
  await page.getByRole("button", { name: "Open a book" }).click();

  const dialog = page.getByRole("dialog", { name: "This book is encrypted" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
  expect((await api<{ open: boolean }>(request, "book.status")).open).toBe(false);
  expect(fs.readFileSync(noise, "utf8")).toBe("this is just some text, not a database");
});

test("opening an existing book by path shows it", async ({ page, request }) => {
  const path = bookPath("existing");
  await api(request, "book.create", { path, company_name: "Existing Household", sides: ["household"] });
  await api(request, "book.close");
  await go(page, "/");
  await answerPrompt(page, path);
  await page.getByRole("button", { name: "Open a book" }).click();

  await expect(page.locator(".sidebar").getByText("Existing Household")).toBeVisible();
  expect(await api(request, "book.status")).toMatchObject({ open: true, path, company_name: "Existing Household" });
});

test("cancelling the path prompt does nothing", async ({ page, request }) => {
  await go(page, "/");
  await answerPrompt(page, null);
  await page.getByRole("button", { name: "Open a book" }).click();
  await expect(page.getByRole("button", { name: "Create a new book" })).toBeVisible();
  await expect(page.locator(".toast.error")).toHaveCount(0);
  expect((await api<{ open: boolean }>(request, "book.status")).open).toBe(false);
});
