# Security

## Telling about a security hole

Please do not open a public issue. Use *Report a vulnerability* under the *Security* tab of
the repository on GitHub, which reaches the maintainer alone, and say what you found and how
it can be done again. Ographis is kept up in spare time, so an answer may take some days.

## What Ographis is exposed to

Ographis is a program on the desktop and a command line. It runs no server that others can
reach: the server for assistants (`og mcp`) talks only to the program that started it.

- **The files and answers it reads** are where a hole could be reached: a zip, an XML or IFC
  file, a GeoPackage or an answer from a web service made to harm. Ographis is written in
  Rust, which rules out errors with memory, such as writing past the end of a buffer, in its
  own code; the few places marked `unsafe` are calls into GDAL. A crafted file can at worst
  make a run stop or take much time or memory.
- **GDAL, PROJ and DuckDB** are written in C and C++. GDAL and PROJ come from OSGeo4W and are
  kept up to date there; DuckDB is built in and follows the version of its Rust library.
- **Passwords and keys** of connections are kept in the credential store of Windows, not in
  models, settings files or logs. A model names a connection, and the settings that are
  exported for others leave the secrets out.

## How the libraries are checked

Ographis is built from some 650 libraries. They are checked against the
[RustSec](https://rustsec.org) database of security holes:

- **At every change** of the libraries, and **every Monday**, by the *Security* workflow on
  GitHub, with [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) and `deny.toml`. It
  also checks that every licence allows Ographis to be shared as MIT or Apache 2.0, and
  that everything comes from crates.io.
- **Dependabot** proposes updates as pull requests: at once for a library with a fixed hole,
  and once a week for the others.
- **By hand**, before a release:

  ```
  cargo install cargo-deny cargo-audit
  cargo deny check
  cargo audit
  ```

A hole that does not reach Ographis, because the part of the library it is in is not used,
is listed in `deny.toml` and `.cargo/audit.toml` with the reason, and taken out again as
soon as a fixed version can be used. Today these are holes in zip and quick-xml in the
libraries that write Excel and LibreOffice Calc files, which extract nothing to disk.
