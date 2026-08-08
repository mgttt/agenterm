//! IR generator — an AUTHORING aid, NOT part of the measured lowerer (X) and NOT
//! shipped. A std host tool that assembles the three payloads' IR (Q3 §2, reusing
//! Q0 semantics) into flat .bin files, later embedded into the runner via env!("DC_IR").
//! Think of it as "writing the IR by hand"; it just spares the manual byte layout.

#[path = "../ir.rs"]
mod ir;
use ir::*;
use std::io::Write;

struct A {
    v: Vec<u8>,
}
impl A {
    fn new() -> A {
        A { v: Vec::new() }
    }
    fn op(&mut self, o: u8) {
        self.v.push(o);
    }
    fn i32(&mut self, x: i32) {
        self.v.extend_from_slice(&x.to_le_bytes());
    }
    fn i64(&mut self, x: i64) {
        self.v.extend_from_slice(&x.to_le_bytes());
    }
    fn imm(&mut self, d: u8, x: i64) {
        self.op(OP_IMM);
        self.v.push(d);
        self.i64(x);
    }
    fn mov(&mut self, d: u8, s: u8) {
        self.op(OP_MOV);
        self.v.push(d);
        self.v.push(s);
    }
    fn alu(&mut self, o: u8, d: u8, s: u8) {
        self.op(o);
        self.v.push(d);
        self.v.push(s);
    }
    fn shr(&mut self, d: u8, imm: u8) {
        self.op(OP_SHR);
        self.v.push(d);
        self.v.push(imm);
    }
    fn ld8(&mut self, d: u8, b: u8, off: i32) {
        self.op(OP_LD8);
        self.v.push(d);
        self.v.push(b);
        self.i32(off);
    }
    fn st8(&mut self, b: u8, off: i32, s: u8) {
        self.op(OP_ST8);
        self.v.push(b);
        self.i32(off);
        self.v.push(s);
    }
    fn lde(&mut self, d: u8, slot: u8) {
        self.op(OP_LDE);
        self.v.push(d);
        self.v.push(slot);
    }
    fn jcc(&mut self, o: u8, a: u8, b: u8, l: u8) {
        self.op(o);
        self.v.push(a);
        self.v.push(b);
        self.v.push(l);
    }
    fn jmp(&mut self, l: u8) {
        self.op(OP_JMP);
        self.v.push(l);
    }
    fn label(&mut self, l: u8) {
        self.op(OP_LABEL);
        self.v.push(l);
    }
    fn call(&mut self, rd: u8, idx: u8, args: &[u8]) {
        self.op(OP_CALL);
        self.v.push(rd);
        self.v.push(idx);
        self.v.push(args.len() as u8);
        for &x in args {
            self.v.push(x);
        }
    }
}

// pure_compute: acc=basis; loop 1e6 { acc=acc*prime+i; i++ }; exit(acc & 0xff)
fn ir_pure() -> Vec<u8> {
    let mut a = A::new();
    a.imm(0, 1469598103934665603); // acc
    a.imm(2, 1099511628211); // prime
    a.imm(5, 0); // i
    a.imm(3, 1000000); // N
    a.imm(6, 1); // one
    a.label(1);
    a.jcc(OP_JAE, 5, 3, 2); // if i>=N -> end
    a.alu(OP_MUL, 0, 2);
    a.alu(OP_ADD, 0, 5);
    a.alu(OP_ADD, 5, 6);
    a.jmp(1);
    a.label(2);
    a.imm(8, 0xff);
    a.alu(OP_AND, 0, 8);
    a.call(RD_DISCARD, ENV_EXIT, &[0]);
    a.v
}

// read_hash_print: buf=alloc; n=read_file(k,path,buf,cap); FNV-1a; hex16; write; exit(0)
fn ir_rhp() -> Vec<u8> {
    let mut a = A::new();
    a.imm(0, 65536);
    a.call(1, ENV_MEM_ALLOC, &[0]); // buf -> r1
    a.lde(0, ENV_K);
    a.lde(2, ENV_PATH);
    a.imm(3, 65536);
    a.call(3, ENV_READ_FILE, &[0, 2, 1, 3]); // n -> r3
    // hash loop: h=basis; while i<n { h ^= buf[i]; h *= prime; i++ }
    a.imm(0, 14695981039346656037u64 as i64); // h
    a.imm(2, 1099511628211); // prime
    a.imm(5, 0); // i
    a.imm(6, 1); // one
    a.label(1);
    a.jcc(OP_JAE, 5, 3, 2);
    a.mov(7, 1);
    a.alu(OP_ADD, 7, 5); // addr = buf+i
    a.ld8(4, 7, 0);
    a.alu(OP_XOR, 0, 4);
    a.alu(OP_MUL, 0, 2);
    a.alu(OP_ADD, 5, 6);
    a.jmp(1);
    a.label(2);
    // hex16 into buf[0..16], buf[16]='\n'
    a.imm(4, 10);
    a.st8(1, 16, 4);
    a.lde(8, ENV_HEX); // hexptr -> r8
    a.imm(10, 0xf); // mask
    a.imm(5, 16); // pos
    a.imm(12, 0); // zero
    a.label(3);
    a.jcc(OP_JE, 5, 12, 4); // pos==0 -> done
    a.alu(OP_SUB, 5, 6); // pos--
    a.mov(7, 0);
    a.alu(OP_AND, 7, 10); // nib = h & 0xf
    a.mov(11, 8);
    a.alu(OP_ADD, 11, 7); // hexptr + nib
    a.ld8(4, 11, 0); // ch
    a.mov(7, 1);
    a.alu(OP_ADD, 7, 5); // buf + pos
    a.st8(7, 0, 4); // buf[pos] = ch
    a.shr(0, 4); // h >>= 4
    a.jmp(3);
    a.label(4);
    a.lde(0, ENV_K);
    a.imm(3, 17);
    a.call(RD_DISCARD, ENV_WRITE, &[0, 1, 3]);
    a.imm(0, 0);
    a.call(RD_DISCARD, ENV_EXIT, &[0]);
    a.v
}

// spawn_echo: code=spawn(k); print "exit=NN\n"; exit(code & 0xff)
fn ir_spawn() -> Vec<u8> {
    let mut a = A::new();
    a.lde(0, ENV_K);
    a.call(1, ENV_SPAWN, &[0]); // code -> r1
    a.mov(3, 1); // code_orig -> r3
    a.imm(8, 0xff);
    a.alu(OP_AND, 1, 8); // c = code & 0xff
    a.imm(2, 0); // tens
    a.imm(6, 10); // ten
    a.imm(5, 1); // one
    a.label(1);
    a.jcc(OP_JB, 1, 6, 2); // c<10 -> done
    a.alu(OP_SUB, 1, 6);
    a.alu(OP_ADD, 2, 5);
    a.jmp(1);
    a.label(2);
    a.imm(0, 8);
    a.call(0, ENV_MEM_ALLOC, &[0]); // buf -> r0
    a.imm(4, 101);
    a.st8(0, 0, 4); // 'e'
    a.imm(4, 120);
    a.st8(0, 1, 4); // 'x'
    a.imm(4, 105);
    a.st8(0, 2, 4); // 'i'
    a.imm(4, 116);
    a.st8(0, 3, 4); // 't'
    a.imm(4, 61);
    a.st8(0, 4, 4); // '='
    a.imm(4, 48); // '0'
    a.mov(7, 2);
    a.alu(OP_ADD, 7, 4);
    a.st8(0, 5, 7); // tens digit
    a.mov(7, 1);
    a.alu(OP_ADD, 7, 4);
    a.st8(0, 6, 7); // ones digit
    a.imm(4, 10);
    a.st8(0, 7, 4); // '\n'
    a.lde(1, ENV_K);
    a.imm(2, 8);
    a.call(RD_DISCARD, ENV_WRITE, &[1, 0, 2]);
    a.mov(0, 3);
    a.imm(8, 0xff);
    a.alu(OP_AND, 0, 8);
    a.call(RD_DISCARD, ENV_EXIT, &[0]);
    a.v
}

fn main() {
    let out = std::env::args().nth(1).expect("usage: ir_gen <out_dir>");
    for (name, bytes) in [
        ("ir_pure.bin", ir_pure()),
        ("ir_rhp.bin", ir_rhp()),
        ("ir_spawn.bin", ir_spawn()),
    ] {
        let path = format!("{}/{}", out, name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(&bytes).expect("write");
        println!("{} = {} bytes", name, bytes.len());
    }
}
