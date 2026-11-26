# Design audit: desktop app and browser extension

Every screen of Layó's Download Manager reviewed against Apple's design principles (five lenses: accessibility, platform conventions, craft, interaction, copy), Impeccable's critique, Emil Kowalski's motion rules, and the anti-slop rules in Layó's design skill. Screenshots of each screen before and after the fixes are in `screenshots/before` and `screenshots/after`.

Severity: **High** = hurts use or accessibility; **Medium** = clearly visible craft problem; **Low** = polish.

## Desktop app

### Accessibility
| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| A1 | Status is shown by colour and a boxed tag only; the boxes also add a lot of visual noise to every row. | High | Status as text with a small shape that differs per state (filled, outlined, paused bars, warning triangle), coloured by meaning. Readable without colour. |
| A2 | Muted text (`--ink-muted` on `--surface`) and disabled buttons at 45% opacity fall near or below 4.5:1. | High | Muted text darkened in light mode, lightened in dark mode; disabled controls keep 4.5:1 text and lose only their border. |
| A3 | The focus ring is 2px of the accent, which on the accent-filled primary button disappears. | High | 2px ring in ink with a 2px paper gap, visible on every surface, shown instantly. |
| A4 | Rows can't be reached or acted on from the keyboard (Tab skips the table; no arrow-key movement). | High | Table is focusable; ↑/↓ move the selection, Shift extends it, Enter opens or continues, Space pauses or continues, the Menu key opens the context menu. |
| A5 | Icon-only buttons (settings, new queue) are 22–32px; the queue "+" is 22px. | Medium | Minimum 32px hit area for every control. |
| A6 | The section map has no text alternative beyond a tooltip. | Low | It keeps its aria-label and gets a caption ("16 sections · 8 downloading") under it. |

### Platform conventions (Windows)
| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| P1 | No right-click menu on downloads: on Windows that's where people look first. | High | Context menu with Open, Show in folder, Continue/Pause, Sections, Queue, Copy address, Remove. |
| P2 | "Continue all" and "Pause all" sit in the toolbar as borderless text, reading as labels rather than buttons, and compete with per-selection actions. | Medium | Moved into a single "All downloads" menu button (Continue all, Pause all, Remove finished), so the toolbar acts only on the selection. |
| P3 | Dialogs open instantly with no backdrop animation and can't be dismissed by clicking outside. | Medium | 160ms fade and slight scale in (transform and opacity only), Esc and outside click close, with reduced-motion fallback. |
| P4 | Settings uses tabs with a heavy underline, and a long unordered list of checkboxes. | Medium | Grouped settings with section headings and one-line explanations under each option. |

### Craft (Impeccable: hierarchy, density, rhythm, colour)
| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| C1 | Colours are written as hex in `tokens.css`, although the brand defines them in OKLCH. | Medium | Tokens rewritten in OKLCH from the portfolio site's source values. |
| C2 | Three competing "boxes" on the main screen (the warning banner, the boxed status tags and the selection band) make it feel busy. | Medium | Status tags become text; the banner is a slim tinted strip; the selection is a solid tint with no border. |
| C3 | The details title repeats the status tag that's already in the row, and "Can continue after pausing" wraps awkwardly. | Low | Title stands alone with a quiet status line under it; the fact is labelled "Resume". |
| C4 | Progress has no number in the list; the percentage only appears in a tooltip. | Medium | Percentage next to the bar in tabular figures. |
| C5 | The empty state has no action. | Medium | It offers the next step: Add a download, and a hint that links pasted anywhere are picked up. |
| C6 | Queue names in the sidebar carry "(running)" as text, which truncates. | Low | A small running indicator shape; the name stays whole. |
| C7 | Spacing mixes 6, 10, 12, 14 and 22px. | Low | A 4px scale (4, 8, 12, 16, 24, 32) used throughout. |
| C8 | No "loading" state on Check in the add dialog beyond changing its label. | Low | Busy state with a spinner, the button keeps its width. |

### Interaction and motion (Emil Kowalski)
| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| I1 | The progress bar animates `width`, which re-lays out every row each frame. | Medium | Bars use `transform: scaleX()`, composited. |
| I2 | Transitions use the browser's default `ease`. | Low | Named curves only: `--ease-out` for entering, `--ease-in-out` for moving. |
| I3 | Toasts appear and vanish abruptly and can stack on top of each other. | Low | Slide-and-fade in from the bottom, replaced rather than stacked, paused on hover. |
| I4 | No feedback when a download finishes while you're looking. | Low | The row's bar settles to the success colour and the status changes; no confetti or bounce. |

### Copy
| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| W1 | "Download later" next to a Queue picker is ambiguous. | Medium | "Add to list" when no queue is chosen, "Add to queue" when one is. |
| W2 | Buttons say "Remove" while the dialog talks about deleting files. | Low | "Remove from list" consistently; deleting files is the explicit extra choice. |
| W3 | Privacy banner sentence is long and repeats the settings path. | Low | "Torrents are waiting for a VPN or proxy." with a "Set up" button. |

## Browser extension

| # | Finding | Sev. | Fix |
| --- | --- | --- | --- |
| E1 | The popup stacks three bordered cards inside a page; nested containers add noise. | Medium | One grouped list: status row, the switch row, then downloads, separated by hairlines. |
| E2 | Same status-tag noise as the app; "Not installed" is a red tag above a paragraph. | Medium | Status as a line with a shape; red only where something is actually wrong. |
| E3 | The settings page repeats the app's long checkbox lists in tall cards with small explanatory text. | Low | Grouped sections, 4px rhythm, explanations at a readable size and contrast. |
| E4 | The on-video menu uses hard-coded colours inside its shadow DOM and a heavy blue glow. | Medium | Tokens declared inside the shadow root (OKLCH), a neutral shadow, the same status and focus styles as the app. |
| E5 | The on-video button can sit over the player's own controls in small players. | Low | It moves inside the top-right corner with a margin and shrinks to an icon in players narrower than 360px. |
| E6 | Focus ring and reduced-motion handling differ from the app. | Low | Shared rules. |

## New screens (this release)

The queue scheduler and the site grabber were designed with the same rules from the start: grouped fields, one primary action per dialog, clear empty and loading states, and copy that says what will happen ("Starts at 22:30 on weekdays, stops at 06:00").
