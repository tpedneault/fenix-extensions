# Hex viewer

Binary files as a hex dump in a pane: a row each 16 bytes, its offset,
its bytes in hex, and the same bytes as text, what isn't printable ASCII
as `.`. The bytes under the cursor are read beside the dump as numbers
and as UTF-8.

```text
firmware.bin  1,048,576 bytes (1.0 MiB) · 16 a row
offset    00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f  0123456789abcdef         little-endian         big-endian
00000000  7f 45 4c 46 02 01 01 00  00 00 00 00 00 00 00 00  .ELF............   u8    127
00000010  02 00 3e 00 01 00 00 00  c5 48 40 00 00 00 00 00  ..>......H@.....   i8    127
00000020  40 00 00 00 00 00 00 00  70 0b 10 00 00 00 00 00  @.......p.......   bits  0111 1111
                                                                                 u16   17791                 32581
                                                                                 ...
                                                                                 UTF-8 U+007F · 1 byte
0x0 · 0 of 1,048,576 · 0%
```

What's only ever bytes opens in it however you open it -- the explorer,
the file picker, `:e`: `*.bin`, `*.img`, `*.rom`, `*.iso`, `*.dmp`,
`*.exe`, `*.dll`, `*.so`, `*.dylib`, `*.o`, `*.obj`, `*.a`, `*.lib`,
`*.elf`, `*.class`, `*.pyc` and `*.wasm`. Not `*.hex`: that's mostly
Intel HEX, which is text, and so are `*.dat` and `*.srec` more often than
not. Any file at all, those too, is shown in hex with **Hex: show this
file as a hex dump** (`hex.open`) from the command palette.

A file in the project is read where it is, a block at a time as it's
shown, so a file of gigabytes opens, and goes to its end, at once. One
elsewhere is read whole, up to the 64 MiB Fenix gives an extension; open
a bigger one from its project.

| Key | What it does |
| --- | --- |
| `h` `l`, Left Right | A byte back, on |
| `j` `k`, Down Up | A row down, up |
| `w` `b` | On to the next 8 bytes, back to the last |
| `0` `$`, Home End | The row's first and last byte |
| `g` `G` | The file's first and last byte |
| `J` `K`, PageDown PageUp, Ctrl+F Ctrl+B | A screen down, up |
| Ctrl+D Ctrl+U | Half a screen down, up |
| `o` | Go to an offset: `0x1f0` or `1f0h` hex (a hex letter makes it hex too), `496` decimal, `50%`, `+16` or `-0x10` from the cursor |
| `/` | Search: hex bytes (`de ad be ef`, `0xdeadbeef`, `??` for any byte), or `"text"` quoted; with the cursor in the text column, what's typed is text |
| `n` `N` | The next match, the one before -- round the file's end |
| `*` | Search for the selected bytes, or the cursor's |
| Tab | The cursor to the text column and back |
| `v` | Start selecting, or stop |
| Shift with a move | Select as it moves |
| `y` | Copy the selection (or the cursor's byte) as hex: `de ad be ef` |
| `Y` | Copy it as text; what isn't UTF-8 as `�` |
| `c` | Copy it as code: `0xde, 0xad, 0xbe, 0xef` |
| `i` | Show or hide the inspector |
| `x` | Clear the selection and the match |
| `R` | Read the file again |
| `q`, Esc | Close it |

With the mouse: a click puts the cursor on a byte (in hex or in the
text), a drag selects, Shift with a click selects to it, and the wheel
scrolls.

The **inspector** reads the bytes from the cursor as `u8`, `i8` and its
bits, then `u16`, `i16`, `u32`, `i32`, `u64`, `i64`, `f32` and `f64` both
little- and big-endian, and the UTF-8 character they start. It's beside
the dump when the pane is wide enough, else below it.

Bytes are coloured by what they are: nul dimmed, printable ASCII plain,
control characters and bytes past ASCII picked out.

A copy takes up to 1 MiB.

## Settings

| Setting | What it does |
| --- | --- |
| `hex.bytesPerRow` | Bytes on each row, up to 64; `0` fits as many as the pane takes (8, 16 or 32). 16 unless set |

## Permissions

| Permission | Why |
| --- | --- |
| `filesystem = "project-read"` | A file in the project is read where it is, as far as each read needs |
| `editor = "read"` | Which file `hex.open` shows, and where the project is |
