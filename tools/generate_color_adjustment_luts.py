"""Regenerate the bundled RGB8 tables from a DDS Builder v2.1 source directory."""

import argparse
import sys
from pathlib import Path

import numpy as np
from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("builder", type=Path)
    builder = parser.parse_args().builder.resolve(strict=True)
    sys.path.insert(0, str(builder))
    sys.dont_write_bytecode = True
    from wt_color_compensation import A3_SHADOW_RECIPE, RECIPE, compensate

    output = Path(__file__).resolve().parents[1] / "assets" / "color-adjustment"
    output.mkdir(parents=True, exist_ok=True)

    # Row-major index is (red << 16) | (green << 8) | blue.
    size = 4096
    colors = np.empty((size, size, 3), dtype=np.uint8)
    for row in range(0, size, 128):
        indices = np.arange(row * size, (row + 128) * size, dtype=np.uint32)
        block = colors[row:row + 128].reshape(-1, 3)
        block[:, 0] = (indices >> 16) & 255
        block[:, 1] = (indices >> 8) & 255
        block[:, 2] = indices & 255

    source = Image.fromarray(colors)
    for name, recipe in [("full-v2", RECIPE), ("a3-shadows", A3_SHADOW_RECIPE)]:
        path = output / f"{name}.png"
        compensate(source, recipe=recipe).save(path, compress_level=9)
        print(f"{path.name} ({path.stat().st_size:,} bytes)", flush=True)


if __name__ == "__main__":
    main()
