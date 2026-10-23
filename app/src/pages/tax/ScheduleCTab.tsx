// Taxes → Schedule C: which business account feeds which form line, the
// line-by-line worksheet, and confirming Tallybook's guesses.

import { CheckCheck, Info, Pencil, Printer, X } from "lucide-react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { useAction, useApi } from "../../api";
import { Card, Loading, Money, useToast } from "../../components/ui";
import { date, money, plural } from "../../format";
import type { Cents, LineInfo, ScheduleCLine, ScheduleCResponse, TaxMappingRow, TaxProfile } from "../../types";
import type { TaxTabProps } from "../Taxes";
import "./tax.css";
import { TaxError, firstError } from "./txShared";

/** Part I lines (income side). Everything else except not_deductible is Part II. */
const PART_I: ScheduleCLine[] = ["l1_gross_receipts", "l2_returns", "l4_cost_of_goods", "l6_other_income"];
/** Lines that usually need records or a pro, flagged in the worksheet. */
const NEEDS_RECORDS: Partial<Record<ScheduleCLine, string>> = {
  l9_car_truck: "Needs mileage or actual-cost records.",
  l13_depreciation: "Depreciation rules are tricky — ask a pro (Form 4562).",
};

export default function ScheduleCTab({ year }: TaxTabProps) {
  const toast = useToast();
  const sc = useApi<ScheduleCResponse>("tax.schedule_c", { year });
  const lines = useApi<LineInfo[]>("tax.lines");
  const setLine = useAction<void, { account_id: number; line: ScheduleCLine | null }>("tax.set_line");
  const confirmAll = useAction<void, { year: number }>("tax.confirm_all");

  const [selected, setSelected] = useState<ScheduleCLine | null>(null);
  const [showAll, setShowAll] = useState(false);
  const mapRef = useRef<HTMLDivElement>(null);

  const lineInfo = useMemo(() => new Map((lines.data ?? []).map((l) => [l.line, l])), [lines.data]);

  const error = firstError(sc.error, lines.error);
  if (error) return <TaxError error={error} year={year} />;
  if (!sc.data || !lines.data) return <Loading />;

  const { report, home_office_sqft } = sc.data;
  const s = report.schedule;
  const toggle = (l: ScheduleCLine) => {
    setSelected((cur) => (cur === l ? null : l));
    // Keep the filtered accounts in view when the worksheet is scrolled.
    requestAnimationFrame(() => {
      const el = mapRef.current;
      if (el && el.getBoundingClientRect().top < 0) el.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  };
  const label = (l: ScheduleCLine) => {
    const i = lineInfo.get(l);
    return i ? `${i.number} · ${i.label}` : l;
  };

  const changeLine = async (row: TaxMappingRow, line: ScheduleCLine) => {
    try {
      await setLine.mutateAsync({ account_id: row.account_id, line });
      toast({ text: line === "not_deductible" ? `${row.name} left off Schedule C` : `${row.name} → line ${label(line)}` });
    } catch (e) {
      toast.error(e);
    }
  };

  const doConfirmAll = async () => {
    try {
      await confirmAll.mutateAsync({ year });
      toast({ text: `Confirmed ${plural(report.unconfirmed, "suggestion")}` });
    } catch (e) {
      toast.error(e);
    }
  };

  // Mapping rows: income first, then expenses; hide zero rows unless asked.
  const rows = report.rows
    .filter((r) => r.kind === "income" || r.kind === "expense")
    .filter((r) => showAll || r.amount !== 0)
    .filter((r) => !selected || r.line === selected)
    .sort((a, b) => (a.kind === b.kind ? 0 : a.kind === "income" ? -1 : 1));
  const hiddenZero = report.rows.filter((r) => r.amount === 0).length;

  return (
    <div className="stack" style={{ gap: 16 }}>
      <div className="row-between tx-print-hide" style={{ flexWrap: "wrap" }}>
        <div className="muted small" style={{ maxWidth: 640 }}>
          Your books, laid out the way Schedule C asks for them. Each line on the left is built from the accounts on the right — click a line to see
          which accounts feed it.
        </div>
        <button className="btn btn-secondary" onClick={() => window.print()}>
          <Printer size={16} /> Print worksheet
        </button>
      </div>

      <div className="tx-sc-grid">
        {/* ---------------------------------------------------------- worksheet */}
        <section className="tx-sheet" aria-label="Schedule C worksheet">
          <div className="tx-sheet-head">
            <div className="tx-sheet-form">Schedule C (Form 1040) · worksheet</div>
            <div className="tx-sheet-title">Profit or Loss From Business · {year}</div>
            <div className="tx-sheet-meta">
              From your books, {date(report.from)} – {date(report.to)}. A worksheet to copy from, not the official form.
            </div>
          </div>

          {report.unconfirmed > 0 && (
            <div className="tx-note warn no-print" style={{ margin: "12px 16px 4px" }}>
              <Info size={15} />
              <span>
                <strong>{plural(report.unconfirmed, "account")}</strong> still use Tallybook's suggested line. Glance at the mapping and confirm — the
                numbers here update as you go.
              </span>
            </div>
          )}

          <div className="tx-part">
            <span className="tag">PART I</span> Income
          </div>
          <Line no="1" text="Gross receipts or sales" amount={s.line1_gross_receipts} line="l1_gross_receipts" sel={selected} onPick={toggle} />
          <Line no="2" text="Returns and allowances" amount={s.line2_returns} line="l2_returns" sel={selected} onPick={toggle} />
          <Line no="3" text="Subtract line 2 from line 1" amount={s.line3} sum />
          <Line no="4" text="Cost of goods sold" amount={s.line4_cogs} line="l4_cost_of_goods" sel={selected} onPick={toggle} />
          <Line no="5" text="Gross profit (line 3 minus line 4)" amount={s.line5_gross_profit} sum />
          <Line no="6" text="Other income" amount={s.line6_other_income} line="l6_other_income" sel={selected} onPick={toggle} />
          <Line no="7" text="Gross income (line 5 plus line 6)" amount={s.line7_gross_income} sum />

          <div className="tx-part">
            <span className="tag">PART II</span> Expenses
          </div>
          {s.expenses.length === 0 && <div className="tx-line subtle small">No expenses mapped to Schedule C yet.</div>}
          {s.expenses.map((x) => (
            <Line
              key={x.line}
              no={x.number}
              text={x.label}
              amount={x.amount}
              line={x.line}
              sel={selected}
              onPick={toggle}
              sub={
                x.line === "l24b_meals" ? (
                  <>
                    booked {money(x.booked)} · 50% deductible
                  </>
                ) : (
                  NEEDS_RECORDS[x.line]
                )
              }
            />
          ))}
          <Line no="28" text="Total expenses" amount={s.line28_total_expenses} sum />
          <Line no="29" text="Tentative profit (line 7 minus line 28)" amount={s.line29_tentative_profit} sum />
          <HomeOffice amount={s.line30_home_office} sqft={home_office_sqft} />
          <Line no="31" text="Net profit (or loss)" amount={s.line31_net_profit} total />

          <div className="tx-sheet-foot">
            Line 31 also flows to Schedule 1 (line 3) and Schedule SE. Prepared by Tallybook from your books — not tax advice, and nothing has been
            filed.
          </div>
        </section>

        {/* ---------------------------------------------------------- mapping */}
        <div className="stack tx-print-hide" style={{ gap: 16, scrollMarginTop: 16 }} ref={mapRef}>
          <Card
            title="Which account goes where"
            action={
              <button className="btn btn-primary btn-sm" onClick={doConfirmAll} disabled={report.unconfirmed === 0 || confirmAll.isPending}>
                <CheckCheck size={14} /> Confirm all suggestions ({report.unconfirmed})
              </button>
            }
            pad={false}
          >
            <div className="tx-map-tools" style={{ marginTop: 12 }}>
              {selected ? (
                <span className="row" style={{ gap: 8 }}>
                  <span className="small muted">Accounts on</span>
                  <span className="pill pill-brand">
                    {label(selected)}
                    <button
                      onClick={() => setSelected(null)}
                      aria-label="Show all lines"
                      style={{ border: 0, background: "none", color: "inherit", cursor: "pointer", padding: 0, display: "inline-flex" }}
                    >
                      <X size={12} />
                    </button>
                  </span>
                </span>
              ) : (
                <span className="small muted">Pick the Schedule C line for each account.</span>
              )}
              <label className="switch small">
                <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
                Show all accounts{hiddenZero > 0 && !showAll ? ` (+${hiddenZero} with no activity)` : ""}
              </label>
            </div>
            {rows.length === 0 ? (
              <div className="card-pad subtle small">
                {selected ? "No accounts with activity map to this line." : "No income or expense activity this year yet."}
              </div>
            ) : (
              <table className="tx-map">
                <thead>
                  <tr>
                    <th>Account</th>
                    <th style={{ textAlign: "right" }}>{year}</th>
                    <th style={{ width: "48%" }}>Schedule C line</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r) => (
                    <MapRow
                      key={r.account_id}
                      row={r}
                      lines={lines.data}
                      info={lineInfo.get(r.line)}
                      busy={setLine.isPending}
                      onChange={(l) => changeLine(r, l)}
                    />
                  ))}
                </tbody>
              </table>
            )}
          </Card>

          <Card title="A few judgement calls">
            <ul className="tx-list">
              <li>
                <Info size={14} /> <span>
                  <strong>Meals</strong> are usually only 50% deductible — Tallybook halves line 24b for you. Entertainment generally isn't deductible
                  at all.
                </span>
              </li>
              <li>
                <Info size={14} /> <span>
                  <strong>Equipment and vehicles</strong> (lines 9 and 13) need mileage logs or depreciation records. Worth asking a pro before you
                  rely on those numbers.
                </span>
              </li>
              <li>
                <Info size={14} /> <span>
                  <strong>Owner draws and transfers</strong> between your own accounts aren't expenses — they belong in "Not on Schedule C".
                </span>
              </li>
              <li>
                <Info size={14} /> <span>
                  <strong>Income tax payments</strong> (federal or state estimates) aren't business expenses either. Line 23 is for business taxes
                  and licenses.
                </span>
              </li>
            </ul>
          </Card>
        </div>
      </div>
    </div>
  );
}

// ------------------------------------------------------------- worksheet line

function Line(props: {
  no: string;
  text: string;
  amount: Cents;
  line?: ScheduleCLine;
  sel?: ScheduleCLine | null;
  onPick?: (l: ScheduleCLine) => void;
  sum?: boolean;
  total?: boolean;
  sub?: ReactNode;
}) {
  const body = (
    <>
      <span className="tx-lineno">{props.no}</span>
      <span className="tx-line-label">
        {props.text}
        {props.sub && <span className="tx-line-sub">{props.sub}</span>}
      </span>
      <span className={`tx-line-amt ${props.amount < 0 ? "neg" : ""}`}>{money(props.amount)}</span>
    </>
  );
  const cls = `tx-line ${props.sum ? "sum" : ""} ${props.total ? "total" : ""}`;
  if (props.line && props.onPick) {
    const line = props.line;
    const active = props.sel === line;
    return (
      <button
        type="button"
        className={`${cls} ${active ? "active" : ""}`}
        onClick={() => props.onPick!(line)}
        aria-pressed={active}
        title="Show the accounts behind this line"
      >
        {body}
      </button>
    );
  }
  return <div className={cls}>{body}</div>;
}

// ------------------------------------------------------------- line 30

function HomeOffice({ amount, sqft }: { amount: Cents; sqft: number }) {
  const toast = useToast();
  const profile = useApi<TaxProfile>("tax.profile");
  const save = useAction<void, TaxProfile>("tax.set_profile");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const start = () => {
    setDraft(sqft ? String(sqft) : "");
    setEditing(true);
  };

  const commit = async () => {
    const n = draft.trim() === "" ? 0 : Number(draft);
    if (!Number.isInteger(n) || n < 0 || n > 100_000) {
      toast.error("Enter whole square feet, like 150");
      return;
    }
    if (!profile.data) return;
    try {
      await save.mutateAsync({ ...profile.data, home_office_sqft: n });
      setEditing(false);
      toast({ text: n ? `Home office set to ${n} sq ft` : "Home office removed" });
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <div className="tx-line">
      <span className="tx-lineno">30</span>
      <span className="tx-line-label">
        Business use of your home <span className="subtle">(simplified method)</span>
        <span className="tx-line-sub">
          {sqft > 0 ? `${sqft.toLocaleString()} sq ft × $5` : "No home office entered"}
          {sqft > 300 && " · capped at 300 sq ft"}
        </span>
        {editing ? (
          <span className="tx-sqft no-print">
            <input
              className="input"
              inputMode="numeric"
              value={draft}
              autoFocus
              placeholder="0"
              onChange={(e) => setDraft(e.target.value.replace(/[^\d]/g, ""))}
              onKeyDown={(e) => {
                if (e.key === "Enter") commit();
                if (e.key === "Escape") setEditing(false);
              }}
              aria-label="Home office square feet"
            />
            <span className="small muted">sq ft</span>
            <button className="btn btn-primary btn-sm" onClick={commit} disabled={save.isPending || !profile.data}>
              Save
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => setEditing(false)}>
              Cancel
            </button>
          </span>
        ) : (
          <button className="btn btn-ghost btn-sm no-print" style={{ marginTop: 4, marginLeft: -12 }} onClick={start}>
            <Pencil size={12} /> {sqft > 0 ? "Change square footage" : "Add a home office"}
          </button>
        )}
        {editing && (
          <span className="tx-line-sub no-print" style={{ marginTop: 6 }}>
            $5 per sq ft of space used regularly and only for business, up to 300 sq ft, and never more than line 29. The regular method (Form 8829)
            can be larger but needs a pro or the IRS instructions.
          </span>
        )}
      </span>
      <span className="tx-line-amt">{money(amount)}</span>
    </div>
  );
}

// ------------------------------------------------------------- mapping row

function MapRow(props: {
  row: TaxMappingRow;
  lines: LineInfo[];
  info: LineInfo | undefined;
  busy: boolean;
  onChange: (l: ScheduleCLine) => void;
}) {
  const { row, lines, info } = props;
  const partI = lines.filter((l) => PART_I.includes(l.line));
  const partII = lines.filter((l) => !PART_I.includes(l.line) && l.line !== "not_deductible");
  const off = lines.find((l) => l.line === "not_deductible");
  const opt = (l: LineInfo) => (
    <option key={l.line} value={l.line}>
      {l.number} · {l.label}
    </option>
  );
  return (
    <tr className={row.amount === 0 ? "zero" : ""}>
      <td>
        <Link to={`/accounts/${row.account_id}`} className="acct">
          {row.name}
        </Link>
        <div className="row" style={{ gap: 6, marginTop: 3 }}>
          {row.code && <span className="tiny subtle num">{row.code}</span>}
          {!row.confirmed && <span className="pill pill-violet" style={{ fontSize: 11, padding: "0 8px" }} title="Tallybook's guess — change it or confirm all">Suggested</span>}
        </div>
      </td>
      <td className="right nowrap">
        <Money cents={row.amount} className="bold" />
      </td>
      <td>
        <select
          className="select"
          value={row.line}
          disabled={props.busy}
          title={info?.help}
          onChange={(e) => props.onChange(e.target.value as ScheduleCLine)}
          aria-label={`Schedule C line for ${row.name}`}
        >
          <optgroup label="Part I · Income">{partI.map(opt)}</optgroup>
          <optgroup label="Part II · Expenses">{partII.map(opt)}</optgroup>
          {off && <option value={off.line}>— · Not on Schedule C</option>}
        </select>
        {info?.help && <div className="tx-help">{info.help}</div>}
      </td>
    </tr>
  );
}
