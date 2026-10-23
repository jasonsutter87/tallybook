// The link step: match each provider account to a ledger account (or make a new one)
// and suggest an opening balance.

import { Link2 } from "lucide-react";
import { useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { call, queryClient } from "../../api";
import { useSides } from "../../book";
import { ErrorBanner, Modal, Spinner, useToast } from "../../components/ui";
import { iso, plural } from "../../format";
import type { Account, AccountSubtype, FeedAccountKind, OpeningSuggestion, SyncReport } from "../../types";
import type { LinkTarget } from "./shared";
import FeedAccountRow, { type LinkChoice } from "./FeedAccountRow";
import OpeningBalancesModal, { type OpeningDraft } from "./OpeningBalancesModal";

/** First day of the month, three months back. */
function defaultStartDate(): string {
  const d = new Date();
  return iso(new Date(d.getFullYear(), d.getMonth() - 3, 1));
}

const KIND_SUBTYPE: Record<FeedAccountKind, AccountSubtype> = {
  depository: "bank",
  credit: "credit_card",
  loan: "long_term_liability",
  investment: "other_current_asset",
  other: "other_current_asset",
};

export default function LinkStepModal(props: { target: LinkTarget; accounts: Account[]; onClose: () => void }) {
  const toast = useToast();
  const navigate = useNavigate();
  const { target } = props;
  const sides = useSides();
  const [choices, setChoices] = useState<LinkChoice[]>(() =>
    target.accounts.map((a) => ({
      target: "new",
      name: a.name,
      subtype: KIND_SUBTYPE[a.kind],
      scope: sides.main,
      start_date: defaultStartDate(),
    })),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [openings, setOpenings] = useState<OpeningDraft[] | null>(null);
  const [added, setAdded] = useState(0);

  const existing = useMemo(
    () => props.accounts.filter((a) => !a.archived && !a.link && ["bank", "credit_card", "long_term_liability", "other_current_asset"].includes(a.subtype)),
    [props.accounts],
  );
  const chosenExisting = new Set(choices.map((c) => c.target).filter((t): t is number => typeof t === "number"));
  const active = choices.filter((c) => c.target !== "skip");
  const valid = active.length > 0 && active.every((c) => c.start_date && (c.target !== "new" || c.name.trim()));

  const set = (i: number, patch: Partial<LinkChoice>) => setChoices((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));

  const finish = (count: number) => {
    void queryClient.invalidateQueries();
    toast({ text: count ? `${target.institution} connected · ${plural(count, "transaction")} to review` : `${target.institution} connected` });
    props.onClose();
    navigate("/review");
  };

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const created: { id: number; name: string }[] = [];
      for (let i = 0; i < target.accounts.length; i++) {
        const fa = target.accounts[i];
        const c = choices[i];
        if (c.target === "skip") continue;
        const r = await call<{ link_id: number; account_id: number }>("bank.link", {
          connection_id: target.connection_id,
          external_account_id: fa.external_id,
          name: fa.name,
          mask: fa.mask,
          account_id: typeof c.target === "number" ? c.target : null,
          new_account: c.target === "new" ? { name: c.name.trim(), subtype: c.subtype, scope: c.scope } : null,
          start_date: c.start_date,
        });
        if (c.target === "new") created.push({ id: r.account_id, name: c.name.trim() });
      }
      const reports = await call<SyncReport[]>("bank.sync", { connection_id: target.connection_id });
      const count = reports.reduce((n, r) => n + r.imported.added - r.imported.auto_posted, 0);
      const failed = reports.find((r) => !r.ok);
      if (failed) toast.error(`${failed.institution}: ${failed.error}`);
      setAdded(count);

      // Offer opening balances for brand-new ledger accounts.
      const drafts: OpeningDraft[] = [];
      for (const a of created) {
        try {
          const s = await call<OpeningSuggestion>("bank.suggest_opening", { account_id: a.id });
          drafts.push({ account_id: a.id, name: a.name, date: s.date, amount: s.amount, bank_balance: s.bank_balance, include: true });
        } catch {
          // No balance from the bank yet — the user can set it later from the account.
        }
      }
      await queryClient.invalidateQueries();
      if (drafts.length) setOpenings(drafts);
      else finish(count);
    } catch (e) {
      setError(e as Error);
      await queryClient.invalidateQueries();
    } finally {
      setBusy(false);
    }
  };

  const saveOpenings = async () => {
    if (!openings) return;
    setError(null);
    setBusy(true);
    try {
      for (const o of openings) {
        if (!o.include || o.amount == null || !o.date) continue;
        await call("accounts.opening_balance", { id: o.account_id, date: o.date, amount: o.amount });
      }
      finish(added);
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  // ---- opening balances phase
  if (openings) {
    return <OpeningBalancesModal openings={openings} setOpenings={setOpenings} busy={busy} error={error} onSave={saveOpenings} onFinish={() => finish(added)} />;
  }

  // ---- choose ledger accounts phase
  return (
    <Modal
      open
      onClose={props.onClose}
      width={680}
      title={`Link ${target.institution} accounts`}
      subtitle="Choose where each account's transactions should go in your books."
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={!valid || busy}>
            {busy ? <Spinner /> : <Link2 size={16} />} {busy ? "Linking & syncing…" : `Link ${plural(active.length, "account")}`}
          </button>
        </>
      }
    >
      <div className="stack" style={{ gap: 12 }}>
        <ErrorBanner error={error} />
        {target.accounts.length === 0 && <div className="banner warn">The bank didn't share any accounts.</div>}
        {target.accounts.map((fa, i) => (
          <FeedAccountRow
            key={fa.external_id}
            fa={fa}
            c={choices[i]}
            onChange={(patch) => set(i, patch)}
            existing={existing}
            chosenExisting={chosenExisting}
            sides={sides}
          />
        ))}
        <div className="subtle small">
          Transactions before the start date are ignored — set an opening balance for that date so your books line up with the bank. We'll suggest one
          next.
        </div>
      </div>
    </Modal>
  );
}
