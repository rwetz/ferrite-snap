# Snap

![Snap icon](assets/logo.svg)

A native visual-regression workbench for the real Ferrite component gallery. Capture Controls, Forms, Data & input, Navigation, Charts, ASCII, Layout, and Motion in every shipped scheme. Choose light or dark captures independently. View baseline, current, and accent-highlighted differences side by side.

Captures use a fixed 1180x900 logical viewport, the October 10, 2026 fixture date, and reduced motion. Snap measures the physical window at your Windows DPI. Use the same DPI, OS, font build, and viewport for comparable baselines. Captures show each page's initial viewport, not content below the fold or interactive states.

Capture a page or all eight pages, review, then accept its baseline. Replacing a baseline requires a second explicit click. RGB channel differences of 8 or less are ignored. A size mismatch is reported separately. Images persist in %LOCALAPPDATA%/ferrite/snap/{captures,baselines,diffs}. Gallery helper windows open without activation and close after each capture. Snap captures only its own helper windows, with no desktop input.

## Install

Download the Windows x64 zip from Releases, or install Snap through Lodestone. Each release has SHA256SUMS.txt. Main pushes automatically publish versioned releases. Native capture currently requires Windows.

## Develop

Rust 1.92 or newer: cargo run. cargo test and cargo clippy --all-targets validate comparison and release rules. `--version` prints the version; `--smoke` checks the pixel comparison. `--capture PAGE_INDEX SCHEME OUTPUT.png [--light]` captures a real native gallery page (0 through 7).

The gallery source is vendored from rwetz/ferrite-design at commit 4ddbb40bf34e219278784fdcb4e5d0e4e76f4cdc, with capture-specific entrypoint, date, and reduced-motion changes. Update that source and the dependency together when updating the design system. Apache-2.0.
