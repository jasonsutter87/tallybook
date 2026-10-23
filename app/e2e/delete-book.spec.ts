import fs from "node:fs";
import { expect, test } from "@playwright/test";
import { api, bookPath, go, license } from "./helpers";

// Deleting a book asks for its name first. A wrong name leaves the book open
// and on disk; the right one closes it and takes the file away. (The dev
// server sends the file to the OS Trash, so these tests only ever delete
// throwaway books made inside e2e/.data.)

test("a wrong confirmation name leaves the book open, and the right name deletes it", async ({ page, request }) => {
  license("free");
  const path = bookPath("throwaway");
  await api(request, "book.create", { path, company_name: "Throwaway Household", sides: ["household"] });
  await go(page, "/settings");

  await page.getByRole("button", { name: /Delete Throwaway Household/ }).click();
  const dialog = page.getByRole("dialog", { name: "Delete Throwaway Household?" });
  const confirm = dialog.getByRole("button", { name: "Delete book" });
  const typed = dialog.getByLabel(/Type .* to confirm/);

  // Nearly right is still wrong: the button stays off and nothing is deleted.
  await typed.fill("Throwaway Houseold");
  await expect(confirm).toBeDisabled();
  await typed.press("Enter");
  expect(fs.existsSync(path), "the file is still there").toBe(true);
  expect((await api<{ open: boolean }>(request, "book.status")).open, "the book is still open").toBe(true);

  // The backend refuses a wrong name on its own, whatever the screen allows.
  await expect(api(request, "book.delete", { path, confirm_name: "nope" })).rejects.toThrow(/rule/);
  const status = await api<{ open: boolean; company_name: string }>(request, "book.status");
  expect(status).toMatchObject({ open: true, company_name: "Throwaway Household" });
  expect(fs.existsSync(path)).toBe(true);

  await typed.fill("Throwaway Household");
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(page.getByText(/Throwaway Household moved to the Trash/)).toBeVisible();

  await expect.poll(() => fs.existsSync(path)).toBe(false);
  expect((await api<{ open: boolean }>(request, "book.status")).open, "no book is open any more").toBe(false);
  const recent = await api<{ path: string }[]>(request, "book.recent");
  expect(recent.map((r) => r.path)).not.toContain(path);
});
