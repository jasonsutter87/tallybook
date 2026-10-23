import path from "node:path";
import { fileURLToPath } from "node:url";

/** Everything an e2e run creates; wiped at the start of each run. */
export const DATA_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), ".data");
