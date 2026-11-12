# Contributing to Ographis

Thank you for wanting to help. Bug reports, ideas and pull requests are all welcome. How the
code hangs together is in `ARCHITECTURE.md`; read that first if you want to change it.

## Reporting a bug

Open an issue with the bug template. What helps most:

- the version, from Help → About Ographis or `og version`,
- the model file (`.ographis`), and a small piece of the data if you can share it,
- the log (the *Copy log* button above the Log tab),
- `crash-log.txt` from `%APPDATA%\Ographis` if the program says a fault happened.

## Building

**Windows 11:** `build.cmd` does it all, step by step in `docs/BUILD-WINDOWS.md`.

**Linux** (Debian or Ubuntu), which is also the quickest way to run the tests:

```
sudo apt install libgdal-dev clang pkg-config
cargo build
cargo run -p og-gui --bin ographis -- examples/skaraborg.ographis
cargo run -p og-cli --bin og -- run examples/buildtest.ographis
```

When a file is removed or turned into a folder, add its path to `tools/removed-files.txt`,
so that `build.cmd` can tidy away what is left when a new version is unpacked over an old
one.

## Testing

The check on GitHub uses Rust 1.97.0, so that a newer Rust with new lints of clippy does
not turn it red by itself. A newer Rust builds Ographis too; the version of the check is
raised by hand, with the code made clean for its clippy at the same time.

```
cargo test -p og-engine                 # no GDAL needed
cargo test --workspace --exclude og-gui # with GDAL
cargo test -p og-gui                    # the program, without a window
cargo fmt --all
cargo clippy --workspace --all-targets
```

The tests of the program need a graphics driver; Mesa's software renderer will do
(`sudo apt install mesa-vulkan-drivers`). They save screenshots in `target/screenshots`,
which are worth a look when you change something that is drawn.

A pull request should leave every test passing, `cargo fmt` without changes and clippy
without warnings. A new tool or a fixed bug comes with a test.

Before a release, run the performance tests in a release build and add a row to
docs/PERFORMANCE.md:
`cargo test --release -p og-gdal --test performance -- --ignored --nocapture`.

## Adding a tool, in five steps

*Reverse lines* (`LineReverser`) is a small tool to copy from: it is in
`crates/og-engine/src/nodes/geom_transform.rs`.

1. **Choose the file** for its category in `crates/og-engine/src/nodes/` (the table in
   `ARCHITECTURE.md`), and start a section: `// ---------- Reverse lines (LineReverser) ----------`.
2. **Write its settings and its function.** The settings are a list of `ParamSpec`: a key,
   a label, a kind of field (text, a choice, an attribute of the input, a file…), a default
   and a hint. The function gets the settings (`P`), the tables that came in (`Inputs`) and
   the context (`NodeCtx`, with the log as `ctx.notes`), and returns a table per output.
   Tell the user in the log what it did: `ctx.notes.info(...)`, and warn about what was left
   out. **No tool may lose data without a word.** Every feature left out, every setting
   that makes the result empty or tiny, every value that could not be read is told in the
   log, from the first version of the tool:
   - an error (`err(...)`) when nothing sensible can come out, saying what to do about it:
     *"The network is in degrees. Reproject it to metres first"*;
   - a warning (`ctx.notes.warn(...)`) when some of it was left out, with how many and why:
     *"12 features have no geometry and went to Remnants"*;
   - a warning when the settings are allowed but surely not meant, such as limits in
     metres that look like minutes;
   - info (`ctx.notes.info(...)`) with what was made, in the units the user thinks in.

   The runner adds a warning of its own when features come in and nothing goes out, or an
   input reads nothing, and the tool has not said why; a tool should say it better itself.
3. **Register it** in the `TYPES` list at the end of the file: a name for model files (never
   change it later), a title as a verb, the category, a description, the ports, the settings,
   the function and a one-line summary. The palette, the form and `og nodes` follow from this.
   A setting that only applies sometimes gets a rule in `relevant()` in
   `crates/og-gui/src/params.rs`.
4. **Test it** with a small model in `crates/og-engine/tests/flows.rs` (see
   `rotate_and_reverse`), or in `crates/og-gdal/tests/roundtrip.rs` if it needs GDAL.
5. **Document it** with a row in the table of tools in `docs/TOOLS.md`.

## How the code is written

- Comments and messages say what something is for, in plain words, as to a user of the
  program. A message that ends a run says what went wrong and what to do about it.
- What a user sees is in English. The names in model files stay as they are, so that every
  earlier model still opens; a change of format is made in `Workspace::from_json`, which
  brings old files up to date.
- Ographis is its own program. Code, comments, documentation, commit messages, pull
  requests and issues describe what Ographis does in its own words, without comparing it
  with other ETL programs or saying that something works "as in" one of them.
- No new dependency without a reason; `tools/third_party.py` writes
  `THIRD-PARTY-LICENSES.md` again when they change.

## Licence

Ographis is licensed under MIT or Apache 2.0, as you choose. Unless you say otherwise, a
contribution you send in is licensed in the same way.

## Example models

The nodes of the example models in `examples/` keep the names of their tools, such as
*Draw map* or *Spatial join*, so that it is easy to see which tool each is. What a node does
in the model goes in its `comment`, such as "Places on OpenStreetMap".
