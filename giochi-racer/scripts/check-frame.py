#!/usr/bin/env python3
"""Check that a captured frame actually contains a scene.

A game that renders nothing still exits 0: the loop ran, the frame was presented, the
window closed. The only way to tell "the racer drew the track" from "the racer cleared the
screen 180 times" from a headless run is to look at the pixels.

Usage:
    python3 giochi-racer/scripts/check-frame.py /tmp/race.bmp

Exits non-zero, with the reason, when the frame is uniform or the header is wrong. The
thresholds are deliberately loose: this is a smoke test for "something was drawn", not a
golden-image comparison, because a golden image would fail on every driver that rounds a
shade differently.
"""

import struct
import sys

# Below this many distinct colours the frame is a clear colour with an accident on it.
MIN_DISTINCT_COLOURS = 24
# A 32-bit BMP header is 54 bytes; anything shorter is not a capture.
HEADER = 54


def read_bmp(path):
    """Returns (width, height, pixels) for an uncompressed 32-bit BMP."""
    data = open(path, "rb").read()
    if len(data) < HEADER or data[:2] != b"BM":
        raise SystemExit(f"{path}: not a BMP")
    width, height = struct.unpack_from("<ii", data, 18)
    bits = struct.unpack_from("<H", data, 28)[0]
    if bits != 32:
        raise SystemExit(f"{path}: expected 32 bits per pixel, found {bits}")
    if width <= 0 or height <= 0:
        raise SystemExit(f"{path}: impossible size {width}x{height}")
    expected = HEADER + width * height * 4
    if len(data) < expected:
        raise SystemExit(f"{path}: {len(data)} bytes, expected at least {expected}")
    pixels = data[HEADER:expected]
    return width, height, [pixels[i : i + 4] for i in range(0, len(pixels), 4)]


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} FRAME.bmp", file=sys.stderr)
        return 2
    path = sys.argv[1]
    width, height, pixels = read_bmp(path)

    # Every 97th pixel: prime, so a striped scene cannot alias into one colour.
    sampled = {pixel for pixel in pixels[::97]}
    if len(sampled) < MIN_DISTINCT_COLOURS:
        print(
            f"{path}: only {len(sampled)} distinct colours in a {width}x{height} frame; "
            "the renderer drew nothing",
            file=sys.stderr,
        )
        return 1

    # A frame with the track in it has sky along one edge and road along the other; a
    # cleared frame has the same colour everywhere.
    top = set(pixels[:width])
    bottom = set(pixels[-width:])
    if top == bottom:
        print(f"{path}: the top and bottom rows are identical", file=sys.stderr)
        return 1

    print(f"frame ok: {path} {width}x{height}, {len(sampled)} sampled colours")
    return 0


if __name__ == "__main__":
    sys.exit(main())
