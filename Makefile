.PHONY: all kernel limine iso run clean

KERNEL_BIN := kernel/target/x86_64-unknown-none/release/cios-kernel

all: iso

kernel:
	cd kernel && cargo build --release \
		-Z build-std=core,alloc,compiler_builtins \
		-Z build-std-features=compiler-builtins-mem \
		--target x86_64-unknown-none

limine:
	@if [ ! -d limine ]; then \
		git clone https://github.com/limine-bootloader/limine.git --branch=v7.x-binary --depth=1 ; \
		$(MAKE) -C limine ; \
	fi

iso: kernel limine
	rm -rf iso_root
	mkdir -p iso_root/boot/limine
	cp $(KERNEL_BIN) iso_root/boot/cios.elf
	cp limine.cfg iso_root/boot/limine/
	cp limine/limine-bios.sys iso_root/boot/limine/
	cp limine/limine-bios-cd.bin iso_root/boot/limine/
	cp limine/limine-uefi-cd.bin iso_root/boot/limine/
	mkdir -p iso_root/EFI/BOOT
	cp limine/BOOTX64.EFI iso_root/EFI/BOOT/
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o cios.iso
	./limine/limine bios-install cios.iso

run: iso
	qemu-system-x86_64 -cdrom cios.iso -serial stdio -m 256M

clean:
	rm -rf iso_root cios.iso kernel/target
