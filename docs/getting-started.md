# Getting started

This guide gets a local kSEE build running and explains the browser and
toolchain requirements. For repository internals and the full command
reference, see [development.md](development.md).

## Supported setup hosts

`setup.sh` automates setup on:

- macOS with Homebrew; and
- Ubuntu 26.04 with `apt`.

Other hosts can work with a manually installed toolchain, but the setup script
will reject them.

The build requires:

- LLVM 22: Clang, `wasm-ld`, `llvm-config`, and the pass-plugin headers;
- GNU make and sed;
- `mkfs.ext4` from e2fsprogs;
- `gzip`;
- Rust through rustup, with the `wasm32-unknown-unknown` target; and
- a browser with JSPI support.

On Linux, setup also installs the ordinary kernel build prerequisites, and
Rust links host code with `clang-22` (`.cargo/config.toml`), so no system `cc`
is needed. On macOS, it installs versioned Homebrew formulae for LLVM and LLD
and puts the Homebrew GNU tools ahead of the system tools during the build.

## Install the toolchain

From the repository root:

```sh
./setup.sh
```

The script installs the supported host dependencies, installs rustup if it is
missing, and adds the Rust WebAssembly target. It does **not** install Chromium
or another browser.

### Use a custom LLVM 22 build

The build puts Homebrew's `llvm@22` (macOS) or `/usr/lib/llvm-22/bin` (Linux)
ahead of `PATH`. Without that installation, it uses the LLVM on `PATH`:

```sh
PATH=/absolute/path/to/llvm/bin:$PATH ./ksee.sh build
```

The build checks `llvm-config --version` and refuses any LLVM major version
other than 22.

## Build and serve

The default command builds what changed and starts the local server:

```sh
./ksee.sh
```

Open `http://127.0.0.1:8000/`. To separate the steps:

```sh
./ksee.sh build
./ksee.sh serve
```

`serve` uses the existing build. Pass a different port as
`./ksee.sh serve --port=9000`.

The first build:

1. fetches Linux stable `v7.2.7` into `linux/` and applies `linux.patch`;
2. builds the LLVM instrumentation plugin and kernel;
3. builds the Rust page/runtime WebAssembly module;
4. packs kernel metadata and sources;
5. builds `/init`, `/bin/prog`, and the initramfs; and
6. writes the deployable site to `target/site/`.

Later builds reuse outputs whose inputs have not changed. Use one LLVM 22
toolchain for a checkout after its first build.

## Browser requirement

kSEE uses JavaScript Promise Integration to suspend and resume Linux task call
stacks. The durable capability requirement is JSPI, exposed through
`WebAssembly.Suspending`; a browser name or version alone is not the contract.

If the kernel does not start, verify that the browser build enables that API.
The local server does not require cross-origin isolation because the kernel and
runtime use one thread and one non-shared WebAssembly memory.

## Run tests

Browser tests find Chrome or Chromium on `PATH`, or in their standard macOS
application locations. Set `CHROME` to override the detected executable:

```sh
./ksee.sh test
CHROME=/absolute/path/to/chromium ./ksee.sh test --ui
```

The override may name Chromium, Chrome, or another compatible binary that
accepts Chromium's headless flags. `setup.sh` intentionally leaves browser
installation to you.

The default test command runs native `ksee-core` and `ksee-cli` tests first,
then performs a full build and runs the kernel/browser self-test. `--ui` adds
the interactive page suites. Generated test pages are placed under
`target/test/site/`.

See [development.md#tests](development.md#tests) for the test layers and their
order.

## Troubleshooting

### `tapi ... unknown architecture` on macOS

If linking fails with a `tapi ... unknown architecture` error, Xcode or its
Command Line Tools and the selected developer directory may not match. Update
or reinstall the tools, then inspect the active directory with:

```sh
xcode-select --print-path
```

Select the matching Xcode or Command Line Tools installation with
`xcode-select --switch`, then run the build again. Do not delete SDK
directories.

### Wrong LLVM is selected

Install LLVM 22 with `./setup.sh`, or put another LLVM 22 first on `PATH` (see
[Use a custom LLVM 22 build](#use-a-custom-llvm-22-build)). The error reports
the version seen through `llvm-config`. The directory must also contain
compatible `clang` and `wasm-ld` binaries.

### Browser tests ask for `CHROME`

Set `CHROME` to the browser executable itself, not its containing directory:

```sh
CHROME=/Applications/Chromium.app/Contents/MacOS/Chromium ./ksee.sh test
```
