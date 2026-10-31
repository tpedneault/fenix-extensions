"""The pets' effects, drawn here as maps of palette letters and written
as a PNG to ../sprites/fx.png: python src/fx.py

A row of 16x16 frames (src/fx.rs names them):
   0  heart          1  heart, small
   2  z              3  Z
   4-7  a sparkle, bursting and fading
   8-11 a flame, flickering
  12-15 a puff of smoke, spreading and fading
  16-17 a snowflake, turning
  18-21 confetti: four colours
  22  a star        23  a note
  24  a drop        25  !
"""

import os
import struct
import zlib

PALETTE = {
    "h": (240, 70, 90, 255), "H": (255, 150, 165, 255), "d": (150, 30, 50, 255),
    "z": (200, 210, 240, 255), "Z": (140, 150, 200, 255),
    "y": (255, 230, 120, 255), "Y": (255, 250, 210, 255), "o": (255, 150, 40, 255),
    "r": (235, 70, 20, 255), "R": (170, 40, 10, 255),
    "s": (150, 150, 155, 220), "S": (110, 110, 118, 170), "t": (190, 190, 195, 120), "T": (170, 170, 175, 70),
    "w": (245, 250, 255, 255), "b": (170, 200, 240, 255),
    "1": (255, 90, 120, 255), "2": (80, 200, 255, 255), "3": (255, 210, 60, 255), "4": (120, 230, 120, 255),
    "k": (40, 40, 50, 255), "c": (120, 190, 255, 255), "C": (210, 235, 255, 255),
}


def frame(rows):
    """A 16x16 frame from up to 16 rows, centred."""
    rows = [r for r in rows]
    top = (16 - len(rows)) // 2
    out = [[(0, 0, 0, 0)] * 16 for _ in range(16)]
    for y, row in enumerate(rows):
        left = (16 - len(row)) // 2
        for x, ch in enumerate(row):
            if ch != ".":
                out[top + y][left + x] = PALETTE[ch]
    return out


HEART = [".HH...HH.", "HhhH.Hhhh", "Hhhhhhhhh", "hhhhhhhhh", ".hhhhhhhd", "..hhhhhd.", "...hhhd..", "....hd..."]
HEART_SMALL = [".H.H.", "Hhhhh", "hhhhd", ".hhd.", "..d.."]
Z_SMALL = ["zzzz", "..Z.", ".Z..", "zzzz"]
Z_BIG = ["zzzzzz", "zZZZZz", "...ZZ.", "..ZZ..", ".ZZ...", "zZZZZz", "zzzzzz"]
SPARKS = [
    ["..Y..", ".YyY.", "YyYyY", ".YyY.", "..Y.."],
    ["...Y...", "...y...", ".y.Y.y.", "YyYYYyY", ".y.Y.y.", "...y...", "...Y..."],
    ["....Y....", "....y....", "..y...y..", ".........", "Yy..Y..yY", ".........", "..y...y..", "....y....", "....Y...."],
    ["....y....", ".........", "..o...o..", ".........", "y.......y", ".........", "..o...o..", ".........", "....y...."],
]
FLAMES = [
    ["...y...", "..yy...", "..yYy..", ".yoYoy.", ".oyYyo.", "oorYroo", "orrrrro", ".rRRRr."],
    ["....y..", "...yy..", "..yYy..", ".yoYyo.", ".oyYYo.", "orryroo", "orrrrro", ".rRRRr."],
    ["..y....", "..yy...", ".yYYy..", ".oyYoy.", "oyYYyo.", "oorYroo", "orrrrro", ".rRRRr."],
    ["...y...", "...yy..", "..yYy..", ".yYYoy.", ".oyYyo.", "oorrroo", "orrRrro", ".rRRRr."],
]
SMOKES = [
    ["..ss..", ".sSSs.", "sSSSSs", ".sSSs.", "..ss.."],
    ["...ss...", ".ssSSss.", "sSSSSSSs", "sSSSSSSs", ".ssSSss.", "...ss..."],
    ["...tt.t...", ".ttssstt..", "tsssssssst", "tssssssst.", ".ttssstt..", "...tt.t..."],
    ["...T..T....", ".TT.TTT.T..", "T.TTTTTT.T.", ".TTTTTTTT.T", "T.TT.TTT...", "..T...T...."],
]
FLAKES = [
    ["...w...", ".w.b.w.", "..wbw..", "wbbwbbw", "..wbw..", ".w.b.w.", "...w..."],
    [".w...w.", "..wbw..", ".bwbwb.", "..bwb..", ".bwbwb.", "..wbw..", ".w...w."],
]
CONFETTI = [["1111", "1111"], ["22", "22", "22"], ["333", "33."], [".44", "444", "44."]]
STAR = ["....y....", "....y....", "...yYy...", "yyyyYyyyy", ".yYYYYYy.", "..yYYYy..", ".yYy.yYy.", ".yy...yy."]
NOTE = ["...wwww", "...wwww", "...w..w", "...w..w", "...w..w", ".www.ww", "wwww.ww", ".ww...."]
DROP = ["..C..", "..C..", ".CcC.", "CcccC", "CcccC", ".ccc."]
BANG = ["oo", "oo", "oo", "oo", "oo", "..", "oo"]

FRAMES = [HEART, HEART_SMALL, Z_SMALL, Z_BIG] + SPARKS + FLAMES + SMOKES + FLAKES + CONFETTI + [STAR, NOTE, DROP, BANG]


def png(path, width, height, pixels):
    raw = b"".join(b"\x00" + b"".join(struct.pack("BBBB", *p) for p in row) for row in pixels)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    frames = [frame(f) for f in FRAMES]
    sheet = [sum((frames[i][y] for i in range(len(frames))), []) for y in range(16)]
    png(os.path.join(here, "..", "sprites", "fx.png"), 16 * len(frames), 16, sheet)


if __name__ == "__main__":
    main()
