#!/usr/bin/env python3
"""Convert a little-endian ELF32 to a Pico LiPo 2 XL W (RP2350 ARM Secure) UF2 file."""

from __future__ import annotations

import struct
import sys
from pathlib import Path

UF2_MAGIC_START0 = 0x0A324655
UF2_MAGIC_START1 = 0x9E5D5157
UF2_MAGIC_END = 0x0AB16F30
UF2_FLAG_FAMILY_ID_PRESENT = 0x00002000
RP2350_ARM_S_FAMILY_ID = 0xE48BFF59
PAGE_SIZE = 256
FLASH_START = 0x10000000
FLASH_END = 0x11000000  # Pico LiPo 2 XL W: 16 MiB


def load_flash_pages(elf_path: Path) -> dict[int, bytearray]:
    data = elf_path.read_bytes()
    if data[:4] != b"\x7fELF":
        raise SystemExit(f"{elf_path} is not an ELF file")
    if data[4] != 1 or data[5] != 1:
        raise SystemExit("expected little-endian ELF32 (RP2350 ARM image)")

    (
        _e_type,
        _e_machine,
        _e_version,
        _e_entry,
        e_phoff,
        _e_shoff,
        _e_flags,
        _e_ehsize,
        e_phentsize,
        e_phnum,
    ) = struct.unpack_from("<HHIIIIIHHH", data, 16)

    pages: dict[int, bytearray] = {}
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        (
            p_type,
            p_offset,
            p_vaddr,
            p_paddr,
            p_filesz,
            _p_memsz,
            _p_flags,
            _p_align,
        ) = struct.unpack_from("<IIIIIIII", data, off)
        if p_type != 1 or p_filesz == 0:
            continue
        addr = p_paddr or p_vaddr
        if addr < FLASH_START or addr >= FLASH_END:
            continue
        chunk = data[p_offset : p_offset + p_filesz]
        for index, byte in enumerate(chunk):
            absolute = addr + index
            if absolute >= FLASH_END:
                break
            page = absolute & ~(PAGE_SIZE - 1)
            pages.setdefault(page, bytearray(PAGE_SIZE))
            pages[page][absolute - page] = byte
    if not pages:
        raise SystemExit(f"no flash contents in {elf_path}")
    return pages


def write_uf2(pages: dict[int, bytearray], uf2_path: Path) -> None:
    addrs = sorted(pages)
    num_blocks = len(addrs)
    out = bytearray()
    for block_no, addr in enumerate(addrs):
        payload = pages[addr]
        header = struct.pack(
            "<IIIIIIII",
            UF2_MAGIC_START0,
            UF2_MAGIC_START1,
            UF2_FLAG_FAMILY_ID_PRESENT,
            addr,
            PAGE_SIZE,
            block_no,
            num_blocks,
            RP2350_ARM_S_FAMILY_ID,
        )
        block = header + payload + bytes(476 - PAGE_SIZE) + struct.pack("<I", UF2_MAGIC_END)
        if len(block) != 512:
            raise SystemExit("internal UF2 block size error")
        out += block
    uf2_path.write_bytes(out)


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: elf2uf2_rp2350.py <input.elf> <output.uf2>")
    elf_path = Path(sys.argv[1])
    uf2_path = Path(sys.argv[2])
    write_uf2(load_flash_pages(elf_path), uf2_path)


if __name__ == "__main__":
    main()
