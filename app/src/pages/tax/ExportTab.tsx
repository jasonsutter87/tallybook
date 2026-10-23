// Taxes → For your accountant: writes the tax package (PDFs, CSVs and a
// README) to a "Tax <year>" folder next to the book, plus printable statements.
// The package isn't encrypted, so for an encrypted book the user picks the folder.

import { ArrowRight, BookOpen, Check, Copy, FileText, FolderOpen, Info, Package, Scale, ShieldCheck, Table, TrendingUp } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { isTauri, useAction, useApi } from "../../api";
import { Card, Loading, useToast } from "../../components/ui";
import { plural } from "../../format";
import type { BookStatus, ScheduleCResponse } from "../../types";
import type { TaxTabProps } from "../Taxes";
import "./tax.css";
import { TaxError } from "./txShared";

const FILES: { name: string; desc: string; icon: ReactNode }[] = [
  { name: "Tax summary.pdf", desc: "The year at a glance: Schedule C, account mapping, 1099s and estimates.", icon: <FileText size={16} /> },
  { name: "General ledger.pdf", desc: "Every entry for the year, laid out to read or print.", icon: <FileText size={16} /> },
  { name: "Schedule C worksheet.csv", desc: "Line-by-line figures, meals already limited to 50%.", icon: <Table size={16} /> },
  { name: "Schedule C mapping.csv", desc: "Which account feeds each line, and whether you confirmed it.", icon: <Table size={16} /> },
  { name: "General ledger.csv", desc: "Every entry for the year, so nothing has to be taken on faith.", icon: <BookOpen size={16} /> },
  { name: "1099 summary.csv", desc: "Contractors, paid by bank vs. card, and who crosses the threshold.", icon: <Table size={16} /> },
  { name: "Estimated tax payments.csv", desc: "What you've paid toward this year's estimates.", icon: <Table size={16} /> },
  { name: "README.txt", desc: "A plain-English cover note explaining each file.", icon: <FileText size={16} /> },
];

export default function ExportTab({ year, go }: TaxTabProps) {
  const toast = useToast();
  const sc = useApi<ScheduleCResponse>("tax.schedule_c", { year });
  const status = useApi<BookStatus>("book.status");
  const exp = useAction<string, { year: number; dir?: string }>("tax.export");
  const encrypted = status.data?.encrypted ?? false;
  const [path, setPath] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  if (sc.error) return <TaxError error={sc.error} year={year} />;
  if (!sc.data) return <Loading />;
  const unconfirmed = sc.data.report.unconfirmed;

  const create = async () => {
    try {
      let dir: string | undefined;
      if (encrypted && isTauri) {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({ directory: true, multiple: false, title: "Where should the tax package go?" });
        if (typeof picked !== "string") return;
        dir = picked;
      }
      setPath(await exp.mutateAsync(dir ? { year, dir } : { year }));
      setCopied(false);
    } catch (e) {
      toast.error(e);
    }
  };

  const copy = async () => {
    if (!path) return;
    try {
      await navigator.clipboard.writeText(path);
      setCopied(true);
      toast({ text: "Path copied" });
    } catch (e) {
      toast.error(e);
    }
  };

  const reveal = async () => {
    if (!path) return;
    try {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path);
    } catch (e) {
      toast.error(e);
    }
  };

  return (
    <div className="stack" style={{ gap: 20 }}>
      <Card
        title={
          <span className="row" style={{ gap: 12 }}>
            <span className="icon-bubble violet">
              <Package size={18} />
            </span>
            A tax package for your accountant · {year}
          </span>
        }
      >
        <div className="stack" style={{ gap: 16 }}>
          <p className="muted" style={{ maxWidth: 680 }}>
            One folder with everything a tax preparer usually asks for, saved {encrypted ? "in a folder you choose" : "next to your book file"}. The PDFs are for reading; the CSVs open in
            Excel, Numbers or Google Sheets. Nothing is sent anywhere, and nothing is filed.
          </p>

          <div className="tx-manifest">
            {FILES.map((f) => (
              <div key={f.name} className="tx-manifest-row">
                <span className="tx-manifest-name">
                  <span className="icon-bubble" style={{ width: 28, height: 28, borderRadius: 8 }}>
                    {f.icon}
                  </span>
                  {f.name}
                </span>
                <span className="tx-file-desc">{f.desc}</span>
              </div>
            ))}
          </div>

          {encrypted && (
            <div className="tx-note warn">
              <Info size={15} />
              <span>
                <strong>The package isn't encrypted.</strong> Your book is, but these files can be read by anyone who has them, so you'll choose where
                to save them. A folder that syncs to the cloud or is shared with others isn't a good choice.
              </span>
            </div>
          )}

          <div className="tx-note">
            <ShieldCheck size={15} className="pos" />
            <span>
              <strong>Full tax IDs are never exported.</strong> Contractor SSNs and EINs appear only as the last four digits — your accountant
              will get the full numbers from the W-9s.
            </span>
          </div>

          {unconfirmed > 0 && (
            <div className="tx-note warn" style={{ alignItems: "center" }}>
              <Info size={15} style={{ marginTop: 0 }} />
              <span className="grow">
                {plural(unconfirmed, "account")} still use Tallybook's suggested Schedule C line. You can export now, but a quick review first makes
                the package more useful.
              </span>
              <button className="btn btn-secondary btn-sm" onClick={() => go("schedule-c")}>
                Review mapping <ArrowRight size={14} />
              </button>
            </div>
          )}

          <div className="row">
            <button className="btn btn-primary btn-lg" onClick={create} disabled={exp.isPending}>
              <Package size={18} /> {exp.isPending ? "Creating…" : path ? "Create it again" : "Create tax package"}
            </button>
            {!path && <span className="small subtle">Running it again replaces the files with fresh ones.</span>}
          </div>
        </div>
      </Card>

      {path && (
        <div className="card card-pad" style={{ borderColor: "var(--pos)" }}>
          <div className="row" style={{ gap: 14, alignItems: "flex-start" }}>
            <span className="icon-bubble pos">
              <Check size={18} strokeWidth={3} />
            </span>
            <div className="grow stack-sm">
              <div className="bold">Your {year} tax package is ready</div>
              <div className="tx-path">{path}</div>
              <div className="row" style={{ flexWrap: "wrap" }}>
                <button className="btn btn-secondary btn-sm" onClick={copy}>
                  {copied ? <Check size={14} /> : <Copy size={14} />} {copied ? "Copied" : "Copy path"}
                </button>
                {isTauri && (
                  <button className="btn btn-secondary btn-sm" onClick={reveal}>
                    <FolderOpen size={14} /> Show in Finder
                  </button>
                )}
              </div>
              <div className="small muted">
                Zip the folder or drop it into a shared drive. Good to mention to your accountant: these are worksheets prepared from your books, not
                a filed return.
              </div>
            </div>
          </div>
        </div>
      )}

      <Card title="Printable statements" action={<span className="subtle small">Most preparers like these too</span>}>
        <div className="grid-2">
          <Link to={`/reports/pnl?from=${year}-01-01&to=${year}-12-31`} className="tx-file" style={{ color: "inherit", alignItems: "center" }}>
            <span className="icon-bubble pos">
              <TrendingUp size={16} />
            </span>
            <div className="grow">
              <div className="tx-file-name">Profit & loss · {year}</div>
              <div className="tx-file-desc">Income minus expenses for the year.</div>
            </div>
            <ArrowRight size={16} className="subtle" />
          </Link>
          <Link to={`/reports/balance-sheet?as_of=${year}-12-31`} className="tx-file" style={{ color: "inherit", alignItems: "center" }}>
            <span className="icon-bubble">
              <Scale size={16} />
            </span>
            <div className="grow">
              <div className="tx-file-name">Balance sheet · Dec 31, {year}</div>
              <div className="tx-file-desc">What the business owned and owed at year end.</div>
            </div>
            <ArrowRight size={16} className="subtle" />
          </Link>
        </div>
      </Card>
    </div>
  );
}
