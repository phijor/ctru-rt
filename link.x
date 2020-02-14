/* vim: set ft=ld: */

OUTPUT_FORMAT("elf32-littlearm", "elf32-bigarm", "elf32-littlearm")
OUTPUT_ARCH(arm)
ENTRY(_start)

PHDRS
{
	CODE   PT_LOAD FLAGS(5) /* Read | Execute */;
	RODATA PT_LOAD FLAGS(4) /* Read */;
	DATA   PT_LOAD FLAGS(6) /* Read | Write */;
}

SECTIONS
{
    PROVIDE(__start__ = 0x100000);

    . = __start__;

    .text ALIGN(4K) :
    {
        KEEP( ctru-rt.o(.text) )
        KEEP( ctru-rt.o(.crt0) )
        KEEP( ctru-rt.o(.rsrt0) )
        KEEP( *(.rsrt0) )
        KEEP( *(.init) )
        . = ALIGN(4);

        *(.text)
        *(.text.*)

        . = ALIGN(4);
    } : CODE

    .rodata : ALIGN(4K)
    {
        *(.rodata)
        *(.rodata.*)
		. = ALIGN(4);
    } : RODATA

	.ARM.extab : { *(.ARM.extab* .gnu.linkonce.armextab.*) } : RODATA
	__exidx_start = .;
	ARM.exidx : { *(.ARM.exidx* .gnu.linkonce.armexidx.*) } : RODATA
	__exidx_end = .;

    .data : ALIGN(4K)
	{
        __data_start__ = .;
		*(.data)
		*(.data.*)
        . = ALIGN(4);
        __data_end__ = .;
	} : DATA

    .bss : ALIGN(4K)
	{
        __bss_start__ = .;
        *(.sbss .sbss.* .bss .bss.*);
        . = ALIGN(4);

        /* Reserve space for the TLS segment of the main thread */
		__tls_start = .;
		/* . += + SIZEOF(.tdata) + SIZEOF(.tbss); */
		__tls_end = .;

        __bss_end__ = .;
	} : DATA

	.preinit_array ALIGN(4) :
	{
		PROVIDE (__preinit_array_start = .);
		KEEP (*(.preinit_array))
		PROVIDE (__preinit_array_end = .);
	} : DATA

	.init_array ALIGN(4) :
	{
		PROVIDE (__init_array_start = .);
		KEEP (*(SORT(.init_array.*)))
		KEEP (*(.init_array))
		PROVIDE (__init_array_end = .);
	} : DATA

    __end__ = ABSOLUTE(.) ;

    /* Stabs debugging sections. */
	.stab          0 : { *(.stab) }
	.stabstr       0 : { *(.stabstr) }
	.stab.excl     0 : { *(.stab.excl) }
	.stab.exclstr  0 : { *(.stab.exclstr) }
	.stab.index    0 : { *(.stab.index) }
	.stab.indexstr 0 : { *(.stab.indexstr) }

	/* DWARF debug sections.
	   Symbols in the DWARF debugging sections are relative to the beginning
	   of the section so we begin them at 0. */

	/* DWARF 1 */
	.debug          0 : { *(.debug) }
	.line           0 : { *(.line) }

	/* GNU DWARF 1 extensions */
	.debug_srcinfo  0 : { *(.debug_srcinfo) }
	.debug_sfnames  0 : { *(.debug_sfnames) }

	/* DWARF 1.1 and DWARF 2 */
	.debug_aranges  0 : { *(.debug_aranges) }
	.debug_pubnames 0 : { *(.debug_pubnames) }

	/* DWARF 2 */
	.debug_info     0 : { *(.debug_info) }
	.debug_abbrev   0 : { *(.debug_abbrev) }
	.debug_line     0 : { *(.debug_line) }
	.debug_frame    0 : { *(.debug_frame) }
	.debug_str      0 : { *(.debug_str) }
	.debug_loc      0 : { *(.debug_loc) }
	.debug_macinfo  0 : { *(.debug_macinfo) }
}
