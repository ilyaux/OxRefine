import { isTauri } from "@/api"
import { toast } from "@/components/ui/toast"

/** Desktop app: look for a newer release on GitHub and offer to install it. Quiet when offline or
 * already up to date. */
export async function checkForUpdates() {
  if (!isTauri) return
  try {
    const { check } = await import("@tauri-apps/plugin-updater")
    const update = await check()
    if (!update) return
    toast.add({
      title: `OxRefine ${update.version} is available`,
      description: "It installs in the background; the app restarts when it's done.",
      timeout: 0,
      actionProps: {
        children: "Install and restart",
        onClick: async () => {
          toast.add({ title: `Downloading OxRefine ${update.version}…`, timeout: 0 })
          try {
            await update.downloadAndInstall()
            const { relaunch } = await import("@tauri-apps/plugin-process")
            await relaunch()
          } catch (e) {
            toast.add({ title: "The update didn't install", description: String((e as Error).message ?? e), type: "error" })
          }
        },
      },
    })
  } catch {
    // no network, or no release to compare with
  }
}
