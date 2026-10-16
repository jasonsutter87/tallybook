// Small pieces shared by the Overview, Schedule C and export tabs.

import { CalendarX2 } from "lucide-react";
import type { ApiError } from "../../api";
import { Card, EmptyState, ErrorBanner } from "../../components/ui";

/** First error among several queries, or null. */
export function firstError(...errors: (ApiError | null | undefined)[]): ApiError | null {
  return errors.find((e) => e) ?? null;
}

/** Friendly screen for errors; `unsupported_year` gets a calm explanation instead of a red banner. */
export function TaxError({ error, year }: { error: ApiError; year: number }) {
  if (error.code === "unsupported_year") {
    return (
      <Card>
        <EmptyState
          icon={<CalendarX2 size={24} />}
          title={`No verified tax tables for ${year} yet`}
          body={
            <>
              Tallybook only prepares figures for years whose IRS numbers (brackets, deductions, thresholds) have been checked by hand. Rather
              than guess, it leaves {year} alone. Pick a supported year from the menu above.
            </>
          }
        />
      </Card>
    );
  }
  return <ErrorBanner error={error} />;
}
