import { useEffect, useState } from "react"
import { call, type Engine, type Op } from "@/api"
import {
  FullView,
  Metric,
  Spacer,
  StripAction,
  SummaryStrip,
  Tag,
} from "@/components/instrument"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { cn } from "@/lib/utils"

type Candidate = { id: string; name: string; types: string[]; score: number }
type Recon = {
  j: string
  service: string
  identifierSpace: string | null
  schemaSpace: string | null
  m?: Candidate
  e?: string
  c?: Candidate[]
  f?: unknown[]
  judgmentAction?: string
  judgmentBatchSize?: number
}
type ValueGroup = {
  row: number
  value: string
  count: number
  eligible: number
  missing: number
  typeCounts: [string, number][]
  variantTotal: number
  bulkSafe: boolean
  bulkReason: string | null
}
type Variant = { row: number; count: number; recon: Recon | null }
type Detail = ValueGroup & {
  variantStart: number
  variants: Variant[]
  selected: Variant & { candidateTotal: number; candidateStart: number }
}
type Results = {
  total: number
  rows: number
  start: number
  limit: number
  values: ValueGroup[]
  detail: Detail | null
}
const PAGE = 20
const message = (e: unknown) => String((e as Error).message ?? e)
const label = (g: ValueGroup) =>
  g.typeCounts.every(([t]) => t === "blank")
    ? "(blank)"
    : g.value === ""
      ? "(empty text)"
      : g.value

/** Judge saved reconciliation answers by matching text, without asking the service again. */
export function ReconciliationValues({
  column,
  engine,
  version,
  onApply,
  onClose,
  onReconcile,
}: {
  column: string
  engine: Engine
  version: string
  onApply: (op: Op) => Promise<void>
  onClose: () => void
  onReconcile: () => void
}) {
  const [start, setStart] = useState(0)
  const [selected, setSelected] = useState<number | null>(null)
  const [variantStart, setVariantStart] = useState(0)
  const [variantRow, setVariantRow] = useState<number | null>(null)
  const [candidateStart, setCandidateStart] = useState(0)
  const [refresh, setRefresh] = useState(0)
  const [result, setResult] = useState<{ key: string; data: Results } | null>(
    null
  )
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const scope = JSON.stringify(engine)
  const requestKey = JSON.stringify([
    column,
    scope,
    version,
    start,
    selected,
    variantStart,
    variantRow,
    candidateStart,
    refresh,
  ])
  const data = result?.key === requestKey ? result.data : null
  useEffect(() => {
    setStart(0)
    setSelected(null)
    setVariantStart(0)
    setVariantRow(null)
    setCandidateStart(0)
  }, [column, scope])
  useEffect(() => {
    let live = true
    setResult(null)
    setError("")
    call<Results>(
      "recon_values",
      {
        column,
        engine,
        start,
        limit: PAGE,
        params: {
          ...(selected === null ? {} : { row: selected }),
          variantStart,
          ...(variantRow === null ? {} : { variantRow }),
          candidateStart,
        },
      },
      { quiet: true }
    ).then(
      ({ data }) => {
        if (live) setResult({ key: requestKey, data })
      },
      (e) => {
        if (live) setError(message(e))
      }
    )
    return () => {
      live = false
    }
  }, [
    column,
    engine,
    version,
    start,
    selected,
    variantStart,
    variantRow,
    candidateStart,
    refresh,
    requestKey,
  ])
  const detail = data?.detail
  const chosen = detail?.selected
  const recon = chosen?.recon
  const choose = (row: number) => {
    setSelected(row)
    setVariantStart(0)
    setVariantRow(null)
    setCandidateStart(0)
  }
  const judge = async (candidate: Candidate, all: boolean) => {
    if (!detail || !chosen || busy || (all && !detail.bulkSafe)) return
    setBusy(true)
    setError("")
    try {
      await onApply(
        all
          ? {
              op: "core/recon-judge-similar-cells",
              engineConfig: engine,
              columnName: column,
              similarValue: detail.value,
              judgment: "matched",
              match: candidate,
            }
          : {
              op: "ox/recon-judge-one-cell",
              row: chosen.row,
              columnName: column,
              judgment: "matched",
              match: candidate,
            }
      )
      setRefresh((n) => n + 1)
    } catch (e) {
      setError(message(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <FullView
      crumbs={["Reconciliation results", <Tag key="column">{column}</Tag>]}
      onClose={onClose}
    >
      <SummaryStrip>
        <Metric label="matching values">
          {data?.total.toLocaleString() ?? "…"}
        </Metric>
        <Metric label="selected rows">
          {data?.rows.toLocaleString() ?? "…"}
        </Metric>
        <Spacer />
        <StripAction disabled={busy} onClick={onReconcile}>
          Reconcile again…
        </StripAction>
      </SummaryStrip>
      {error && (
        <p role="alert" className="border-b px-5 py-3 text-destructive">
          {error}
        </p>
      )}
      {!data && !error && (
        <p role="status" className="flex items-center gap-2 px-5 py-4">
          <Spinner />
          Loading saved results…
        </p>
      )}
      {data && data.total === 0 && (
        <p className="px-5 py-4 text-muted-foreground">
          No values match the current facets.
        </p>
      )}
      {data && data.total > 0 && (
        <div className="flex min-h-0 flex-1 flex-col lg:flex-row">
          <section
            aria-label="Values to reconcile"
            className="flex min-h-60 min-w-0 flex-1 flex-col border-b lg:w-2/5 lg:flex-none lg:border-r lg:border-b-0"
          >
            <div className="min-h-0 flex-1 overflow-y-auto">
              <table className="w-full table-fixed border-collapse text-left">
                <thead className="sticky top-0 bg-background">
                  <tr className="border-b border-b-foreground text-xs text-muted-foreground">
                    <th className="px-5 py-2 font-normal">Value</th>
                    <th className="w-20 px-2 py-2 text-right font-normal">
                      Cells
                    </th>
                    <th className="w-24 px-3 py-2 text-right font-normal">
                      Results
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {data.values.map((g) => (
                    <tr
                      key={g.row}
                      className={cn(
                        "border-b border-row-rule",
                        detail?.row === g.row && "bg-secondary"
                      )}
                    >
                      <td className="min-w-0 p-0">
                        <button
                          type="button"
                          aria-pressed={detail?.row === g.row}
                          className="flex w-full min-w-0 flex-col gap-1 px-5 py-3 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                          onClick={() => choose(g.row)}
                        >
                          <span className="break-all whitespace-pre-wrap">
                            {label(g)}
                          </span>
                          <span className="text-xs text-muted-foreground">
                            {g.typeCounts
                              .map(([t, n]) => `${t} × ${n.toLocaleString()}`)
                              .join(" · ")}
                          </span>
                        </button>
                      </td>
                      <td className="px-2 py-3 text-right font-mono text-xs">
                        {g.count.toLocaleString()}
                      </td>
                      <td className="px-3 py-3 text-right text-xs">
                        {g.missing === g.count
                          ? "not reconciled"
                          : `${g.variantTotal} state${g.variantTotal === 1 ? "" : "s"}`}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <nav
              aria-label="Value pages"
              className="flex shrink-0 items-center gap-3 border-t px-4 py-3 text-xs"
            >
              <Button
                variant="outline"
                size="sm"
                disabled={data.start === 0}
                onClick={() => {
                  setStart(Math.max(0, data.start - PAGE))
                  setSelected(null)
                  setVariantStart(0)
                  setVariantRow(null)
                  setCandidateStart(0)
                }}
              >
                Previous
              </Button>
              <span className="flex-1 text-center">
                {data.start + 1}–{Math.min(data.start + data.limit, data.total)}{" "}
                of {data.total.toLocaleString()}
              </span>
              <Button
                variant="outline"
                size="sm"
                disabled={data.start + data.limit >= data.total}
                onClick={() => {
                  setStart(data.start + PAGE)
                  setSelected(null)
                  setVariantStart(0)
                  setVariantRow(null)
                  setCandidateStart(0)
                }}
              >
                Next
              </Button>
            </nav>
          </section>
          {detail && chosen && (
            <section
              aria-label="Candidates for the selected value"
              className="min-h-0 min-w-0 flex-1 overflow-y-auto"
            >
              <div className="space-y-2 border-b px-5 py-4">
                <h2 className="font-semibold break-all">{label(detail)}</h2>
                <p className="text-xs text-muted-foreground">
                  Match all uses the same text in the current filtered rows,
                  including different cell types shown on the left.{" "}
                  {engine.mode === "record-based" &&
                    "All rows of matching records are included."}
                </p>
                {detail.bulkReason && (
                  <p className="text-xs text-muted-foreground">
                    {detail.bulkReason}
                  </p>
                )}
              </div>
              <div className="space-y-2 border-b px-5 py-3">
                <label className="flex flex-wrap items-center gap-2 text-xs">
                  <span>
                    {detail.variantTotal} saved result state
                    {detail.variantTotal === 1 ? "" : "s"}
                  </span>
                  <select
                    aria-label="Saved result state"
                    value={chosen.row}
                    onChange={(e) => {
                      setVariantRow(Number(e.target.value))
                      setCandidateStart(0)
                    }}
                    className="max-w-full min-w-0 rounded-[3px] border border-input bg-background px-2 py-1"
                  >
                    {detail.variants.map((v) => (
                      <option key={v.row} value={v.row}>
                        Row {v.row + 1} · {v.count} cells ·{" "}
                        {v.recon?.j ?? "not reconciled"}
                      </option>
                    ))}
                  </select>
                </label>
                {detail.variantTotal > PAGE && (
                  <div className="flex items-center gap-2 text-xs">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={detail.variantStart === 0}
                      onClick={() => {
                        setVariantStart(Math.max(0, detail.variantStart - PAGE))
                        setVariantRow(null)
                        setCandidateStart(0)
                      }}
                    >
                      Previous states
                    </Button>
                    <span>
                      {detail.variantStart + 1}–
                      {Math.min(
                        detail.variantStart + PAGE,
                        detail.variantTotal
                      )}{" "}
                      of {detail.variantTotal}
                    </span>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={
                        detail.variantStart + PAGE >= detail.variantTotal
                      }
                      onClick={() => {
                        setVariantStart(detail.variantStart + PAGE)
                        setVariantRow(null)
                        setCandidateStart(0)
                      }}
                    >
                      Next states
                    </Button>
                  </div>
                )}
                <p className="text-xs text-muted-foreground">
                  Match cell changes only row {chosen.row + 1}, the
                  representative of this state ({chosen.count.toLocaleString()}{" "}
                  cells).
                </p>
                {recon && (
                  <>
                    <p className="text-xs break-all text-muted-foreground">
                      Service: {recon.service}
                    </p>
                    {recon.identifierSpace && (
                      <p className="text-xs break-all text-muted-foreground">
                        Identifier space: {recon.identifierSpace}
                      </p>
                    )}
                    {recon.schemaSpace && (
                      <p className="text-xs break-all text-muted-foreground">
                        Schema space: {recon.schemaSpace}
                      </p>
                    )}
                    <p className="text-xs text-muted-foreground">
                      Judgment: {recon.j}
                      {recon.judgmentAction && ` · ${recon.judgmentAction}`}
                      {recon.judgmentBatchSize !== undefined &&
                        ` · batch ${recon.judgmentBatchSize}`}
                    </p>
                    {recon.m && (
                      <p className="text-sm">
                        Matched to {recon.m.name} ({recon.m.id})
                      </p>
                    )}
                    {recon.e && (
                      <p className="text-sm text-destructive">{recon.e}</p>
                    )}
                    {recon.f?.some((f) => f !== null) && (
                      <p className="text-xs text-muted-foreground">
                        {[
                          "Type match",
                          "Exact name",
                          "Name edit distance",
                          "Name word distance",
                        ]
                          .map((name, i) =>
                            recon.f?.[i] === null
                              ? null
                              : `${name}: ${String(recon.f?.[i])}`
                          )
                          .filter(Boolean)
                          .join(" · ")}
                      </p>
                    )}
                  </>
                )}
              </div>
              {!recon && (
                <p className="px-5 py-4 text-muted-foreground">
                  These cells have not been reconciled. Reconcile again to ask a
                  service for candidates.
                </p>
              )}
              {recon && chosen.candidateTotal === 0 && (
                <p className="px-5 py-4 text-muted-foreground">
                  No saved candidates for this result state.
                </p>
              )}
              {recon &&
                (recon.c ?? []).map((c, i) => (
                  <div
                    key={`${c.id}:${i}`}
                    className="flex flex-wrap items-center gap-3 border-b border-row-rule px-5 py-3"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="font-medium break-words">{c.name}</p>
                      <p className="text-xs break-all text-muted-foreground">
                        {c.id} · score {c.score}
                        {c.types.length > 0 && ` · ${c.types.join(", ")}`}
                      </p>
                    </div>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy || detail.eligible === 0}
                      aria-label={`Match ${c.name} to cell in row ${chosen.row + 1}`}
                      onClick={() => judge(c, false)}
                    >
                      Match cell
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy || !detail.bulkSafe}
                      title={detail.bulkReason ?? undefined}
                      aria-label={`Match ${c.name} to all ${detail.eligible} matching cells`}
                      onClick={() => judge(c, true)}
                    >
                      Match all {detail.eligible.toLocaleString()}
                    </Button>
                  </div>
                ))}
              {chosen.candidateTotal > PAGE && (
                <nav
                  aria-label="Candidate pages"
                  className="flex items-center gap-3 px-5 py-3 text-xs"
                >
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={chosen.candidateStart === 0}
                    onClick={() =>
                      setCandidateStart(
                        Math.max(0, chosen.candidateStart - PAGE)
                      )
                    }
                  >
                    Previous candidates
                  </Button>
                  <span>
                    {chosen.candidateStart + 1}–
                    {Math.min(
                      chosen.candidateStart + PAGE,
                      chosen.candidateTotal
                    )}{" "}
                    of {chosen.candidateTotal}
                  </span>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={
                      chosen.candidateStart + PAGE >= chosen.candidateTotal
                    }
                    onClick={() =>
                      setCandidateStart(chosen.candidateStart + PAGE)
                    }
                  >
                    Next candidates
                  </Button>
                </nav>
              )}
            </section>
          )}
        </div>
      )}
    </FullView>
  )
}
