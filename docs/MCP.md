# Ographis for assistants: `og mcp`

`og mcp` makes Ographis a server of the [Model Context Protocol](https://modelcontextprotocol.io),
so that an assistant such as Claude can work with your data through Ographis: look at what is
in your files, find the tools, build a model while you watch it on the canvas, try it on a
sample, run it and look at maps of what came out.

The data stays on your computer and in Ographis. The assistant gets what it needs to do the
work: summaries, a page of rows at a time, and pictures of maps; not whole files.

## Starting

### Claude Desktop, as an extension

Double-click `Ographis-<version>-windows.mcpb` from the release, or drag it into
*Settings → Extensions* of Claude Desktop. Choose the folder of your data and models, and tick
whether the models may use the web and write to databases.

### Claude Desktop, by hand

In *Settings → Developer → Edit config*, under `mcpServers`:

```json
"ographis": {
  "command": "C:\\Ographis\\og.exe",
  "args": ["mcp", "--folder", "C:\\data\\maps"]
}
```

### Claude Code, VS Code and other editors

```
claude mcp add ographis -- C:\Ographis\og.exe mcp --folder C:\data\maps
```

Other editors take the same command and arguments in their settings of MCP servers.

### What it may do

| Option | |
| --- | --- |
| `--folder DIR` | A folder the assistant may read and write in, with the folders in it. May be given more than once. Without it, the folder `og mcp` is started in. |
| `--allow-web` | Models may read from the web: addresses, web services, satellite pictures, *Fetch a place*, background maps. |
| `--allow-databases` | Models may write to databases, such as PostGIS. |
| `--allow-overwrite` | Files that are there already may be written over. |

The extension of Claude Desktop sets the same through `OG_MCP_ALLOW_WEB`,
`OG_MCP_ALLOW_DATABASES` and `OG_MCP_ALLOW_OVERWRITE` (`true`), and `OG_MCP_FOLDERS`.

## What the assistant can do

| Tool | |
| --- | --- |
| `list_files` | The files of data and the models in a folder. |
| `describe` | A file or service in short: layers, attributes with their types and figures, coordinate system, extent, number of features and a few rows. |
| `find_nodes`, `node_help` | The tools of Ographis by what they do, and the settings of one with their keys, choices and defaults. |
| `read_model`, `open_model` | A model, or the one open in Ographis now with the changes you have not saved and the node you have selected. |
| `check_model` | Checks a model without running it, and says what is wrong and what would be right. |
| `save_model` | Saves a model, checks it and shows it in Ographis: in the window that is open, which reads it again as it changes, or in a new one. |
| `run_model` | Tries a model on the first 100 features of each input without writing anything; then, with `full`, runs it and writes its outputs. |
| `inspect` | The rows, figures, warnings and log of what came out of a node, a page at a time. |
| `look` | A picture of what came out of a node: a map, a 3D view, a chart, or the picture itself, so that the assistant sees the result. |

As resources it has the guide to the model file (`AGENTS.md`), the reference of every node
(`nodes.json`), the JSON Schema of the model, the examples and the models in the folders. Its
prompts are *Build a model*, *Describe data*, *Fix a model* and *Explain a model*.

## What it may not do

- **Only the folders given.** A path outside them is refused, also through `..`.
- **No passwords or keys.** Connections are kept in the settings of Ographis and used there;
  what the assistant reads of a model, a log or an error has them taken out.
- **Nothing written over.** A file that is there is not written over, by a model or by an
  output, unless the assistant made it in the same session. It is told to choose a new name.
- **A trial first.** A model runs in full only when a trial of it, as it is, went well. A
  trial writes nothing.
- **Nothing from the web, nothing into a database**, unless allowed when the server is
  started.

## Uses

- *"What is in this GeoPackage?"* The assistant describes each layer, explains the attributes
  and shows a map.
- *"Make a GeoPackage of the schools within 500 m of a bus stop."* It finds the tools, builds
  the model on the canvas while you watch, tries it, shows a map of the result and runs it
  when you agree. The model stays: you can open it, change it and run it again without the
  assistant, or on a schedule.
- *"Why does this model give nothing at the end?"* It checks and tries the model open in
  Ographis, looks at what each node gives, finds the filter that lets nothing through and
  saves a mended copy.
- *"Explain this model to a new colleague."* It reads the model and tells what each step does.
- *"Do the same for every municipality."* It changes the model you made, with variables, and
  tries it for one first.

## Testing

`crates/og-cli/tests/mcp.rs` speaks the protocol with `og mcp` as an assistant would, does the
typical tasks of `crates/og-cli/tests/mcp_tasks.json` with the tools, and tries the limits.

`tools/mcp-eval/run.py` gives the same tasks to real assistants and judges what they made by
the same checks, for several models at a time:

```
python tools/mcp-eval/run.py --og target\release\og.exe --models sonnet opus haiku
```

It uses Claude Code by default; `--agent` gives another command.

## The MCP registry

`mcp/server.json` is the entry of Ographis in the [MCP registry](https://registry.modelcontextprotocol.io),
and `mcp/manifest.json` the manifest of the extension. `build.cmd -Package` makes
`dist\Ographis-<version>-windows.mcpb` and `dist\server.json` with the checksum of the
extension filled in. To publish:

1. Attach the `.mcpb` file to the release `v<version>` on GitHub.
2. `mcp-publisher login github`, then `mcp-publisher publish dist\server.json`.
