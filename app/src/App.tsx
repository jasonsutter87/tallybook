import { lazy, Suspense } from "react";
import { Route, Routes } from "react-router-dom";
import { useApi } from "./api";
import Sidebar from "./components/Sidebar";
import UpdateBanner from "./components/UpdateBanner";
import AutoCloudCopy from "./components/AutoCloudCopy";
import { Loading } from "./components/ui";
import Dashboard from "./pages/Dashboard";
import Welcome from "./pages/Welcome";
import type { BookStatus } from "./types";

const Review = lazy(() => import("./pages/Review"));
const Banking = lazy(() => import("./pages/Banking"));
const Accounts = lazy(() => import("./pages/Accounts"));
const Register = lazy(() => import("./pages/Register"));
const Reconcile = lazy(() => import("./pages/Reconcile"));
const Invoices = lazy(() => import("./pages/Invoices"));
const InvoiceEditor = lazy(() => import("./pages/InvoiceEditor"));
const InvoiceView = lazy(() => import("./pages/InvoiceView"));
const Bills = lazy(() => import("./pages/Bills"));
const BillEditor = lazy(() => import("./pages/BillEditor"));
const BillView = lazy(() => import("./pages/BillView"));
const Contacts = lazy(() => import("./pages/Contacts"));
const Reports = lazy(() => import("./pages/Reports"));
const Settings = lazy(() => import("./pages/Settings"));
const Rules = lazy(() => import("./pages/Rules"));
const Taxes = lazy(() => import("./pages/Taxes"));

export default function App() {
  const status = useApi<BookStatus>("book.status");
  if (status.isLoading) return <Loading label="Opening Tallybook…" />;
  if (!status.data?.open) return <Welcome />;
  return (
    <div className="shell">
      <Sidebar companyName={status.data.company_name ?? "My Business"} />
      <main className="main">
        <Suspense fallback={<Loading />}>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/review" element={<Review />} />
            <Route path="/banking" element={<Banking />} />
            <Route path="/accounts" element={<Accounts />} />
            <Route path="/accounts/:id" element={<Register />} />
            <Route path="/accounts/:id/reconcile" element={<Reconcile />} />
            <Route path="/invoices" element={<Invoices />} />
            <Route path="/invoices/new" element={<InvoiceEditor />} />
            <Route path="/invoices/:id" element={<InvoiceView />} />
            <Route path="/invoices/:id/edit" element={<InvoiceEditor />} />
            <Route path="/bills" element={<Bills />} />
            <Route path="/bills/new" element={<BillEditor />} />
            <Route path="/bills/:id" element={<BillView />} />
            <Route path="/contacts" element={<Contacts />} />
            <Route path="/reports" element={<Reports />} />
            <Route path="/reports/:report" element={<Reports />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/rules" element={<Rules />} />
            <Route path="/taxes" element={<Taxes />} />
            <Route path="/taxes/:tab" element={<Taxes />} />
            <Route path="*" element={<Dashboard />} />
          </Routes>
        </Suspense>
      </main>
      <UpdateBanner />
      <AutoCloudCopy key={status.data.path ?? ""} />
    </div>
  );
}
