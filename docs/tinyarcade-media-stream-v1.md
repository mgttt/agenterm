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

## `tinyarcade:tones/v1`

```text
"TAT1"             4 bytes
version            u16 = 1
event_count        u16
```

Exactly `event_count` eight-byte events follow:

```text
kind               u8; 1 lock, 2 deck clear, 3 game over
reserved           u8 = 0
frequency_hz       u16; 40..20000
duration_ms        u16; 1..2000
amplitude_milli    u16; 0..1000
```

The host owns the synthesizer, mixing, mute policy, interruption behavior and
audio session. A cartridge can request only these bounded semantic cues; it
cannot supply native audio code or address system audio APIs.
