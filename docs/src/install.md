# Install

## Desktop app

Download the installer for your system from the [latest release](https://github.com/ilyaux/OxRefine/releases/latest):

| System | File |
|---|---|
| Windows | `OxRefine_<version>_x64-setup.exe` installs for your user only and needs no administrator rights; there is also an `.msi` |
| macOS | `OxRefine_<version>_universal.dmg`, for both Apple silicon and Intel |
| Linux | `OxRefine_<version>_amd64.AppImage` runs anywhere; there are also `.deb` and `.rpm` packages |

The installers aren't code-signed yet, so your system may warn you the first time:

- **Windows** shows a SmartScreen notice. Choose *More info* → *Run anyway*.
- **macOS** says the app is from an unidentified developer. Right-click the app, choose *Open*, then confirm.
- **Linux AppImage:** make the file executable with `chmod +x`, then run it.

### Updates

When the app starts, it checks for a newer release. If one exists, it offers to install it and restarts when that's done. Updates are signed, and the app refuses any update that isn't.

### Where projects are kept

Projects are saved in a folder in your user profile, so they survive restarts and updates:

- **Windows:** `%APPDATA%\OxRefine\projects`
- **macOS:** `~/Library/Application Support/OxRefine/projects`
- **Linux:** `~/.local/share/oxrefine/projects` (or under `$XDG_DATA_HOME`)

Set the `OXREFINE_HOME` environment variable to keep them somewhere else.

## Command line

The release also has an `ox` archive for each system. It contains a single program:

- `ox apply` replays an OpenRefine or OxRefine recipe on a file, with no window;
- `ox serve` runs the full app in your browser, like OpenRefine does.

See [Command line and server](command-line.md).

## Docker

```bash
git clone https://github.com/ilyaux/OxRefine.git && cd OxRefine
docker build -t oxrefine .
docker run -p 3030:3030 -v oxrefine-data:/data oxrefine
```

The log prints a link that includes an access token; open it in a browser. Projects are saved in the `oxrefine-data` volume.

## From source

You need Rust (stable) and Node.js 22.

```bash
git clone https://github.com/ilyaux/OxRefine.git && cd OxRefine

# desktop app
cd ui && npm install && npm run tauri dev

# or in a browser: build the UI once, then `ox serve` has it built in
cd ui && npm install && npm run build && cd ..
cargo run --release -p ox-cli -- serve      # http://127.0.0.1:3030/
```
