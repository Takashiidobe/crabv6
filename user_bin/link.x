ENTRY(_start);

SECTIONS
{
  . = ORIGIN(REGION_TEXT);

  .text :
  {
    *(.text.start)
    *(.text*)
  } > REGION_TEXT

  .rodata :
  {
    *(.rodata*)
  } > REGION_RODATA

  .data :
  {
    *(.data*)
  } > REGION_DATA

  .bss (NOLOAD) :
  {
    *(.bss*)
    *(COMMON)
  } > REGION_BSS

  /DISCARD/ : { *(.eh_frame*) *(.note*) }
}
