import { Undo2 } from "lucide-react";
import { useState } from "react";
import { useAction } from "../api";
import { money } from "../format";
import { ConfirmModal, useToast } from "./ui";

/** Small "Void" action for a recorded payment (invoice or bill payment history). */
export default function VoidPaymentButton({ paymentId, amount }: { paymentId: number; amount: number }) {
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const voidPayment = useAction("payments.void");
  return (
    <>
      <button className="btn btn-ghost btn-sm no-print" onClick={() => setOpen(true)} title="Void this payment">
        <Undo2 size={13} /> Void
      </button>
      <ConfirmModal
        open={open}
        onClose={() => setOpen(false)}
        title="Void this payment?"
        body={`The ${money(amount)} payment will be reversed and the balance it paid becomes due again. If it was split across several documents, all of them reopen. History is kept.`}
        confirmLabel="Void payment"
        danger
        onConfirm={async () => {
          try {
            await voidPayment.mutateAsync({ id: paymentId });
            toast({ text: "Payment voided" });
          } catch (e) {
            toast.error(e);
          }
        }}
      />
    </>
  );
}
