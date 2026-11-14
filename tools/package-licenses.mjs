// Share the desktop resource inventory with the CLI archives; copy text files only.
import { copyFileSync, mkdirSync, readFileSync, realpathSync } from "node:fs"
import { dirname, isAbsolute, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const ROOT = resolve(import.meta.dirname, "..")
const inside = (root, path) => {
  const rel = relative(root, path)
  return rel !== "" && !isAbsolute(rel) && rel !== ".." && !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`)
}

export function packageLicenses(target, root = ROOT, resources) {
  const configDir = resolve(root, "ui/src-tauri")
  resources ??= JSON.parse(readFileSync(resolve(configDir, "tauri.conf.json"), "utf8")).bundle.resources
  if (!resources || Array.isArray(resources) || typeof resources !== "object") throw new Error("Expected an explicit license resource map")
  const output = resolve(target)
  const repo = realpathSync(root)
  const seen = new Set()
  const files = Object.entries(resources).filter(([, dest]) => dest === "LICENSE" || dest === "NOTICE" || dest.startsWith("licenses/"))
    .map(([source, dest]) => {
      const input = realpathSync(resolve(configDir, source))
      const destination = resolve(output, dest)
      if (!inside(repo, input) || !inside(output, destination)) throw new Error("License resource path leaves its repository or package")
      const key = relative(output, destination).replaceAll("\\", "/").toLowerCase()
      if (seen.has(key)) throw new Error(`Duplicate license destination: ${dest}`)
      seen.add(key)
      const data = readFileSync(input)
      if (!data.length) throw new Error(`Empty license resource: ${source}`)
      return { input, destination, dest, data }
    })
  if (!seen.has("license") || !seen.has("notice")) throw new Error("Package license inventory must include LICENSE and NOTICE")
  for (const { input, destination, data } of files) {
    mkdirSync(dirname(destination), { recursive: true })
    copyFileSync(input, destination)
    if (!readFileSync(destination).equals(data)) throw new Error(`Packaged license differs from its source: ${destination}`)
  }
  return files.map(({ dest }) => dest)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (!process.argv[2]) throw new Error("usage: node tools/package-licenses.mjs <package-directory>")
    const files = packageLicenses(process.argv[2])
    console.log(`Packaged and byte-verified ${files.length} license/credit files:\n${files.join("\n")}`)
  } catch (error) { console.error(error.message); process.exitCode = 1 }
}
