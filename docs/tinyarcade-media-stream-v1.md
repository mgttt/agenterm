# TinyArcade media streams v1

`submit_render` and `submit_audio` carry self-identifying, bounded binary
streams. They are not native pointers, GPU commands, JavaScript objects or
archived platform types. Converters emit little-endian records; native hosts
validate a whole stream before rendering or scheduling audio.

## `tinyarcade:grid3d/v1`

The grid frame starts with a 32-byte header:

```text
"TAG3"             4 bytes
version            u16 = 1
header_bytes       u16 = 32
board_width        u16
board_depth        u16
board_height       u16
cell_count         u16
score              u32
cleared_decks      u32
level              u32
flags              u32; bit 0 = game over
```

Exactly `cell_count` eight-byte records follow:

```text
x, y, z            u8 each
kind               u8; 1 settled, 2 active, 3 landing ghost
rgba               u32 little-endian RGBA8
```

Dimensions are non-zero, every coordinate is inside the declared board, every
kind is known, unknown flag bits are rejected and trailing bytes are forbidden.
Consumers use `kind` to draw settled, ghost and active cells in stable visual
priority independent of record order.

The Swift SDK owns the immutable frame bytes and exposes typed cells through
`TinyArcadeGrid3DFrame.forEachCell`, so SceneKit or Metal renderers can walk the
validated records without building another cell array each frame. The `cells`
property is retained as an allocating compatibility view; it is not the render
hot path.

## `tinyarcade:indexed2d/v1`

The indexed frame is a complete, uncompressed 2D pixel plane. Its 16-byte
header is:

```text
"TAI2"             4 bytes
version            u16 = 1
header_bytes       u16 = 16
width              u16; 1..512
height             u16; 1..512
palette_count      u16; 1..256
flags              u16; bit 0 = application metadata trailer
```

Exactly `palette_count` four-byte colors follow. Each color is encoded as the
four bytes R, G, B, A and is exposed by the Rust/Swift SDKs as one
little-endian RGBA8 `u32`/`UInt32`. The next `width × height` bytes are
one-byte palette indices in row-major top-to-bottom order. Every index must be
less than `palette_count`.

With flags bit 0 clear, the pixel plane must end the stream exactly. With bit 0
set, this bounded trailer follows the pixel plane:

```text
"TAM1"             4 bytes
application_schema u32; non-zero, cartridge/app-owned
metadata_bytes     u16; 1..1024
reserved           u16 = 0
metadata           exactly metadata_bytes opaque bytes
```

The cartridge must then import and check
`indexed2d_metadata_version() -> i32` from `tinyarcade:core/v1`. The SDK
validates, bounds and transports the schema-tagged bytes but never interprets
game rules. This lets a native HUD/accessibility layer consume state already
produced by the tick instead of calling `game_suspend` every display frame.
Unknown flags, malformed trailers and trailing bytes are rejected before
native presentation. Base v1 frames remain byte-for-byte compatible.

Each dimension is at most 512, the pixel plane is at most 65,535 bytes and the
whole stream is at most 64 KiB. Therefore ordinary 256 × 240 and 320 × 200
frames with full 256-color palettes fit the default render budget. This v1
format is deliberately a whole frame rather than a delta, compressed payload
or GPU command list. The native host owns nearest-neighbour scaling, aspect
fit, color-space conversion, compositing and display refresh; the cartridge
cannot address Metal, Core Graphics or platform objects.

The iOS SDK includes a host-side convenience that expands the validated plane
to canonical sRGB RGBA8 and a UIKit view configured for aspect-fit,
nearest-neighbour presentation. This does not change the cartridge protocol:
custom Metal renderers may consume the same palette and indices directly, and
no Apple framework type crosses the WASM boundary.

Each cartridge must import and check the `() -> i32` version function for every
media schema it emits: `grid3d_version`, `indexed2d_version` and/or
`tones_version` in `tinyarcade:core/v1`; indexed application metadata also
requires `indexed2d_metadata_version`. These ordinary WASM imports make
compatibility fail at load on runtimes that predate a format; emitting `TAG3`,
`TAI2` or `TAT1` without its declaration traps the current cartridge.

## `tinyarcade:tones/v1`

```text
"TAT1"             4 bytes
version            u16 = 1
event_count        u16
```

Exactly `event_count` eight-byte events follow:

```text
kind               u8; 1 impact, 2 success, 3 failure
reserved           u8 = 0
frequency_hz       u16; 40..20000
duration_ms        u16; 1..2000
amplitude_milli    u16; 0..1000
```

`event_count` is at most 16 and the sum of all `duration_ms` values is at most
4,000 ms. Events are scheduled sequentially in record order. A host may insert
a small fixed transition gap, but must not turn one bounded batch into
unbounded or concurrent audio work. These aggregate bounds apply in addition
to the encoded-byte budget: a small command stream must not schedule an
arbitrarily long native operation.

The host owns the synthesizer, mixing, mute policy, interruption behavior and
audio session. A cartridge can request only these bounded semantic cues; it
cannot supply native audio code or address system audio APIs.

The three kinds describe host feedback intent, not one game's rules. Depth
Well maps lock/clear/game-over to impact/success/failure; a paddle game may map
rebound/field-clear/life-loss to the same stable meanings. Frequency, duration
and amplitude remain explicit cartridge parameters within the bounds above.
Waveform and timbre are host presentation choices, so converters and fan-made
cartridges depend only on the versioned semantic event contract rather than an
Apple audio implementation.
