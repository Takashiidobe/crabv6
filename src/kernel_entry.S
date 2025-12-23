		.section .text
    .globl enter_user_trampoline
enter_user_trampoline:
    # a0 = user entry point (_start)
    # a1 = user stack pointer
    # a2 = argc
    # a3 = argv pointer

    # Save kernel stack and return address
    la      t0, KERNEL_STACK_POINTER
    sd      sp, 0(t0)
    la      t0, KERNEL_RETURN_ADDRESS
    sd      ra, 0(t0)

    # Record kernel stack pointer for trap entry swapping.
    csrw    sscratch, sp

    # Preserve user entry before repurposing a0
    mv      t1, a0

    # Switch to user stack
    mv      sp, a1

    # Move argument registers into ABI positions for user-space
    mv      a0, a2        # argc
    mv      a1, a3        # argv

    # Set program counter to user entry
    csrw    sepc, t1
    sret                  # return to U-mode

    .section .text
    .globl kernel_resume_from_user
kernel_resume_from_user:
    # Restore kernel stack pointer before returning to Rust code
    la      t0, KERNEL_STACK_POINTER
    ld      sp, 0(t0)
    csrw    sscratch, zero
    ret

    .section .trap, "ax"
    .align 2
    .globl _start_trap
_start_trap:
    csrrw   sp, sscratch, sp
    beqz    sp, 1f               # sscratch was zero => trap from supervisor

    # Trap from user mode: kernel stack pointer now in sp.
    addi    sp, sp, -128
    j       2f

1:  # Trap from supervisor mode: restore original stack pointer.
    csrrw   sp, sscratch, sp      # Swap back so sp holds the supervisor stack.
    addi    sp, sp, -128

2:
    sd      ra,   0(sp)
    sd      t0,   8(sp)
    sd      t1,  16(sp)
    sd      t2,  24(sp)
    sd      t3,  32(sp)
    sd      t4,  40(sp)
    sd      t5,  48(sp)
    sd      t6,  56(sp)
    sd      a0,  64(sp)
    sd      a1,  72(sp)
    sd      a2,  80(sp)
    sd      a3,  88(sp)
    sd      a4,  96(sp)
    sd      a5, 104(sp)
    sd      a6, 112(sp)
    sd      a7, 120(sp)

    mv      a0, sp
    jal     ra, _start_trap_rust

    csrr    t6, sstatus
    andi    t6, t6, 0x100

    ld      ra,   0(sp)
    ld      t1,  16(sp)
    ld      t2,  24(sp)
    ld      t3,  32(sp)
    ld      t4,  40(sp)
    ld      t5,  48(sp)
    ld      a0,  64(sp)
    ld      a1,  72(sp)
    ld      a2,  80(sp)
    ld      a3,  88(sp)
    ld      a4,  96(sp)
    ld      a5, 104(sp)
    ld      a6, 112(sp)
    ld      a7, 120(sp)

    beqz    t6, 3f
    ld      t6, 56(sp)
    ld      t0,  8(sp)
    addi    sp, sp, 128
    csrw    sscratch, zero
    sret

3:
    ld      t6, 56(sp)
    ld      t0,  8(sp)
    addi    sp, sp, 128
    csrrw   sp, sscratch, sp
    sret
