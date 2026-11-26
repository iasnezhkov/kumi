INCLUDE runtime-memory.x
ENTRY(_start)
SECTIONS {
  .storage : ALIGN(4) {
    *(.text.storage .text.storage.* .text.cold .text.cold.*)
    *(.text.*kumi_audio* .text.*kumi_dsp* .text.*kumi_icd* .text.*sha2* .text.*libm*)
    *(.text.*kumi_settings* .text.*kumi_service* .text.*kumi_control* .text.*kumi_diag* .text.*kumi_boot_core* .text.*kumi_runtime_core*)
    *(.text.*kumi_keymap* .text.*kumi_kbdlink* .text.*kumi_chip* .text.*kumi_layout*)
    *(.text.*serde_core* .text.*core3fmt* .text.*core..fmt..* .text.*df3* .text.*df2* .text.*extendsfdf2* .text.*truncdfsf2*)
    *(.text.*kumi_fw3uac* .text.*kumi_fw..uac..* .rodata.*kumi_fw3uac*)
    *(.text.*kumi_fw3app* .text.*kumi_fw..app..*)
    *(.text.*kumi_fw3lab* .text.*kumi_fw..lab..* .text.*kumi_fw5audio* .text.*kumi_fw..audio..*)
    *(.text.*embassy_usb* .text.*postcard*)
    *(.text.*kumi_fw3usb6Driver* .text.*kumi_fw..usb..Driver$u20*)
    . = ALIGN(4);
  } > RT40
  .text : ALIGN(4) {
    KEEP(*(.text.entry))
    *(.text.hot .text.hot.*)
    *(.text .text.*)
    *(.rodata .rodata.* .srodata .srodata.*)
    . = ALIGN(4);
  } > ILM
  .data : ALIGN(4) {
    __data_start = .;
    *(.data .data.* .sdata .sdata.*)
    . = ALIGN(4);
    __data_end = .;
  } > DLM
  __global_pointer$ = __data_start + 0x800;
  .bss (NOLOAD) : ALIGN(16) {
    __bss_start = .;
    *(.bss .bss.* .sbss .sbss.* COMMON)
    . = ALIGN(16);
    __bss_end = .;
  } > DLM
  __trap_stack_top = ORIGIN(DLM) + LENGTH(DLM);
  __stack_top = __trap_stack_top - KUMI_RUNTIME_TRAP_STACK;
  /DISCARD/ : { *(.eh_frame*) *(.riscv.attributes) *(.comment) }
  ASSERT(_start == KUMI_RUNTIME_ENTRY, "RAM entry must match the image entry")
  ASSERT(__stack_top - __bss_end >= KUMI_RUNTIME_MIN_STACK, "runtime needs at least 64 KiB of stack")
}
