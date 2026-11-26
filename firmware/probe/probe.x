ENTRY(_start)
MEMORY {
  ILM (rx) : ORIGIN = 0x2000, LENGTH = 0x2000
}
SECTIONS {
  .text : ALIGN(4) {
    KEEP(*(.text.entry))
    *(.text .text.*)
    *(.rodata .rodata.*)
    *(.srodata .srodata.*)
  } > ILM
  .data : { *(.data .data.* .sdata .sdata.*) } > ILM
  .bss (NOLOAD) : { *(.bss .bss.* .sbss .sbss.* COMMON) } > ILM
  /DISCARD/ : { *(.eh_frame*) *(.riscv.attributes) *(.comment) }
  ASSERT(SIZEOF(.data) == 0, "probe must not need initialized data")
  ASSERT(SIZEOF(.bss) == 0, "probe must not need zeroed data")
  ASSERT(_start == 0x2000, "entry must be the first word of the image")
}
