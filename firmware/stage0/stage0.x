INCLUDE stage0-memory.x
ENTRY(_start)
SECTIONS {
  .header : {
    FILL(0x00000000);
    . = ORIGIN(FLASH) + KUMI_SPV_HEADER_RESERVED;
  } > FLASH
  .xip : {
    KEEP(*(.text.xip))
    FILL(0x00000000);
    . = KUMI_STAGE0_RAM_LMA;
  } > FLASH
  .ram : ALIGN(4) {
    __ram_start = .;
    KEEP(*(.text.vectors))
    *(.text .text.*)
    *(.rodata .rodata.* .srodata .srodata.*)
    KEEP(*(.rodata.kumi_key))
    *(.data .data.* .sdata .sdata.*)
    . = ALIGN(4);
    __ram_end = .;
  } > RAM AT > FLASH
  __ram_lma = LOADADDR(.ram);
  .bss (NOLOAD) : ALIGN(4) {
    __bss_start = .;
    *(.bss .bss.* .sbss .sbss.* COMMON)
    . = ALIGN(4);
    __bss_end = .;
  } > RAM
  .trap_stack (NOLOAD) : ALIGN(16) {
    . += KUMI_STAGE0_TRAP_STACK;
    __trap_stack_top = .;
  } > RAM
  __stack_top = ORIGIN(RAM) + LENGTH(RAM);
  /DISCARD/ : { *(.eh_frame*) *(.riscv.attributes) *(.comment) }
  ASSERT(_start == KUMI_STAGE0_XIP_ENTRY, "the boot ROM path enters right after the 256-byte header")
  ASSERT(__ram_lma == KUMI_STAGE0_RAM_LMA, "host tools expect the RAM image at offset 0x200")
  ASSERT(kumi_stage0_vectors == KUMI_STAGE0_BOOT_ENTRY, "the boot entry is at the start of stage0 RAM")
  ASSERT(kumi_stage0_dry_vector == KUMI_STAGE0_DRY_ENTRY, "the dry-run entry is where the G2 tools call it")
  ASSERT(kumi_stage0_verify_vector == KUMI_STAGE0_VERIFY_ENTRY, "the verify entry is where the runtime calls it")
  ASSERT(kumi_stage0_verify_magic == KUMI_STAGE0_VERIFY_MAGIC_ADDRESS, "the runtime checks the verify marker here")
  ASSERT(kumi_stage0_g2b_vector == KUMI_STAGE0_G2B_ENTRY, "the G2b entry is where the gate tool calls it")
  ASSERT(__stack_top - __trap_stack_top >= KUMI_STAGE0_MIN_STACK, "stage0 needs at least 16 KiB of stack")
  ASSERT(!DEFINED(__global_pointer$), "stage0 code is called from the runtime, which owns gp")
}
