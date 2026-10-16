import type { TaxTabProps } from "../Taxes";

export default function Estimates({ year }: TaxTabProps) {
  return <div className="card card-pad">Estimates · {year} — coming soon</div>;
}
