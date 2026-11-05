# Changelog

## 1.0.1

- Go to an offset is on `o`: `:` never reached the page, since Fenix keeps it for its command line.
- `x` clears the selection and the match. Esc closes the page, as on every page, so it couldn't.
- The search prompt opens empty, so what you type isn't added to the last search.

## 1.0.0

- The first release, for Fenix 1.0: binary files as a hex dump -- offsets, bytes and their text, read a block at a time so a file of any size opens at once, searched for bytes or text, the cursor's bytes read as integers and floats both ways round and as UTF-8, selections copied as hex, text or code -- and any file at all shown in hex with `hex.open`.
