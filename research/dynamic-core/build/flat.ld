/* Flatten a payload object into a raw position-independent code image.
 * `entry` is forced to offset 0 so the loader can jump to blob_base + 0.
 * Unwind/metadata sections are discarded so nothing precedes the code. */
ENTRY(entry)
SECTIONS {
  . = 0;
  .all : {
    *(.text.entry)
    *(.text*)
    *(.rodata*)
    *(.data.rel.ro*)
    *(.data*)
    *(.got*)
  }
  /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) *(.debug*) *(.llvm*) }
}
