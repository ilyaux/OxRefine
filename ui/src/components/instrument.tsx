/* The "Instrument" parts every screen is built from (design/README.md): bars made of full-height cells
   divided by 1 px rules, one ink cell for the main action, mono numbers, a status bar, metric cells,
   and full-window views with a breadcrumb that close on Esc. */
import { createContext, Fragment, useContext, useEffect, type ComponentProps, type ReactNode } from "react"
import { cn } from "@/lib/utils"

/** The logo: three stacked bars, widest first. */
export function Logo() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" className="shrink-0">
      <rect x="1" y="2" width="14" height="2.4" fill="currentColor" />
      <rect x="3.5" y="6.8" width="9" height="2.4" fill="currentColor" />
      <rect x="6" y="11.6" width="4" height="2.4" fill="currentColor" />
    </svg>
  )
}

/** A 44 px bar of cells (the top bar, a toolbar). */
export function Bar({ className, ...props }: ComponentProps<"header">) {
  return <header className={cn("flex h-11 shrink-0 items-stretch border-b bg-background", className)} {...props} />
}

/** A static cell: text with a rule on its right. */
export function BarCell({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex min-w-0 items-center gap-2.5 border-r px-4", className)} {...props} />
}

export const Spacer = () => <span className="flex-1" />

type CellProps = ComponentProps<"button"> & {
  /** the key that does the same */
  keys?: string
  /** the screen's one main action: an ink cell */
  ink?: boolean
  /** which side carries the rule: cells after the spacer have it on the left */
  rule?: "left" | "right"
  active?: boolean
}

/** A full-height button cell. */
export function CellButton({ keys, ink, rule = "right", active, className, children, type = "button", ...props }: CellProps) {
  return (
    <button
      type={type}
      className={cn(
        "flex shrink-0 items-center gap-2 px-4 whitespace-nowrap outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset disabled:pointer-events-none disabled:opacity-50",
        rule === "left" ? "border-l" : "border-r",
        ink ? "bg-primary text-primary-foreground hover:bg-primary/90" : active ? "bg-secondary" : "hover:bg-accent",
        className,
      )}
      aria-pressed={active}
      {...props}
    >
      {children}
      {keys && <kbd className={cn(ink && "text-primary-foreground/70")}>{keys}</kbd>}
    </button>
  )
}

/** A full-height link cell (Docs, Coming from OpenRefine). */
export function CellLink({ className, rule = "left", ...props }: ComponentProps<"a"> & { rule?: "left" | "right" }) {
  return (
    <a
      className={cn("flex items-center px-4 text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset", rule === "left" ? "border-l" : "border-r", className)}
      target="_blank"
      rel="noreferrer"
      {...props}
    />
  )
}

/** The logo cell that starts every top bar. */
export function BrandCell({ version, onClick }: { version?: string; onClick?: () => void }) {
  const inner = (
    <>
      <Logo />
      <span className="font-semibold tracking-[-0.01em]">OxRefine</span>
      {version && <span className="font-mono text-[11px] text-muted-foreground">{version}</span>}
    </>
  )
  return onClick ? (
    <button type="button" onClick={onClick} aria-label="OxRefine: back to projects" className="flex items-center gap-2.5 border-r px-4 outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset">
      {inner}
    </button>
  ) : (
    <div className="flex items-center gap-2.5 border-r px-4">{inner}</div>
  )
}

/** `a / b / c` with faint separators; the last part is the current place. */
export function Crumbs({ parts }: { parts: ReactNode[] }) {
  return (
    <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-2 border-r px-4">
      {parts.map((p, i) => (
        <Fragment key={i}>
          {i > 0 && (
            <span aria-hidden="true" className="text-faint">
              /
            </span>
          )}
          <span className={cn("flex min-w-0 items-center gap-2 truncate", i === parts.length - 1 ? "font-medium" : "text-muted-foreground")}>{p}</span>
        </Fragment>
      ))}
    </nav>
  )
}

/** A small mono tag, like the column a view works on. */
export function Tag({ className, ...props }: ComponentProps<"span">) {
  return <span className={cn("rounded-[3px] border border-input px-1.5 font-mono text-[11.5px] leading-5 text-ink-2", className)} {...props} />
}

/** The 28 px status bar of mono cells. */
export function StatusBar({ className, ...props }: ComponentProps<"footer">) {
  return <footer className={cn("flex h-7 shrink-0 items-stretch border-t bg-muted font-mono text-[11.5px] text-muted-foreground", className)} {...props} />
}

export function StatusCell({ className, rule = "right", ...props }: ComponentProps<"div"> & { rule?: "left" | "right" }) {
  return <div className={cn("flex min-w-0 items-center gap-1.5 px-3 whitespace-nowrap", rule === "left" ? "border-l" : "border-r", className)} {...props} />
}

/** The green "ready" square, or amber while something runs. */
export function StateDot({ busy }: { busy?: boolean }) {
  return <span aria-hidden="true" className={cn("size-1.5 shrink-0", busy ? "bg-warn" : "bg-ok")} />
}

/** A metric cell in a summary strip: a muted label over a large mono value. */
export function Metric({ label, children, className }: { label: string; children: ReactNode; className?: string }) {
  return (
    <div className={cn("flex min-w-40 flex-col justify-center gap-1 border-r px-5", className)}>
      <span className="text-[11.5px] text-muted-foreground">{label}</span>
      <span className="font-mono text-[22px] leading-7 font-medium">{children}</span>
    </div>
  )
}

/** A row of metric cells with action cells on the right. */
export function SummaryStrip({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex min-h-19 shrink-0 items-stretch overflow-x-auto border-b", className)} {...props} />
}

/** A big action cell for a summary strip. */
export function StripAction({ className, ...props }: CellProps) {
  return <CellButton rule="left" className={cn("px-6", className)} {...props} />
}

/** The open project's name, which every full view's breadcrumb starts with. */
export const ProjectName = createContext<string | null>(null)

/** A full-window view that replaces a dialog: the top bar with the breadcrumb and a Close cell, Esc closes. */
export function FullView({
  crumbs,
  root,
  closeLabel = "Close",
  onClose,
  bar,
  children,
  className,
}: {
  crumbs: ReactNode[]
  /** what the breadcrumb starts with instead of the open project's name */
  root?: ReactNode
  closeLabel?: string
  onClose: () => void
  bar?: ReactNode
  children: ReactNode
  className?: string
}) {
  const open = useContext(ProjectName)
  const project = root === undefined ? open : root
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Esc closes the view unless a menu or popup inside it is open and takes the key first
      if (e.key === "Escape" && !e.defaultPrevented && !document.querySelector("[data-open][role=menu],[data-open][role=listbox],[role=dialog][data-open]")) onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])
  return (
    <div role="region" aria-label={typeof crumbs[0] === "string" ? crumbs[0] : undefined} className={cn("fixed inset-0 z-40 flex flex-col bg-background text-foreground", className)}>
      <Bar>
        <BrandCell />
        <Crumbs parts={project ? [project, ...crumbs] : crumbs} />
        <Spacer />
        {bar}
        <CellButton rule="left" keys="Esc" onClick={onClose} className="text-muted-foreground">
          {closeLabel}
        </CellButton>
      </Bar>
      {children}
    </div>
  )
}

/** A label row in a side rail ("Method", "Keying function"). */
export function RailLabel({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("px-3.5 pt-4 pb-2 text-[11.5px] text-muted-foreground", className)} {...props} />
}
