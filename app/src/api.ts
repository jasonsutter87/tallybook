// The UI's only door to the backend. Every request is `call(method, params)`:
// in the desktop app it goes through Tauri IPC, in a browser it goes to
// tally-devserver via Vite's /api proxy. Same backend either way.

import { invoke } from "@tauri-apps/api/core";
import {
  QueryClient,
  useMutation,
  useQuery,
  useQueryClient,
  type UseQueryOptions,
} from "@tanstack/react-query";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export class ApiError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

export async function call<T = unknown>(method: string, params?: unknown): Promise<T> {
  if (isTauri) {
    try {
      return await invoke<T>("call", { method, params: params ?? null });
    } catch (e) {
      const err = e as { code?: string; message?: string };
      throw new ApiError(err?.code ?? "internal", err?.message ?? String(e));
    }
  }
  const res = await fetch(`/api/${method}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(params ?? null),
  });
  const body = await res.json().catch(() => null);
  if (!res.ok) throw new ApiError(body?.code ?? "http", body?.message ?? res.statusText);
  return body as T;
}

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 5_000, refetchOnWindowFocus: false, retry: false },
  },
});

/** Read data: `useApi<Account[]>("accounts.list")`. */
export function useApi<T>(
  method: string,
  params?: unknown,
  options?: Omit<UseQueryOptions<T, ApiError>, "queryKey" | "queryFn">,
) {
  return useQuery<T, ApiError>({
    queryKey: [method, params ?? null],
    queryFn: () => call<T>(method, params),
    ...options,
  });
}

/**
 * Change data: `const save = useAction<number>("invoices.create")`, then
 * `save.mutateAsync(draft)`. Every successful action refreshes all queries;
 * the books are small and correctness beats cleverness.
 */
export function useAction<T = unknown, P = unknown>(method: string) {
  const qc = useQueryClient();
  return useMutation<T, ApiError, P>({
    mutationFn: (params: P) => call<T>(method, params),
    onSuccess: () => qc.invalidateQueries(),
  });
}

/** Opens a URL in the system browser (desktop) or a new tab (browser). */
export async function openExternal(url: string) {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}
