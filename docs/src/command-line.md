# Command line and server

The `ox` program comes in an archive for each system on the [releases page](https://github.com/ilyaux/OxRefine/releases/latest). It is one file with two commands.

## `ox apply`: recipes in scripts

```bash
ox apply recipe.json input.csv -o cleaned.csv
```

This replays an OpenRefine or OxRefine recipe on a file without opening a window. Use it for the same cleanup on every month's export, or as a step in a data pipeline.

- **The input** can be any format OxRefine imports. It is read with the import defaults, as OpenRefine's wizard reads it.
- **The output format** follows the file extension: `.csv`, `.tsv`, `.html`, `.xlsx`, `.xls`, `.ods` or `.sql`.
- **Either side can be an OpenRefine project** (`.openrefine.tar.gz`). The output project then carries the recipe's steps as its history.
- **Timings:** each step's time is printed to the console.

## `ox serve`: OxRefine in a browser

```bash
ox serve                    # http://127.0.0.1:3030/
ox serve data.csv           # opens a file straight away
ox serve --port 8080 --data ~/oxrefine
```

The full app runs in your browser, as OpenRefine does. Projects are saved in the folder given with `--data`, or in your user profile by default.

### On a server

```bash
ox serve --host 0.0.0.0 --data /srv/oxrefine
```

- **Access token:** when OxRefine listens beyond your own machine, it requires one. It prints a link that includes the token; share the link with the people who should have access. Add `--token <secret>` to choose the token yourself.
- **Files from your computer:** in the browser, files are uploaded to the server, and exports download back to you.
- **Security:** anyone with the link can read and write files as the user OxRefine runs as. Keep the token private and run OxRefine under a user with limited rights.

### Docker

```bash
docker build -t oxrefine .
docker run -p 3030:3030 -v oxrefine-data:/data oxrefine
```

The container runs `ox serve --host 0.0.0.0` and keeps projects in `/data`. The log prints the link with the token. Pass `--token <secret>` after the image name to set your own.

## The JSON API

The browser app talks to `ox serve` through a small JSON API: `POST /api/<command>` with JSON arguments. Every command the app uses is available there. The API is not OpenRefine's `/command/core/...` API, so OpenRefine client libraries don't work with it yet. For batch work, `ox apply` is usually simpler.
