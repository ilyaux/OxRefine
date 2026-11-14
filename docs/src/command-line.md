# Command line and server

The `ox` program comes in an archive for each system on the [releases page](https://github.com/ilyaux/OxRefine/releases/latest). The commands below run recipes and host the browser app; [`ox mcp`](mcp.md) connects an assistant to a running local server.

## `ox apply`: recipes in scripts

```bash
ox apply recipe.json input.csv -o cleaned.csv
```

This replays an OpenRefine or OxRefine recipe on a file without opening a window. Use it for the same cleanup on every month's export, or as a step in a data pipeline.

- **The input** can be any format OxRefine imports. It is read with the import defaults, as OpenRefine's wizard reads it.
- **The output format** follows the file extension: `.csv`, `.tsv`, `.html`, `.xlsx`, `.xls`, `.ods` or `.sql`.
- **Either side can be an OpenRefine project** (`.openrefine.tar.gz`). The output project then carries the recipe's steps as its history.
- **Timings:** import, recipe application and export times are printed to the console.

If a recipe fails, `ox apply` exits with a nonzero status and identifies the step. It does not write the output file; an existing output remains untouched. Check the input's column names, types and any extension dependencies when [reusing a recipe](moving-over.md#reusing-a-recipe-on-another-file).

## `ox serve`: OxRefine in a browser

```bash
ox serve                    # http://127.0.0.1:3030/
ox serve data.csv           # opens a file straight away
ox serve --port 8080 --data ~/oxrefine
```

The full app runs in your browser, as OpenRefine does. Projects are saved in the folder given with `--data`, or in your user profile by default.

For a command that takes more than a moment, **Jobs** shows the work stages, their elapsed times and the result: completed, failed or cancelled. Streaming CSV reads, parsing and optional type detection happen together and are shown as a combined stage. Cancel takes effect at the next cancellation check; it cannot interrupt every library call immediately. Completed jobs seen by the panel remain visible until dismissed or another long-running job is observed. The server retains at most 20 recent finished jobs for up to 60 seconds; these timings are not saved in the project.

The status bar and Jobs show the OxRefine process's current resident memory and its peak reported by the operating system. In browser mode this is the server process, including all its open projects; in the desktop app it is the backend process. It does not include the browser's JavaScript heap, and it is separate from a project's size on disk. Linux's kernel figures are approximate. An unavailable measurement is shown explicitly instead of zero.

### On a server

```bash
ox serve --host 0.0.0.0 --data /srv/oxrefine
```

- **Access token:** when OxRefine listens beyond your own machine, it requires one. It prints a link that includes the token; share the link with the people who should have access. Add `--token <secret>` to choose the token yourself.
- **Files from your computer:** in the browser, files are uploaded to the server, and exports download back to you.
- **Security:** anyone with the link can read and write files as the user OxRefine runs as. Keep the token private and run OxRefine under a user with limited rights.

### Browser sessions and handing work over

Each browser page chooses its own active project. Opening or closing a project in one page does not redirect another page's commands. To continue on another computer, open the same server link and select the saved project from **Projects**; its data and undo/redo history remain on the server.

The project list shows the recorded history length, including undone steps that can still be redone, and the size of its saved files. That size includes any preserved original OpenRefine archive; it is separate from the memory used while the project is open. A missing, unreadable or oversized history, or a directory containing links, can make a statistic unavailable; this does not mean the project is empty.

When two pages open the same project, each command carries the project id and the revision the page displayed. If another page has changed the data, schema, history cursor or metadata, stale reads and edits are rejected. The app closes the old dialog, clears its facets and refreshes the project with a warning. Review the refreshed data and repeat your intended action; a rejected edit is never retried automatically. Reloading the page also refreshes the current project. There is no live push of another editor's changes.

The server keeps one authoritative in-memory project per id and normally caches up to four projects. A failed mutation or save pins that project in memory until a successful saving command, so opening unrelated projects cannot discard its unsaved state. A disk-write error still needs attention: data held only in memory cannot survive a server stop. Commands remain serialized across projects; progress and cancellation can answer while one runs. Jobs shown in a browser belong to that page; reloading starts a new page identity.

Run only one server against a project data folder. The shared token grants the server user's file access; it does not create accounts, roles or separate private project libraries. `ox serve file.csv` opens an unsaved startup project; import the file through the app to keep its edits across server restarts.

### Docker

```bash
docker build -t oxrefine .
docker run -p 3030:3030 -v oxrefine-data:/data oxrefine
```

The container runs `ox serve --host 0.0.0.0` and keeps projects in `/data`. The log prints the link with the token. Pass `--token <secret>` after the image name to set your own.

## The JSON API

The browser app talks to `ox serve` through a small JSON API: `POST /api/<command>` with JSON arguments. Every command the app uses is available there. The API is not OpenRefine's `/command/core/...` API, so OpenRefine client libraries don't work with it yet. For batch work, `ox apply` is usually simpler.

Browser requests send `X-Ox-Session`, `X-Ox-Project` and `X-Ox-Revision`. Successful replies include `context: {project, revision}`; `summary` and `open_project` explicitly refresh it. Other project reads and writes reject stale revisions with HTTP 409 and `code: "stale_project"`; a missing project returns HTTP 404 with `code: "project_gone"`. Server restart invalidates old revisions. Legacy requests without a session header retain the shared active selection and server-wide job controls for existing scripts, and have no stale-edit protection. They carry the same trusted-token privileges.
