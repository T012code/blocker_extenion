# Third-party software in Layó's Download Manager

Layó's Download Manager is free software under the GNU General Public License v3 or later.
It includes the following, each under its own licence:

| Software | What it does here | Licence | Source |
| --- | --- | --- | --- |
| yt-dlp | Lists and downloads video formats | The Unlicense (public domain) | https://github.com/yt-dlp/yt-dlp |
| FFmpeg | Joins video and audio tracks | GNU GPL v3 (this build) | https://ffmpeg.org, build from https://github.com/BtbN/FFmpeg-Builds |
| Deno | Runs the small scripts yt-dlp needs to read YouTube's formats | MIT License | https://github.com/denoland/deno |
| librqbit | BitTorrent engine | Apache License 2.0 | https://github.com/ikatson/rqbit |
| Tauri | App framework | MIT or Apache License 2.0 | https://github.com/tauri-apps/tauri |
| Archivo | Typeface | SIL Open Font License 1.1 | https://github.com/Omnibus-Type/Archivo |
| SQLite | Download database | Public domain | https://sqlite.org |
| Lucide | Interface icons (app and extension) | ISC License; icons derived from Feather under the MIT License (texts in `icons.js`) | https://github.com/lucide-icons/lucide |

The full source of Layó's Download Manager, including the small change made to
librqbit-tracker-comms (see `vendor/librqbit-tracker-comms/NOTICE.md`), is at
https://github.com/t012code/blocker_extenion.
