// The steps of the connect modal that are only forms and words: the provider
// choice, the Plaid keys form, the cancelled notice and the SimpleFIN token form.
// The state and the buttons stay in ConnectModal.

import { KeyRound, Landmark, Link2, XCircle } from "lucide-react";
import { openExternal } from "../../api";
import { Field, Spinner } from "../../components/ui";
import type { PlaidConfig } from "../../types";

export function ChooseStep(props: { plaid: PlaidConfig | undefined; busy: boolean; onChoosePlaid: () => void; onChooseSimplefin: () => void; onChangeKeys: () => void }) {
  return (
    <>
      <button className="bk-choice" onClick={props.onChoosePlaid} disabled={props.busy}>
        <span className="icon-bubble">{props.busy ? <Spinner /> : <Landmark size={18} />}</span>
        <div className="grow">
          <div className="row-between">
            <span className="bold" style={{ fontSize: 15 }}>
              Plaid
            </span>
            <span className="pill pill-brand">Any US bank</span>
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            Works with 12,000+ banks and cards. Uses your own free Plaid developer keys.
          </div>
          {props.plaid?.configured && (
            <div className="subtle tiny" style={{ marginTop: 6 }}>
              Using keys {props.plaid.client_id_hint} · {props.plaid.environment}{" "}
              <button
                type="button"
                className="link-button"
                onClick={(e) => {
                  e.stopPropagation();
                  props.onChangeKeys();
                }}
              >
                Change keys
              </button>
            </div>
          )}
        </div>
      </button>
      <button className="bk-choice" onClick={props.onChooseSimplefin} disabled={props.busy}>
        <span className="icon-bubble violet">
          <Link2 size={18} />
        </span>
        <div className="grow">
          <div className="row-between">
            <span className="bold" style={{ fontSize: 15 }}>
              SimpleFIN Bridge
            </span>
            <span className="pill pill-violet">~$15/yr</span>
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            No developer approval needed. Paste a setup token from beta-bridge.simplefin.org and you're done.
          </div>
        </div>
      </button>
    </>
  );
}

export function PlaidSetupStep(props: { clientId: string; setClientId: (v: string) => void; secret: string; setSecret: (v: string) => void; env: "sandbox" | "production"; setEnv: (v: "sandbox" | "production") => void }) {
  return (
    <>
      <div className="banner small" style={{ alignItems: "flex-start" }}>
        <KeyRound size={16} style={{ flexShrink: 0, marginTop: 2 }} />
        <span>
          Self-hosted Tallybook uses your own free Plaid developer keys. Get them at{" "}
          <a
            href="https://dashboard.plaid.com/developers/keys"
            onClick={(e) => {
              e.preventDefault();
              void openExternal("https://dashboard.plaid.com/developers/keys");
            }}
          >
            dashboard.plaid.com → Developers → Keys
          </a>
          . Sandbox works with test banks (<b>user_good</b> / <b>pass_good</b>).
        </span>
      </div>
      <Field label="Client ID">
        <input className="input" value={props.clientId} onChange={(e) => props.setClientId(e.target.value)} autoFocus spellCheck={false} />
      </Field>
      <Field label="Secret" hint="Stored in your computer's keychain, never in the book file.">
        <input className="input" type="password" value={props.secret} onChange={(e) => props.setSecret(e.target.value)} spellCheck={false} />
      </Field>
      <Field label="Environment">
        <select className="select" value={props.env} onChange={(e) => props.setEnv(e.target.value as "sandbox" | "production")}>
          <option value="sandbox">Sandbox (test banks)</option>
          <option value="production">Production (real banks)</option>
        </select>
      </Field>
    </>
  );
}

export function CancelledStep() {
  return (
    <div className="bk-wait">
      <div className="bk-wait-ring" style={{ animation: "none", background: "var(--surface-hover)", color: "var(--ink-3)" }}>
        <XCircle size={26} />
      </div>
      <div className="bold" style={{ fontSize: 17 }}>
        Cancelled
      </div>
      <div className="muted small" style={{ marginTop: 6 }}>
        The bank connection wasn't finished. You can try again whenever you're ready.
      </div>
    </div>
  );
}

export function SimplefinStep(props: { token: string; setToken: (v: string) => void }) {
  return (
    <>
      <div className="muted small">
        1. Create an account at{" "}
        <a
          href="https://beta-bridge.simplefin.org"
          onClick={(e) => {
            e.preventDefault();
            void openExternal("https://beta-bridge.simplefin.org");
          }}
        >
          beta-bridge.simplefin.org
        </a>{" "}
        and connect your banks there.
        <br />
        2. Click <b>New connection</b> → copy the <b>setup token</b> and paste it below. It works once.
      </div>
      <Field label="Setup token">
        <textarea
          className="textarea"
          value={props.token}
          onChange={(e) => props.setToken(e.target.value)}
          placeholder="aHR0cHM6Ly9iZXRhLWJyaWRnZS5zaW1wbGVmaW4ub3Jn…"
          spellCheck={false}
          autoFocus
          style={{ fontFamily: "var(--mono)", fontSize: 12.5 }}
        />
      </Field>
    </>
  );
}
