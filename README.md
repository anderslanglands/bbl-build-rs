# `bbl-build` — [`babble`](https://github.com/anderslanglands/babble) Build

A companion crate to build `babble`-wrapped C++ dependencies as part of a Rust
project.

## Project Setup With Babble-Wrapped C++ Lib

We are going to wrap a lib called `foo`.

Refer to the
[`babble` documentation](https://github.com/anderslanglands/babble/blob/main/README.md)
on how to set up the `bbl-foo` folder.

```
bbl-foo
  ├── bind
  ├── CMakeLists.txt
  └── gen
build.rs
Cargo.toml
src
  └── lib.rs
```

## Usage

Add this to your `Cargo.toml`:

```toml
[build-dependencies]
bbl-build = { git = "https://github.com/anderslanglands/bbl-build-rs.git" }
```

Call this somewhere in your `build.rs`:

```rust
let binding_dest = Config::new("foo", "bbl-foo")
    .define("BBL_LANGUAGES", "rust")
    .build();

println!("cargo:rerun-if-changed=bbl-foo");
```

If you have a different project layout make sure you adjust the location of the
`bbl-foo` folder in both the call to `Config::new()` and the `println!()`
invocation.

Bindings will be generated in `$OUT_DIR/build/foo.rs`.

To ingest them, use `include!`, similar to `bindgen`-generated bindings:

```rust
include!(concat!(env!("OUT_DIR"), "/build/foo.rs"));
```

## Pitfalls

Make sure `BBL_PLUGIN_PATH` is set to where the Rust plugin for `babble` can be
found.

On a Linux system a typical location would be `/usr/local/plugins/libbbl-rust`.

I.e. you'd have:

```shell
export BBL_PLUGIN_PATH=/usr/local/plugins/
```
