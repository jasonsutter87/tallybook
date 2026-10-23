// Delete book card: opens the confirmation flow.

import { Trash2 } from "lucide-react";
import { useState } from "react";
import { useApi } from "../../api";
import type { BookStatus } from "../../types";
import DeleteBookModal from "../../flows/DeleteBookModal";
import { SettingsCard } from "./shared";

export default function DeleteCard() {
  const status = useApi<BookStatus>("book.status");
  const [open, setOpen] = useState(false);
  const path = status.data?.path;
  const name = status.data?.company_name ?? "";
  return (
    <SettingsCard
      id="delete"
      icon={<Trash2 size={18} />}
      title="Delete this book"
      desc="Closes the book, disconnects its banks, removes their keys from your keychain, and moves the file to the Trash."
    >
      <button className="btn btn-danger" disabled={!path} onClick={() => setOpen(true)}>
        <Trash2 size={16} /> Delete {name || "book"}…
      </button>
      {path && <DeleteBookModal open={open} onClose={() => setOpen(false)} path={path} name={name} />}
    </SettingsCard>
  );
}
