# Getting started

This guide gets a local kSEE build running and explains the browser and
toolchain requirements. For repository internals and the full command
reference, see [development.md](development.md).

## Supported setup host

`setup.sh` automates setup on Ubuntu 26.04, everything from `apt` (LLVM 22 is
the release's own). Other Linux hosts can work with a manually installed
toolchain.

The build requires:

- LLVM 22: Clang, `wasm-ld`, `llvm-config`, and the pass-plugin headers;
- make;
- `mkfs.ext4` from e2fsprogs;
- `gzip`;
- Rust through rustup, with the `wasm32-unknown-unknown` target; and
- a browser with JSPI support.

Setup also installs the ordinary kernel build prerequisites, and Rust links
host code with `clang-22` (`.cargo/config.toml`), so no system `cc` is needed.

## Install the toolchain

From the repository root:

```sh
./setup.sh
```

The script installs the host dependencies and rustup (or updates it);
`rust-toolchain.toml` makes rustup add the Rust WebAssembly target on first
use. It also installs the libraries the tests' browser needs; the browser
itself, Chrome's headless shell, is fetched by `./ksee.sh test`.

### Use a custom LLVM 22 build

The Makefile puts `/usr/lib/llvm-22/bin` ahead of `PATH`. Without that
installation, it uses the LLVM on `PATH`:

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

The first build fetches Linux stable `v7.2.7` into `linux/` and builds
everything, into `target/site/` (the steps: [development.md#build-pipeline](development.md#build-pipeline)).
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

```sh
./ksee.sh test
./ksee.sh test --ui
```

The browser suites run in Chrome's headless shell, fetched once into
`target/chrome/` for x86-64 or arm64 Linux; `--ui` adds the page's. What
runs, in order: [development.md#tests](development.md#tests).

## Troubleshooting

### Wrong LLVM is selected

Install LLVM 22 with `./setup.sh`, or put another LLVM 22 first on `PATH` (see
[Use a custom LLVM 22 build](#use-a-custom-llvm-22-build)). The error reports
the version seen through `llvm-config`. The directory must also contain
compatible `clang` and `wasm-ld` binaries.

### The browser does not start

Run `target/chrome/chrome-headless-shell --version`: a missing library is
named there. `./setup.sh` installs the libraries and fetches the shell (again
when its `CHROME_VERSION` changes, or after `target/chrome/` is removed).
