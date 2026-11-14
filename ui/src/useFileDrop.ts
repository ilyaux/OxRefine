import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { isTauri } from "@/api"
import { listenForFileDrops, type DroppedSource, type NativeDrop } from "@/fileDrop"

export function useFileDrop({ enabled, onDrop, onError }: {
  enabled: boolean
  onDrop: (source: DroppedSource, current: () => boolean) => Promise<void>
  onError: (error: unknown) => void
}) {
  const [dragging, setDragging] = useState(false)
  const [busy, setBusy] = useState(false)
  const latest = useRef({ enabled, onDrop, onError })
  useLayoutEffect(() => { latest.current = { enabled, onDrop, onError } }, [enabled, onDrop, onError])
  const controller = useRef<ReturnType<typeof listenForFileDrops> | null>(null)
  const cancel = useCallback(() => controller.current?.cancel(), [])
  useEffect(() => {
    const connected = listenForFileDrops({
      surface: window,
      // Toasts also use dialog roles; only our actual dialog popups block importing.
      canDrop: () => latest.current.enabled && !document.querySelector('[data-slot="dialog-content"]:not([hidden]), [data-slot="alert-dialog-content"]:not([hidden])'),
      onDrop: (source, current) => latest.current.onDrop(source, current),
      onError: (error) => latest.current.onError(error),
      onDragging: setDragging,
      onBusy: setBusy,
      ...(isTauri ? { listenNative: (handler: (payload: NativeDrop) => void) => getCurrentWebview().onDragDropEvent((event) => handler(event.payload)) } : {}),
    })
    controller.current = connected
    return () => { controller.current = null; connected.dispose() }
  }, [])
  useEffect(() => { if (!enabled) cancel() }, [enabled, cancel])
  return { dragging, busy, cancel }
}
