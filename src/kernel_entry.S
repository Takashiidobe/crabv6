    .section .text
    .globl enter_user_trampoline
enter_user_trampoline:
    # Store current kernel context so we can restore it after the user program exits
    la t0, KERNEL_STACK_POINTER
    sd sp, 0(t0)
    la t0, KERNEL_RETURN_ADDRESS
    sd ra, 0(t0)

    # Switch to the user stack (a1) and jump to the user entry point (a0)
    mv sp, a1
    csrw sepc, a0
    sret

    .section .text
    .globl kernel_resume_from_user
kernel_resume_from_user:
    # Restore the saved kernel stack pointer before returning to Rust code
    la t0, KERNEL_STACK_POINTER
    ld sp, 0(t0)
    ret
