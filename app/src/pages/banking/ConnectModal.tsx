// Connect a bank: choose a provider, set up Plaid keys, or paste a SimpleFIN token.

import { KeyRound, Link2, RefreshCw } from "lucide-react";
import { useState, type ReactNode } from "react";
import { call, openExternal, queryClient, useApi } from "../../api";
import { ErrorBanner, Modal, Spinner } from "../../components/ui";
import type { HostedLink, PlaidConfig, PollResult } from "../../types";
import { PLAID_ENABLED, type LinkTarget } from "./shared";
import { usePlaidPoll, Waiting } from "./PlaidPoll";
import { CancelledStep, ChooseStep, PlaidSetupStep, SimplefinStep } from "./connectSteps";

type ConnectStep = "choose" | "plaid_setup" | "plaid_wait" | "plaid_exited" | "simplefin";

export default function ConnectModal(props: { initial: "choose" | "simplefin"; onClose: () => void; onLinked: (t: LinkTarget) => void }) {
  const config = useApi<PlaidConfig>("bank.plaid_config_get", undefined, { enabled: PLAID_ENABLED });
  const [step, setStep] = useState<ConnectStep>(props.initial);
  const [error, setError] = useState<Error | null>(null);
  const [busy, setBusy] = useState(false);
  const [link, setLink] = useState<HostedLink | null>(null);

  // Plaid setup form
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const [env, setEnv] = useState<"sandbox" | "production">("sandbox");
  // SimpleFIN
  const [token, setToken] = useState("");

  const startPlaid = async () => {
    setError(null);
    setBusy(true);
    try {
      const l = await call<HostedLink>("bank.plaid_start");
      setLink(l);
      setStep("plaid_wait");
      await openExternal(l.url);
    } catch (e) {
      const err = e as Error & { code?: string };
      if (err.code === "plaid_not_configured") setStep("plaid_setup");
      else setError(err);
    } finally {
      setBusy(false);
    }
  };

  usePlaidPoll(
    step === "plaid_wait" ? (link?.link_token ?? null) : null,
    undefined,
    (r) => {
      if (r.status === "linked") {
        void queryClient.invalidateQueries();
        props.onLinked({ connection_id: r.connection_id, institution: r.institution, accounts: r.accounts });
      } else if (r.status === "exited") setStep("plaid_exited");
    },
    (e) => {
      setError(e as Error);
      setStep("plaid_exited");
    },
  );

  const choosePlaid = () => {
    setError(null);
    if (config.data && !config.data.configured) setStep("plaid_setup");
    else void startPlaid();
  };

  const saveKeys = async () => {
    setError(null);
    setBusy(true);
    try {
      await call("bank.plaid_config_set", { client_id: clientId.trim(), secret: secret.trim(), environment: env });
      await queryClient.invalidateQueries({ queryKey: ["bank.plaid_config_get"] });
      setBusy(false);
      await startPlaid();
    } catch (e) {
      setError(e as Error);
      setBusy(false);
    }
  };

  const connectSimplefin = async () => {
    setError(null);
    setBusy(true);
    try {
      const r = await call<PollResult>("bank.simplefin_connect", { setup_token: token.trim() });
      if (r.status === "linked") {
        void queryClient.invalidateQueries();
        props.onLinked({ connection_id: r.connection_id, institution: r.institution, accounts: r.accounts });
      }
    } catch (e) {
      setError(e as Error);
    } finally {
      setBusy(false);
    }
  };

  const titles: Record<ConnectStep, string> = {
    choose: "Connect a bank",
    plaid_setup: "Add your Plaid keys",
    plaid_wait: "Connecting with Plaid",
    plaid_exited: "Connection cancelled",
    simplefin: "Connect with SimpleFIN",
  };

  const back = (
    <button className="btn btn-ghost" onClick={() => setStep("choose")} disabled={busy}>
      Back
    </button>
  );

  let footer: ReactNode = null;
  if (step === "plaid_setup")
    footer = (
      <>
        {back}
        <button className="btn btn-primary" onClick={saveKeys} disabled={busy || !clientId.trim() || !secret.trim()}>
          {busy ? <Spinner /> : <KeyRound size={16} />} Save keys & continue
        </button>
      </>
    );
  else if (step === "simplefin")
    footer = (
      <>
        {props.initial === "choose" ? back : <button className="btn btn-secondary" onClick={props.onClose}>Cancel</button>}
        <button className="btn btn-primary" onClick={connectSimplefin} disabled={busy || !token.trim()}>
          {busy ? <Spinner /> : <Link2 size={16} />} Connect
        </button>
      </>
    );
  else if (step === "plaid_exited")
    footer = (
      <>
        <button className="btn btn-secondary" onClick={props.onClose}>
          Close
        </button>
        <button className="btn btn-primary" onClick={startPlaid} disabled={busy}>
          {busy ? <Spinner /> : <RefreshCw size={16} />} Try again
        </button>
      </>
    );

  return (
    <Modal open onClose={props.onClose} title={titles[step]} width={560} footer={footer}>
      <div className="stack">
        <ErrorBanner error={error} />

        {step === "choose" && (
          <ChooseStep
            plaid={config.data}
            busy={busy}
            onChoosePlaid={choosePlaid}
            onChooseSimplefin={() => setStep("simplefin")}
            onChangeKeys={() => setStep("plaid_setup")}
          />
        )}

        {step === "plaid_setup" && (
          <PlaidSetupStep clientId={clientId} setClientId={setClientId} secret={secret} setSecret={setSecret} env={env} setEnv={setEnv} />
        )}

        {step === "plaid_wait" && link && (
          <Waiting
            url={link.url}
            onCancel={() => {
              setLink(null);
              setStep("choose");
            }}
          />
        )}

        {step === "plaid_exited" && <CancelledStep />}

        {step === "simplefin" && <SimplefinStep token={token} setToken={setToken} />}
      </div>
    </Modal>
  );
}
