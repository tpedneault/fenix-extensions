# Changelog

## 1.2.0

- For Fenix 1.5 (extension API 1.5): built against API 1.5, as the shared
  `ui-kit` it draws its pages with now is. The swatches, the picker, the
  contrast checker and the palette work as they did; on Fenix 1.0 to 1.4,
  1.1.0 stays the one you have. The README gives standard keys beside
  Vim's.

## 1.1.0

- `colours.palette` can be run from outside Fenix, over the control channel
  (`fenix/runCommand`): its page opened by a script or another tool, as
  the screenshots' scripts do.

## 1.0.0

- The first release, for Fenix 1.0: colour literals made visible and editable: a swatch beside each (hex, rgb(), hsl(), oklch(), CSS names, a theme's), a picker that writes it back as it was written, conversions between hex, RGB, HSL and OKLCH, a WCAG contrast checker, and the project's palette.
