//! Tiny ELF64 reader: prints the byte size of each PROGBITS section whose name
//! contains "text". Used to compare the four-primitive kernel's code size across
//! ISAs without an external `objdump`/`size` tool.
use std::env;
use std::fs;

fn rd_u16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn rd_u32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }
fn rd_u64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7]])
}

fn main() {
    let path = env::args().nth(1).expect("usage: textsize <elf.o>");
    let b = fs::read(&path).unwrap();
    assert_eq!(&b[0..4], b"\x7fELF", "not ELF");
    let e_shoff = rd_u64(&b, 0x28) as usize;
    let e_shentsize = rd_u16(&b, 0x3a) as usize;
    let e_shnum = rd_u16(&b, 0x3c) as usize;
    let e_shstrndx = rd_u16(&b, 0x3e) as usize;
    let strtab_off = rd_u64(&b, e_shoff + e_shstrndx * e_shentsize + 0x18) as usize;
    let name_of = |name_off: u32| -> String {
        let mut s = String::new();
        let mut p = strtab_off + name_off as usize;
        while b[p] != 0 { s.push(b[p] as char); p += 1; }
        s
    };
    let mut total_text = 0u64;
    for i in 0..e_shnum {
        let sh = e_shoff + i * e_shentsize;
        let sh_name = rd_u32(&b, sh);
        let sh_type = rd_u32(&b, sh + 4);
        let sh_size = rd_u64(&b, sh + 0x20);
        let nm = name_of(sh_name);
        if sh_type == 1 /*PROGBITS*/ && nm.contains("text") {
            println!("  {:<28} {:>6} bytes", nm, sh_size);
            total_text += sh_size;
        }
    }
    println!("  TOTAL .text* = {total_text} bytes");
}
