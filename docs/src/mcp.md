# Connecting an assistant with MCP

`ox mcp` lets an MCP client such as Claude Desktop or Claude Code look at and clean the projects in an OxRefine data folder. Steps it applies go into the normal undo history, and the projects are the same ones the browser app shows.

There is nothing to start first. The client launches `ox mcp` as a subprocess. If an `ox serve` already runs for the data folder, `ox mcp` uses it, so the assistant and your browser share one view of each project. Otherwise `ox mcp` serves the folder itself on a free local port, and prints in its log an address where you can watch the projects in a browser. A second `ox mcp` for the same folder, say Claude Desktop and Claude Code at once, attaches to that same host. One data folder never has two servers: a second `ox serve` for it refuses to start and names the address of the one already running.

```powershell
# The MCP client runs this; the data folder is created if needed.
C:\Tools\ox.exe mcp --data C:\OxRefineData --allow C:\Data
```

Without `--data` the default data folder is used, the same one `ox serve` uses. To use a particular server, give its address with `--url http://127.0.0.1:8080`; only HTTP addresses on `127.0.0.1` or `localhost` are accepted. A server found by itself passes its token along. For `--url`, give a token-protected server's token through `OXREFINE_TOKEN` in the adapter's environment, or `--token`. The adapter verifies that the server really uses the given data folder.

The desktop app keeps its open project in memory and does not use this mechanism. Don't change the same project from the desktop app and an assistant at the same time; use the browser app (`ox serve`) to watch an assistant's work.

## Claude Desktop

In **Settings → Developer → Edit Config**, merge an entry into the existing `mcpServers` object. On Windows this file is `%APPDATA%\Claude\claude_desktop_config.json`. Use your own absolute executable and folder paths, then restart Claude Desktop. These are the standard [Desktop configuration steps](https://github.com/modelcontextprotocol/docs/blob/main/quickstart/user.mdx).

```json
{
  "mcpServers": {
    "oxrefine": {
      "command": "C:\\Tools\\ox.exe",
      "args": ["mcp", "--data", "C:\\OxRefineData", "--allow", "C:\\Data"]
    }
  }
}
```

OxRefine does not edit the client's settings.

## Claude Code

Register the local adapter using Claude Code's [stdio setup](https://code.claude.com/docs/en/mcp#option-3-add-a-local-stdio-server):

```powershell
claude mcp add --transport stdio oxrefine -- C:\Tools\ox.exe mcp --data C:\OxRefineData --allow C:\Data
```

Run `/mcp` in Claude Code to inspect the connection.

## Over HTTP

`ox serve` also answers MCP at `/mcp` ([Streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports#streamable-http)), so a client can connect by address, without a subprocess:

```powershell
C:\Tools\ox.exe serve --data C:\OxRefineData --allow C:\Data --token your-server-token
claude mcp add --transport http oxrefine http://127.0.0.1:3030/mcp --header "Authorization: Bearer your-server-token"
```

The endpoint uses the server's access rules. With a token, every request needs `Authorization: Bearer TOKEN`. Without one, which is the default on this machine, requests need this server's own `Host`, and a web page may call it only from the server's own origin. Behind a [reverse proxy](command-line.md#behind-a-reverse-proxy) the endpoint is at `/refine/mcp`, if the app is at `/refine/`. File tools use the data folder and the paths given with `ox serve --allow`; those paths are on the server's computer. Each `initialize` starts a session named by the `Mcp-Session-Id` reply header. A client ends it with `DELETE /mcp`. The server keeps at most 64 sessions and ends the oldest idle one beyond that; its client then gets 404 and initializes again. Every POST gets one JSON reply; the server sends no event streams or server-initiated messages.

## Tools and the editing workflow

| Tool | What it does |
|---|---|
| `list_projects` | Lists saved projects, metadata and revisions, with `start`/`limit` pagination. |
| `preview_import` | Shows how an allowed file would import, without creating a project: the guessed format and OpenRefine import options, plus the first rows. |
| `create_project` | Creates a saved project from an allowed file (`path`), a web address or Google Sheets link (`url`), or pasted data (`text`). `archive: true` imports an OpenRefine project file with its history. |
| `open_project` | Inspects a project's columns, history and current cursor, and refreshes its revision. |
| `rename_project` | Renames a saved project. |
| `delete_project` | Permanently deletes a saved project and its history. It is marked destructive, so clients can ask you first. |
| `profile_columns` | Profiles types, blanks, errors and statistics across the whole project. |
| `read_rows` | Reads a page of rows, optionally filtered with OpenRefine `engine` facets and rows/records mode. |
| `facet_counts` | Computes the facets supplied in `engine`, respecting the server's facet-choice limit. |
| `cluster_column` | Suggests clusters with the normal keyers or nearest-neighbour methods. |
| `preview_expression` | Previews an expression on up to 100 matching rows without recording a step. GREL by default; `python:` and `clojure:` prefixes select [those languages](transforming.md). |
| `list_functions` | The GREL function and control reference, optionally filtered by a word. |
| `list_extensions` | Installed [extensions](extensions.md) with their functions, importers, exporters and operations. |
| `guess_reconcile_types` | Asks a reconciliation service which types fit a column, best first, for a `core/recon` operation. |
| `apply_operation` | Applies one OpenRefine operation object as a saved, undoable step. |
| `apply_recipe` | Applies an array of up to 1,000 operations through normal history. |
| `undo` | Moves to an explicit history cursor; it can undo or redo steps. |
| `export` | Writes an export, project archive, preserved original archive or recipe to a new allowed file. |

Successful tool results contain text and `structuredContent` with `{data, ms, context}`. After importing or opening a project, pass `context.project` and `context.revision` explicitly to its tools. `open_project` is the exception: it takes just the project id to obtain the current revision. Reading the project list does not authorize an edit against an older inspected state.

A useful request to an assistant is: “Open my customer project, profile the columns and show possible duplicate spellings in city. Preview the proposed change before applying it, then export to C:\Data\customers-clean.csv.” Review the suggested operations in your client. Saved edits also appear in the browser's history; an already open browser view refreshes when its next request detects a conflicting revision.

If another browser or assistant changed the project, the adapter reports the stale revision and does not retry the edit. A failed recipe can leave earlier steps applied. A lost connection, timeout or oversized reply can also occur after an edit or export has taken effect. Inspect `open_project` and the destination file before retrying; use `undo` explicitly when needed.

## File access and current limits

Imports and exports are limited to the data folder and paths granted with repeated `--allow FILE` or `--allow DIRECTORY` options. Grants must exist at startup. A file grant permits that exact existing file; a directory grant permits its descendants. Paths are resolved before use, including links, and the server's managed `projects` and `staging` trees are blocked even when explicitly granted. Put input files and exports in a separate folder such as `C:\Data`.

Only regular-file imports are exposed. Export requires an existing parent directory and a new filename; an existing destination is refused. Publication uses an atomic hard link so simultaneous exports cannot overwrite each other; the destination filesystem must support hard links. Supported export formats are CSV, TSV, custom-separated text, HTML, XLS/XLSX, ODS, SQL, templating, project archives, preserved original archives and recipes. These grants govern the file tools; `create_project` with a `url`, operations that fetch URLs and reconciliation services use the server's network access. They do not create separate user accounts or isolate project libraries.

While a server runs it keeps `host.lock` and `host.json` in the data folder; `host.json` holds its local address and token, readable only by you on Linux and macOS. On a filesystem that can't lock files, such as some network shares, the one-server rule isn't enforced.

The adapter implements tools over [MCP 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports), stdio with newline-delimited UTF-8 JSON-RPC, or Streamable HTTP as above. In stdio mode, standard output carries protocol messages and standard error carries diagnostics. There are no MCP resources, prompts or background task notifications. Stdio calls run one at a time, with a bounded waiting queue; cancellation is cooperative through the server's job controls. Requests are limited to 1 MiB, tool data to 2 MiB and the complete reply to 4 MiB; use pagination or file export for large results. A very long history can also exceed the unpaginated `open_project` reply limit; inspect that project in the browser or backend API. Calls to a separate server time out after ten minutes, and an individual library call may delay cancellation.
