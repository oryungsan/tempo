"""Tempo 실행 파일 리소스(아이콘 · 버전 정보) 만들기.

    python tools/make_res.py <버전 예: 1.0.0> <windres.exe 경로>

assets/tempo.ico · assets/tempo.res 를 만들고 windres 로 assets/tempo-res.o(COFF) 로 바꾼다.
build.rs 가 그 .o 를 링크하므로 평소 빌드에는 windres 가 필요 없다 (버전·아이콘을 바꿀 때만 다시 실행).
아이콘 그림 = src/ui/theme.rs icon_rgba 와 같은 모양 (여기서는 4배 슈퍼샘플링으로 부드럽게).
"""
import math, os, struct, subprocess, sys, zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ASSETS = os.path.join(ROOT, "assets")


def pixel(x, y, n):
    c0 = n / 2.0
    s = n / 64.0
    dx, dy = x - c0, y - c0
    d = math.hypot(dx, dy)
    if d >= 30.0 * s:
        return (0, 0, 0, 0)
    ang = math.degrees(math.atan2(dy, dx))
    sweep = (ang + 90.0 + 360.0) % 360.0
    if 19.0 * s <= d < 25.0 * s and sweep < 270.0:
        return (0x5B, 0x8D, 0xEF, 255)
    if d < 3.2 * s or (abs(dx) < 1.6 * s and -13.0 * s < dy < 0.0):
        return (0xEC, 0xEE, 0xF1, 255)
    return (0x15, 0x17, 0x1C, 255)


def render(n, ss=4):
    out = bytearray()
    for y in range(n):
        out.append(0)  # PNG 필터 바이트
        for x in range(n):
            acc = [0, 0, 0, 0]
            for j in range(ss):
                for i in range(ss):
                    r, g, b, a = pixel(x + (i + 0.5) / ss, y + (j + 0.5) / ss, n)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            a = acc[3]
            if a == 0:
                out += b"\0\0\0\0"
            else:
                out += bytes([acc[0] // a, acc[1] // a, acc[2] // a, a // (ss * ss)])
    return bytes(out)


def png(n):
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", n, n, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(render(n), 9)) + chunk(b"IEND", b"")


def res_entry(rtype, name, data, lang=0x0412):
    hdr = struct.pack("<II", len(data), 32) + struct.pack("<HH", 0xFFFF, rtype) + struct.pack("<HH", 0xFFFF, name)
    hdr += struct.pack("<IHHII", 0, 0x1030, lang, 0, 0)
    pad = (-len(data)) % 4
    return hdr + data + b"\0" * pad


def ver_block(key, value=b"", vtype=1, children=b"", is_text=True):
    k = (key + "\0").encode("utf-16-le")
    head_len = 6 + len(k)
    pad1 = (-head_len) % 4
    if is_text and isinstance(value, str):
        v = (value + "\0").encode("utf-16-le")
        vlen = len(v) // 2
    else:
        v = value
        vlen = len(v)
    body = k + b"\0" * pad1 + v
    pad2 = (-(6 + len(body))) % 4
    body += b"\0" * pad2 + children
    return struct.pack("<HHH", 6 + len(body), vlen, vtype) + body


def version_res(ver):
    a, b, c = (int(x) for x in ver.split("."))
    ms, ls = (a << 16) | b, (c << 16)
    fixed = struct.pack("<13I", 0xFEEF04BD, 0x00010000, ms, ls, ms, ls, 0x3F, 0, 0x4, 0x1, 0, 0, 0)
    strings = [
        ("CompanyName", "Tempo"),
        ("FileDescription", "Tempo — 타이머 · 알람 · 리마인더"),
        ("FileVersion", ver),
        ("InternalName", "tempo"),
        ("LegalCopyright", "MIT License"),
        ("OriginalFilename", "tempo.exe"),
        ("ProductName", "Tempo"),
        ("ProductVersion", ver),
    ]
    sblocks = b"".join(ver_block(k, v, 1) for k, v in strings)
    table = ver_block("041204B0", b"", 1, sblocks, is_text=False)
    sfi = ver_block("StringFileInfo", b"", 1, table, is_text=False)
    var = ver_block("Translation", struct.pack("<HH", 0x0412, 0x04B0), 0, is_text=False)
    vfi = ver_block("VarFileInfo", b"", 1, var, is_text=False)
    return ver_block("VS_VERSION_INFO", fixed, 0, sfi + vfi, is_text=False)


def main():
    ver = sys.argv[1] if len(sys.argv) > 1 else "1.0.0"
    windres = sys.argv[2] if len(sys.argv) > 2 else "windres"
    os.makedirs(ASSETS, exist_ok=True)
    sizes = [16, 24, 32, 48, 64, 128, 256]
    imgs = [png(n) for n in sizes]
    # .ico
    ico = struct.pack("<HHH", 0, 1, len(sizes))
    off = 6 + 16 * len(sizes)
    for n, im in zip(sizes, imgs):
        ico += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(im), off)
        off += len(im)
    ico += b"".join(imgs)
    open(os.path.join(ASSETS, "tempo.ico"), "wb").write(ico)
    # .res : 빈 머리 + RT_ICON(3) 들 + RT_GROUP_ICON(14) + RT_VERSION(16)
    res = struct.pack("<II", 0, 32) + struct.pack("<HHHH", 0xFFFF, 0, 0xFFFF, 0) + b"\0" * 16
    grp = struct.pack("<HHH", 0, 1, len(sizes))
    for i, (n, im) in enumerate(zip(sizes, imgs), start=1):
        res += res_entry(3, i, im)
        grp += struct.pack("<BBBBHHIH", n % 256, n % 256, 0, 0, 1, 32, len(im), i)
    res += res_entry(14, 1, grp)
    res += res_entry(16, 1, version_res(ver))
    rp = os.path.join(ASSETS, "tempo.res")
    open(rp, "wb").write(res)
    op = os.path.join(ASSETS, "tempo-res.o")
    subprocess.run([windres, "-J", "res", "-O", "coff", "-i", rp, "-o", op], check=True)
    print("ok", os.path.getsize(op), "bytes")


if __name__ == "__main__":
    main()
