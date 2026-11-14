export type DroppedSource = { file: File } | { path: string }
export type NativeDrop = { type: "enter" | "drop"; paths: string[] } | { type: "over" | "leave" }
type Unlisten = () => void | Promise<void>

const fileDrag = (data: DataTransfer | null) => !!data && (Array.from(data.types).includes("Files") || Array.from(data.items ?? []).some((item) => item.kind === "file"))

function browserSource(data: DataTransfer): DroppedSource {
  const items = Array.from(data.items ?? []).filter((item) => item.kind === "file")
  if (items.some((item) => item.webkitGetAsEntry?.()?.isDirectory)) throw new Error("Folders cannot be uploaded by dropping them here. Zip the folder or use a folder path on the server.")
  const files = Array.from(data.files)
  if (files.length !== 1) throw new Error("Drop one file at a time. Nothing was imported.")
  if (files[0].webkitRelativePath) throw new Error("Drop a file rather than a folder, or zip the folder first.")
  return { file: files[0] }
}

/** File-only window protection also prevents navigation when a project or dialog blocks importing. */
export function listenForFileDrops({ surface, canDrop, onDrop, onError, onDragging, onBusy, listenNative }: {
  surface: Window
  canDrop: () => boolean
  onDrop: (source: DroppedSource, current: () => boolean) => Promise<void>
  onError: (error: unknown) => void
  onDragging: (dragging: boolean) => void
  onBusy: (busy: boolean) => void
  listenNative?: (handler: (payload: NativeDrop) => void) => Promise<Unlisten>
}) {
  let live = true, request = 0, busy = false, depth = 0
  let unlisten: Unlisten | undefined
  const cancel = () => { request++; busy = false; depth = 0; onDragging(false); onBusy(false) }
  const accept = async (source: () => DroppedSource) => {
    onDragging(false); depth = 0
    if (!canDrop()) return onError(new Error("Go to Projects and close any dialogs before dropping a file."))
    if (busy) return onError(new Error("A file is already being prepared. Wait for its preview or cancel it first."))
    let value: DroppedSource
    try { value = source() } catch (error) { onError(error); return }
    const ticket = ++request
    busy = true; onBusy(true)
    const current = () => live && request === ticket && canDrop()
    try { await onDrop(value, current) }
    catch (error) { if (current()) onError(error) }
    finally { if (live && request === ticket) { busy = false; onBusy(false) } }
  }
  const dragEnter = (event: DragEvent) => {
    if (!fileDrag(event.dataTransfer)) return
    event.preventDefault()
    depth++; onDragging(canDrop() && !busy)
  }
  const dragOver = (event: DragEvent) => {
    if (!fileDrag(event.dataTransfer)) return
    event.preventDefault()
    if (event.dataTransfer) event.dataTransfer.dropEffect = canDrop() && !busy ? "copy" : "none"
  }
  const dragLeave = (event: DragEvent) => {
    if (!fileDrag(event.dataTransfer)) return
    depth = Math.max(0, depth - 1)
    if (!depth || !event.relatedTarget) onDragging(false)
  }
  const drop = (event: DragEvent) => {
    if (!fileDrag(event.dataTransfer)) return
    event.preventDefault()
    if (event.dataTransfer) void accept(() => browserSource(event.dataTransfer!))
  }
  const release = (off: Unlisten) => { void Promise.resolve().then(off).catch((error) => { if (live) onError(error) }) }
  if (listenNative) {
    void listenNative((payload) => {
      if (!live) return
      if (payload.type === "leave") { depth = 0; onDragging(false) }
      else if (payload.type === "drop") void accept(() => {
        if (payload.paths.length !== 1 || !payload.paths[0].trim()) throw new Error("Drop one file or folder at a time. Nothing was imported.")
        return { path: payload.paths[0] }
      })
      else onDragging(canDrop() && !busy)
    }).then((off) => { if (live) unlisten = off; else release(off) }).catch((error) => { if (live) onError(error) })
  } else {
    surface.addEventListener("dragenter", dragEnter)
    surface.addEventListener("dragover", dragOver)
    surface.addEventListener("dragleave", dragLeave)
    surface.addEventListener("drop", drop)
  }
  return {
    cancel,
    dispose() {
      live = false; request++
      surface.removeEventListener("dragenter", dragEnter)
      surface.removeEventListener("dragover", dragOver)
      surface.removeEventListener("dragleave", dragLeave)
      surface.removeEventListener("drop", drop)
      if (unlisten) release(unlisten)
    },
  }
}

type StagedDrop = { path: string; name?: string; origin?: string }
/** Uploads and inspection do not change the project; confirmation or the wizard owns creation. */
export async function prepareDroppedImport(source: DroppedSource, current: () => boolean, actions: {
  upload: (file: File) => Promise<{ path: string; name: string }>
  inspect: (path: string) => Promise<{ format: string }>
  stage: (source: StagedDrop) => void
  confirmArchive: (source: StagedDrop) => void
}) {
  const uploaded = "file" in source ? await actions.upload(source.file) : { path: source.path, name: source.path.split(/[\\/]/).pop() ?? source.path }
  if (!current()) return
  const info = await actions.inspect(uploaded.path)
  if (!current()) return
  const staged = { ...uploaded, ...("file" in source ? { origin: source.file.name } : {}) }
  if (info.format === "openrefine-project") actions.confirmArchive(staged)
  else actions.stage(staged)
}
