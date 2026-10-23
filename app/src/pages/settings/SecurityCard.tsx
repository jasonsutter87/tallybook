// Encryption card: encrypt or decrypt the book and show its recovery key.

import { Copy, KeyRound, ShieldOff, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { call, useAction, useApi } from "../../api";
import { ConfirmModal, Modal, useToast } from "../../components/ui";
import type { BookStatus, EncryptResult } from "../../types";
import { SettingsCard } from "./shared";

/** Shows a recovery key with copy, and a "saved it" acknowledgement. */
function RecoveryKeyView(props: { recoveryKey: string }) {
  const toast = useToast();
  return (
    <div className="stack-sm">
      <div
        style={{
          fontFamily: "var(--mono)",
          fontSize: 15,
          lineHeight: 1.7,
          letterSpacing: ".04em",
          padding: "14px 16px",
          borderRadius: 12,
          background: "var(--surface-2)",
          border: "1px dashed var(--line-strong)",
          wordBreak: "break-all",
          userSelect: "all",
        }}
      >
        {props.recoveryKey}
      </div>
      <button
        className="btn btn-secondary btn-sm"
        style={{ alignSelf: "flex-start" }}
        onClick={async () => {
          try {
            await navigator.clipboard.writeText(props.recoveryKey);
            toast({ text: "Recovery key copied. Paste it into your password manager." });
          } catch (e) {
            toast.error(e);
          }
        }}
      >
        <Copy size={14} /> Copy
      </button>
    </div>
  );
}

export default function SecurityCard() {
  const toast = useToast();
  const status = useApi<BookStatus>("book.status");
  const encrypt = useAction<EncryptResult>("book.encrypt");
  const decrypt = useAction("book.decrypt");
  const dropPlain = useAction<number>("book.delete_plain_backups");
  const [confirming, setConfirming] = useState(false);
  const [result, setResult] = useState<EncryptResult | null>(null);
  const [saved, setSaved] = useState(false);
  const [shown, setShown] = useState<string | null>(null);
  const [turningOff, setTurningOff] = useState(false);
  const encrypted = !!status.data?.encrypted;

  return (
    <SettingsCard
      id="security"
      icon={<ShieldCheck size={18} />}
      title="Encryption"
      desc="Lock the whole book file with a random 256-bit key. Without the key the file is unreadable, whether it's on a lost laptop, in a backup, or in the cloud."
    >
      {encrypted ? (
        <div className="stack">
          <div className="banner" style={{ background: "var(--pos-soft)", color: "var(--pos)" }}>
            <ShieldCheck size={18} /> This book is encrypted. It opens automatically on this computer because its key is in your keychain.
          </div>
          <div className="row" style={{ gap: 10, flexWrap: "wrap" }}>
            <button
              className="btn btn-secondary"
              onClick={async () => {
                try {
                  setShown(await call<string>("book.recovery_key"));
                } catch (e) {
                  toast.error(e);
                }
              }}
            >
              <KeyRound size={16} /> Show recovery key
            </button>
            <button className="btn btn-ghost" onClick={() => setTurningOff(true)}>
              <ShieldOff size={16} /> Turn off encryption
            </button>
          </div>
          {shown && (
            <div className="stack-sm">
              <div className="label">Recovery key</div>
              <RecoveryKeyView recoveryKey={shown} />
              <button className="btn btn-ghost btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => setShown(null)}>
                Hide
              </button>
            </div>
          )}
        </div>
      ) : (
        <div className="stack">
          <ul className="muted" style={{ margin: 0, paddingLeft: 20, lineHeight: 1.7 }}>
            <li>Day to day nothing changes: the key is kept in your system keychain.</li>
            <li>You get a <b>recovery key</b> to open the book on another computer or restore a cloud copy. Keep it somewhere safe, like a password manager.</li>
            <li>Backups and cloud copies made from now on are encrypted too.</li>
          </ul>
          <button className="btn btn-primary" style={{ alignSelf: "flex-start" }} onClick={() => setConfirming(true)}>
            <ShieldCheck size={16} /> Encrypt this book
          </button>
        </div>
      )}

      <Modal
        open={confirming}
        onClose={() => {
          if (!result || saved) {
            setConfirming(false);
            setResult(null);
            setSaved(false);
          }
        }}
        title={result ? "Save your recovery key" : "Encrypt this book?"}
        footer={
          result ? (
            <button
              className="btn btn-primary"
              disabled={!saved}
              onClick={() => {
                setConfirming(false);
                setResult(null);
                setSaved(false);
              }}
            >
              Done
            </button>
          ) : (
            <>
              <button className="btn btn-secondary" onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button
                className="btn btn-primary"
                disabled={encrypt.isPending}
                onClick={async () => {
                  try {
                    setResult(await encrypt.mutateAsync(null));
                  } catch (e) {
                    toast.error(e);
                  }
                }}
              >
                {encrypt.isPending ? "Encrypting…" : "Encrypt"}
              </button>
            </>
          )
        }
      >
        {result ? (
          <div className="stack">
            <div className="muted">
              This is the <b>only</b> way to open this book on another computer, or to restore it if this computer's keychain is lost. Tallybook can't recover it for you.
            </div>
            <RecoveryKeyView recoveryKey={result.recovery_key} />
            {result.plain_backups > 0 && (
              <div className="banner warn" style={{ alignItems: "flex-start" }}>
                <span style={{ fontWeight: 500 }}>
                  {result.plain_backups === 1 ? "1 backup" : `${result.plain_backups} backups`} from before encryption can still be read by anyone with the file.{" "}
                  <button
                    className="btn btn-secondary btn-sm"
                    style={{ marginTop: 8 }}
                    onClick={async () => {
                      try {
                        const n = await dropPlain.mutateAsync(null);
                        setResult({ ...result, plain_backups: 0 });
                        toast({ text: `Deleted ${n} unencrypted ${n === 1 ? "backup" : "backups"}` });
                      } catch (e) {
                        toast.error(e);
                      }
                    }}
                  >
                    Delete them permanently
                  </button>
                </span>
              </div>
            )}
            <label className="row" style={{ gap: 10, cursor: "pointer", fontWeight: 600 }}>
              <input type="checkbox" className="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
              I've saved my recovery key somewhere safe
            </label>
          </div>
        ) : (
          <div className="muted">
            Tallybook will make an encrypted copy, check that it matches your book exactly, and then replace the original. It takes a moment. You'll see your recovery key next.
            <p className="small" style={{ marginTop: 8 }}>
              Earlier, unencrypted versions of the file may still exist in Time Machine or your sync service's version history.
            </p>
          </div>
        )}
      </Modal>

      <ConfirmModal
        open={turningOff}
        onClose={() => setTurningOff(false)}
        title="Turn off encryption?"
        body="The book file will be stored unencrypted again. Existing encrypted backups and cloud copies stay encrypted and still need the recovery key."
        confirmLabel="Turn off encryption"
        danger
        onConfirm={async () => {
          try {
            await decrypt.mutateAsync(null);
            toast({ text: "Encryption turned off" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </SettingsCard>
  );
}
