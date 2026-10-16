import type { TaxTabProps } from "../Taxes";

export default function Overview({ year }: TaxTabProps) {
  return <div className="card card-pad">Overview · {year} — coming soon</div>;
}
