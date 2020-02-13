/* vim: set ft=ld: */

OUTPUT_FORMAT("elf32-littlearm", "elf32-bigarm", "elf32-littlearm")
OUTPUT_ARCH(arm)
ENTRY(_start)

PHDRS
{
	code   PT_LOAD FLAGS(5) /* Read | Execute */;
	rodata PT_LOAD FLAGS(4) /* Read */;
	data   PT_LOAD FLAGS(6) /* Read | Write */;
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
    } : code

    .rodata : ALIGN(4K)
    {
        *(.rodata)
        *(.rodata.*)
		. = ALIGN(4);
    } : rodata

    .data : ALIGN(4K)
	{
        __data_start__ = .;
		*(.data)
		*(.data.*)
        . = ALIGN(4);
        __data_end__ = .;
	} : data

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
	} : data

	.preinit_array ALIGN(4) :
	{
		PROVIDE (__preinit_array_start = .);
		KEEP (*(.preinit_array))
		PROVIDE (__preinit_array_end = .);
	} : data

	.init_array ALIGN(4) :
	{
		PROVIDE (__init_array_start = .);
		KEEP (*(SORT(.init_array.*)))
		KEEP (*(.init_array))
		PROVIDE (__init_array_end = .);
	} : data

    __end__ = ABSOLUTE(.) ;
}
