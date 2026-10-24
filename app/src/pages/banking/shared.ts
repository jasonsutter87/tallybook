// What the Banking page and its flows pass around: the link step's target.

import type { FeedAccount } from "../../types";

/** A pending link-step: which connection, which provider accounts. */
export interface LinkTarget {
  connection_id: number;
  institution: string;
  accounts: FeedAccount[];
}
