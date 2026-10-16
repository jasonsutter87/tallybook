import { ShieldAlert } from "lucide-react";
import { lazy, Suspense } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useApi } from "../api";
import { Loading, PageHeader, Tabs } from "../components/ui";
import type { TaxYears } from "../types";

const Overview = lazy(() => import("./tax/Overview"));
const ScheduleCTab = lazy(() => import("./tax/ScheduleCTab"));
const Estimates = lazy(() => import("./tax/Estimates"));
const Ten99 = lazy(() => import("./tax/Ten99"));
const ExportTab = lazy(() => import("./tax/ExportTab"));

type Tab = "overview" | "schedule-c" | "estimates" | "1099" | "export";

/** Props every tax tab receives. */
export interface TaxTabProps {
  year: number;
  /** Switch tabs from inside a tab (e.g. "Review your mapping →"). */
  go: (tab: Tab) => void;
}

export default function Taxes() {
  const { tab = "overview" } = useParams<{ tab: Tab }>();
  const navigate = useNavigate();
  const [search, setSearch] = useSearchParams();
  const years = useApi<TaxYears>("tax.years");

  if (years.isLoading || !years.data) return <Loading />;
  const supported = years.data.supported;
  const fallback = supported.includes(years.data.current) ? years.data.current : supported[supported.length - 1];
  const requested = Number(search.get("year"));
  const year = supported.includes(requested) ? requested : fallback;
  const go = (t: Tab) => navigate(`/taxes/${t}?year=${year}`);

  const props: TaxTabProps = { year, go };
  return (
    <div className="page">
      <PageHeader
        title="Taxes"
        subtitle="Worksheets, estimates and forms prepared from your books."
        actions={
          <select className="select" style={{ width: 120 }} value={year} onChange={(e) => setSearch({ year: e.target.value })} aria-label="Tax year">
            {supported.map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
        }
      />

      <div className="banner warn no-print" style={{ marginBottom: 20, alignItems: "flex-start" }}>
        <ShieldAlert size={18} style={{ flexShrink: 0, marginTop: 2 }} />
        <span style={{ fontWeight: 500 }}>{years.data.disclaimer}</span>
      </div>

      <div className="no-print" style={{ marginBottom: 20 }}>
        <Tabs<Tab>
          value={tab}
          onChange={go}
          tabs={[
            { value: "overview", label: "Overview" },
            { value: "schedule-c", label: "Schedule C" },
            { value: "estimates", label: "Quarterly estimates" },
            { value: "1099", label: "1099s" },
            { value: "export", label: "For your accountant" },
          ]}
        />
      </div>

      <Suspense fallback={<Loading />}>
        {tab === "overview" && <Overview {...props} />}
        {tab === "schedule-c" && <ScheduleCTab {...props} />}
        {tab === "estimates" && <Estimates {...props} />}
        {tab === "1099" && <Ten99 {...props} />}
        {tab === "export" && <ExportTab {...props} />}
      </Suspense>
    </div>
  );
}
