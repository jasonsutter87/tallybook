// The left-hand navigation: household, business and setup links, shown for
// the sides this book keeps.

import {
  BarChart3,
  CalendarClock,
  CalendarDays,
  Building2,
  FileText,
  Home,
  Inbox,
  Landmark,
  LogOut,
  Receipt,
  Scale,
  Settings,
  Target,
  Users,
  Wand2,
} from "lucide-react";
import { NavLink } from "react-router-dom";
import { useAction, useApi } from "../api";
import { useSides } from "../book";

type NavItem = { to: string; label: string; icon: typeof Home; end?: boolean; badge?: boolean };

// Household links show only when the book keeps a household side; the
// business links only when it keeps a business one.
const MONTH: NavItem = { to: "/month", label: "This month", icon: CalendarDays };
const HOUSEHOLD: NavItem[] = [
  { to: "/due", label: "Bills & due dates", icon: CalendarClock },
  { to: "/goals", label: "Goals", icon: Target },
];

const MAIN: NavItem[] = [
  { to: "/", label: "Home", icon: Home, end: true },
  { to: "/review", label: "Review", icon: Inbox, badge: true },
  { to: "/accounts", label: "Accounts", icon: Landmark },
];

const MONEY: NavItem[] = [
  { to: "/invoices", label: "Invoices", icon: FileText },
  { to: "/bills", label: "Bills", icon: Receipt },
  { to: "/contacts", label: "Customers & vendors", icon: Users },
];

const SETUP: NavItem[] = [
  { to: "/banking", label: "Bank connections", icon: Building2 },
  { to: "/rules", label: "Rules", icon: Wand2 },
  { to: "/settings", label: "Settings", icon: Settings },
];

export default function Sidebar({ companyName }: { companyName: string }) {
  const inbox = useApi<number>("inbox.count", undefined, { refetchInterval: 30_000 });
  const close = useAction("book.close");
  const { business, household } = useSides();
  // A household-only book's home is its month, so the month needs no second link.
  const householdNav = business ? [MONTH, ...HOUSEHOLD] : HOUSEHOLD;
  const insight: NavItem[] = [
    { to: "/reports", label: "Reports", icon: BarChart3 },
    ...(business ? [{ to: "/taxes", label: "Taxes", icon: Scale }] : []),
    ...SETUP,
  ];
  const item = (n: NavItem) => (
    <NavLink key={n.to} to={n.to} end={n.end} className={({ isActive }) => `nav-item ${isActive ? "active" : ""}`}>
      <n.icon size={18} strokeWidth={2.2} />
      {n.label}
      {n.badge && !!inbox.data && <span className="nav-badge">{inbox.data}</span>}
    </NavLink>
  );
  return (
    <aside className="sidebar no-print">
      <div className="brand">
        <div className="brand-mark">T</div>
        <div style={{ minWidth: 0 }}>
          <div className="brand-name">Tallybook</div>
          <div className="brand-company truncate">{companyName}</div>
        </div>
      </div>
      {MAIN.map(item)}
      {household && (
        <>
          <div className="nav-section">Household</div>
          {householdNav.map(item)}
        </>
      )}
      {business && (
        <>
          <div className="nav-section">{household ? "Business" : "Money in & out"}</div>
          {/* Beside household bills, the business ones need their own name. */}
          {MONEY.map((n) => item(household && n.to === "/bills" ? { ...n, label: "Vendor bills" } : n))}
        </>
      )}
      <div className="nav-section">Insights & setup</div>
      {insight.map(item)}
      <div className="sidebar-foot">
        <button className="nav-item" style={{ border: 0, background: "none", width: "100%", cursor: "pointer" }} onClick={() => close.mutate(undefined)}>
          <LogOut size={18} strokeWidth={2.2} />
          Switch book
        </button>
      </div>
    </aside>
  );
}
