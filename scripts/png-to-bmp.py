#!/usr/bin/env python3
"""Converts the repository PNG assets into their 32-bit BMP twins.

Why this exists: the PNG decoder in SDL_image is optional at build time, and the
GIOCHI target image ships a `libSDL2_image` that links only `libSDL2`, `libm` and
`libc` -- PNG and JPEG are compiled as "not supported" stubs (see
docs/KMSDRM.md section 8.4). BMP needs no external library, so it decodes on
every build. The demo therefore prefers `player.png` and falls back to
`player.bmp`.

Keep the two files byte-identical in pixels, so the fallback is visually
indistinguishable. Re-run from the repository root after touching a PNG:

    python3 scripts/png-to-bmp.py assets/player.png assets/checker.png

and `scripts/gates.sh` runs it in `--check` mode, which fails when a `.bmp` has
drifted from its `.png`.

Only 8-bit non-interlaced colour type 2 (RGB) and 6 (RGBA) inputs are accepted,
which covers everything in `assets/`.
"""

from __future__ import annotations

import struct
import sys
import zlib

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

# BITMAPV4HEADER field masks for a little-endian 0xAARRGGBB pixel.
MASKS = (0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)
BI_BITFIELDS = 3
BI_RGB = 0
LCS_sRGB = 0x73524742  # 'sRGB'


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def decode_png(path: str) -> tuple[int, int, list[list[tuple[int, int, int, int]]]]:
    """Returns (width, height, rows) with rows top-to-bottom as RGBA tuples."""
    with open(path, "rb") as handle:
        data = handle.read()

    if not data.startswith(PNG_SIGNATURE):
        raise SystemExit(f"{path}: not a PNG file")

    header = None
    compressed = bytearray()
    position = len(PNG_SIGNATURE)
    while position < len(data):
        length = struct.unpack(">I", data[position : position + 4])[0]
        kind = data[position + 4 : position + 8]
        payload = data[position + 8 : position + 8 + length]
        position += 12 + length
        if kind == b"IHDR":
            header = payload
        elif kind == b"IDAT":
            compressed += payload
        elif kind == b"IEND":
            break

    if header is None:
        raise SystemExit(f"{path}: missing IHDR")

    width, height, depth, colour, compression, filtering, interlace = struct.unpack(
        ">IIBBBBB", header[:13]
    )
    if (depth, compression, filtering, interlace) != (8, 0, 0, 0):
        raise SystemExit(
            f"{path}: only 8-bit non-interlaced PNGs are supported, "
            f"got depth={depth} interlace={interlace}"
        )
    if colour not in (2, 6):
        raise SystemExit(f"{path}: only RGB/RGBA colour types are supported, got {colour}")

    channels = 3 if colour == 2 else 4
    stride = width * channels
    raw = zlib.decompress(bytes(compressed))

    rows: list[list[tuple[int, int, int, int]]] = []
    previous = bytearray(stride)
    offset = 0
    for _ in range(height):
        method = raw[offset]
        offset += 1
        line = bytearray(raw[offset : offset + stride])
        offset += stride

        if method == 1:
            for index in range(channels, stride):
                line[index] = (line[index] + line[index - channels]) & 0xFF
        elif method == 2:
            for index in range(stride):
                line[index] = (line[index] + previous[index]) & 0xFF
        elif method == 3:
            for index in range(stride):
                left = line[index - channels] if index >= channels else 0
                line[index] = (line[index] + ((left + previous[index]) >> 1)) & 0xFF
        elif method == 4:
            for index in range(stride):
                left = line[index - channels] if index >= channels else 0
                up_left = previous[index - channels] if index >= channels else 0
                line[index] = (line[index] + _paeth(left, previous[index], up_left)) & 0xFF
        elif method != 0:
            raise SystemExit(f"{path}: unknown filter {method}")

        row = []
        for index in range(0, stride, channels):
            red, green, blue = line[index], line[index + 1], line[index + 2]
            alpha = line[index + 3] if channels == 4 else 255
            row.append((red, green, blue, alpha))
        rows.append(row)
        previous = line

    return width, height, rows


def encode_bmp(width: int, height: int, rows: list[list[tuple[int, int, int, int]]]) -> bytes:
    """32-bit BI_BITFIELDS BMP with a BITMAPV4HEADER, bottom-up like the format demands."""
    pixels = bytearray()
    for row in reversed(rows):
        for red, green, blue, alpha in row:
            pixels += bytes((blue, green, red, alpha))

    header_size = 108
    offset = 14 + header_size
    file_size = offset + len(pixels)

    file_header = struct.pack("<2sIHHI", b"BM", file_size, 0, 0, offset)

    # size, width, height (positive: bottom-up), planes, bpp, compression, image size,
    # x ppm, y ppm, colours used, important colours, then the four channel masks,
    # colour space and the unused endpoints/gamma tail.
    info_header = struct.pack(
        "<IiiHHIIiiII",
        header_size, width, height, 1, 32, BI_BITFIELDS, len(pixels), 2835, 2835, 0, 0,
    )
    info_header += struct.pack("<IIII", *MASKS)
    info_header += struct.pack("<I", LCS_sRGB)
    info_header += bytes(header_size - len(info_header))

    return file_header + info_header + bytes(pixels)


def convert(source: str, destination: str) -> None:
    width, height, rows = decode_png(source)
    payload = encode_bmp(width, height, rows)
    with open(destination, "wb") as handle:
        handle.write(payload)
    print(f"{destination}: {width}x{height}, {len(payload)} bytes")


def check(source: str, destination: str) -> bool:
    """Returns True when the on-disk BMP already matches what the PNG encodes."""
    width, height, rows = decode_png(source)
    expected = encode_bmp(width, height, rows)
    try:
        with open(destination, "rb") as handle:
            actual = handle.read()
    except FileNotFoundError:
        print(f"{destination}: missing; run scripts/png-to-bmp.py {source}", file=sys.stderr)
        return False

    if actual != expected:
        print(f"{destination}: out of date; run scripts/png-to-bmp.py {source}", file=sys.stderr)
        return False

    print(f"{destination}: up to date")
    return True


def main(argv: list[str]) -> int:
    args = argv[1:]
    verify = False
    if args and args[0] == "--check":
        verify = True
        args = args[1:]

    if not args:
        print(__doc__.strip().splitlines()[0], file=sys.stderr)
        print(f"usage: {argv[0]} [--check] <file.png> [more.png ...]", file=sys.stderr)
        return 2

    failed = False
    for source in args:
        if not source.lower().endswith(".png"):
            print(f"{source}: expected a .png file", file=sys.stderr)
            return 2
        destination = source[: -len(".png")] + ".bmp"
        if verify:
            failed |= not check(source, destination)
        else:
            convert(source, destination)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
