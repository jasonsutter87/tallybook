// Statement import: CSV (with a column-mapping step) or OFX/QFX. Imported
// transactions land in the For Review inbox; nothing is posted except by
// auto-post rules.

import { CheckCircle2, FileSpreadsheet, FileText, Upload } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import { useNavigate } from "react-router-dom";
import { call, useAction, useApi } from "../api";
import { AccountSelect, ErrorBanner, Field, Modal, Spinner } from "../components/ui";
import { date, money, parseMoney, plural } from "../format";
import type { Account, AccountSubtype, CsvMapping, CsvPreview, DateFormat, ImportSummary, OfxPreview } from "../types";

const DEST_SUBTYPES: AccountSubtype[] = ["bank", "credit_card", "other_current_asset", "other_current_liability", "long_term_liability"];

/** Column choices may be unset (-1) until the user picks them. */
interface Draft {
  has_header: boolean;
  date_column: number;
  date_format: DateFormat;
  description_column: number;
  amount_kind: "single" | "split";
  amount_column: number;
  negate: boolean;
  debit: number;
  credit: number;
}

const EMPTY_DRAFT: Draft = {
  has_header: true,
  date_column: -1,
  date_format: "month_day_year",
  description_column: -1,
  amount_kind: "single",
  amount_column: -1,
  negate: false,
  debit: -1,
  credit: -1,
};

function draftFrom(m: CsvMapping | null): Draft {
  if (!m) return EMPTY_DRAFT;
  return {
    has_header: m.has_header,
    date_column: m.date_column,
    date_format: m.date_format,
    description_column: m.description_column,
    amount_kind: m.amount.kind,
    amount_column: m.amount.kind === "single" ? m.amount.column : -1,
    negate: m.amount.kind === "single" ? m.amount.negate : false,
    debit: m.amount.kind === "split" ? m.amount.debit : -1,
    credit: m.amount.kind === "split" ? m.amount.credit : -1,
  };
}

function mappingFrom(d: Draft): CsvMapping | null {
  if (d.date_column < 0 || d.description_column < 0) return null;
  if (d.amount_kind === "single") {
    if (d.amount_column < 0) return null;
    return {
      has_header: d.has_header,
      date_column: d.date_column,
      date_format: d.date_format,
      description_column: d.description_column,
      amount: { kind: "single", column: d.amount_column, negate: d.negate },
    };
  }
  if (d.debit < 0 || d.credit < 0) return null;
  return {
    has_header: d.has_header,
    date_column: d.date_column,
    date_format: d.date_format,
    description_column: d.description_column,
    amount: { kind: "split", debit: d.debit, credit: d.credit },
  };
}

/** Approximate client-side date parse, for the sanity-check preview only. */
function parseDate(s: string, f: DateFormat): string | null {
  const t = s.trim().split(/[ T]/)[0];
  const p = (n: number) => String(n).padStart(2, "0");
  const fixYear = (y: number) => (y < 100 ? 2000 + y : y);
  if (f === "iso") {
    const m = t.match(/^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$/);
    return m ? `${m[1]}-${p(+m[2])}-${p(+m[3])}` : null;
  }
  const m = t.match(/^(\d{1,2})[-/.](\d{1,2})[-/.](\d{2,4})$/);
  if (!m) return null;
  const [a, b, y] = [+m[1], +m[2], fixYear(+m[3])];
  const [mo, d] = f === "month_day_year" ? [a, b] : [b, a];
  if (mo < 1 || mo > 12 || d < 1 || d > 31) return null;
  return `${y}-${p(mo)}-${p(d)}`;
}

function cents(s: string | undefined): number | null {
  if (s == null || s.trim() === "") return 0;
  return parseMoney(s.replace(/^\+/, ""));
}

function Switch(props: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="switch">
      <input type="checkbox" checked={props.checked} onChange={(e) => props.onChange(e.target.checked)} />
      <span className="small">{props.label}</span>
    </label>
  );
}

function ColumnSelect(props: { value: number; onChange: (v: number) => void; columns: string[] }) {
  return (
    <select className="select" value={props.value} onChange={(e) => props.onChange(Number(e.target.value))}>
      <option value={-1}>Choose a column…</option>
      {props.columns.map((c, i) => (
        <option key={i} value={i}>
          {c}
        </option>
      ))}
    </select>
  );
}

type Kind = "csv" | "ofx";

export default function ImportModal({ open, onClose, defaultAccountId }: { open: boolean; onClose: () => void; defaultAccountId?: number | null }) {
  const navigate = useNavigate();
  const accounts = useApi<Account[]>("accounts.list");
  const fileInput = useRef<HTMLInputElement>(null);

  const [fileName, setFileName] = useState<string | null>(null);
  const [content, setContent] = useState<string | null>(null);
  const [kind, setKind] = useState<Kind | null>(null);
  const [accountId, setAccountId] = useState<number | null>(defaultAccountId ?? null);
  const [csv, setCsv] = useState<CsvPreview | null>(null);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [ofx, setOfx] = useState<OfxPreview[] | null>(null);
  const [ofxIndex, setOfxIndex] = useState(0);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [dragging, setDragging] = useState(false);
  const [summary, setSummary] = useState<ImportSummary | null>(null);

  const importCsv = useAction<ImportSummary, { content: string; mapping: CsvMapping; account_id: number }>("import.csv");
  const importOfx = useAction<ImportSummary, { content: string; index: number; account_id: number }>("import.ofx");

  // Fresh state each time the modal opens.
  useEffect(() => {
    if (!open) return;
    setFileName(null);
    setContent(null);
    setKind(null);
    setAccountId(defaultAccountId ?? null);
    setCsv(null);
    setDraft(EMPTY_DRAFT);
    setOfx(null);
    setOfxIndex(0);
    setError(null);
    setSummary(null);
    // Only on open: re-running when defaultAccountId changes would wipe a file the user is mapping.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const readFile = async (file: File) => {
    setError(null);
    setReading(true);
    setCsv(null);
    setOfx(null);
    try {
      const text = await file.text();
      setFileName(file.name);
      setContent(text);
      if (/<OFX>/i.test(text)) {
        setKind("ofx");
        const statements = await call<OfxPreview[]>("import.ofx_preview", { content: text });
        setOfx(statements);
        setOfxIndex(0);
        // If the statement's last four digits match a linked account, preselect it.
        const mask = statements[0]?.account.mask;
        const hit = mask ? accounts.data?.find((a) => a.link?.mask === mask) : undefined;
        if (hit && accountId == null) setAccountId(hit.id);
      } else {
        setKind("csv");
        const p = await call<CsvPreview>("import.csv_preview", { content: text });
        setCsv(p);
        setDraft(draftFrom(p.mapping));
      }
    } catch (e) {
      setError(e as Error);
      setKind(null);
    } finally {
      setReading(false);
    }
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    const f = e.dataTransfer.files?.[0];
    if (f) void readFile(f);
  };

  const mapping = useMemo(() => mappingFrom(draft), [draft]);

  // Column labels + data rows depend on whether row 1 is a header.
  const columns = useMemo(() => {
    if (!csv) return [];
    const width = Math.max(csv.headers.length, ...csv.rows.map((r) => r.length));
    return Array.from({ length: width }, (_, i) => (draft.has_header && csv.headers[i] ? csv.headers[i] : `Column ${i + 1}`));
  }, [csv, draft.has_header]);
  const dataRows = useMemo(() => (csv ? (draft.has_header ? csv.rows : [csv.headers, ...csv.rows]) : []), [csv, draft.has_header]);

  const parsed = useMemo(() => {
    if (!mapping) return [];
    return dataRows.slice(0, 5).map((r) => {
      const d = parseDate(r[mapping.date_column] ?? "", mapping.date_format);
      let amount: number | null;
      if (mapping.amount.kind === "single") {
        const a = cents(r[mapping.amount.column]);
        amount = a == null ? null : mapping.amount.negate ? -a : a;
      } else {
        const out = cents(r[mapping.amount.debit]);
        const inn = cents(r[mapping.amount.credit]);
        amount = out == null || inn == null ? null : Math.abs(inn) - Math.abs(out);
      }
      return { date: d, description: r[mapping.description_column] ?? "", amount };
    });
  }, [mapping, dataRows]);

  const ofxStatement = ofx?.[ofxIndex];
  const canImport =
    !!content &&
    accountId != null &&
    ((kind === "csv" && mapping != null) || (kind === "ofx" && ofxStatement != null));
  const busy = importCsv.isPending || importOfx.isPending;

  const doImport = async () => {
    if (!content || accountId == null) return;
    setError(null);
    try {
      const s =
        kind === "csv" && mapping
          ? await importCsv.mutateAsync({ content, mapping, account_id: accountId })
          : await importOfx.mutateAsync({ content, index: ofxIndex, account_id: accountId });
      setSummary(s);
    } catch (e) {
      setError(e as Error);
    }
  };

  const set = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }));

  // ------------------------------------------------------------- success

  if (summary) {
    const toReview = summary.added - summary.auto_posted;
    const accountName = accounts.data?.find((a) => a.id === accountId)?.name;
    return (
      <Modal
        open={open}
        onClose={onClose}
        title="Import complete"
        footer={
          <>
            <button className="btn btn-secondary" onClick={onClose}>
              Done
            </button>
            {toReview > 0 && (
              <button
                className="btn btn-primary"
                onClick={() => {
                  onClose();
                  navigate("/review");
                }}
              >
                Review now
              </button>
            )}
          </>
        }
      >
        <div style={{ textAlign: "center", padding: "8px 0 4px" }}>
          <div className="empty-icon" style={{ background: "var(--pos-soft)", color: "var(--pos)" }}>
            <CheckCircle2 size={26} />
          </div>
          <div className="stat-value num" style={{ marginTop: 0 }}>
            {plural(toReview, "new transaction")}
          </div>
          <div className="muted" style={{ marginTop: 4 }}>
            {toReview > 0 ? "ready to review" : summary.added > 0 ? "all categorized for you" : "nothing new in this file"}
            {accountName ? ` · ${accountName}` : ""}
          </div>
          <div className="stack-sm" style={{ marginTop: 18, alignItems: "center" }}>
            {summary.auto_posted > 0 && <span className="pill pill-violet">{summary.auto_posted} auto-categorized by rules</span>}
            {summary.skipped > 0 && <span className="pill">{summary.skipped} already imported, skipped</span>}
            {summary.updated > 0 && <span className="pill pill-brand">{summary.updated} updated</span>}
          </div>
        </div>
      </Modal>
    );
  }

  // ------------------------------------------------------------- setup

  return (
    <Modal
      open={open}
      onClose={onClose}
      width={kind === "csv" ? 820 : 600}
      title="Import a statement"
      subtitle="Download a CSV, OFX or QFX file from your bank's website and drop it here."
      footer={
        <>
          <button className="btn btn-secondary" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary" disabled={!canImport || busy} onClick={doImport}>
            {busy ? <Spinner /> : <Upload size={16} />}
            {kind === "ofx" && ofxStatement ? `Import ${plural(ofxStatement.count, "transaction")}` : "Import"}
          </button>
        </>
      }
    >
      <div className="stack">
        <input
          ref={fileInput}
          type="file"
          accept=".csv,.ofx,.qfx,.txt"
          style={{ display: "none" }}
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) void readFile(f);
            e.target.value = "";
          }}
        />
        <div
          role="button"
          tabIndex={0}
          onClick={() => fileInput.current?.click()}
          onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && fileInput.current?.click()}
          onDragOver={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={onDrop}
          style={{
            border: `2px dashed ${dragging ? "var(--brand)" : "var(--line-strong)"}`,
            background: dragging ? "var(--brand-soft)" : "var(--surface-2)",
            borderRadius: "var(--radius-lg)",
            padding: fileName ? "14px 18px" : "28px 18px",
            cursor: "pointer",
            transition: "background .12s, border-color .12s",
          }}
        >
          {fileName ? (
            <div className="row" style={{ gap: 12 }}>
              <span className="icon-bubble">{kind === "ofx" ? <FileText size={18} /> : <FileSpreadsheet size={18} />}</span>
              <div className="grow">
                <div className="bold truncate">{fileName}</div>
                <div className="subtle small">
                  {reading ? "Reading…" : kind === "ofx" ? "OFX / QFX statement" : kind === "csv" ? "CSV spreadsheet" : "Couldn't read this file"}
                </div>
              </div>
              <span className="btn btn-ghost btn-sm">Choose another</span>
            </div>
          ) : (
            <div style={{ textAlign: "center" }}>
              <div className="empty-icon" style={{ marginBottom: 10 }}>
                {reading ? <Spinner /> : <Upload size={22} />}
              </div>
              <div className="bold">Drop a statement file here</div>
              <div className="subtle small">or click to browse · CSV, OFX, QFX</div>
            </div>
          )}
        </div>

        <Field label="Which account is this statement for?">
          <AccountSelect value={accountId} onChange={setAccountId} subtypes={DEST_SUBTYPES} placeholder="Choose a bank or card account" />
        </Field>

        <ErrorBanner error={error} />

        {kind === "ofx" && ofx && (
          <div className="stack-sm">
            <div className="label">{ofx.length > 1 ? "This file has several accounts — pick one" : "Statement"}</div>
            {ofx.length === 0 && <div className="banner warn">No statements found in this file.</div>}
            {ofx.map((st, i) => (
              <label
                key={i}
                className="card row"
                style={{
                  padding: "12px 16px",
                  gap: 12,
                  cursor: "pointer",
                  borderColor: i === ofxIndex ? "var(--brand)" : undefined,
                  boxShadow: i === ofxIndex ? "var(--focus)" : undefined,
                }}
              >
                <input type="radio" className="checkbox" checked={i === ofxIndex} onChange={() => setOfxIndex(i)} />
                <div className="grow">
                  <div className="bold">
                    {st.account.name || "Account"} {st.account.mask && <span className="subtle">···{st.account.mask}</span>}
                  </div>
                  <div className="subtle small">
                    {plural(st.count, "transaction")}
                    {st.from && st.to ? ` · ${date(st.from)} – ${date(st.to)}` : ""}
                  </div>
                </div>
                {st.account.current_balance != null && <span className="num bold">{money(st.account.current_balance)}</span>}
              </label>
            ))}
          </div>
        )}

        {kind === "csv" && csv && (
          <>
            <div>
              <div className="label" style={{ marginBottom: 6 }}>
                Your file
              </div>
              <div style={{ overflowX: "auto", border: "1px solid var(--line)", borderRadius: 14, maxHeight: 240 }}>
                <table className="table small">
                  <thead>
                    <tr>
                      {columns.map((c, i) => (
                        <th key={i}>{c}</th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {dataRows.slice(0, 8).map((r, ri) => (
                      <tr key={ri}>
                        {columns.map((_, ci) => (
                          <td key={ci} className="nowrap" style={{ padding: "8px 16px", maxWidth: 260, overflow: "hidden", textOverflow: "ellipsis" }}>
                            {r[ci] ?? ""}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>

            {!csv.mapping && <div className="banner warn">We couldn't guess the columns in this file. Tell us which is which below.</div>}

            <div className="form-grid">
              <Field label="Date column">
                <ColumnSelect value={draft.date_column} onChange={(v) => set({ date_column: v })} columns={columns} />
              </Field>
              <Field label="Date format">
                <select className="select" value={draft.date_format} onChange={(e) => set({ date_format: e.target.value as DateFormat })}>
                  <option value="month_day_year">MM/DD/YYYY (US)</option>
                  <option value="day_month_year">DD/MM/YYYY</option>
                  <option value="iso">YYYY-MM-DD</option>
                </select>
              </Field>
              <Field label="Description column">
                <ColumnSelect value={draft.description_column} onChange={(v) => set({ description_column: v })} columns={columns} />
              </Field>
              <Field label="Amounts are in">
                <select className="select" value={draft.amount_kind} onChange={(e) => set({ amount_kind: e.target.value as Draft["amount_kind"] })}>
                  <option value="single">One column (+ and −)</option>
                  <option value="split">Two columns (money out / money in)</option>
                </select>
              </Field>
              {draft.amount_kind === "single" ? (
                <Field label="Amount column">
                  <ColumnSelect value={draft.amount_column} onChange={(v) => set({ amount_column: v })} columns={columns} />
                </Field>
              ) : (
                <>
                  <Field label="Money out column">
                    <ColumnSelect value={draft.debit} onChange={(v) => set({ debit: v })} columns={columns} />
                  </Field>
                  <Field label="Money in column">
                    <ColumnSelect value={draft.credit} onChange={(v) => set({ credit: v })} columns={columns} />
                  </Field>
                </>
              )}
              <div className="stack-sm span-2" style={{ gap: 10, paddingTop: 2 }}>
                <Switch checked={draft.has_header} onChange={(v) => set({ has_header: v })} label="First row is column names" />
                {draft.amount_kind === "single" && (
                  <Switch checked={draft.negate} onChange={(v) => set({ negate: v })} label="Flip signs (purchases are positive in this file)" />
                )}
              </div>
            </div>

            {mapping && (
              <div className="card" style={{ background: "var(--surface-2)" }}>
                <div className="card-header" style={{ paddingTop: 14 }}>
                  <h3 className="card-title">How we'll read it</h3>
                  <span className="subtle small">Money out should be negative</span>
                </div>
                <div style={{ padding: "6px 6px 8px" }}>
                  <table className="table small">
                    <tbody>
                      {parsed.map((r, i) => (
                        <tr key={i}>
                          <td className="nowrap" style={{ padding: "7px 16px", width: 120 }}>
                            {r.date ? date(r.date) : <span className="neg">Bad date</span>}
                          </td>
                          <td className="truncate" style={{ padding: "7px 16px", maxWidth: 380 }}>
                            {r.description}
                          </td>
                          <td className="right nowrap" style={{ padding: "7px 16px" }}>
                            {r.amount == null ? (
                              <span className="neg">Bad amount</span>
                            ) : (
                              <span className={`num bold ${r.amount > 0 ? "pos" : ""}`}>{money(r.amount, { signed: true })}</span>
                            )}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </Modal>
  );
}
