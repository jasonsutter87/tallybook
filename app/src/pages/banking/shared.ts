// What the Banking page and its flows pass around: the link step's target and
// the switch that hides new Plaid connections.

import type { FeedAccount } from "../../types";

export const PLAID_ENABLED = false;

export const CONNECT_START = PLAID_ENABLED ? "choose" : "simplefin";

/** A pending link-step: which connection, which provider accounts. */
export interface LinkTarget {
  connection_id: number;
  institution: string;
  accounts: FeedAccount[];
}
