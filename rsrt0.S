/* vim: ft=armasm:
*/
/*--------------------------------------------------------------------------------
	This Source Code Form is subject to the terms of the Mozilla Public License,
	v. 2.0. If a copy of the MPL was not distributed with this file, You can
	obtain one at https://mozilla.org/MPL/2.0/.
--------------------------------------------------------------------------------*/

@---------------------------------------------------------------------------------
@ 3DS processor selection
@---------------------------------------------------------------------------------
	.cpu mpcore
@---------------------------------------------------------------------------------

@---------------------------------------------------------------------------------
	.section ".crt0","ax"
	.global _start, __service_ptr, __apt_appid, __heap_size, __linear_heap_size, __system_arglist, __system_runflags
@---------------------------------------------------------------------------------
	.align 4
	.arm
@---------------------------------------------------------------------------------
_start:
@---------------------------------------------------------------------------------
	b startup
	.ascii "_prm"
__service_ptr:
	.word 0 @ Pointer to service handle override list -- if non-NULL it is assumed that we have been launched from a homebrew launcher
__apt_appid:
	.word 0x300 @ Program APPID
__heap_size:
	.word 24*1024*1024 @ Default heap size (24 MiB)
__linear_heap_size:
	.word 32*1024*1024 @ Default linear heap size (32 MiB)
__system_arglist:
	.word 0 @ Pointer to argument list (argc (u32) followed by that many NULL terminated strings)
__system_runflags:
	.word 0 @ Flags to signal runtime restrictions to ctrulib
startup:
	@ Save return address
	mov r4, lr

	@ Jump to user code
	bl _ctru_rt_start
	@ Return to saved address if we are running from homebrew
	ldr r0, __service_ptr
	cmp r0, #0
	bxne  r4
	@ Exit process otherwise
	svc 0x03
