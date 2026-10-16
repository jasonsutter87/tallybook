import { ArrowRight, Landmark, Pencil, Plus, Sparkles, Tag, Trash2, Wand2, Zap } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { useAction, useApi } from "../api";
import { AccountSelect, ConfirmModal, EmptyState, ErrorBanner, Field, Loading, Modal, MoneyInput, PageHeader, useAccountNames, useToast } from "../components/ui";
import { money } from "../format";
import type { Cents, Direction, MatchOp, NewRule, Rule } from "../types";

const OP_LABEL: Record<MatchOp, string> = { contains: "contains", equals: "is exactly", starts_with: "starts with" };
const DIRECTION_LABEL: Record<Direction, string> = { any: "any direction", inflow: "money in", outflow: "money out" };

function amountRange(min: Cents | null, max: Cents | null): string | null {
  if (min != null && max != null) return min === max ? `Exactly ${money(min)}` : `${money(min)} – ${money(max)}`;
  if (min != null) return `${money(min)} or more`;
  if (max != null) return `Up to ${money(max)}`;
  return null;
}

export default function Rules() {
  const { data, error, isLoading } = useApi<Rule[]>("rules.list");
  const accountName = useAccountNames();
  const toast = useToast();
  const del = useAction<void, { id: number }>("rules.delete");
  const [editing, setEditing] = useState<Rule | "new" | null>(null);
  const [deleting, setDeleting] = useState<Rule | null>(null);

  return (
    <div className="page page-narrow">
      <PageHeader
        title="Rules"
        subtitle="Rules categorize bank transactions automatically. Tallybook also learns from what you accept."
        actions={
          <button className="btn btn-primary" onClick={() => setEditing("new")}>
            <Plus size={16} /> New rule
          </button>
        }
      />

      <div className="banner" style={{ marginBottom: 20 }}>
        <Sparkles size={18} />
        <span>
          Rules run top to bottom — the first one that matches wins. Rules marked <b>Auto-add</b> skip the{" "}
          <Link to="/review" style={{ textDecoration: "underline", color: "inherit" }}>
            review inbox
          </Link>{" "}
          entirely.
        </span>
      </div>

      {isLoading ? (
        <Loading />
      ) : error || !data ? (
        <ErrorBanner error={error} />
      ) : data.length === 0 ? (
        <div className="card">
          <EmptyState
            icon={<Wand2 size={24} />}
            title="No rules yet"
            body={'Create one for anything you see every month — like "When it says Figma, it\'s Software & Subscriptions." Or tick "remember" when you categorize in Review and Tallybook makes one for you.'}
            action={
              <button className="btn btn-primary" onClick={() => setEditing("new")}>
                <Plus size={16} /> Create your first rule
              </button>
            }
          />
        </div>
      ) : (
        <div className="card">
          {data.map((r, i) => (
            <RuleRow key={r.id} rule={r} index={i} accountName={accountName} onEdit={() => setEditing(r)} onDelete={() => setDeleting(r)} />
          ))}
        </div>
      )}

      {editing && <RuleModal rule={editing === "new" ? null : editing} nextPriority={nextPriority(data)} onClose={() => setEditing(null)} />}

      <ConfirmModal
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title="Delete this rule?"
        body={
          <>
            Future transactions matching <b>"{deleting?.pattern}"</b> will go to your review inbox instead. Transactions it already categorized stay as they are.
          </>
        }
        confirmLabel="Delete rule"
        danger
        onConfirm={async () => {
          if (!deleting) return;
          try {
            await del.mutateAsync({ id: deleting.id });
            toast({ text: "Rule deleted" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </div>
  );
}

function nextPriority(rules: Rule[] | undefined): number {
  if (!rules?.length) return 100;
  return Math.max(...rules.map((r) => r.priority));
}

function RuleRow(props: { rule: Rule; index: number; accountName: (id: number | null | undefined) => string; onEdit: () => void; onDelete: () => void }) {
  const r = props.rule;
  const range = amountRange(r.amount_min, r.amount_max);
  return (
    <div className="list-row" style={{ alignItems: "flex-start", paddingTop: 16, paddingBottom: 16 }}>
      <span className={`icon-bubble ${r.direction === "inflow" ? "pos" : r.direction === "outflow" ? "" : "violet"}`} title={`Runs #${props.index + 1}`}>
        <Zap size={16} />
      </span>
      <div className="grow">
        <div style={{ lineHeight: 1.6 }}>
          <span className="muted">When description</span> <b>{OP_LABEL[r.op]}</b> <span className="bold">"{r.pattern}"</span>
          {r.direction !== "any" && (
            <>
              {" "}
              <span className="muted">and it's</span> <b>{DIRECTION_LABEL[r.direction]}</b>
            </>
          )}{" "}
          <ArrowRight size={14} className="subtle" style={{ verticalAlign: "-2px", margin: "0 2px" }} />{" "}
          <Link to={`/accounts/${r.target_account_id}`} className="bold">
            {props.accountName(r.target_account_id)}
          </Link>
        </div>
        <div className="row" style={{ marginTop: 8, flexWrap: "wrap", gap: 6 }}>
          {r.name && r.name.toLowerCase() !== r.pattern.toLowerCase() && <span className="subtle small" style={{ marginRight: 4 }}>{r.name}</span>}
          {r.auto_post ? (
            <span className="pill pill-brand">
              <Zap size={11} /> Auto-add
            </span>
          ) : (
            <span className="pill">Suggest in review</span>
          )}
          {range && <span className="pill pill-violet">{range}</span>}
          {r.bank_account_id != null && (
            <span className="pill">
              <Landmark size={11} /> Only {props.accountName(r.bank_account_id)}
            </span>
          )}
          {r.rename_to && (
            <span className="pill pill-pos">
              <Tag size={11} /> Rename to "{r.rename_to}"
            </span>
          )}
          <span className="pill" title="Lower numbers run first">
            Priority {r.priority}
          </span>
        </div>
      </div>
      <div className="row" style={{ gap: 2 }}>
        <button className="btn btn-ghost btn-icon" onClick={props.onEdit} aria-label="Edit rule" title="Edit">
          <Pencil size={16} />
        </button>
        <button className="btn btn-ghost btn-icon" onClick={props.onDelete} aria-label="Delete rule" title="Delete">
          <Trash2 size={16} />
        </button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- editor

function RuleModal(props: { rule: Rule | null; nextPriority: number; onClose: () => void }) {
  const toast = useToast();
  const create = useAction<number, NewRule>("rules.create");
  const update = useAction<void, { id: number; rule: NewRule }>("rules.update");
  const accountName = useAccountNames();
  const r = props.rule;

  const [name, setName] = useState(r?.name ?? "");
  const [op, setOp] = useState<MatchOp>(r?.op ?? "contains");
  const [pattern, setPattern] = useState(r?.pattern ?? "");
  const [direction, setDirection] = useState<Direction>(r?.direction ?? "outflow");
  const [min, setMin] = useState<Cents | null>(r?.amount_min ?? null);
  const [max, setMax] = useState<Cents | null>(r?.amount_max ?? null);
  const [bank, setBank] = useState<number | null>(r?.bank_account_id ?? null);
  const [target, setTarget] = useState<number | null>(r?.target_account_id ?? null);
  const [rename, setRename] = useState(r?.rename_to ?? "");
  const [priority, setPriority] = useState(String(r?.priority ?? props.nextPriority));
  const [autoPost, setAutoPost] = useState(r?.auto_post ?? false);
  const [tried, setTried] = useState(false);

  const prio = Number(priority);
  const errors = {
    pattern: pattern.trim() === "" ? "Enter the text to look for." : null,
    target: target == null ? "Choose where these transactions go." : null,
    range: min != null && max != null && min > max ? "The minimum is more than the maximum." : null,
    priority: !Number.isInteger(prio) || priority.trim() === "" ? "Use a whole number." : null,
  };
  const valid = !Object.values(errors).some(Boolean);
  const busy = create.isPending || update.isPending;
  const err = (k: keyof typeof errors) => (tried && errors[k] ? <span className="neg">{errors[k]}</span> : undefined);

  const submit = async () => {
    setTried(true);
    if (!valid || target == null) return;
    const rule: NewRule = {
      name: name.trim() || pattern.trim(),
      priority: prio,
      op,
      pattern: pattern.trim(),
      direction,
      amount_min: min == null ? null : Math.abs(min),
      amount_max: max == null ? null : Math.abs(max),
      bank_account_id: bank,
      target_account_id: target,
      rename_to: rename.trim() || null,
      auto_post: autoPost,
    };
    try {
      if (r) await update.mutateAsync({ id: r.id, rule });
      else await create.mutateAsync(rule);
      toast({ text: r ? "Rule updated" : "Rule created" });
      props.onClose();
    } catch (e) {
      toast.error(e);
    }
  };

  const preview = pattern.trim() ? (
    <>
      When description <b>{OP_LABEL[op]}</b> <b>"{pattern.trim()}"</b>
      {direction !== "any" && (
        <>
          {" "}
          and it's <b>{DIRECTION_LABEL[direction]}</b>
        </>
      )}
      {" → "}
      <b>{target != null ? accountName(target) : "…"}</b>
    </>
  ) : (
    "Fill in the text to match and a category to see your rule."
  );

  return (
    <Modal
      open
      onClose={props.onClose}
      title={r ? "Edit rule" : "New rule"}
      subtitle="Matching transactions are categorized for you."
      width={600}
      footer={
        <>
          <button className="btn btn-secondary" onClick={props.onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" onClick={submit} disabled={busy}>
            {busy ? "Saving…" : r ? "Save rule" : "Create rule"}
          </button>
        </>
      }
    >
      <form
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="banner" style={{ fontWeight: 500 }}>
          <Wand2 size={16} style={{ flexShrink: 0 }} />
          <span>{preview}</span>
        </div>

        <div className="form-grid">
          <Field label="When the description…">
            <select className="select" value={op} onChange={(e) => setOp(e.target.value as MatchOp)}>
              <option value="contains">contains</option>
              <option value="equals">is exactly</option>
              <option value="starts_with">starts with</option>
            </select>
          </Field>
          <Field label="this text" hint={err("pattern") ?? "Not case-sensitive."}>
            <input className="input" autoFocus value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder="e.g. figma" />
          </Field>

          <Field label="And the money is…">
            <select className="select" value={direction} onChange={(e) => setDirection(e.target.value as Direction)}>
              <option value="any">Either way</option>
              <option value="inflow">Money in</option>
              <option value="outflow">Money out</option>
            </select>
          </Field>
          <Field label="Only for this account" hint="Optional — leave blank to match every bank and card.">
            <AccountSelect value={bank} onChange={setBank} subtypes={["bank", "credit_card"]} placeholder="Any bank or card" />
          </Field>

          <Field label="Amount at least" hint="Optional">
            <MoneyInput value={min} onChange={setMin} placeholder="No minimum" />
          </Field>
          <Field label="Amount at most" hint={err("range") ?? "Optional"}>
            <MoneyInput value={max} onChange={setMax} placeholder="No maximum" />
          </Field>

          <Field label="Then categorize as" className="span-2" hint={err("target")}>
            <AccountSelect value={target} onChange={setTarget} kinds={["income", "expense", "asset", "liability", "equity"]} />
          </Field>

          <Field label="Rename to" hint="Optional — a cleaner name for your books, like “Figma”.">
            <input className="input" value={rename} onChange={(e) => setRename(e.target.value)} placeholder="Keep the bank's description" />
          </Field>
          <Field label="Rule name" hint="Optional — for your reference.">
            <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder={pattern.trim() || "e.g. Figma"} />
          </Field>

          <Field label="Priority" hint={err("priority") ?? "Lower numbers run first."}>
            <input className="input num" inputMode="numeric" value={priority} onChange={(e) => setPriority(e.target.value)} style={{ maxWidth: 140, textAlign: "left" }} />
          </Field>
          <div />

          <div className="span-2" style={{ background: "var(--surface-2)", border: "1px solid var(--line)", borderRadius: 14, padding: "12px 14px" }}>
            <label className="switch">
              <input type="checkbox" checked={autoPost} onChange={(e) => setAutoPost(e.target.checked)} />
              Add automatically without review
            </label>
            <div className="hint" style={{ marginTop: 6, marginLeft: 46 }}>
              {autoPost
                ? "Matching transactions go straight into your books. You can still find and change them in the account register."
                : "Matching transactions wait in Review with this category suggested — one click to accept."}
            </div>
          </div>
        </div>
        <button type="submit" hidden />
      </form>
    </Modal>
  );
}
