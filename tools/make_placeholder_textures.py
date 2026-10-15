# 生成引擎自带的一张 1x1 白纹理。
#
# 用途：UI 要画纯色块（backlog 的半透明底、存读档槽位底、淡入淡出的黑幕），
# 而 sprite 管线只画带纹理的四边形。把这张白纹理铺开、再用 shader 的
# `params.yzw` 乘上颜色，就等于纯色块——省掉一条单独的纯色管线。
#
# 用法（在仓库根目录）：
#     python tools/make_placeholder_textures.py
#
# 这不是构建步骤，只在需要重新生成资源时手动跑一次；生成结果已提交。

import os
import struct
import zlib


def png_chunk(tag: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def write_rgba_png(path: str, width: int, height: int, pixel: bytes) -> None:
    raw = b"".join(b"\x00" + pixel * width for _ in range(height))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(raw, 9))
        + png_chunk(b"IEND", b"")
    )
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)
    print(f"{path}  {len(png)} bytes")


if __name__ == "__main__":
    white = b"\xff\xff\xff\xff"
    for project in ("examples/minimal", "examples/phone"):
        write_rgba_png(os.path.join(project, "assets/ui/white.png"), 2, 2, white)
