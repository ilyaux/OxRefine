# Connecting an assistant with MCP

`ox mcp` lets an MCP client inspect and clean projects through a running local `ox serve`. Both use the same project state and saved undo history. Start the server first, then let the client launch the adapter. Use the same OxRefine build and the same absolute data-folder path for both.

```powershell
# Keep this running. OxRefine creates the data folder if needed.
C:\Tools\ox.exe serve --data C:\OxRefineData

# The MCP client launches this command as a subprocess:
C:\Tools\ox.exe mcp --data C:\OxRefineData --allow C:\Data
```

The default backend is `http://127.0.0.1:3030`; use `--url http://127.0.0.1:8080` for a different server port. Only HTTP URLs on `127.0.0.1` or `localhost` are accepted. If the server uses a token, give the adapter the same token through `OXREFINE_TOKEN` in its environment, or `--token`. The adapter verifies that the server actually uses the specified data folder.

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

For a token-protected server, add an `env` object inside the `oxrefine` entry with `"OXREFINE_TOKEN": "your-server-token"`. Keep that token out of shared configuration files. OxRefine does not edit the client's settings.

## Claude Code

Register the local adapter using Claude Code's [stdio setup](https://code.claude.com/docs/en/mcp#option-3-add-a-local-stdio-server):

```powershell
claude mcp add --transport stdio oxrefine -- C:\Tools\ox.exe mcp --data C:\OxRefineData --allow C:\Data
```

Run `/mcp` in Claude Code to inspect the connection. The server must already be running when the client connects. For a token-protected server, supply `OXREFINE_TOKEN` to the adapter process as above; Claude Code also supports `--env` before the `--` separator.

## Tools and the editing workflow

| Tool | What it does |
|---|---|
| `list_projects` | Lists saved projects, metadata and revisions, with `start`/`limit` pagination. |
| `create_project` | Imports an allowed file into a new saved project. `archive: true` imports an OpenRefine project with its history. |
| `open_project` | Inspects a project's columns, history and current cursor, and refreshes its revision. |
| `profile_columns` | Profiles types, blanks, errors and statistics across the whole project. |
| `read_rows` | Reads a page of rows, optionally filtered with OpenRefine `engine` facets and rows/records mode. |
| `facet_counts` | Computes the facets supplied in `engine`, respecting the server's facet-choice limit. |
| `cluster_column` | Suggests clusters with the normal keyers or nearest-neighbour methods. |
| `preview_expression` | Previews a GREL expression on up to 100 matching rows without recording a step. |
| `apply_operation` | Applies one OpenRefine operation object as a saved, undoable step. |
| `apply_recipe` | Applies an array of up to 1,000 operations through normal history. |
| `undo` | Moves to an explicit history cursor; it can undo or redo steps. |
| `export` | Writes an export, project archive, preserved original archive or recipe to a new allowed file. |

Successful tool results contain text and `structuredContent` with `{data, ms, context}`. After importing or opening a project, pass `context.project` and `context.revision` explicitly to its tools. `open_project` is the exception: it takes just the project id to obtain the current revision. Reading the project list does not authorize an edit against an older inspected state.

A useful request to an assistant is: “Open my customer project, profile the columns and show possible duplicate spellings in city. Preview the proposed change before applying it, then export to C:\Data\customers-clean.csv.” Review the suggested operations in your client. Saved edits also appear in the browser's history; an already open browser view refreshes when its next request detects a conflicting revision.

If another browser or assistant changed the project, the adapter reports the stale revision and does not retry the edit. A failed recipe can leave earlier steps applied. A lost connection, timeout or oversized reply can also occur after an edit or export has taken effect. Inspect `open_project` and the destination file before retrying; use `undo` explicitly when needed.

## File access and current limits

Imports and exports are limited to the data folder and paths granted with repeated `--allow FILE` or `--allow DIRECTORY` options. Grants must exist at startup. A file grant permits that exact existing file; a directory grant permits its descendants. Paths are resolved before use, including links, and the server's managed `projects` and `staging` trees are blocked even when explicitly granted. Put input files and exports in a separate folder such as `C:\Data`.

Only regular-file imports are exposed. Export requires an existing parent directory and a new filename; an existing destination is refused. Publication uses an atomic hard link so simultaneous exports cannot overwrite each other; the destination filesystem must support hard links. Supported export formats are CSV, TSV, custom-separated text, HTML, XLS/XLSX, ODS, SQL, templating, project archives, preserved original archives and recipes. These grants govern the adapter's file tools; operations that fetch URLs or contact reconciliation services still use the server's network access. They do not create separate user accounts or isolate project libraries.

The adapter implements tools over [MCP 2025-11-25 stdio](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports), with newline-delimited UTF-8 JSON-RPC. Standard output carries protocol messages and standard error carries diagnostics. There are no MCP resources, prompts, remote HTTP transport or background task notifications. Calls run serially, with a bounded waiting queue; cancellation is cooperative through the server's job controls. Requests are limited to 1 MiB, tool data to 2 MiB and the complete reply to 4 MiB; use pagination or file export for large results. A very long history can also exceed the unpaginated `open_project` reply limit; inspect that project in the browser or backend API. Backend calls time out after ten minutes, and an individual library call may delay cancellation.
