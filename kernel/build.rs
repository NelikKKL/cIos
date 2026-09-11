//! Собирает C-версию nano (kernel/src/nano_c/*.c) и линкует её в ядро
//! как статическую библиотеку. См. NANO_C.md за подробностями и
//! известными рисками сборки.
//!
//! Флаги ниже НЕ произвольны:
//!   -mcmodel=kernel  -- linker.ld грузит ядро по адресу
//!                        0xffffffff80000000 (верхние 2 ГБ, классическая
//!                        схема адресов ядра x86_64) -- это ровно то, для
//!                        чего существует code model "kernel"; с моделью
//!                        по умолчанию ("small", низкие 2 ГБ) релокации
//!                        для такого адреса были бы в принципе неверны.
//!   -mno-red-zone    -- этот C-код выполняется в основном цикле ядра с
//!                        включёнными прерываниями (см. interrupts.rs);
//!                        red zone (128 байт под RSP, которые leaf-функции
//!                        используют без сдвига RSP) может быть затёрта
//!                        асинхронным прерыванием. Без этого флага —
//!                        редкая, тяжело воспроизводимая порча стека.
//!   -ffreestanding, -fno-stack-protector, -fno-pic/-fno-pie --
//!                        нет libc (со стороны C увидит nanolibc.c),
//!                        нет __stack_chk_fail, позиционно-зависимый
//!                        код (совпадает с -C relocation-model=static
//!                        и -no-pie у Rust-части в kernel/.cargo/config.toml).
//!   -mgeneral-regs-only -- на всякий случай запрещает компилятору
//!                        генерировать SSE/x87-инструкции (C-код их и
//!                        так не использует, тут просто с запасом).
use std::env;

fn main() {
    let mut build = cc::Build::new();

    // Осознанно НЕ полагаемся на автоопределение cc-crate по имени
    // таргета ("x86_64-unknown-none" -- оно попыталось бы найти
    // кросс-компилятор "x86_64-unknown-none-gcc", которого почти
    // наверняка нет). Обычный системный cc/gcc/clang прекрасно
    // компилирует freestanding-код под ту же архитектуру (x86_64) --
    // кросс-компилятор тут не нужен, см. NANO_C.md. Переопределить
    // можно переменной окружения CC, как обычно для cc-crate.
    if env::var_os("CC").is_none() {
        build.compiler("cc");
    }

    build
        .files(["src/nano_c/nano.c", "src/nano_c/nanolibc.c"])
        .include("src/nano_c")
        .opt_level(2)
        .warnings(true);

    // Критичные флаги -- если компилятор их не понимает, лучше упасть
    // на сборке, чем молча собрать нерабочее/небезопасное ядро.
    for flag in ["-ffreestanding", "-mno-red-zone", "-mcmodel=kernel"] {
        build.flag(flag);
    }

    // Второстепенные -- если конкретный компилятор не поддерживает,
    // пропускаем, а не падаем.
    for flag in [
        "-fno-stack-protector",
        "-fno-pic",
        "-fno-pie",
        "-mgeneral-regs-only",
        "-fno-asynchronous-unwind-tables",
        "-Wall",
        "-Wextra",
    ] {
        build.flag_if_supported(flag);
    }

    build.compile("nano_c");

    println!("cargo:rerun-if-changed=src/nano_c/nano.c");
    println!("cargo:rerun-if-changed=src/nano_c/nano.h");
    println!("cargo:rerun-if-changed=src/nano_c/nanolibc.c");
    println!("cargo:rerun-if-changed=src/nano_c/nanolibc.h");
}
