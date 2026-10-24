// Connect a bank: on a plan with bank sync, sign in to the bank through
// Plaid's Hosted Link (Tallybook Connect holds the Plaid keys); otherwise,
// the plans that include it.

import { ExternalLink, RefreshCw, ShieldCheck } from "lucide-react";
import { useState, type ReactNode } from "react";
import { call, openExternal, queryClient } from "../../api";
import { ErrorBanner, Modal, Spinner } from "../../components/ui";
import { RenewPlan, UpgradeOffer, useLicense } from "../../flows/License";
import type { HostedLink } from "../../types";
import type { LinkTarget } from "./shared";
import { usePlaidPoll, Waiting } from "./PlaidPoll";
import { CancelledStep } from "./connectSteps";

type ConnectStep = "intro" | "wait" | "exited";

export default function ConnectModal(props: { onClose: () => void; onLinked: (t: LinkTarget) => void; onImport: () => void }) {
  const lic = useLicense();
  const [step, setStep] = useState<ConnectStep>("intro");
  const [error, setError] = useState<Error | null>(null);
  const [busy, setBusy] = useState(false);
  const [link, setLink] = useState<HostedLink | null>(null);
  const st = lic.data;

  const start = async () => {
    setError(null);
    setBusy(true);
    try {
      const l = await call<HostedLink>("bank.plaid_start");
      setLink(l);
      setStep("wait");
      await openExternal(l.url);
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  usePlaidPoll(
    step === "wait" ? (link?.link_token ?? null) : null,
    undefined,
    (r) => {
      if (r.status === "linked") {
        void queryClient.invalidateQueries();
        props.onLinked({ connection_id: r.connection_id, institution: r.institution, accounts: r.accounts });
      } else if (r.status === "exited") setStep("exited");
    },
    (e) => {
      setError(e as Error);
      setStep("exited");
    },
  );

  const importInstead = (
    <button className="btn btn-secondary" onClick={props.onImport}>
      Import a file instead
    </button>
  );

  let body: ReactNode;
  let footer: ReactNode = null;
  let title = "Connect a bank";
  if (!st) {
    body = <Spinner />;
  } else if (!st.syncs) {
    // No plan with bank sync, or one that has ended.
    title = "Bank sync comes with a plan";
    body =
      st.active === false ? (
        <RenewPlan note="Your plan has ended, so bank sync is paused. Renew it to connect banks again." />
      ) : (
        <>
          <UpgradeOffer need="personal" />
          <p className="muted small">Statement files (CSV, OFX, QFX) from any bank import for free, on every plan.</p>
        </>
      );
    footer = importInstead;
  } else if (step === "intro") {
    body = (
      <div className="row" style={{ gap: 12, alignItems: "flex-start" }}>
        <span className="icon-bubble pos">
          <ShieldCheck size={18} />
        </span>
        <div className="muted small">
          You'll sign in to your bank in your browser, on Plaid's secure page. Tallybook never sees your bank password. When you're done, come back
          here and choose which accounts to bring in.
        </div>
      </div>
    );
    footer = (
      <>
        {importInstead}
        <button className="btn btn-primary" onClick={start} disabled={busy}>
          {busy ? <Spinner /> : <ExternalLink size={16} />} Continue to my bank
        </button>
      </>
    );
  } else if (step === "wait" && link) {
    title = "Waiting for your bank";
    body = (
      <Waiting
        url={link.url}
        onCancel={() => {
          setLink(null);
          setStep("intro");
        }}
      />
    );
  } else {
    title = "Connection cancelled";
    body = <CancelledStep />;
    footer = (
      <>
        <button className="btn btn-secondary" onClick={props.onClose}>
          Close
        </button>
        <button className="btn btn-primary" onClick={start} disabled={busy}>
          {busy ? <Spinner /> : <RefreshCw size={16} />} Try again
        </button>
      </>
    );
  }

  return (
    <Modal open onClose={props.onClose} title={title} width={560} footer={footer}>
      <div className="stack">
        <ErrorBanner error={error} />
        {body}
      </div>
    </Modal>
  );
}
