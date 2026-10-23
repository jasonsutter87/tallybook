import fs from "node:fs";
import { DATA_DIR } from "./paths";

// A clean slate each run. The dev server is already up by now, but it keeps
// nothing in memory until a book is opened, so clearing its folder is safe.
export default function globalSetup() {
  fs.rmSync(DATA_DIR, { recursive: true, force: true });
  fs.mkdirSync(`${DATA_DIR}/.devserver`, { recursive: true });
}
