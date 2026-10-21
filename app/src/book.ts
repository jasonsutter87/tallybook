// What kind of book is open: a business, a household, or both.

import { useApi } from "./api";
import type { BookStatus, Scope } from "./types";

export interface BookSides {
  sides: Scope[];
  business: boolean;
  household: boolean;
  /** Keeps both sides, so the UI has to say which side things are on. */
  both: boolean;
  /** The side the book is mostly about. */
  main: Scope;
}

export function useSides(): BookSides {
  const { data } = useApi<BookStatus>("book.status");
  const sides = data?.sides?.length ? data.sides : (["business"] as Scope[]);
  const business = sides.includes("business");
  const household = sides.includes("household");
  return { sides, business, household, both: business && household, main: sides[0] };
}

export const SCOPE_LABEL: Record<Scope, string> = { business: "Business", household: "Household" };
