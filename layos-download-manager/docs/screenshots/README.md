# Screenshots: before and after the design audit

Every screen of the desktop app and the browser extension, before and after the fixes in [../design-audit.md](../design-audit.md).
Taken by `tools/screenshots.mjs` (the app runs against its simulated backend; the extension without the desktop app installed).

## Before and after

### Main window (light)

![Main window (light): before and after](compare/app-main-light.png)

### Main window (dark)

![Main window (dark): before and after](compare/app-main-dark.png)

### Add a download

![Add a download: before and after](compare/app-add.png)

### Add a video (formats)

![Add a video (formats): before and after](compare/app-add-video.png)

### Downloads sent by the browser

![Downloads sent by the browser: before and after](compare/app-browser-downloads.png)

### Empty view

![Empty view: before and after](compare/app-empty.png)

### New queue

![New queue: before and after](compare/app-queue-new.png)

### A queue

![A queue: before and after](compare/app-queue.png)

### Remove from list

![Remove from list: before and after](compare/app-remove.png)

### Settings: General

![Settings: General: before and after](compare/app-settings-general.png)

### Settings: Downloads

![Settings: Downloads: before and after](compare/app-settings-connection.png)

### Settings: Torrents and privacy

![Settings: Torrents and privacy: before and after](compare/app-settings-privacy.png)

### Settings: About

![Settings: About: before and after](compare/app-settings-about.png)

### Extension pop-up (light)

![Extension pop-up (light): before and after](compare/ext-popup-light.png)

### Extension pop-up (dark)

![Extension pop-up (dark): before and after](compare/ext-popup-dark.png)

### Extension settings (light)

![Extension settings (light): before and after](compare/ext-options-light.png)

### Extension settings (dark)

![Extension settings (dark): before and after](compare/ext-options-dark.png)

### Download button on a video

![Download button on a video: before and after](compare/ext-video-button.png)

### Download button menu

![Download button menu: before and after](compare/ext-video-menu.png)

## New in this release

These screens didn't exist before.

### Right-click menu

![Right-click menu](after/app-context-menu.png)

### "All downloads" menu

!["All downloads" menu](after/app-more-menu.png)

### Queue schedule

![Queue schedule](after/app-schedule.png)

### Site grabber results

![Site grabber results](after/app-grab-results.png)

## Icons, columns and download windows

The second round: Lucide icons throughout, motion on interaction, a Type column and more columns with sorting and sideways scrolling, a side bar and details panel that fold and resize, status colours that only mean something (green done, amber waiting on you, red failed), and a window per download plus a mini window. Screens in [icons-and-windows/after](icons-and-windows/after).

### Before and after

![Main window (light): before and after](icons-and-windows/compare/app-main-light.png)

![Main window (dark): before and after](icons-and-windows/compare/app-main-dark.png)

![Right-click menu: before and after](icons-and-windows/compare/app-context-menu.png)

![All downloads menu: before and after](icons-and-windows/compare/app-more-menu.png)

![Queue: before and after](icons-and-windows/compare/app-queue.png)

![Settings, General (now with the download window options): before and after](icons-and-windows/compare/app-settings-general.png)

![Empty view: before and after](icons-and-windows/compare/app-empty.png)

### New

| | |
| --- | --- |
| 155 downloads, scrolling | ![](icons-and-windows/after/app-long-list.png) |
| Columns menu | ![](icons-and-windows/after/app-columns-menu.png) |
| More columns, sorted by size, scrolled sideways | ![](icons-and-windows/after/app-columns-sorted.png) |
| Side bar and details folded | ![](icons-and-windows/after/app-folded.png) |
| A download's window | ![](icons-and-windows/after/window-progress-light.png) ![](icons-and-windows/after/window-progress-dark.png) |
| The same, as one line | ![](icons-and-windows/after/window-progress-compact-light.png) ![](icons-and-windows/after/window-progress-compact-dark.png) |
| When the link has expired | ![](icons-and-windows/after/window-expired-light.png) ![](icons-and-windows/after/window-expired-dark.png) |
| Download complete | ![](icons-and-windows/after/window-done-light.png) ![](icons-and-windows/after/window-done-dark.png) |
| Mini window | ![](icons-and-windows/after/window-mini-light.png) ![](icons-and-windows/after/window-mini-dark.png) |
| Mini window, expanded | ![](icons-and-windows/after/window-mini-open-light.png) ![](icons-and-windows/after/window-mini-open-dark.png) |

## Folder picker, refreshing expired links, pop-up

The folder field is one control (folder, path, ×, Choose…) on its own row; expired links can be refreshed (the same link is tried, then the page opens and the next download of the file continues this one); the pop-up drops the status marker and keeps "Developed by Layó" at the bottom. Screens in [refresh-and-folders/after](refresh-and-folders/after).

![Add a download: before and after](refresh-and-folders/compare/app-add.png)

![Add a video: before and after](refresh-and-folders/compare/app-add-video.png)

![Settings, Downloads: before and after](refresh-and-folders/compare/app-settings-connection.png)

![Main window: before and after](refresh-and-folders/compare/app-main-light.png)

![Extension pop-up (light): before and after](refresh-and-folders/compare/ext-popup-light.png)

![Extension pop-up (dark): before and after](refresh-and-folders/compare/ext-popup-dark.png)

![Extension settings: before and after](refresh-and-folders/compare/ext-options-light.png)

### New

| | |
| --- | --- |
| A folder chosen (× goes back to automatic) | ![](refresh-and-folders/after/app-add-folder-chosen.png) |
| Refresh the link of an expired download | ![](refresh-and-folders/after/app-refresh-link.png) |

## Side bar, tooltips, columns and Exit

The full name in the side bar on two lines; "Hide side bar" and "Exit" as labelled buttons at its foot; tooltips naming every icon button (with its shortcut); no Open button in the details; columns that narrow to any width (Name too) and move by dragging their header. Screens in [sidebar-columns-exit/after](sidebar-columns-exit/after).

![Main window (light): before and after](sidebar-columns-exit/compare/app-main-light.png)

![Main window (dark): before and after](sidebar-columns-exit/compare/app-main-dark.png)

![Side bar folded: before and after](sidebar-columns-exit/compare/app-folded.png)

![More columns, sorted: before and after](sidebar-columns-exit/compare/app-columns-sorted.png)

![Columns menu: before and after](sidebar-columns-exit/compare/app-columns-menu.png)

![Settings, General: before and after](sidebar-columns-exit/compare/app-settings-general.png)

### New

| | |
| --- | --- |
| Tooltip on an icon button | ![](sidebar-columns-exit/after/app-tooltip.png) |
| Moving a column by dragging its header | ![](sidebar-columns-exit/after/app-column-move.png) |

## Offline, folders and duplicates

| | |
| --- | --- |
| The network went away: downloads paused, with the reason | ![](offline-folders-duplicates/app-offline.png) |
| A folder for each type of file, and what to do with a link downloaded before | ![](offline-folders-duplicates/app-settings-folders.png) |
| The Add window catches a link downloaded before | ![](offline-folders-duplicates/app-add-duplicate.png) |
| A link that comes in again from the browser: its download's window asks | ![](offline-folders-duplicates/window-duplicate.png) |

## Download windows without the app

| | |
| --- | --- |
| A new download from the browser asks in its own window (already downloading) | ![](windows-without-app/window-confirm.png) |
| Its progress | ![](windows-without-app/window-progress.png) |
| Download complete | ![](windows-without-app/window-done.png) |
| The mini window | ![](windows-without-app/window-mini.png) |
