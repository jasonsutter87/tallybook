import { expect, type APIRequestContext, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { DATA_DIR } from "./paths";

const API = "http://127.0.0.1:1441/api";

/** Calls the backend directly, for setting up and checking state. */
export async function api<T = unknown>(request: APIRequestContext, method: string, params: unknown = null): Promise<T> {
  // The dev server takes only JSON requests, bodies or not.
  const res = await request.post(`${API}/${method}`, { headers: { "content-type": "application/json" }, data: JSON.stringify(params) });
  const body = await res.json().catch(() => null);
  if (!res.ok()) throw new Error(`${method}: ${body?.code} ${body?.message}`);
  return body as T;
}

export type Tier = "free" | "personal" | "business" | "tax";

/**
 * Licenses this copy as `tier`, the way a successful activation would. A tax
 * license covers this tax year and last unless `taxYears` says otherwise.
 */
export function license(tier: Tier, taxYears?: number[], opts: { active?: boolean } = {}) {
  const file = path.join(DATA_DIR, ".devserver", "license.json");
  if (tier === "free") return fs.rmSync(file, { force: true });
  const year = new Date().getFullYear();
  const years = tier === "tax" ? (taxYears ?? [year - 1, year]) : [];
  fs.writeFileSync(
    file,
    JSON.stringify({
      tier,
      key: "E2E-TEST-KEY-0000",
      instance_id: "e2e",
      email: "alice@example.com",
      activated_on: `${year}-01-01`,
      tax_years: years,
      active: opts.active ?? true,
      checked_on: `${year}-01-01`,
    }),
  );
}

let n = 0;
/** A fresh book file per call, so tests never share state. */
export function bookPath(name: string) {
  n += 1;
  return path.join(DATA_DIR, `${name}-${Date.now()}-${n}.tallybook`);
}

/** Opens the Northwind Studio demo: a business with a year of activity. */
export async function demoBook(request: APIRequestContext) {
  return api(request, "book.create_demo", { path: bookPath("demo") });
}

/** A new, empty household book. */
export async function householdBook(request: APIRequestContext, name = "The Rivera household") {
  return api(request, "book.create", { path: bookPath("household"), company_name: name, sides: ["household"] });
}

/** Collects console errors and uncaught exceptions; assert on them at the end. */
export function watchErrors(page: Page) {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`console: ${m.text()}`);
  });
  return errors;
}

/** Navigates within the app (hash routes) and waits for the page to settle. */
export async function go(page: Page, route: string) {
  // A fresh load every time (a hash-only change would keep the old page and its
  // cached data), so each step sees the backend as it is now.
  await page.goto(`/?t=${Date.now()}#${route}`);
  await page.waitForLoadState("networkidle");
  await expect(page.locator(".spinner")).toHaveCount(0);
}
