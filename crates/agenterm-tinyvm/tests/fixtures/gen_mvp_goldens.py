#!/usr/bin/env python3
"""Emit independent WASM 1.0 MVP goldens.

Expected values are computed here from the spec (two's-complement wrap,
IEEE-754 bits, WASM traps). This script never imports or runs the Rust
interpreter. Re-run from the crate tests/fixtures directory if the catalog
must be regenerated.
"""

from __future__ import annotations

import math
import struct
from pathlib import Path

# --- WASM binary helpers (module format only; not an interpreter) ---


def uleb(n: int) -> bytes:
    n = int(n)
    if n < 0:
        raise ValueError(n)
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def sleb(n: int) -> bytes:
    n = int(n)
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n == 0 and (byte & 0x40) == 0:
            out.append(byte)
            break
        if n == -1 and (byte & 0x40) != 0:
            out.append(byte)
            break
        out.append(byte | 0x80)
    return bytes(out)


def name(s: str) -> bytes:
    b = s.encode("utf-8")
    return uleb(len(b)) + b


def section(sid: int, payload: bytes) -> bytes:
    return bytes([sid]) + uleb(len(payload)) + payload


I32, I64, F32, F64 = 0x7F, 0x7E, 0x7D, 0x7C


def i32c(n: int) -> bytes:
    return bytes([0x41]) + sleb(n)


def i64c(n: int) -> bytes:
    return bytes([0x42]) + sleb(n)


def f32c(x: float) -> bytes:
    return bytes([0x43]) + struct.pack("<f", float(x))


def f32c_bits(bits: int) -> bytes:
    """f32.const from a raw bit pattern (NaN payloads, signed zeros)."""
    return bytes([0x43]) + struct.pack("<I", bits & 0xFFFFFFFF)


def f64c_bits(bits: int) -> bytes:
    """f64.const from a raw bit pattern."""
    return bytes([0x44]) + struct.pack("<Q", bits & 0xFFFFFFFFFFFFFFFF)


def f64c(x: float) -> bytes:
    return bytes([0x44]) + struct.pack("<d", float(x))


def encode_module(
    *,
    types: list[tuple[list[int], list[int]]],
    func_types: list[int],
    codes: list[tuple[int, bytes]],
    exports: list[tuple[str, int]] | None = None,
    imports: list[tuple[str, str, int]] | None = None,
    globals_: list[tuple[int, bool, bytes]] | None = None,
    table_min: int | None = None,
    elems: list[tuple[int, list[int]]] | None = None,
    start: int | None = None,
    memory_min: int | None = None,
    memory_max: int | None = None,
    data: list[tuple[int, bytes]] | None = None,
    local_decls: list[list[tuple[int, int]]] | None = None,
) -> bytes:
    out = bytearray(b"\x00asm\x01\x00\x00\x00")
    tpay = bytearray(uleb(len(types)))
    for params, results in types:
        tpay += bytes([0x60]) + uleb(len(params)) + bytes(params)
        tpay += uleb(len(results)) + bytes(results)
    out += section(1, bytes(tpay))
    if imports:
        ip = bytearray(uleb(len(imports)))
        for mod, field, tidx in imports:
            ip += name(mod) + name(field) + bytes([0x00]) + uleb(tidx)
        out += section(2, bytes(ip))
    fp = bytearray(uleb(len(func_types)))
    for t in func_types:
        fp += uleb(t)
    out += section(3, bytes(fp))
    if table_min is not None:
        out += section(4, bytes([0x01, 0x70, 0x00]) + uleb(table_min))
    if memory_min is not None:
        if memory_max is None:
            out += section(5, bytes([0x01, 0x00]) + uleb(memory_min))
        else:
            out += section(
                5, bytes([0x01, 0x01]) + uleb(memory_min) + uleb(memory_max)
            )
    if globals_:
        gp = bytearray(uleb(len(globals_)))
        for vt, mut, init in globals_:
            gp += bytes([vt, 0x01 if mut else 0x00]) + init
        out += section(6, bytes(gp))
    if exports:
        ep = bytearray(uleb(len(exports)))
        for nm, idx in exports:
            ep += name(nm) + bytes([0x00]) + uleb(idx)
        out += section(7, bytes(ep))
    if start is not None:
        out += section(8, uleb(start))
    if elems:
        el = bytearray(uleb(len(elems)))
        for off, idxs in elems:
            el += bytes([0x00]) + i32c(off) + bytes([0x0B]) + uleb(len(idxs))
            for i in idxs:
                el += uleb(i)
        out += section(9, bytes(el))
    cp = bytearray(uleb(len(codes)))
    for fi, (nloc, expr) in enumerate(codes):
        body = bytearray()
        decls = local_decls[fi] if local_decls else None
        if decls:
            body += uleb(len(decls))
            for cnt, vt in decls:
                body += uleb(cnt) + bytes([vt])
        elif nloc:
            body += uleb(1) + uleb(nloc) + bytes([I32])
        else:
            body += uleb(0)
        body += expr
        cp += uleb(len(body)) + body
    out += section(10, bytes(cp))
    if data:
        dp = bytearray(uleb(len(data)))
        for off, payload in data:
            dp += bytes([0x00]) + i32c(off) + bytes([0x0B])
            dp += uleb(len(payload)) + payload
        out += section(11, bytes(dp))
    return bytes(out)


def simple(
    expr: bytes,
    *,
    result: int | None = I32,
    nloc: int = 0,
    params: list[int] | None = None,
    memory_min: int | None = None,
    memory_max: int | None = None,
    data: list[tuple[int, bytes]] | None = None,
    local_decls: list[tuple[int, int]] | None = None,
) -> bytes:
    params = params or []
    results = [result] if result is not None else []
    return encode_module(
        types=[(params, results)],
        func_types=[0],
        codes=[(nloc, expr)],
        exports=[("main", 0)],
        memory_min=memory_min,
        memory_max=memory_max,
        data=data,
        local_decls=[local_decls] if local_decls else None,
    )


def hexb(b: bytes) -> str:
    return b.hex()


def i32(n: int) -> int:
    n &= 0xFFFFFFFF
    return n - 0x100000000 if n >= 0x80000000 else n


def i64(n: int) -> int:
    n &= 0xFFFFFFFFFFFFFFFF
    return n - 0x10000000000000000 if n >= 0x8000000000000000 else n


def rotl32(x: int, n: int) -> int:
    n &= 31
    x &= 0xFFFFFFFF
    if n == 0:
        return i32(x)
    return i32(((x << n) | (x >> (32 - n))) & 0xFFFFFFFF)


def rotr32(x: int, n: int) -> int:
    n &= 31
    x &= 0xFFFFFFFF
    if n == 0:
        return i32(x)
    return i32(((x >> n) | (x << (32 - n))) & 0xFFFFFFFF)


def rotl64(x: int, n: int) -> int:
    n &= 63
    x &= 0xFFFFFFFFFFFFFFFF
    if n == 0:
        return i64(x)
    return i64(((x << n) | (x >> (64 - n))) & 0xFFFFFFFFFFFFFFFF)


def rotr64(x: int, n: int) -> int:
    n &= 63
    x &= 0xFFFFFFFFFFFFFFFF
    if n == 0:
        return i64(x)
    return i64(((x >> n) | (x << (64 - n))) & 0xFFFFFFFFFFFFFFFF)


def clz32(x: int) -> int:
    x &= 0xFFFFFFFF
    return 32 if x == 0 else 32 - x.bit_length()


def ctz32(x: int) -> int:
    x &= 0xFFFFFFFF
    return 32 if x == 0 else (x & -x).bit_length() - 1


def pop32(x: int) -> int:
    return (x & 0xFFFFFFFF).bit_count()


def clz64(x: int) -> int:
    x &= 0xFFFFFFFFFFFFFFFF
    return 64 if x == 0 else 64 - x.bit_length()


def ctz64(x: int) -> int:
    x &= 0xFFFFFFFFFFFFFFFF
    return 64 if x == 0 else (x & -x).bit_length() - 1


def pop64(x: int) -> int:
    return (x & 0xFFFFFFFFFFFFFFFF).bit_count()


def f32bits(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", float(x)))[0]


def f64bits(x: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", float(x)))[0]


def nearest_f(x: float) -> float:
    # IEEE ties-to-even; match WASM f*.nearest for the finite cases we emit.
    return float(round(x))


# opcode name -> byte
OPS = {
    "unreachable": 0x00,
    "nop": 0x01,
    "block": 0x02,
    "loop": 0x03,
    "if": 0x04,
    "else": 0x05,
    "end": 0x0B,
    "br": 0x0C,
    "br_if": 0x0D,
    "br_table": 0x0E,
    "return": 0x0F,
    "call": 0x10,
    "call_indirect": 0x11,
    "drop": 0x1A,
    "select": 0x1B,
    "local.get": 0x20,
    "local.set": 0x21,
    "local.tee": 0x22,
    "global.get": 0x23,
    "global.set": 0x24,
    "i32.load": 0x28,
    "i64.load": 0x29,
    "f32.load": 0x2A,
    "f64.load": 0x2B,
    "i32.load8_s": 0x2C,
    "i32.load8_u": 0x2D,
    "i32.load16_s": 0x2E,
    "i32.load16_u": 0x2F,
    "i64.load8_s": 0x30,
    "i64.load8_u": 0x31,
    "i64.load16_s": 0x32,
    "i64.load16_u": 0x33,
    "i64.load32_s": 0x34,
    "i64.load32_u": 0x35,
    "i32.store": 0x36,
    "i64.store": 0x37,
    "f32.store": 0x38,
    "f64.store": 0x39,
    "i32.store8": 0x3A,
    "i32.store16": 0x3B,
    "i64.store8": 0x3C,
    "i64.store16": 0x3D,
    "i64.store32": 0x3E,
    "memory.size": 0x3F,
    "memory.grow": 0x40,
    "i32.const": 0x41,
    "i64.const": 0x42,
    "f32.const": 0x43,
    "f64.const": 0x44,
    "i32.eqz": 0x45,
    "i32.eq": 0x46,
    "i32.ne": 0x47,
    "i32.lt_s": 0x48,
    "i32.lt_u": 0x49,
    "i32.gt_s": 0x4A,
    "i32.gt_u": 0x4B,
    "i32.le_s": 0x4C,
    "i32.le_u": 0x4D,
    "i32.ge_s": 0x4E,
    "i32.ge_u": 0x4F,
    "i64.eqz": 0x50,
    "i64.eq": 0x51,
    "i64.ne": 0x52,
    "i64.lt_s": 0x53,
    "i64.lt_u": 0x54,
    "i64.gt_s": 0x55,
    "i64.gt_u": 0x56,
    "i64.le_s": 0x57,
    "i64.le_u": 0x58,
    "i64.ge_s": 0x59,
    "i64.ge_u": 0x5A,
    "f32.eq": 0x5B,
    "f32.ne": 0x5C,
    "f32.lt": 0x5D,
    "f32.gt": 0x5E,
    "f32.le": 0x5F,
    "f32.ge": 0x60,
    "f64.eq": 0x61,
    "f64.ne": 0x62,
    "f64.lt": 0x63,
    "f64.gt": 0x64,
    "f64.le": 0x65,
    "f64.ge": 0x66,
    "i32.clz": 0x67,
    "i32.ctz": 0x68,
    "i32.popcnt": 0x69,
    "i32.add": 0x6A,
    "i32.sub": 0x6B,
    "i32.mul": 0x6C,
    "i32.div_s": 0x6D,
    "i32.div_u": 0x6E,
    "i32.rem_s": 0x6F,
    "i32.rem_u": 0x70,
    "i32.and": 0x71,
    "i32.or": 0x72,
    "i32.xor": 0x73,
    "i32.shl": 0x74,
    "i32.shr_s": 0x75,
    "i32.shr_u": 0x76,
    "i32.rotl": 0x77,
    "i32.rotr": 0x78,
    "i64.clz": 0x79,
    "i64.ctz": 0x7A,
    "i64.popcnt": 0x7B,
    "i64.add": 0x7C,
    "i64.sub": 0x7D,
    "i64.mul": 0x7E,
    "i64.div_s": 0x7F,
    "i64.div_u": 0x80,
    "i64.rem_s": 0x81,
    "i64.rem_u": 0x82,
    "i64.and": 0x83,
    "i64.or": 0x84,
    "i64.xor": 0x85,
    "i64.shl": 0x86,
    "i64.shr_s": 0x87,
    "i64.shr_u": 0x88,
    "i64.rotl": 0x89,
    "i64.rotr": 0x8A,
    "f32.abs": 0x8B,
    "f32.neg": 0x8C,
    "f32.ceil": 0x8D,
    "f32.floor": 0x8E,
    "f32.trunc": 0x8F,
    "f32.nearest": 0x90,
    "f32.sqrt": 0x91,
    "f32.add": 0x92,
    "f32.sub": 0x93,
    "f32.mul": 0x94,
    "f32.div": 0x95,
    "f32.min": 0x96,
    "f32.max": 0x97,
    "f32.copysign": 0x98,
    "f64.abs": 0x99,
    "f64.neg": 0x9A,
    "f64.ceil": 0x9B,
    "f64.floor": 0x9C,
    "f64.trunc": 0x9D,
    "f64.nearest": 0x9E,
    "f64.sqrt": 0x9F,
    "f64.add": 0xA0,
    "f64.sub": 0xA1,
    "f64.mul": 0xA2,
    "f64.div": 0xA3,
    "f64.min": 0xA4,
    "f64.max": 0xA5,
    "f64.copysign": 0xA6,
    "i32.wrap_i64": 0xA7,
    "i32.trunc_f32_s": 0xA8,
    "i32.trunc_f32_u": 0xA9,
    "i32.trunc_f64_s": 0xAA,
    "i32.trunc_f64_u": 0xAB,
    "i64.extend_i32_s": 0xAC,
    "i64.extend_i32_u": 0xAD,
    "i64.trunc_f32_s": 0xAE,
    "i64.trunc_f32_u": 0xAF,
    "i64.trunc_f64_s": 0xB0,
    "i64.trunc_f64_u": 0xB1,
    "f32.convert_i32_s": 0xB2,
    "f32.convert_i32_u": 0xB3,
    "f32.convert_i64_s": 0xB4,
    "f32.convert_i64_u": 0xB5,
    "f32.demote_f64": 0xB6,
    "f64.convert_i32_s": 0xB7,
    "f64.convert_i32_u": 0xB8,
    "f64.convert_i64_s": 0xB9,
    "f64.convert_i64_u": 0xBA,
    "f64.promote_f32": 0xBB,
    "i32.reinterpret_f32": 0xBC,
    "i64.reinterpret_f64": 0xBD,
    "f32.reinterpret_i32": 0xBE,
    "f64.reinterpret_i64": 0xBF,
}

assert len(OPS) == 172, len(OPS)


class Case:
    def __init__(
        self,
        cid: str,
        family: str,
        opcodes: list[str],
        expect: str,
        wasm: bytes,
        bind: str = "",
    ):
        self.cid = cid
        self.family = family
        self.opcodes = opcodes
        self.expect = expect
        self.wasm = wasm
        self.bind = bind


def ops(*names: str) -> list[str]:
    for n in names:
        if n not in OPS:
            raise KeyError(n)
    return list(names)


def i32_bin(opname: str, a: int, b: int, expect: int) -> Case:
    expr = i32c(a) + i32c(b) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "i32",
        ops("i32.const", opname, "end"),
        f"i32:{i32(expect)}",
        simple(expr),
    )


def i32_un(opname: str, a: int, expect: int) -> Case:
    expr = i32c(a) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "i32",
        ops("i32.const", opname, "end"),
        f"i32:{i32(expect)}",
        simple(expr),
    )


def i64_bin(opname: str, a: int, b: int, expect: int) -> Case:
    expr = i64c(a) + i64c(b) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "i64",
        ops("i64.const", opname, "end"),
        f"i64:{i64(expect)}",
        simple(expr, result=I64),
    )


def i64_un(opname: str, a: int, expect: int) -> Case:
    expr = i64c(a) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "i64",
        ops("i64.const", opname, "end"),
        f"i64:{i64(expect)}",
        simple(expr, result=I64),
    )


def i64_cmp(opname: str, a: int, b: int, expect: int) -> Case:
    expr = i64c(a) + i64c(b) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "i64",
        ops("i64.const", opname, "end"),
        f"i32:{expect}",
        simple(expr, result=I32),
    )


def f32_bin(opname: str, a: float, b: float, expect: float, rkind: str) -> Case:
    expr = f32c(a) + f32c(b) + bytes([OPS[opname], 0x0B])
    if rkind == "i32":
        exp = f"i32:{int(expect)}"
        res = I32
    else:
        exp = f"f32bits:{f32bits(expect)}"
        res = F32
    return Case(
        opname,
        "f32",
        ops("f32.const", opname, "end"),
        exp,
        simple(expr, result=res),
    )


def f32_un(opname: str, a: float, expect: float) -> Case:
    expr = f32c(a) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "f32",
        ops("f32.const", opname, "end"),
        f"f32bits:{f32bits(expect)}",
        simple(expr, result=F32),
    )


def f64_bin(opname: str, a: float, b: float, expect: float, rkind: str) -> Case:
    expr = f64c(a) + f64c(b) + bytes([OPS[opname], 0x0B])
    if rkind == "i32":
        exp = f"i32:{int(expect)}"
        res = I32
    else:
        exp = f"f64bits:{f64bits(expect)}"
        res = F64
    return Case(
        opname,
        "f64",
        ops("f64.const", opname, "end"),
        exp,
        simple(expr, result=res),
    )


def f64_un(opname: str, a: float, expect: float) -> Case:
    expr = f64c(a) + bytes([OPS[opname], 0x0B])
    return Case(
        opname,
        "f64",
        ops("f64.const", opname, "end"),
        f"f64bits:{f64bits(expect)}",
        simple(expr, result=F64),
    )


def memarg() -> bytes:
    return bytes([0x00, 0x00])  # align 0, offset 0


def build_cases() -> list[Case]:
    cases: list[Case] = []

    # --- control ---
    cases.append(
        Case(
            "unreachable",
            "control",
            ops("unreachable", "end"),
            "trap",
            simple(bytes([0x00, 0x0B]), result=None),
        )
    )
    cases.append(
        Case(
            "nop",
            "control",
            ops("nop", "i32.const", "end"),
            "i32:17",
            simple(bytes([0x01]) + i32c(17) + bytes([0x0B])),
        )
    )
    cases.append(
        Case(
            "block",
            "control",
            ops("block", "i32.const", "end"),
            "i32:17",
            simple(bytes([0x02, I32]) + i32c(17) + bytes([0x0B, 0x0B])),
        )
    )
    cases.append(
        Case(
            "loop",
            "control",
            ops("loop", "i32.const", "end"),
            "i32:19",
            simple(bytes([0x03, I32]) + i32c(19) + bytes([0x0B, 0x0B])),
        )
    )
    # if/else: cond 1 -> 100 (not in-crate 10/20)
    cases.append(
        Case(
            "if",
            "control",
            ops("if", "else", "i32.const", "end"),
            "i32:100",
            simple(
                i32c(1)
                + bytes([0x04, I32])
                + i32c(100)
                + bytes([0x05])
                + i32c(200)
                + bytes([0x0B, 0x0B])
            ),
        )
    )
    cases.append(
        Case(
            "else",
            "control",
            ops("if", "else", "i32.const", "end"),
            "i32:200",
            simple(
                i32c(0)
                + bytes([0x04, I32])
                + i32c(100)
                + bytes([0x05])
                + i32c(200)
                + bytes([0x0B, 0x0B])
            ),
        )
    )
    cases.append(
        Case(
            "end",
            "control",
            ops("end", "i32.const"),
            "i32:13",
            simple(i32c(13) + bytes([0x0B])),
        )
    )
    cases.append(
        Case(
            "br",
            "control",
            ops("block", "br", "i32.const", "end"),
            "i32:21",
            simple(
                bytes([0x02, I32])
                + i32c(21)
                + bytes([0x0C, 0x00])
                + i32c(99)
                + bytes([0x0B, 0x0B])
            ),
        )
    )
    # br_if taken: leave 23 on the stack (outer value, empty block)
    cases.append(
        Case(
            "br_if",
            "control",
            ops("block", "br_if", "i32.const", "drop", "end"),
            "i32:23",
            simple(
                i32c(23)
                + bytes([0x02, 0x40])
                + i32c(1)
                + bytes([0x0D, 0x00])
                + bytes([0x1A])
                + i32c(99)
                + bytes([0x0B, 0x0B])
            ),
        )
    )
    # br_table index 2 -> default 200 (in-crate used 10/20/30 via param)
    cases.append(
        Case(
            "br_table",
            "control",
            ops("block", "br_table", "i32.const", "return", "end"),
            "i32:200",
            simple(
                bytes([0x02, 0x40, 0x02, 0x40, 0x02, 0x40])
                + i32c(2)
                + bytes([0x0E, 0x02, 0x00, 0x01, 0x02])
                + bytes([0x0B])
                + i32c(80)
                + bytes([0x0F, 0x0B])
                + i32c(90)
                + bytes([0x0F, 0x0B])
                + i32c(200)
                + bytes([0x0F, 0x0B])
            ),
        )
    )
    cases.append(
        Case(
            "return",
            "control",
            ops("return", "i32.const", "end"),
            "i32:27",
            simple(i32c(27) + bytes([0x0F]) + i32c(9) + bytes([0x0B])),
        )
    )
    # call: defined func 0 calls func 1 which returns 77 (not 42)
    cases.append(
        Case(
            "call",
            "control",
            ops("call", "i32.const", "end"),
            "i32:77",
            encode_module(
                types=[([], [I32])],
                func_types=[0, 0],
                codes=[
                    (0, bytes([0x10, 0x01, 0x0B])),
                    (0, i32c(77) + bytes([0x0B])),
                ],
                exports=[("main", 0)],
            ),
        )
    )
    # call_indirect: table[1] = func 0 returning 88 (in-crate used 7 / 41+1)
    cases.append(
        Case(
            "call_indirect",
            "control",
            ops("call_indirect", "i32.const", "end"),
            "i32:88",
            encode_module(
                types=[([], [I32])],
                func_types=[0, 0],
                codes=[
                    (0, i32c(88) + bytes([0x0B])),
                    (0, i32c(1) + bytes([0x11, 0x00, 0x00, 0x0B])),
                ],
                exports=[("main", 1)],
                table_min=2,
                elems=[(0, [0, 0])],
            ),
        )
    )

    # --- parametric ---
    cases.append(
        Case(
            "drop",
            "parametric",
            ops("drop", "i32.const", "end"),
            "i32:29",
            simple(i32c(29) + i32c(99) + bytes([0x1A, 0x0B])),
        )
    )
    cases.append(
        Case(
            "select",
            "parametric",
            ops("select", "i32.const", "end"),
            "i32:31",
            simple(i32c(31) + i32c(33) + i32c(1) + bytes([0x1B, 0x0B])),
        )
    )

    # --- locals / globals ---
    cases.append(
        Case(
            "local.get",
            "locals",
            ops("local.get", "local.set", "i32.const", "end"),
            "i32:33",
            simple(
                i32c(33) + bytes([0x21, 0x00, 0x20, 0x00, 0x0B]),
                nloc=1,
            ),
        )
    )
    cases.append(
        Case(
            "local.set",
            "locals",
            ops("local.set", "local.get", "i32.const", "end"),
            "i32:34",
            simple(
                i32c(34) + bytes([0x21, 0x00, 0x20, 0x00, 0x0B]),
                nloc=1,
            ),
        )
    )
    cases.append(
        Case(
            "local.tee",
            "locals",
            ops("local.tee", "i32.const", "end"),
            "i32:35",
            simple(i32c(35) + bytes([0x22, 0x00, 0x0B]), nloc=1),
        )
    )
    cases.append(
        Case(
            "global.get",
            "locals",
            ops("global.get", "end"),
            "i32:37",
            encode_module(
                types=[([], [I32])],
                func_types=[0],
                codes=[(0, bytes([0x23, 0x00, 0x0B]))],
                exports=[("main", 0)],
                globals_=[(I32, False, i32c(37) + bytes([0x0B]))],
            ),
        )
    )
    cases.append(
        Case(
            "global.set",
            "locals",
            ops("global.set", "global.get", "i32.const", "end"),
            "i32:39",
            encode_module(
                types=[([], [I32])],
                func_types=[0],
                codes=[
                    (
                        0,
                        i32c(39) + bytes([0x24, 0x00, 0x23, 0x00, 0x0B]),
                    )
                ],
                exports=[("main", 0)],
                globals_=[(I32, True, i32c(1) + bytes([0x0B]))],
            ),
        )
    )

    # --- memory: addr 16 / 24 / 32 / 40, values not 42 ---
    def store_load(store: str, load: str, addr: int, val_expr: bytes, result: int, expect: str) -> Case:
        expr = (
            i32c(addr)
            + val_expr
            + bytes([OPS[store]])
            + memarg()
            + i32c(addr)
            + bytes([OPS[load]])
            + memarg()
            + bytes([0x0B])
        )
        return Case(
            load,
            "memory",
            ops(store, load, "i32.const", "end"),
            expect,
            simple(expr, result=result),
        )

    cases.append(
        store_load(
            "i32.store",
            "i32.load",
            16,
            i32c(0x11223344),
            I32,
            f"i32:{i32(0x11223344)}",
        )
    )
    cases.append(
        store_load(
            "i64.store",
            "i64.load",
            24,
            i64c(0x0102030405060708),
            I64,
            f"i64:{i64(0x0102030405060708)}",
        )
    )
    cases.append(
        store_load(
            "f32.store",
            "f32.load",
            32,
            f32c(8.5),
            F32,
            f"f32bits:{f32bits(8.5)}",
        )
    )
    cases.append(
        store_load(
            "f64.store",
            "f64.load",
            40,
            f64c(9.25),
            F64,
            f"f64bits:{f64bits(9.25)}",
        )
    )

    def narrow_i32(store: str, load: str, addr: int, stored: int, loaded: int) -> Case:
        expr = (
            i32c(addr)
            + i32c(stored)
            + bytes([OPS[store]])
            + memarg()
            + i32c(addr)
            + bytes([OPS[load]])
            + memarg()
            + bytes([0x0B])
        )
        return Case(
            load,
            "memory",
            ops(store, load, "i32.const", "end"),
            f"i32:{i32(loaded)}",
            simple(expr),
        )

    cases.append(narrow_i32("i32.store8", "i32.load8_s", 8, 0x9C, i32(0xFFFFFF9C)))
    cases.append(narrow_i32("i32.store8", "i32.load8_u", 8, 0x9C, 0x9C))
    cases.append(narrow_i32("i32.store16", "i32.load16_s", 10, 0xBEEF, i32(0xFFFFBEEF)))
    cases.append(narrow_i32("i32.store16", "i32.load16_u", 10, 0xBEEF, 0xBEEF))

    def narrow_i64(store: str, load: str, addr: int, stored: int, loaded: int) -> Case:
        expr = (
            i32c(addr)
            + i64c(stored)
            + bytes([OPS[store]])
            + memarg()
            + i32c(addr)
            + bytes([OPS[load]])
            + memarg()
            + bytes([0x0B])
        )
        return Case(
            load,
            "memory",
            ops(store, load, "i32.const", "i64.const", "end"),
            f"i64:{i64(loaded)}",
            simple(expr, result=I64),
        )

    cases.append(narrow_i64("i64.store8", "i64.load8_s", 12, 0x9C, i64(-100)))
    cases.append(narrow_i64("i64.store8", "i64.load8_u", 12, 0x9C, 0x9C))
    cases.append(narrow_i64("i64.store16", "i64.load16_s", 14, 0xBEEF, i64(-16657)))
    cases.append(narrow_i64("i64.store16", "i64.load16_u", 14, 0xBEEF, 0xBEEF))
    cases.append(narrow_i64("i64.store32", "i64.load32_s", 20, 0x80000001, i64(-2147483647)))
    cases.append(narrow_i64("i64.store32", "i64.load32_u", 20, 0x80000001, 0x80000001))

    # store-only opcodes still credited via the pairs above; add named store rows
    # that reuse the same modules by aliasing ids — coverage uses opcode names.
    # memory.size / grow: grow by 3, old size 1 (in-crate grew by 2 or 1)
    cases.append(
        Case(
            "memory.size",
            "memory",
            ops("memory.size", "end"),
            "i32:1",
            simple(bytes([0x3F, 0x00, 0x0B])),
        )
    )
    cases.append(
        Case(
            "memory.grow",
            "memory",
            ops("memory.grow", "i32.const", "end"),
            "i32:1",
            simple(i32c(3) + bytes([0x40, 0x00, 0x0B])),
        )
    )
    # Store-named rows: each builds its OWN module (store, then read back with
    # the load named in its opcode column) instead of cloning a load row's
    # bytes, so the column never claims an opcode the module lacks.
    def store_roundtrip(cid, store, load, addr, push, expect, result=I32):
        return Case(
            cid,
            "memory",
            ops(store, load, "end"),
            expect,
            simple(
                i32c(addr)
                + push
                + bytes([OPS[store], 0x00, 0x00])
                + i32c(addr)
                + bytes([OPS[load], 0x00, 0x00, 0x0B]),
                result=result,
            ),
        )

    cases.append(
        store_roundtrip(
            "i32.store", "i32.store", "i32.load", 32, i32c(0x12345678),
            f"i32:{i32(0x12345678)}",
        )
    )
    cases.append(
        store_roundtrip("i32.store8", "i32.store8", "i32.load8_u", 36, i32c(0x1FF), "i32:255")
    )
    cases.append(
        store_roundtrip(
            "i32.store16", "i32.store16", "i32.load16_u", 38, i32c(0x1BEEF), "i32:48879"
        )
    )
    cases.append(
        store_roundtrip(
            "i64.store", "i64.store", "i64.load", 40, i64c(0x0102030405060708),
            f"i64:{0x0102030405060708}", result=I64,
        )
    )
    cases.append(
        store_roundtrip(
            "i64.store8", "i64.store8", "i64.load8_u", 48, i64c(0x1FF), "i64:255", result=I64
        )
    )
    cases.append(
        store_roundtrip(
            "i64.store16", "i64.store16", "i64.load16_u", 50, i64c(0x1BEEF), "i64:48879",
            result=I64,
        )
    )
    cases.append(
        store_roundtrip(
            "i64.store32", "i64.store32", "i64.load32_u", 52, i64c(0x180000001),
            "i64:2147483649", result=I64,
        )
    )
    cases.append(
        store_roundtrip(
            "f32.store", "f32.store", "f32.load", 56, f32c(12.5),
            f"f32bits:{f32bits(12.5)}", result=F32,
        )
    )
    cases.append(
        store_roundtrip(
            "f64.store", "f64.store", "f64.load", 64, f64c(12.5),
            f"f64bits:{f64bits(12.5)}", result=F64,
        )
    )

    # --- i32 (operands 17/31, not 40/2 or 6/7) ---
    cases.append(
        Case(
            "i32.const",
            "i32",
            ops("i32.const", "end"),
            "i32:17",
            simple(i32c(17) + bytes([0x0B])),
        )
    )
    cases.append(i32_un("i32.eqz", 17, 0))
    cases.append(i32_bin("i32.eq", 17, 17, 1))
    cases.append(i32_bin("i32.ne", 17, 31, 1))
    cases.append(i32_bin("i32.lt_s", -17, 31, 1))
    cases.append(i32_bin("i32.lt_u", -17, 31, 0))
    cases.append(i32_bin("i32.gt_s", 31, -17, 1))
    cases.append(i32_bin("i32.gt_u", 31, -17, 0))
    cases.append(i32_bin("i32.le_s", 17, 31, 1))
    cases.append(i32_bin("i32.le_u", 17, 17, 1))
    cases.append(i32_bin("i32.ge_s", 31, 17, 1))
    cases.append(i32_bin("i32.ge_u", -17, 31, 1))
    cases.append(i32_un("i32.clz", 16, 27))
    cases.append(i32_un("i32.ctz", 16, 4))
    cases.append(i32_un("i32.popcnt", 0xF0, 4))
    cases.append(i32_bin("i32.add", 17, 31, 48))
    cases.append(i32_bin("i32.sub", 31, 17, 14))
    cases.append(i32_bin("i32.mul", 17, 3, 51))
    cases.append(i32_bin("i32.div_s", 99, 4, 24))
    cases.append(i32_bin("i32.div_u", 99, 4, 24))
    cases.append(i32_bin("i32.rem_s", 99, 4, 3))
    cases.append(i32_bin("i32.rem_u", 99, 4, 3))
    cases.append(i32_bin("i32.and", 0x3C, 0x0F, 0x0C))
    cases.append(i32_bin("i32.or", 0x30, 0x0C, 0x3C))
    cases.append(i32_bin("i32.xor", 0x3C, 0x0F, 0x33))
    cases.append(i32_bin("i32.shl", 3, 4, 48))
    cases.append(i32_bin("i32.shr_s", -16, 2, -4))
    cases.append(i32_bin("i32.shr_u", -16, 2, i32(0x3FFFFFFC)))
    cases.append(i32_bin("i32.rotl", 0xA5, 4, rotl32(0xA5, 4)))
    cases.append(i32_bin("i32.rotr", 0xA5, 4, rotr32(0xA5, 4)))

    # --- i64 (100/23, not 42/1 or 6/7) ---
    cases.append(
        Case(
            "i64.const",
            "i64",
            ops("i64.const", "end"),
            "i64:100",
            simple(i64c(100) + bytes([0x0B]), result=I64),
        )
    )
    cases.append(
        Case(
            "i64.eqz",
            "i64",
            ops("i64.eqz", "i64.const", "end"),
            "i32:0",
            simple(i64c(100) + bytes([0x50, 0x0B])),
        )
    )
    cases.append(i64_cmp("i64.eq", 100, 100, 1))
    cases.append(i64_cmp("i64.ne", 100, 23, 1))
    cases.append(i64_cmp("i64.lt_s", -100, 23, 1))
    cases.append(i64_cmp("i64.lt_u", -100, 23, 0))
    cases.append(i64_cmp("i64.gt_s", 23, -100, 1))
    cases.append(i64_cmp("i64.gt_u", 23, -100, 0))
    cases.append(i64_cmp("i64.le_s", 23, 100, 1))
    cases.append(i64_cmp("i64.le_u", 100, 100, 1))
    cases.append(i64_cmp("i64.ge_s", 100, 23, 1))
    cases.append(i64_cmp("i64.ge_u", -100, 23, 1))
    cases.append(i64_un("i64.clz", 16, 59))
    cases.append(i64_un("i64.ctz", 16, 4))
    cases.append(i64_un("i64.popcnt", 0xF0, 4))
    cases.append(i64_bin("i64.add", 100, 23, 123))
    cases.append(i64_bin("i64.sub", 100, 23, 77))
    cases.append(i64_bin("i64.mul", 11, 13, 143))
    cases.append(i64_bin("i64.div_s", -99, 4, -24))
    cases.append(i64_bin("i64.div_u", 99, 4, 24))
    cases.append(i64_bin("i64.rem_s", -99, 4, -3))
    cases.append(i64_bin("i64.rem_u", 99, 4, 3))
    cases.append(i64_bin("i64.and", 0x3C, 0x0F, 0x0C))
    cases.append(i64_bin("i64.or", 0x30, 0x0C, 0x3C))
    cases.append(i64_bin("i64.xor", 0x3C, 0x0F, 0x33))
    cases.append(i64_bin("i64.shl", 3, 5, 96))
    cases.append(i64_bin("i64.shr_s", -32, 2, -8))
    cases.append(i64_bin("i64.shr_u", -32, 2, i64(0x3FFFFFFFFFFFFFF8)))
    cases.append(i64_bin("i64.rotl", 0xA5, 8, rotl64(0xA5, 8)))
    cases.append(i64_bin("i64.rotr", 0xA5, 8, rotr64(0xA5, 8)))

    # --- f32 (8.0/2.0, not 1.5/2.5) ---
    cases.append(
        Case(
            "f32.const",
            "f32",
            ops("f32.const", "end"),
            f"f32bits:{f32bits(8.0)}",
            simple(f32c(8.0) + bytes([0x0B]), result=F32),
        )
    )
    cases.append(f32_bin("f32.eq", 8.0, 8.0, 1, "i32"))
    cases.append(f32_bin("f32.ne", 8.0, 2.0, 1, "i32"))
    cases.append(f32_bin("f32.lt", 2.0, 8.0, 1, "i32"))
    cases.append(f32_bin("f32.gt", 8.0, 2.0, 1, "i32"))
    cases.append(f32_bin("f32.le", 8.0, 8.0, 1, "i32"))
    cases.append(f32_bin("f32.ge", 8.0, 2.0, 1, "i32"))
    cases.append(f32_un("f32.abs", -8.25, 8.25))
    cases.append(f32_un("f32.neg", 8.25, -8.25))
    cases.append(f32_un("f32.ceil", 8.25, 9.0))
    cases.append(f32_un("f32.floor", 8.25, 8.0))
    cases.append(f32_un("f32.trunc", -8.75, -8.0))
    cases.append(f32_un("f32.nearest", 8.25, nearest_f(8.25)))
    cases.append(f32_un("f32.sqrt", 16.0, 4.0))
    cases.append(f32_bin("f32.add", 8.0, 2.25, 10.25, "f32"))
    cases.append(f32_bin("f32.sub", 8.0, 2.25, 5.75, "f32"))
    cases.append(f32_bin("f32.mul", 8.0, 2.25, 18.0, "f32"))
    cases.append(f32_bin("f32.div", 9.0, 2.0, 4.5, "f32"))
    cases.append(f32_bin("f32.min", 8.0, 2.0, 2.0, "f32"))
    cases.append(f32_bin("f32.max", 8.0, 2.0, 8.0, "f32"))
    cases.append(f32_bin("f32.copysign", 8.25, -1.0, -8.25, "f32"))

    # --- f64 (16.0/4.0) ---
    cases.append(
        Case(
            "f64.const",
            "f64",
            ops("f64.const", "end"),
            f"f64bits:{f64bits(16.0)}",
            simple(f64c(16.0) + bytes([0x0B]), result=F64),
        )
    )
    cases.append(f64_bin("f64.eq", 16.0, 16.0, 1, "i32"))
    cases.append(f64_bin("f64.ne", 16.0, 4.0, 1, "i32"))
    cases.append(f64_bin("f64.lt", 4.0, 16.0, 1, "i32"))
    cases.append(f64_bin("f64.gt", 16.0, 4.0, 1, "i32"))
    cases.append(f64_bin("f64.le", 16.0, 16.0, 1, "i32"))
    cases.append(f64_bin("f64.ge", 16.0, 4.0, 1, "i32"))
    cases.append(f64_un("f64.abs", -16.5, 16.5))
    cases.append(f64_un("f64.neg", 16.5, -16.5))
    cases.append(f64_un("f64.ceil", 16.25, 17.0))
    cases.append(f64_un("f64.floor", 16.25, 16.0))
    cases.append(f64_un("f64.trunc", -16.75, -16.0))
    cases.append(f64_un("f64.nearest", 16.25, nearest_f(16.25)))
    cases.append(f64_un("f64.sqrt", 64.0, 8.0))
    cases.append(f64_bin("f64.add", 16.0, 4.5, 20.5, "f64"))
    cases.append(f64_bin("f64.sub", 16.0, 4.5, 11.5, "f64"))
    cases.append(f64_bin("f64.mul", 16.0, 4.5, 72.0, "f64"))
    cases.append(f64_bin("f64.div", 18.0, 4.0, 4.5, "f64"))
    cases.append(f64_bin("f64.min", 16.0, 4.0, 4.0, "f64"))
    cases.append(f64_bin("f64.max", 16.0, 4.0, 16.0, "f64"))
    cases.append(f64_bin("f64.copysign", 16.5, -1.0, -16.5, "f64"))

    # --- conversions (values not 42 / 1.0 / 1.5) ---
    def conv(name_: str, expr: bytes, result: int, expect: str) -> Case:
        return Case(
            name_,
            "conv",
            ops(name_, "end"),
            expect,
            simple(expr, result=result),
        )

    cases.append(
        conv(
            "i32.wrap_i64",
            i64c(0x2_0000_0011) + bytes([0xA7, 0x0B]),
            I32,
            "i32:17",
        )
    )
    cases.append(
        conv(
            "i32.trunc_f32_s",
            f32c(17.9) + bytes([0xA8, 0x0B]),
            I32,
            "i32:17",
        )
    )
    cases.append(
        conv(
            "i32.trunc_f32_u",
            f32c(17.9) + bytes([0xA9, 0x0B]),
            I32,
            "i32:17",
        )
    )
    cases.append(
        conv(
            "i32.trunc_f64_s",
            f64c(-17.9) + bytes([0xAA, 0x0B]),
            I32,
            "i32:-17",
        )
    )
    cases.append(
        conv(
            "i32.trunc_f64_u",
            f64c(17.9) + bytes([0xAB, 0x0B]),
            I32,
            "i32:17",
        )
    )
    cases.append(
        conv(
            "i64.extend_i32_s",
            i32c(-17) + bytes([0xAC, 0x0B]),
            I64,
            "i64:-17",
        )
    )
    cases.append(
        conv(
            "i64.extend_i32_u",
            i32c(-17) + bytes([0xAD, 0x0B]),
            I64,
            f"i64:{i64(0xFFFFFFEF)}",
        )
    )
    cases.append(
        conv(
            "i64.trunc_f32_s",
            f32c(-17.9) + bytes([0xAE, 0x0B]),
            I64,
            "i64:-17",
        )
    )
    cases.append(
        conv(
            "i64.trunc_f32_u",
            f32c(17.9) + bytes([0xAF, 0x0B]),
            I64,
            "i64:17",
        )
    )
    cases.append(
        conv(
            "i64.trunc_f64_s",
            f64c(-17.9) + bytes([0xB0, 0x0B]),
            I64,
            "i64:-17",
        )
    )
    cases.append(
        conv(
            "i64.trunc_f64_u",
            f64c(17.9) + bytes([0xB1, 0x0B]),
            I64,
            "i64:17",
        )
    )
    cases.append(
        conv(
            "f32.convert_i32_s",
            i32c(-17) + bytes([0xB2, 0x0B]),
            F32,
            f"f32bits:{f32bits(-17.0)}",
        )
    )
    cases.append(
        conv(
            "f32.convert_i32_u",
            i32c(-17) + bytes([0xB3, 0x0B]),
            F32,
            f"f32bits:{f32bits(float(0xFFFFFFEF))}",
        )
    )
    cases.append(
        conv(
            "f32.convert_i64_s",
            i64c(-17) + bytes([0xB4, 0x0B]),
            F32,
            f"f32bits:{f32bits(-17.0)}",
        )
    )
    cases.append(
        conv(
            "f32.convert_i64_u",
            i64c(17) + bytes([0xB5, 0x0B]),
            F32,
            f"f32bits:{f32bits(17.0)}",
        )
    )
    cases.append(
        conv(
            "f32.demote_f64",
            f64c(8.5) + bytes([0xB6, 0x0B]),
            F32,
            f"f32bits:{f32bits(8.5)}",
        )
    )
    cases.append(
        conv(
            "f64.convert_i32_s",
            i32c(-17) + bytes([0xB7, 0x0B]),
            F64,
            f"f64bits:{f64bits(-17.0)}",
        )
    )
    cases.append(
        conv(
            "f64.convert_i32_u",
            i32c(-17) + bytes([0xB8, 0x0B]),
            F64,
            f"f64bits:{f64bits(float(0xFFFFFFEF))}",
        )
    )
    cases.append(
        conv(
            "f64.convert_i64_s",
            i64c(-17) + bytes([0xB9, 0x0B]),
            F64,
            f"f64bits:{f64bits(-17.0)}",
        )
    )
    cases.append(
        conv(
            "f64.convert_i64_u",
            i64c(17) + bytes([0xBA, 0x0B]),
            F64,
            f"f64bits:{f64bits(17.0)}",
        )
    )
    cases.append(
        conv(
            "f64.promote_f32",
            f32c(8.5) + bytes([0xBB, 0x0B]),
            F64,
            f"f64bits:{f64bits(8.5)}",
        )
    )
    cases.append(
        conv(
            "i32.reinterpret_f32",
            f32c(8.0) + bytes([0xBC, 0x0B]),
            I32,
            f"i32:{i32(f32bits(8.0))}",
        )
    )
    cases.append(
        conv(
            "i64.reinterpret_f64",
            f64c(8.0) + bytes([0xBD, 0x0B]),
            I64,
            f"i64:{i64(f64bits(8.0))}",
        )
    )
    cases.append(
        conv(
            "f32.reinterpret_i32",
            i32c(f32bits(8.0)) + bytes([0xBE, 0x0B]),
            F32,
            f"f32bits:{f32bits(8.0)}",
        )
    )
    cases.append(
        conv(
            "f64.reinterpret_i64",
            i64c(f64bits(8.0)) + bytes([0xBF, 0x0B]),
            F64,
            f"f64bits:{f64bits(8.0)}",
        )
    )

    # --- host import table ---
    # bound: host.mul (13, 17) -> 221  (in-crate was +1 on 41)
    cases.append(
        Case(
            "host.mul",
            "host",
            ops("call", "i32.const", "end"),
            "i32:221",
            encode_module(
                types=[([I32, I32], [I32]), ([], [I32])],
                func_types=[1],
                codes=[
                    (
                        0,
                        i32c(13) + i32c(17) + bytes([0x10, 0x00, 0x0B]),
                    )
                ],
                exports=[("main", 1)],
                imports=[("host", "mul", 0)],
            ),
            bind="host.mul",
        )
    )
    cases.append(
        Case(
            "host.unbound",
            "host",
            ops("call", "end"),
            "trap",
            encode_module(
                types=[([], [I32]), ([], [I32])],
                func_types=[1],
                codes=[(0, bytes([0x10, 0x00, 0x0B]))],
                exports=[("main", 1)],
                imports=[("env", "missing", 0)],
            ),
        )
    )

    return cases


def extra_cases() -> list[Case]:
    """One extra golden per family; operands/layout differ from in-crate tests."""
    extras: list[Case] = []
    # control: two nested blocks, br 1 leaves 55 (in-crate br left 7)
    extras.append(
        Case(
            "extra.control.nested_br",
            "control",
            ops("block", "br", "i32.const", "end"),
            "i32:55",
            simple(
                bytes([0x02, I32, 0x02, I32])
                + i32c(55)
                + bytes([0x0C, 0x01])
                + i32c(1)
                + bytes([0x0B, 0x0B, 0x0B])
            ),
        )
    )
    # parametric: select false after a dropped dummy, picks 66
    extras.append(
        Case(
            "extra.parametric.select_false",
            "parametric",
            ops("select", "drop", "i32.const", "end"),
            "i32:66",
            simple(
                i32c(99)
                + bytes([0x1A])
                + i32c(55)
                + i32c(66)
                + i32c(0)
                + bytes([0x1B, 0x0B])
            ),
        )
    )
    # locals: two locals, tee 0 then set 1, add them (71+2=73)
    extras.append(
        Case(
            "extra.locals.two_slots",
            "locals",
            ops("local.tee", "local.set", "local.get", "i32.add", "end"),
            "i32:73",
            simple(
                i32c(71)
                + bytes([0x22, 0x00])
                + i32c(2)
                + bytes([0x21, 0x01, 0x20, 0x00, 0x20, 0x01, 0x6A, 0x0B]),
                nloc=2,
            ),
        )
    )
    # memory: addr 4 + offset 12 = 16, value 0x0A0B0C0D (in-crate used addr 0 offset 4 = 7)
    extras.append(
        Case(
            "extra.memory.offset12",
            "memory",
            ops("i32.store", "i32.load", "end"),
            f"i32:{i32(0x0A0B0C0D)}",
            simple(
                i32c(4)
                + i32c(0x0A0B0C0D)
                + bytes([0x36, 0x00, 0x0C])
                + i32c(16)
                + bytes([0x28, 0x00, 0x00, 0x0B])
            ),
        )
    )
    # i32: 17+31 on a stack that first dropped 100
    extras.append(
        Case(
            "extra.i32.deep_add",
            "i32",
            ops("i32.add", "drop", "i32.const", "end"),
            "i32:48",
            simple(i32c(100) + bytes([0x1A]) + i32c(17) + i32c(31) + bytes([0x6A, 0x0B])),
        )
    )
    extras.append(
        Case(
            "extra.i64.sub",
            "i64",
            ops("i64.sub", "i64.const", "i64.mul", "end"),
            "i64:-154",
            simple(
                i64c(23) + i64c(100) + bytes([0x7D]) + i64c(2) + bytes([0x7E, 0x0B]),
                result=I64,
            ),
        )
    )
    extras.append(
        Case(
            "extra.f32.mul",
            "f32",
            ops("f32.mul", "f32.const", "end"),
            f"f32bits:{f32bits(16.5)}",
            simple(f32c(8.25) + f32c(2.0) + bytes([0x94, 0x0B]), result=F32),
        )
    )
    extras.append(
        Case(
            "extra.f64.div",
            "f64",
            ops("f64.div", "f64.const", "end"),
            f"f64bits:{f64bits(8.25)}",
            simple(f64c(16.5) + f64c(2.0) + bytes([0xA3, 0x0B]), result=F64),
        )
    )
    extras.append(
        Case(
            "extra.conv.wrap",
            "conv",
            ops("i32.wrap_i64", "i64.const", "end"),
            "i32:19",
            simple(i64c(0x3_0000_0013) + bytes([0xA7, 0x0B])),
        )
    )
    # host: add19 on 16 -> 35, and host writes i32 35 at mem[8]
    extras.append(
        Case(
            "extra.host.add19",
            "host",
            ops("call", "i32.const", "end"),
            "i32:35",
            encode_module(
                types=[([I32], [I32]), ([], [I32])],
                func_types=[1],
                codes=[(0, i32c(16) + bytes([0x10, 0x00, 0x0B]))],
                exports=[("main", 1)],
                imports=[("host", "add19", 0)],
            ),
            bind="host.add19",
        )
    )
    return extras



def edge_cases() -> list[Case]:
    """Spec-boundary goldens: the cases a wiring-only suite cannot fail.

    Every expectation here is derived from the WASM 1.0 / IEEE-754 text, not
    from running the Rust interpreter. Hardware-chosen NaN results (0/0,
    inf-inf, sqrt(-1)) are asserted as `f32nan`/`f64nan` rather than raw bits,
    because the sign of those NaNs is platform-dependent and the spec admits
    either.
    """
    e: list[Case] = []

    # --- i32: traps, signedness, and the shift-count mask -----------------
    e.append(Case("edge.i32.div_s_overflow", "i32", ops("i32.const", "i32.div_s", "end"),
                  "trap", simple(i32c(-2147483648) + i32c(-1) + bytes([0x6D, 0x0B]))))
    e.append(Case("edge.i32.rem_s_min_mod_neg1", "i32", ops("i32.const", "i32.rem_s", "end"),
                  "i32:0", simple(i32c(-2147483648) + i32c(-1) + bytes([0x6F, 0x0B]))))
    e.append(Case("edge.i32.div_u_by_zero", "i32", ops("i32.const", "i32.div_u", "end"),
                  "trap", simple(i32c(1) + i32c(0) + bytes([0x6E, 0x0B]))))
    e.append(Case("edge.i32.rem_u_by_zero", "i32", ops("i32.const", "i32.rem_u", "end"),
                  "trap", simple(i32c(1) + i32c(0) + bytes([0x70, 0x0B]))))
    e.append(Case("edge.i32.div_s_by_zero", "i32", ops("i32.const", "i32.div_s", "end"),
                  "trap", simple(i32c(1) + i32c(0) + bytes([0x6D, 0x0B]))))
    # truncation is toward zero, not floor: -99/4 = -24 rem -3
    e.append(Case("edge.i32.div_s_truncates_toward_zero", "i32",
                  ops("i32.const", "i32.div_s", "end"), "i32:-24",
                  simple(i32c(-99) + i32c(4) + bytes([0x6D, 0x0B]))))
    e.append(Case("edge.i32.rem_s_sign_follows_dividend", "i32",
                  ops("i32.const", "i32.rem_s", "end"), "i32:-3",
                  simple(i32c(-99) + i32c(4) + bytes([0x6F, 0x0B]))))
    e.append(Case("edge.i32.div_u_unsigned", "i32", ops("i32.const", "i32.div_u", "end"),
                  "i32:2147483647", simple(i32c(-1) + i32c(2) + bytes([0x6E, 0x0B]))))
    e.append(Case("edge.i32.shl_count_mod32", "i32", ops("i32.const", "i32.shl", "end"),
                  "i32:1", simple(i32c(1) + i32c(32) + bytes([0x74, 0x0B]))))
    # count 0x80000001: & 31 == 1, so a 255-wide or 63-wide mask is visible
    e.append(Case("edge.i32.shl_count_mod32_high", "i32", ops("i32.const", "i32.shl", "end"),
                  "i32:2", simple(i32c(1) + i32c(-2147483647) + bytes([0x74, 0x0B]))))
    e.append(Case("edge.i32.shr_u_zero_fill", "i32", ops("i32.const", "i32.shr_u", "end"),
                  "i32:2147483647", simple(i32c(-1) + i32c(1) + bytes([0x76, 0x0B]))))
    e.append(Case("edge.i32.shr_s_sign_fill", "i32", ops("i32.const", "i32.shr_s", "end"),
                  "i32:-1", simple(i32c(-1) + i32c(1) + bytes([0x75, 0x0B]))))
    e.append(Case("edge.i32.rotl_wrap_top_bit", "i32", ops("i32.const", "i32.rotl", "end"),
                  "i32:1", simple(i32c(-2147483648) + i32c(1) + bytes([0x77, 0x0B]))))
    e.append(Case("edge.i32.rotr_count_mod32", "i32", ops("i32.const", "i32.rotr", "end"),
                  "i32:1", simple(i32c(1) + i32c(32) + bytes([0x78, 0x0B]))))
    e.append(Case("edge.i32.clz_zero", "i32", ops("i32.const", "i32.clz", "end"),
                  "i32:32", simple(i32c(0) + bytes([0x67, 0x0B]))))
    e.append(Case("edge.i32.ctz_zero", "i32", ops("i32.const", "i32.ctz", "end"),
                  "i32:32", simple(i32c(0) + bytes([0x68, 0x0B]))))
    e.append(Case("edge.i32.le_u_is_unsigned", "i32", ops("i32.const", "i32.le_u", "end"),
                  "i32:0", simple(i32c(-1) + i32c(1) + bytes([0x4D, 0x0B]))))
    e.append(Case("edge.i32.add_wraps_at_max", "i32", ops("i32.const", "i32.add", "end"),
                  "i32:-2147483648", simple(i32c(2147483647) + i32c(1) + bytes([0x6A, 0x0B]))))

    # --- i64: same edges at 64-bit width ----------------------------------
    e.append(Case("edge.i64.div_s_min_neg1", "i64", ops("i64.const", "i64.div_s", "end"),
                  "trap", simple(i64c(-(2 ** 63)) + i64c(-1) + bytes([0x7F, 0x0B]), result=I64)))
    e.append(Case("edge.i64.rem_s_min_neg1", "i64", ops("i64.const", "i64.rem_s", "end"),
                  "i64:0", simple(i64c(-(2 ** 63)) + i64c(-1) + bytes([0x81, 0x0B]), result=I64)))
    e.append(Case("edge.i64.div_u_by_zero", "i64", ops("i64.const", "i64.div_u", "end"),
                  "trap", simple(i64c(1) + i64c(0) + bytes([0x80, 0x0B]), result=I64)))
    e.append(Case("edge.i64.rem_u_by_zero", "i64", ops("i64.const", "i64.rem_u", "end"),
                  "trap", simple(i64c(1) + i64c(0) + bytes([0x82, 0x0B]), result=I64)))
    e.append(Case("edge.i64.shl_by63", "i64", ops("i64.const", "i64.shl", "end"),
                  f"i64:{i64(1 << 63)}", simple(i64c(1) + i64c(63) + bytes([0x86, 0x0B]), result=I64)))
    e.append(Case("edge.i64.shl_count_mod64", "i64", ops("i64.const", "i64.shl", "end"),
                  "i64:1", simple(i64c(1) + i64c(64) + bytes([0x86, 0x0B]), result=I64)))
    e.append(Case("edge.i64.shr_u_neg1_by32", "i64", ops("i64.const", "i64.shr_u", "end"),
                  "i64:4294967295", simple(i64c(-1) + i64c(32) + bytes([0x88, 0x0B]), result=I64)))
    e.append(Case("edge.i64.rotr_by32", "i64", ops("i64.const", "i64.rotr", "end"),
                  "i64:4294967296", simple(i64c(1) + i64c(32) + bytes([0x8A, 0x0B]), result=I64)))
    e.append(Case("edge.i64.ctz_zero", "i64", ops("i64.const", "i64.ctz", "end"),
                  "i64:64", simple(i64c(0) + bytes([0x7A, 0x0B]), result=I64)))
    e.append(Case("edge.i64.clz_zero", "i64", ops("i64.const", "i64.clz", "end"),
                  "i64:64", simple(i64c(0) + bytes([0x79, 0x0B]), result=I64)))
    e.append(Case("edge.i64.lt_u_high32", "i64", ops("i64.const", "i64.lt_u", "end"),
                  "i32:0", simple(i64c(2 ** 32) + i64c(2 ** 32 - 1) + bytes([0x54, 0x0B]))))
    e.append(Case("edge.i64.eqz_high_bits_only", "i64", ops("i64.const", "i64.eqz", "end"),
                  "i32:0", simple(i64c(2 ** 32) + bytes([0x50, 0x0B]))))

    # --- f32: NaN, signed zero, ties-to-even ------------------------------
    e.append(Case("edge.f32.min_propagates_nan", "f32", ops("f32.const", "f32.min", "end"),
                  "f32nan", simple(f32c_bits(0x7FC00001) + f32c(1.0) + bytes([0x96, 0x0B]), result=F32)))
    e.append(Case("edge.f32.max_propagates_nan", "f32", ops("f32.const", "f32.max", "end"),
                  "f32nan", simple(f32c(1.0) + f32c_bits(0x7FC00001) + bytes([0x97, 0x0B]), result=F32)))
    e.append(Case("edge.f32.min_zero_signs", "f32", ops("f32.const", "f32.min", "end"),
                  "f32bits:0x80000000",
                  simple(f32c_bits(0x00000000) + f32c_bits(0x80000000) + bytes([0x96, 0x0B]), result=F32)))
    e.append(Case("edge.f32.max_zero_signs", "f32", ops("f32.const", "f32.max", "end"),
                  "f32bits:0x0",
                  simple(f32c_bits(0x00000000) + f32c_bits(0x80000000) + bytes([0x97, 0x0B]), result=F32)))
    e.append(Case("edge.f32.nearest_tie_even", "f32", ops("f32.const", "f32.nearest", "end"),
                  f"f32bits:{f32bits(2.0)}", simple(f32c(2.5) + bytes([0x90, 0x0B]), result=F32)))
    e.append(Case("edge.f32.nearest_neg_half", "f32", ops("f32.const", "f32.nearest", "end"),
                  "f32bits:0x80000000", simple(f32c(-0.5) + bytes([0x90, 0x0B]), result=F32)))
    e.append(Case("edge.f32.ceil_neg_half", "f32", ops("f32.const", "f32.ceil", "end"),
                  "f32bits:0x80000000", simple(f32c(-0.5) + bytes([0x8D, 0x0B]), result=F32)))
    e.append(Case("edge.f32.copysign_nan", "f32", ops("f32.const", "f32.copysign", "end"),
                  "f32bits:0xffc00001",
                  simple(f32c_bits(0x7FC00001) + f32c(-1.0) + bytes([0x98, 0x0B]), result=F32)))
    e.append(Case("edge.f32.div_by_neg_zero", "f32", ops("f32.const", "f32.div", "end"),
                  "f32bits:0xff800000",
                  simple(f32c(1.0) + f32c_bits(0x80000000) + bytes([0x95, 0x0B]), result=F32)))
    e.append(Case("edge.f32.div_zero_by_zero_is_nan", "f32", ops("f32.const", "f32.div", "end"),
                  "f32nan", simple(f32c(0.0) + f32c(0.0) + bytes([0x95, 0x0B]), result=F32)))
    e.append(Case("edge.f32.ne_nan_is_true", "f32", ops("f32.const", "f32.ne", "end"),
                  "i32:1", simple(f32c_bits(0x7FC00001) + f32c_bits(0x7FC00001) + bytes([0x5C, 0x0B]))))
    e.append(Case("edge.f32.lt_nan_is_false", "f32", ops("f32.const", "f32.lt", "end"),
                  "i32:0", simple(f32c_bits(0x7FC00001) + f32c(1.0) + bytes([0x5D, 0x0B]))))
    e.append(Case("edge.f32.sqrt_neg_is_nan", "f32", ops("f32.const", "f32.sqrt", "end"),
                  "f32nan", simple(f32c(-1.0) + bytes([0x91, 0x0B]), result=F32)))

    # --- f64: same shape, plus a value f32 cannot hold --------------------
    e.append(Case("edge.f64.nearest_tie_even", "f64", ops("f64.const", "f64.nearest", "end"),
                  f"f64bits:{f64bits(2.0)}", simple(f64c(2.5) + bytes([0x9E, 0x0B]), result=F64)))
    e.append(Case("edge.f64.nearest_neg_half", "f64", ops("f64.const", "f64.nearest", "end"),
                  "f64bits:0x8000000000000000", simple(f64c(-0.5) + bytes([0x9E, 0x0B]), result=F64)))
    e.append(Case("edge.f64.ceil_neg_half", "f64", ops("f64.const", "f64.ceil", "end"),
                  "f64bits:0x8000000000000000", simple(f64c(-0.5) + bytes([0x9B, 0x0B]), result=F64)))
    e.append(Case("edge.f64.min_zero_signs", "f64", ops("f64.const", "f64.min", "end"),
                  "f64bits:0x8000000000000000",
                  simple(f64c(0.0) + f64c_bits(0x8000000000000000) + bytes([0xA4, 0x0B]), result=F64)))
    e.append(Case("edge.f64.max_zero_signs", "f64", ops("f64.const", "f64.max", "end"),
                  "f64bits:0x0",
                  simple(f64c_bits(0x8000000000000000) + f64c(0.0) + bytes([0xA5, 0x0B]), result=F64)))
    e.append(Case("edge.f64.min_propagates_nan", "f64", ops("f64.const", "f64.min", "end"),
                  "f64nan",
                  simple(f64c_bits(0x7FF8000000000001) + f64c(1.0) + bytes([0xA4, 0x0B]), result=F64)))
    e.append(Case("edge.f64.div_zero_inf", "f64", ops("f64.const", "f64.div", "end"),
                  "f64bits:0x7ff0000000000000",
                  simple(f64c(1.0) + f64c(0.0) + bytes([0xA3, 0x0B]), result=F64)))
    e.append(Case("edge.f64.mul_1e300_needs_f64", "f64", ops("f64.const", "f64.mul", "end"),
                  f"f64bits:{f64bits(1e301)}",
                  simple(f64c(1e300) + f64c(10.0) + bytes([0xA2, 0x0B]), result=F64)))
    e.append(Case("edge.f64.lt_nan_is_false", "f64", ops("f64.const", "f64.lt", "end"),
                  "i32:0",
                  simple(f64c_bits(0x7FF8000000000000) + f64c(1.0) + bytes([0x63, 0x0B]))))
    e.append(Case("edge.f64.sqrt_neg_zero", "f64", ops("f64.const", "f64.sqrt", "end"),
                  "f64bits:0x8000000000000000",
                  simple(f64c_bits(0x8000000000000000) + bytes([0x9F, 0x0B]), result=F64)))

    # --- conv: the trunc traps a saturating cast would swallow -------------
    e.append(Case("edge.conv.trunc_f32_s_2p31_traps", "conv",
                  ops("f32.const", "i32.trunc_f32_s", "end"), "trap",
                  simple(f32c(2147483648.0) + bytes([0xA8, 0x0B]))))
    e.append(Case("edge.conv.trunc_f32_s_max_ok", "conv",
                  ops("f32.const", "i32.trunc_f32_s", "end"), "i32:2147483520",
                  simple(f32c(2147483520.0) + bytes([0xA8, 0x0B]))))
    e.append(Case("edge.conv.trunc_f32_s_nan_traps", "conv",
                  ops("f32.const", "i32.trunc_f32_s", "end"), "trap",
                  simple(f32c_bits(0x7FC00000) + bytes([0xA8, 0x0B]))))
    e.append(Case("edge.conv.trunc_f32_u_neg_half_is_zero", "conv",
                  ops("f32.const", "i32.trunc_f32_u", "end"), "i32:0",
                  simple(f32c(-0.5) + bytes([0xA9, 0x0B]))))
    e.append(Case("edge.conv.trunc_f32_u_neg_one_traps", "conv",
                  ops("f32.const", "i32.trunc_f32_u", "end"), "trap",
                  simple(f32c(-1.0) + bytes([0xA9, 0x0B]))))
    e.append(Case("edge.conv.trunc_f64_s_nan_traps", "conv",
                  ops("f64.const", "i32.trunc_f64_s", "end"), "trap",
                  simple(f64c_bits(0x7FF8000000000000) + bytes([0xAA, 0x0B]))))
    e.append(Case("edge.conv.trunc_f64_s_2p63_traps", "conv",
                  ops("f64.const", "i64.trunc_f64_s", "end"), "trap",
                  simple(f64c(9223372036854775808.0) + bytes([0xB0, 0x0B]), result=I64)))
    e.append(Case("edge.conv.trunc_f64_s_inf_traps", "conv",
                  ops("f64.const", "i32.trunc_f64_s", "end"), "trap",
                  simple(f64c_bits(0x7FF0000000000000) + bytes([0xAA, 0x0B]))))
    e.append(Case("edge.conv.convert_i32_u_reads_unsigned", "conv",
                  ops("i32.const", "f32.convert_i32_u", "end"), "f32bits:1333788672",
                  simple(i32c(-1) + bytes([0xB3, 0x0B]), result=F32)))
    e.append(Case("edge.conv.demote_overflows_to_inf", "conv",
                  ops("f64.const", "f32.demote_f64", "end"), "f32bits:0x7f800000",
                  simple(f64c(1e300) + bytes([0xB6, 0x0B]), result=F32)))
    e.append(Case("edge.conv.reinterpret_keeps_nan_payload", "conv",
                  ops("f32.const", "i32.reinterpret_f32", "end"), "i32:2141192193",
                  simple(f32c_bits(0x7FA00001) + bytes([0xBC, 0x0B]))))
    e.append(Case("edge.conv.extend_i32_u_is_unsigned", "conv",
                  ops("i32.const", "i64.extend_i32_u", "end"), "i64:4294967295",
                  simple(i32c(-1) + bytes([0xAD, 0x0B]), result=I64)))

    # --- control: the label semantics a pass-through block cannot show ----
    e.append(Case("edge.control.loop_backedge", "control",
                  ops("loop", "br_if", "local.get", "local.set", "i32.add", "i32.sub", "end"),
                  "i32:35",
                  simple(i32c(5) + bytes([0x21, 0x00]) + i32c(0) + bytes([0x21, 0x01])
                         + bytes([0x03, 0x40])
                         + bytes([0x20, 0x01]) + i32c(7) + bytes([0x6A, 0x21, 0x01])
                         + bytes([0x20, 0x00]) + i32c(1) + bytes([0x6B, 0x21, 0x00])
                         + bytes([0x20, 0x00]) + bytes([0x0D, 0x00])
                         + bytes([0x0B])
                         + bytes([0x20, 0x01]) + bytes([0x0B]), nloc=2)))
    e.append(Case("edge.control.br_table_negative_index_takes_default", "control",
                  ops("block", "br_table", "return", "i32.const", "end"), "i32:30",
                  simple(bytes([0x02, 0x40, 0x02, 0x40, 0x02, 0x40]) + i32c(-1)
                         + bytes([0x0E, 0x02, 0x00, 0x01, 0x02])
                         + bytes([0x0B]) + i32c(10) + bytes([0x0F])
                         + bytes([0x0B]) + i32c(20) + bytes([0x0F])
                         + bytes([0x0B]) + i32c(30) + bytes([0x0F]) + bytes([0x0B]))))
    e.append(Case("edge.control.if_without_else_false", "control",
                  ops("if", "drop", "i32.const", "end"), "i32:11",
                  simple(i32c(0) + bytes([0x04, 0x40]) + i32c(9) + bytes([0x1A])
                         + bytes([0x0B]) + i32c(11) + bytes([0x0B]))))
    e.append(Case("edge.control.return_from_nested_blocks", "control",
                  ops("block", "return", "i32.const", "end"), "i32:77",
                  simple(bytes([0x02, 0x40, 0x02, 0x40, 0x02, 0x40]) + i32c(77) + bytes([0x0F])
                         + bytes([0x0B, 0x0B, 0x0B]) + i32c(0) + bytes([0x0B]))))
    e.append(Case("edge.control.br0_at_top_level_returns", "control",
                  ops("br", "i32.const", "end"), "i32:88",
                  simple(i32c(88) + bytes([0x0C, 0x00]) + i32c(1) + bytes([0x0B]))))
    e.append(Case("edge.control.call_indirect_null_elem_traps", "control",
                  ops("call_indirect", "i32.const", "end"), "trap",
                  encode_module(types=[([], [I32])], func_types=[0, 0],
                                codes=[(0, i32c(3) + bytes([0x11, 0x00, 0x00]) + bytes([0x0B])),
                                       (0, i32c(88) + bytes([0x0B]))],
                                exports=[("main", 0)], table_min=4, elems=[(0, [1])])))
    e.append(Case("edge.control.call_indirect_type_mismatch_traps", "control",
                  ops("call_indirect", "i32.const", "end"), "trap",
                  encode_module(types=[([], [I32]), ([I32], [I32]), ([I64], [I32])],
                                func_types=[0, 2],
                                codes=[(0, i32c(1) + i32c(0) + bytes([0x11, 0x01, 0x00]) + bytes([0x0B])),
                                       (0, i32c(7) + bytes([0x0B]))],
                                exports=[("main", 0)], table_min=1, elems=[(0, [1])])))
    e.append(Case("edge.control.call_recursion_sum6", "control",
                  ops("call", "if", "else", "local.get", "i32.sub", "i32.add", "end"), "i32:21",
                  encode_module(types=[([], [I32]), ([I32], [I32])], func_types=[0, 1],
                                codes=[(0, i32c(6) + bytes([0x10, 0x01]) + bytes([0x0B])),
                                       (0, bytes([0x20, 0x00]) + bytes([0x04, I32])
                                        + bytes([0x20, 0x00]) + bytes([0x20, 0x00]) + i32c(1)
                                        + bytes([0x6B]) + bytes([0x10, 0x01]) + bytes([0x6A])
                                        + bytes([0x05]) + i32c(0)
                                        + bytes([0x0B]) + bytes([0x0B]))],
                                exports=[("main", 0)])))

    # control: unbounded recursion must be a loud trap, never a stack overflow
    e.append(Case("edge.control.unbounded_recursion_traps", "control",
                  ops("call", "i32.add", "local.get", "end"), "trap",
                  encode_module(types=[([], [I32]), ([I32], [I32])], func_types=[0, 1],
                                codes=[(0, i32c(1) + bytes([0x10, 0x01, 0x0B])),
                                       (0, bytes([0x20, 0x00]) + i32c(1)
                                        + bytes([0x6A, 0x10, 0x01, 0x0B]))],
                                exports=[("main", 0)])))

    # --- parametric / locals: types, not just i32 --------------------------
    e.append(Case("edge.parametric.select_i64_cond_zero", "parametric",
                  ops("i64.const", "i32.const", "select", "end"), "i64:9",
                  simple(i64c(7) + i64c(9) + i32c(0) + bytes([0x1B, 0x0B]), result=I64)))
    e.append(Case("edge.parametric.select_i64_cond_negative", "parametric",
                  ops("i64.const", "i32.const", "select", "end"), "i64:7",
                  simple(i64c(7) + i64c(9) + i32c(-1) + bytes([0x1B, 0x0B]), result=I64)))
    e.append(Case("edge.parametric.select_keeps_f64_nan_payload", "parametric",
                  ops("f64.const", "i32.const", "select", "end"),
                  "f64bits:0x7ff8000000000001",
                  simple(f64c_bits(0x7FF8000000000001) + f64c(1.5) + i32c(1)
                         + bytes([0x1B, 0x0B]), result=F64)))
    e.append(Case("edge.parametric.drop_is_type_agnostic", "parametric",
                  ops("f32.const", "f64.const", "drop", "end"), f"f32bits:{f32bits(2.5)}",
                  simple(f32c(2.5) + f64c(1.25) + bytes([0x1A, 0x0B]), result=F32)))
    e.append(Case("edge.locals.default_zero_i64", "locals", ops("local.get", "end"), "i64:0",
                  simple(bytes([0x20, 0x00, 0x0B]), result=I64, local_decls=[(1, I64)])))
    e.append(Case("edge.locals.default_zero_f32_is_usable", "locals",
                  ops("local.get", "f32.const", "f32.add", "end"), f"f32bits:{f32bits(1.5)}",
                  simple(bytes([0x20, 0x00]) + f32c(1.5) + bytes([0x92, 0x0B]),
                         result=F32, local_decls=[(1, F32)])))
    e.append(Case("edge.locals.two_decl_groups", "locals",
                  ops("i64.const", "local.set", "local.get", "end"), "i64:8",
                  simple(i64c(8) + bytes([0x21, 0x02, 0x20, 0x02, 0x0B]),
                         result=I64, local_decls=[(2, I32), (1, I64)])))
    e.append(Case("edge.locals.params_then_locals_index_space", "locals",
                  ops("i32.const", "call", "local.get", "i32.sub", "local.set", "end"), "i32:-11",
                  encode_module(types=[([], [I32]), ([I32, I32], [I32])], func_types=[0, 1],
                                codes=[(0, i32c(11) + i32c(22) + bytes([0x10, 0x01, 0x0B])),
                                       (1, bytes([0x20, 0x00, 0x20, 0x01, 0x6B, 0x21, 0x02,
                                                  0x20, 0x02, 0x0B]))],
                                exports=[("main", 0)])))
    e.append(Case("edge.locals.global_set_immutable_traps", "locals",
                  ops("i32.const", "global.set", "global.get", "end"), "trap",
                  encode_module(types=[([], [I32])], func_types=[0],
                                codes=[(0, i32c(9) + bytes([0x24, 0x00, 0x23, 0x00, 0x0B]))],
                                exports=[("main", 0)],
                                globals_=[(I32, False, i32c(7) + bytes([0x0B]))])))
    e.append(Case("edge.locals.global_i64_set_get", "locals",
                  ops("i64.const", "global.set", "global.get", "end"), "i64:-3",
                  encode_module(types=[([], [I64])], func_types=[0],
                                codes=[(0, i64c(-3) + bytes([0x24, 0x00, 0x23, 0x00, 0x0B]))],
                                exports=[("main", 0)],
                                globals_=[(I64, True, i64c(1) + bytes([0x0B]))])))

    e.append(Case("edge.parametric.select_f32_cond_nonzero", "parametric",
                  ops("f32.const", "i32.const", "select", "end"), f"f32bits:{f32bits(2.5)}",
                  simple(f32c(2.5) + f32c(7.5) + i32c(2) + bytes([0x1B, 0x0B]), result=F32)))
    e.append(Case("edge.parametric.drop_leaves_the_value_below", "parametric",
                  ops("i64.const", "drop", "end"), "i64:-7",
                  simple(i64c(-7) + i64c(1) + bytes([0x1A, 0x0B]), result=I64)))
    e.append(Case("edge.parametric.select_keeps_i64_high_bits", "parametric",
                  ops("i64.const", "i32.const", "select", "end"), "i64:4294967296",
                  simple(i64c(2 ** 32) + i64c(1) + i32c(1) + bytes([0x1B, 0x0B]), result=I64)))

    # --- memory: limits, data segments, bounds -----------------------------
    e.append(Case("edge.memory.store8_truncates_then_load8_s", "memory",
                  ops("i32.const", "i32.store8", "i32.load8_s", "end"), "i32:-1",
                  simple(i32c(0) + i32c(0x1FF) + bytes([0x3A, 0x00, 0x00])
                         + i32c(0) + bytes([0x2C, 0x00, 0x00, 0x0B]), memory_min=1)))
    e.append(Case("edge.memory.little_endian_bytes", "memory",
                  ops("i32.const", "i32.store", "i32.load8_u", "i32.mul", "i32.add", "end"),
                  "i32:8",
                  simple(i32c(8) + i32c(0x04030201) + bytes([0x36, 0x00, 0x00])
                         + i32c(8) + bytes([0x2D, 0x00, 0x00]) + i32c(4) + bytes([0x6C])
                         + i32c(11) + bytes([0x2D, 0x00, 0x00]) + bytes([0x6A, 0x0B]),
                         memory_min=1)))
    e.append(Case("edge.memory.memarg_offset_applies", "memory",
                  ops("i32.const", "i32.store", "i32.load", "end"), "i32:99",
                  simple(i32c(16) + i32c(99) + bytes([0x36, 0x00, 0x04])
                         + i32c(20) + bytes([0x28, 0x00, 0x00, 0x0B]), memory_min=1)))
    e.append(Case("edge.memory.partial_tail_out_of_bounds_traps", "memory",
                  ops("i32.const", "i32.load", "end"), "trap",
                  simple(i32c(65533) + bytes([0x28, 0x00, 0x00, 0x0B]), memory_min=1)))
    e.append(Case("edge.memory.address_plus_offset_overflow_traps", "memory",
                  ops("i32.const", "i32.load", "end"), "trap",
                  simple(i32c(-1) + bytes([0x28, 0x00, 0x01, 0x0B]), memory_min=1)))
    e.append(Case("edge.memory.store_out_of_bounds_traps", "memory",
                  ops("i32.const", "i32.store", "end"), "trap",
                  simple(i32c(65534) + i32c(7) + bytes([0x36, 0x00, 0x00, 0x0B]),
                         result=None, memory_min=1)))
    e.append(Case("edge.memory.grown_page_reads_zero", "memory",
                  ops("i32.const", "memory.grow", "drop", "i32.load", "end"), "i32:0",
                  simple(i32c(1) + bytes([0x40, 0x00, 0x1A])
                         + i32c(65536) + bytes([0x28, 0x00, 0x00, 0x0B]), memory_min=1)))
    e.append(Case("edge.memory.data_segment_initialises", "memory",
                  ops("i32.const", "i32.load", "end"), "i32:-559038737",
                  simple(i32c(0) + bytes([0x28, 0x00, 0x00, 0x0B]),
                         memory_min=1, data=[(0, bytes([0xEF, 0xBE, 0xAD, 0xDE]))])))
    e.append(Case("edge.memory.data_segment_at_offset", "memory",
                  ops("i32.const", "i32.load8_u", "end"), "i32:42",
                  simple(i32c(7) + bytes([0x2D, 0x00, 0x00, 0x0B]),
                         memory_min=1, data=[(7, bytes([42]))])))
    e.append(Case("edge.memory.size_reports_declared_min", "memory",
                  ops("memory.size", "end"), "i32:3",
                  simple(bytes([0x3F, 0x00, 0x0B]), memory_min=3)))
    e.append(Case("edge.memory.grow_past_declared_max_returns_minus_one", "memory",
                  ops("i32.const", "memory.grow", "end"), "i32:-1",
                  simple(i32c(5) + bytes([0x40, 0x00, 0x0B]), memory_min=1, memory_max=2)))
    e.append(Case("edge.memory.grow_returns_previous_size", "memory",
                  ops("i32.const", "memory.grow", "end"), "i32:2",
                  simple(i32c(1) + bytes([0x40, 0x00, 0x0B]), memory_min=2, memory_max=4)))

    # --- host: the import table as the only door ---------------------------
    e.append(Case("edge.host.index_shift_defined_func", "host",
                  ops("call", "i32.const", "end"), "i32:7",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1, 1],
                                codes=[(0, i32c(7) + bytes([0x0B])), (0, bytes([0x10, 0x02, 0x0B]))],
                                exports=[("main", 3)],
                                imports=[("host", "double", 0), ("host", "plus100", 0)])))
    e.append(Case("edge.host.call_reaches_second_import", "host",
                  ops("call", "i32.const", "end"), "i32:105",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1, 1],
                                codes=[(0, i32c(7) + bytes([0x0B])),
                                       (0, i32c(5) + bytes([0x10, 0x01, 0x0B]))],
                                exports=[("main", 3)],
                                imports=[("host", "double", 0), ("host", "plus100", 0)]),
                  bind="host.plus100"))
    e.append(Case("edge.host.two_imports_in_order", "host",
                  ops("i32.const", "call", "end"), "i32:110",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1],
                                codes=[(0, i32c(5) + bytes([0x10, 0x00, 0x10, 0x01, 0x0B]))],
                                exports=[("main", 2)],
                                imports=[("host", "double", 0), ("host", "plus100", 0)]),
                  bind="host.double+host.plus100"))
    e.append(Case("edge.host.sibling_bound_other_still_traps", "host",
                  ops("i32.const", "call", "end"), "trap",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1],
                                codes=[(0, i32c(5) + bytes([0x10, 0x01, 0x0B]))],
                                exports=[("main", 2)],
                                imports=[("host", "double", 0), ("env", "missing", 0)]),
                  bind="host.double"))
    e.append(Case("edge.host.closure_writes_linear_memory", "host",
                  ops("call", "i32.const", "i32.load", "end"), "i32:35",
                  encode_module(types=[([], []), ([], [I32])], func_types=[1],
                                codes=[(0, bytes([0x10, 0x00]) + i32c(0)
                                        + bytes([0x28, 0x02, 0x08, 0x0B]))],
                                exports=[("main", 1)],
                                imports=[("host", "poke", 0)], memory_min=1),
                  bind="host.poke"))
    e.append(Case("edge.host.import_reached_through_table", "host",
                  ops("i32.const", "call_indirect", "end"), "i32:10",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1],
                                codes=[(0, i32c(5) + i32c(0) + bytes([0x11, 0x00, 0x00, 0x0B]))],
                                exports=[("main", 1)],
                                imports=[("host", "double", 0)], table_min=1, elems=[(0, [0])]),
                  bind="host.double"))
    e.append(Case("edge.host.unused_import_does_not_block", "host",
                  ops("i32.const", "end"), "i32:99",
                  encode_module(types=[([I32], [I32]), ([], [I32])], func_types=[1],
                                codes=[(0, i32c(99) + bytes([0x0B]))],
                                exports=[("main", 1)],
                                imports=[("host", "never", 0)])))
    e.append(Case("edge.host.start_writes_are_visible_to_entry", "host",
                  ops("call", "i32.const", "i32.load", "end"), "i32:35",
                  encode_module(types=[([], []), ([], [I32])], func_types=[0, 1],
                                codes=[(0, bytes([0x10, 0x00, 0x0B])),
                                       (0, i32c(0) + bytes([0x28, 0x02, 0x08, 0x0B]))],
                                exports=[("main", 2)],
                                imports=[("host", "poke", 0)], start=1, memory_min=1),
                  bind="host.poke"))

    return e


def write_file(path: Path, cases: list[Case], header: str) -> None:
    lines = [header, "# id|family|opcodes|expect|wasm_hex|bind"]
    for c in cases:
        opc = ",".join(f"{OPS[n]:02X}" for n in c.opcodes)
        bind = c.bind
        lines.append(f"{c.cid}|{c.family}|{opc}|{c.expect}|{hexb(c.wasm)}|{bind}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_opcode_list(path: Path) -> None:
    lines = ["# WASM 1.0 MVP opcode catalog (172). name hex"]
    for name_, byte in sorted(OPS.items(), key=lambda kv: kv[1]):
        lines.append(f"{name_} {byte:02X}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    here = Path(__file__).resolve().parent
    cases = build_cases()
    extras = extra_cases()
    covered = set()
    for c in cases:
        covered.update(c.opcodes)
    missing = [n for n in OPS if n not in covered]
    if missing:
        raise SystemExit(f"catalog missing opcodes: {missing}")
    write_opcode_list(here / "mvp_opcodes.txt")
    write_file(
        here / "mvp_goldens.txt",
        cases,
        "# Independent WASM 1.0 MVP goldens. Expected values from this generator (spec arithmetic), not the Rust interpreter.",
    )
    write_file(
        here / "family_extra.txt",
        extras,
        "# Extra per-family goldens. Operands/layout/control differ from crates/agenterm-tinyvm/src/wasm.rs in-crate tests.",
    )
    edges = edge_cases()
    write_file(
        here / "family_edge.txt",
        edges,
        "# Spec-boundary goldens: traps, signed/unsigned splits, shift-count masks, NaN and signed zero, ties-to-even, memory limits and data segments, and the import table. Expected values are read off the WASM 1.0 / IEEE-754 text, never off this interpreter.",
    )
    print(
        f"wrote {len(cases)} goldens, {len(extras)} extras, {len(edges)} edges, "
        f"{len(OPS)} opcodes"
    )


if __name__ == "__main__":
    main()
