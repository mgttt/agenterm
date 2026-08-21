# tinyvm optional WASI Preview 1 profile

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: partial, development profile

This profile is a separately enabled adapter for ordinary standard
`wasi_snapshot_preview1` imports. It is not enabled by default, is not part of
`tinyarcade:core/v1`, and does not add opcodes or platform calls to the VM
engine. The Cargo feature is `wasi-p1`.

## Layering

```text
standard Wasm import
└── wasi_snapshot_preview1 adapter
    └── HostContext
        ├── guest fd → opaque HostHandle
        ├── rights + bounded process strings
        └── virtual preopen → relative guest path
            └── HostBackend implemented by an embedding/platform
```

The adapter owns canonical WASI signatures, guest-memory layouts and errno
translation. `HostContext` owns guest descriptor identity and capability
checks. A backend owns native handles and OS mechanisms. Neither the adapter nor
the guest sees a physical path, Unix fd, Windows HANDLE or iOS container path.

## Implemented imports

| Import | Standard parameters → results | Host operation |
|---|---|---|
| `args_sizes_get` | `(i32, i32) → i32` | bounded argument count/bytes |
| `args_get` | `(i32, i32) → i32` | pointer table + NUL-terminated arguments |
| `environ_sizes_get` | `(i32, i32) → i32` | bounded environment count/bytes |
| `environ_get` | `(i32, i32) → i32` | pointer table + NUL-terminated environment |
| `clock_time_get` | `(i32, i64, i32) → i32` | selected backend clock, nanoseconds |
| `random_get` | `(i32, i32) → i32` | backend fills the complete borrowed range |
| `fd_prestat_get` | `(i32, i32) → i32` | directory tag + virtual-root byte length |
| `fd_prestat_dir_name` | `(i32, i32, i32) → i32` | virtual-root bytes only |
| `fd_close` | `(i32) → i32` | closes and invalidates the guest mapping |
| `fd_read` | `(i32, i32, i32, i32) → i32` | bounded mutable iovecs through a readable guest descriptor |
| `fd_write` | `(i32, i32, i32, i32) → i32` | bounded immutable iovecs through a writable guest descriptor |
| `fd_seek` | `(i32, i64, i32, i32) → i32` | seek through a seekable guest descriptor and write the new offset |
| `fd_filestat_get` | `(i32, i32) → i32` | standard 64-byte filestat record from descriptor metadata |
| `path_open` | `(i32, i32, i32, i32, i32, i64, i64, i32, i32) → i32` | relative open beneath a virtual preopen, returning a guest descriptor |
| `path_unlink_file` | `(i32, i32, i32) → i32` | relative unlink beneath a virtual preopen |

Every present import is type-checked before instantiation. An unknown
`wasi_snapshot_preview1` field or wrong signature fails binding; it is not left
as a late unbound trap. Guest-memory ranges are checked before host mutation.
Vectored I/O accepts at most 64 records and preflights the complete table, every
buffer and the result pointer before the first backend call. Backend byte counts
must fit the supplied slice, and aggregate counts use checked arithmetic.

`path_open` currently accepts create, directory and truncate open flags and the
read, write, seek and filestat-get base rights represented by `HostContext`.
Symlink-follow, exclusive open, descriptor flags, inheriting rights and other
Preview 1 rights return an explicit unsupported error rather than being
silently ignored. UTF-8 and the common relative-path policy are checked before
a backend sees the request.

## Explicitly not implemented yet

- `proc_exit` and its non-returning instance outcome;
- sockets, polling, threads and ambient network access.

An unimplemented import fails at bind time. Platform absence behind an
implemented import maps to an explicit WASI errno such as `NOSYS` or
`NOTCAPABLE`; no backend fabricates a result.

## Current evidence

`tests/wasi_p1_adapter.rs` builds standards-shaped binary modules with all
fifteen implemented imports. Through real persistent tinyvm instances it verifies
argument/environment layouts, monotonic clock output, random bytes, preopen
metadata/name, descriptor close, vectored I/O, seek and filestat layouts. Other
cases reject an excessive iovec count before any backend write, an unknown field
and a known field with the wrong standard type before instantiation. Path cases
prove guest-fd publication and reject parent traversal before backend dispatch.
