// SPDX-License-Identifier: GPL-3.0-or-later
// Product-owned EL1 adapter; guest application instructions are never patched.
.section .text.trames_kvm,"ax"
.balign 2048
.macro trames_adapter name, high
.global \name\()_vectors_start
\name\()_vectors_start:
.rept 8
    b .Ladapter_fault_\@
    .space 124
.endr
    b .Lsave_\@
    .space 124
.rept 7
    b .Ladapter_fault_\@
    .space 124
.endr

.global \name\()_resume
\name\()_resume:
    clrex
    ldr x0, [sp, #864]
    cbz x0, 1f
    dsb ishst
    .if \high
    ldr x1, [sp, #848]
    tlbi aside1is, x1
    .else
    tlbi vmalle1is
    .endif
    dsb ish
    isb
    tbz x0, #1, 2f
    ic ialluis
    dsb ish
    isb
2:
    str xzr, [sp, #864]
1:
    ldr x0, [sp, #248]
    msr sp_el0, x0
    ldr x0, [sp, #256]
    msr elr_el1, x0
    ldr w0, [sp, #264]
    and x0, x0, #0xf0000000
    orr x0, x0, #0x3c0
    msr spsr_el1, x0
    ldr x0, [sp, #272]
    msr tpidr_el0, x0
    ldr w0, [sp, #288]
    msr fpcr, x0
    ldr w0, [sp, #292]
    msr fpsr, x0
    ldp q0, q1, [sp, #304]
    ldp q2, q3, [sp, #336]
    ldp q4, q5, [sp, #368]
    ldp q6, q7, [sp, #400]
    ldp q8, q9, [sp, #432]
    ldp q10, q11, [sp, #464]
    ldp q12, q13, [sp, #496]
    ldp q14, q15, [sp, #528]
    ldp q16, q17, [sp, #560]
    ldp q18, q19, [sp, #592]
    ldp q20, q21, [sp, #624]
    ldp q22, q23, [sp, #656]
    ldp q24, q25, [sp, #688]
    ldp q26, q27, [sp, #720]
    ldp q28, q29, [sp, #752]
    ldp q30, q31, [sp, #784]
    str xzr, [sp, #856]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]
    ldp x0, x1, [sp]
    eret

.global \name\()_saving
\name\()_saving:
.Lsave_\@:
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
    stp q0, q1, [sp, #304]
    stp q2, q3, [sp, #336]
    stp q4, q5, [sp, #368]
    stp q6, q7, [sp, #400]
    stp q8, q9, [sp, #432]
    stp q10, q11, [sp, #464]
    stp q12, q13, [sp, #496]
    stp q14, q15, [sp, #528]
    stp q16, q17, [sp, #560]
    stp q18, q19, [sp, #592]
    stp q20, q21, [sp, #624]
    stp q22, q23, [sp, #656]
    stp q24, q25, [sp, #688]
    stp q26, q27, [sp, #720]
    stp q28, q29, [sp, #752]
    stp q30, q31, [sp, #784]
    mrs x0, sp_el0
    str x0, [sp, #248]
    mrs x0, elr_el1
    str x0, [sp, #256]
    mrs x0, spsr_el1
    and w0, w0, #0xf0000000
    str w0, [sp, #264]
    mrs x0, tpidr_el0
    str x0, [sp, #272]
    str xzr, [sp, #280] // SME is not enabled by this baseline adapter.
    mrs x0, fpcr
    str w0, [sp, #288]
    mrs x0, fpsr
    str w0, [sp, #292]
    mrs x0, esr_el1
    str x0, [sp, #816]
    mrs x1, far_el1
    str x1, [sp, #824]
    ubfx x0, x0, #26, #6
    cmp x0, #0x15
    mov x0, #1
    mov x1, #2
    csel x0, x0, x1, eq
    str x0, [sp, #832]
    b .Lnotify_\@
.Ladapter_fault_\@:
    mrs x0, esr_el1
    str x0, [sp, #816]
    mrs x0, far_el1
    str x0, [sp, #824]
    mov x0, #3
    str x0, [sp, #832]
.Lnotify_\@:
    mov x0, #1
    str x0, [sp, #856]
    ldr x0, [sp, #840]
    cbz x0, .Lmmio_\@
    movz x0, #0x5452
    movk x0, #0x8600, lsl #16
    hvc #0
.global \name\()_hvc_return
\name\()_hvc_return:
    b \name\()_resume
.Lmmio_\@:
    .if \high
    movn x1, #0x2fff
    .else
    mov x1, #0xd000
    .endif
    mov w0, #1
    str w0, [x1]
.global \name\()_mmio_return
\name\()_mmio_return:
    b \name\()_resume
.global \name\()_vectors_end
\name\()_vectors_end:

.endm
trames_adapter trames_kvm, 0
.balign 2048
trames_adapter trames_engine, 1

// Original, separately loaded fixture instructions. There is no rewriting.
.balign 4
.global trames_kvm_guest_start
trames_kvm_guest_start:
    mov x8, #172
1:  svc #0
    add x9, x0, #1
    b 1b
.global trames_kvm_guest_end
trames_kvm_guest_end:

.global trames_kvm_memory_start
trames_kvm_memory_start:
    mov x10, #0x20000
    ldr x11, [x10]
    add x11, x11, #1
    str x11, [x10]
    mov x8, #172
    svc #0
    add x10, x10, #4096
    str x11, [x10]
    svc #0
    add x10, x10, #4096
    ldr x11, [x10]
    mov x10, #0xa000
    ldr x11, [x10]
    b .
.global trames_kvm_memory_end
trames_kvm_memory_end:

.global trames_kvm_loop_start
trames_kvm_loop_start:
    b .
.global trames_kvm_loop_end
trames_kvm_loop_end:
