# OpenKumite

A 3D fighting game engine in Rust that plays converted game data 1:1, as the original does. It runs natively
(wgpu + winit) and in the browser (WebAssembly), with local two-player and online rollback netplay.

The repository contains **no game data**. Everything game-specific is converted once, in the game, from your own copy
of the game, into standard formats: glTF 2.0, PNG, JSON, WAV/FLAC and AVI. The converters are separate crates the game
links: ok_tekken3_converter (the game, required), ok_tekken2_converter, ok_hl_cs_converter and
ok_tekken3_arcade_converter (optional packs beside it), on ok_converter_common.

## Repository layout

| Path | What it is |
|---|---|
| `app/` | `openkumite`, the game: window, 60 Hz loop, input, menus and screens, HUD, modes, saves, audio, movies, online rooms, debug tools |
| `crates/kumite_sim/` | Deterministic 60 Hz fight simulation driven only by the converted JSON game data (no renderer) |
| `crates/kumite_game/` | Game layer tying the assets to the simulation (bouts, tag, Force mode, effects, sound); no GPU, so fights run headless |
| `crates/kumite_assets/` | Loads the converted assets: skinned glTF characters, stages, JSON game data, AVI movies |
| `crates/kumite_render/` | wgpu renderer: skinned characters, stage geometry and backdrop, debug lines |
| `crates/kumite_fs/` | File reads: the disk natively, the page's web server in a browser |
| `crates/kumite_net/` | Online wire formats: room messages, rollback packet framing, pad input, reliable UDP channel |
| `crates/serve/` | Small local web server for the browser build (Range requests, pre-compressed files) |
| `web/` | Browser build: `index.html` and the build / compress / serve / preload scripts |
| `fuzz/` | cargo-fuzz targets for everything read from files or the network |
| `vendor/matchbox_socket/` | Patched matchbox_socket (rustls instead of the default TLS backend) |
| `vendor/winit/` | Patched winit (a gamepad's stick and d-pad on Android) |
| `flake.nix` | Nix flake for Linux: the game's package and a development shell |
| `check.sh` | Formatting, clippy, tests, `cargo deny` and fuzzing, also run by the git hooks |

## Assets

The game converts your own copies itself, on its **Game data** screen: it opens when there is no game data yet, from
the Game data button on the title screen, and with `--game-data`. It is the same screen on the desktops, in a browser
and on Android (`app/src/gamedata/`), drawn by the engine in the manner of the game's menus, and it takes the keyboard
(arrows, Enter, Esc), the mouse or a touchpad, touches and gamepads; a disc image dropped on the window goes to the game
under the pointer. A conversion shows its progress and the time left, can be cancelled, and its log can be read and
copied; the space it needs is checked before it starts. Choose:

| | | Gives |
|---|---|---|
| Tekken 3 (PlayStation, USA, SLUS-00402) | its disc image | the game (required) |
| Tekken 2 (PlayStation, USA v1.1) | its disc image | its fighters in the roster (optional) |
| Counter-Strike (Half-Life) | the Half-Life folder, with `cstrike/` and `valve/` | its levels as arenas, two more Tekken Force stages (optional) |
| Tekken 3 arcade (Namco System 12) | its ROM set: MAME's `tekken3.zip`, or the folder of its files | its 13 stages, 3D models, in place of the PlayStation game's panoramas (optional) |
| Tekken Tag Tournament (PlayStation 2, USA v2.00, SLUS-20001) | its disc image | a game of its own (needed only as the main game; a first one-on-one fight, not yet in the browser) |

With both Tekken 3 and Tekken Tag Tournament converted, the screen's MAIN GAME row chooses which one the game plays
(kept as `openkumite_game` beside the save; none is Tekken 3). Each keeps its own records: Tekken Tag Tournament's in
`openkumite_save_ttt.json`. This version converts Tekken Tag Tournament but does not play it yet.

With the arcade stages converted, OPTION, GRAPHICS, STAGES chooses ARCADE or PLAYSTATION (`--stages arcade|ps`). They
only change what is drawn: the fingerprint, online play and replays are the same with them or without. Each has the
arcade game's sky behind its model, a panorama that turns with the camera (on a GPU whose textures are at most 4096
wide, the widest skies are left out). What moves on the arcade stages moves here: the torches' flames, the
amusement park's carousel, and the helicopter by the ring on steel towers (it hovers, its rotors turning: its flights
are not worked out). The ROM set also keeps an arena no stage of the arcade game uses, a circular
stone arena: with the arcade stages on, it is Mokujin's home (a one-on-one fight with him on the second side, his fight
of the arcade ladder; an arcade pack converted before 0.5.1 is converted again for it) in place of the forest he shares with Yoshimitsu.

A disc image in any of these formats (ok_converter_common's `disc` module): a `.cue` with its `.bin` files (or a
`.bin` alone: a `.cue` beside it is found, and a split image's `Track 2` and `Track 3`), `.chd`, CloneCD `.ccd` with its
`.img`, Alcohol `.mds` with its `.mdf`, `.ecm`, a popstation `EBOOT.PBP`, or an `.iso`; or any of them in an archive:
`.zip`, `.7z`, `.tar`, `.gz`, `.xz`, `.zst` (unpacked once into a temporary file beside the game data, or in the
browser's storage, and removed after the conversion). In the browser, choose all of the image's files (the `.cue` and
its `.bin` files) at once, or the one archive. An `.iso` keeps only each sector's data: the game plays
from it, without the movies and the streamed (XA) music, and the Game data screen says so; a full image converted again
adds them. The fingerprint (online play) is the same either way: it covers no music or movies.

The Tekken 3 conversion takes about a minute and 1.4 GB; Tekken 2 adds about 1.7 GB, Counter-Strike 0.1 GB. Packs can
be added, converted again or removed later. In the game data's folder:

```text
manifest.json, characters/, stages/, ...   the Tekken 3 pack (as kumite_converter wrote it)
packs/t2/, packs/cs/                       the Tekken 2 and Counter-Strike packs (each from its own copy alone)
packs/t3arcade/                            the arcade stages (read as they are: `assets/arcade/`, not installed)
packs/ttt/                                 Tekken Tag Tournament (a game of its own: its `assets/` its game data)
extras/                                    the packs installed: what they add to or change in Tekken 3's
```

The game reads `extras/` over the packs' `assets/` over the Tekken 3 pack, and `extras/manifest.json` is the whole's (its
fingerprint is what online play compares). The packs are installed again when the Tekken 3 pack changes
(`app/src/packs.rs`).

The native game looks for the game data in this order (a folder of it holds `manifest.json`):

1. `--assets DIR`
2. the `OPENKUMITE_ASSETS` environment variable
3. `assets/` next to the executable
4. per-OS places: on macOS the `.app`'s `Contents/Resources/assets` and `~/Library/Application Support/OpenKumite/assets`;
   on Windows `%APPDATA%\OpenKumite\assets`; on Linux `$XDG_DATA_HOME/openkumite/assets` (`~/.local/share`), then
   `$XDG_DATA_DIRS` (`/usr/local/share`, `/usr/share`)
5. the repository's `assets/`

With none found (and no folder given), the Game data screen converts into the per-user place of 4. A folder that
kumite_converter wrote still plays as it is.

On Android the app asks for the converted folder on its first screen and keeps a copy (see [Android build](#android-build)).

## Running natively

```sh
cargo run --release
cargo run --release -- --help     # the full command line
```

The game starts at the title screen. Modes include Arcade, VS, Team Battle, Time Attack, Survival, Force,
Practice and Watch CPU, plus OpenKumite's own Tag Brawl and Online.

### Nix (Linux)

`flake.nix` builds the game and `serve` for x86_64 and aarch64 Linux (macOS uses the `.app` from the releases).
Flakes must be enabled (`experimental-features = nix-command flakes`).

```sh
nix run github:openkumite/openkumite -- --assets DIR   # play without a checkout
nix profile add github:openkumite/openkumite           # install openkumite and serve (older Nix: install)
nix build                                              # in a checkout: result/bin/openkumite, result/bin/serve
nix develop                                            # the toolchain and libraries for check.sh and the browser build
```

On NixOS, add the flake as an input and put `inputs.openkumite.packages.${pkgs.system}.default` in
`environment.systemPackages` (or `home.packages`). The game finds the assets as above, e.g. in
`~/.local/share/openkumite/assets`. On other distributions a Nix-built program may not find the system's GPU driver;
if the game reports no GPU, start it through [nixGL](https://github.com/nix-community/nixGL).

### Controls

| | Move | Buttons 1 2 3 4 (LP RP LK RK) | Start |
|---|---|---|---|
| One player | arrows or WASD | U I J K | Enter |
| VS, player 1 | WASD | F G V B | |
| VS, player 2 | arrows | ; ' . / (or numpad 4 5 1 2) | |
| Gamepad | d-pad or stick | Square Triangle Cross Circle | Start (Select = back) |

Tap up/down to sidestep. Standing still or holding back guards highs and mids; crouching guards lows. Esc pauses.
Keys and pad buttons can be changed under OPTION (KEYBOARD, KEY CONFIGURATION).

### Debug and testing

F1 debug panel, F2 pause, F3 step a frame, F4 hit volumes, F5 reset round, F6/F7 recording and P2 behaviour,
F8 free camera, F9 infinite health, F10 video statistics, F12 screenshot, `-` / `=` speed, M sound.

Useful flags:

```sh
openkumite --fight --models A,B --stage N        # straight into a free fight
openkumite --shot out.png --frames 120           # headless screenshot
openkumite --screen select --shot select.png     # render a menu screen
openkumite --keys "30:start,60:b1,400:shot" --quit-after 600   # scripted run
openkumite --replay latest --shot out.png --frames 600 --keys "400:right"   # a replay, seeking 5 s on at step 400
openkumite --mode vs --ai 1,2 --ai-style 1:rushdown,2:random   # the AI plays seats 1..4 (--ai-difficulty, --ai-level)
```

Headless example tools live in `crates/kumite_game/examples/` (`cpu_match`, `trace`, `ko_trace`, ...).

## Android build

`sh packaging/android/build.sh` builds `target/android/openkumite.apk` for arm64 phones and tablets (needs
`ANDROID_HOME`, `ANDROID_NDK_HOME`, cargo-ndk and a JDK).

The APK holds no game data. To play:

1. Copy the folder kumite_converter wrote (or a `.zip` of it) onto the phone: over USB into Downloads, an SD card,
   a USB drive or a cloud drive.
2. Install the APK and open it. The first screen is **Game data**: tap **Choose folder** (or **Choose .zip**) and pick it
   in Android's file picker.
3. The app checks it is a conversion and copies it into its own storage (about 3 GB, with a progress bar), then starts.

The **Game data** button on the title and main menu replaces or removes it later. Uninstalling the app deletes the
copy, so a reinstall asks again. It plays with the on-screen pad (stick on the left, buttons on the right) or a gamepad.
For developers, `adb push <assets> /sdcard/Android/data/io.github.openkumite/files/` puts the data in place directly.

## Browser build

```sh
sh web/build.sh          # wasm + JS glue into web/pkg (installs wasm-bindgen-cli on first use); --fast for a quicker build
sh web/serve.sh          # http://127.0.0.1:8770/ ; `sh web/serve.sh 8080 --bind 0.0.0.0` for a LAN
```

On Windows, `web\build.bat [--fast]` does the same (the compressed copies come from `web\compress.ps1`: gzip,
and brotli too under PowerShell 7); serve with `cargo run --release -p serve -- 8770 web`.

`web/serve.sh` links `web/assets` to `OPENKUMITE_ASSETS` (or `../assets`). `sh web/make_preload.sh` regenerates
`web/preload.json`, the files fetched before the first screens show.

The page takes the game data from the first of:

1. the conversion the browser keeps: on the page's setup, choose each game's files (Tekken 3's disc image tracks,
   and optionally Tekken 2's, and the Half-Life folder) and the page converts them in a worker (`web/convert_worker.js`
   running `crates/kumite_convert_web`), into the browser's storage (OPFS): the Tekken 3 pack as `assets.pack` and
   `assets.idx`, the Tekken 2 and Counter-Strike packs as `t2.*` and `cs.*`, and the packs installed as `extras.*`,
   read over the Tekken 3 pack. A folder kumite_converter wrote can still be dropped on the page (or picked with
   **Choose folder…**) and is copied in as the Tekken 3 pack. The **Game data** button over the game shows the size and
   whether the browser protects it from clean-ups, converts games, and replaces or removes the data.
2. a folder remembered in Chrome or Edge (kumite_converter's browser page writing to a folder on the disk)
3. the server's `assets/`, unless the page was built with `--no-server-assets`

`sh web/build.sh --no-server-assets` (`web\build.bat --no-server-assets`) builds a page that never reads the
server's `assets/`: every player brings their own copy. Each release (`vX.Y.Z`, not a pre-release) deploys that build
to Cloudflare and GitHub Pages once its downloads are published (release.yml calls pages.yml), so the site is
always the latest release: https://play.openkumiteengine.dev (and https://openkumite.github.io/openkumite/). Each
site's browser keeps its own copy of the game data, so a game converted on one is converted again on the other.
kumite_converter's browser page at
https://openkumite.github.io/kumite_converter/ is on the same site, so a game it converts there is found by the
game's page on GitHub Pages directly.

Each release's downloads are also on Cloudflare's CDN (R2), at `https://download.openkumiteengine.dev/vX.Y.Z/NAME`, with
`vX.Y.Z/release.json` (the files, sizes and SHA-256) and `latest.json` (the latest release's): release.yml calls cdn.yml,
which runs `packaging/cdn/upload.sh`; the Cloudflare copy of the browser version is `packaging/cloudflare/play.jsonc`
(Workers static assets, Cloudflare's current Pages). Both deploys take `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` from the
repository's `cloudflare` environment. An earlier release goes on the CDN by hand: Actions, cdn, Run workflow, its tag.

The page is a PWA: the browser offers to install it (Chrome and Edge: the install icon in the address bar; Android:
**Add to Home screen**; iPhone and iPad: Share → **Add to Home Screen**), and the installed game opens full screen.
Its service worker (`web/sw.js`) keeps the page and the game itself, so after one visit online it also starts
offline. The game data is never in that cache: it stays the copy the browser keeps. The install icons are in
`packaging/web/`, made from the SVG by `sh packaging/icons.sh`.

## Online play

Players meet in a room on a [matchbox](https://github.com/johanhelsing/matchbox) signalling server and connect
over WebRTC; matches use rollback netcode (ggrs). The room's creator hosts it, with up to four players.

```sh
openkumite --online ROOM [--signal ws://HOST:PORT] [--name NICK] [--locals 1|2] [--host-room]
```

Without `--signal`, a local `matchbox_server` on port 3536 is used.

## Saves

Records, unlocks and settings are kept as JSON in the system's application data folder (macOS
`~/Library/Application Support/OpenKumite`, Windows `%APPDATA%\OpenKumite`, otherwise
`$XDG_DATA_HOME/openkumite`), or in local storage in a browser. `OPENKUMITE_SAVE_DIR` overrides the folder.

## Replays

Every match is recorded and saved as a replay when it ends: Arcade, VS, Team Battle, Time Attack, Survival, Ball and
its 2 ON 2, the extra levels, the tag brawl (Tag Arcade too), each Force stage (players joining on the way too), online
ones as well. Practice is not, nor a match a cheat or a debug tool changed (its inputs no longer make it). The game is deterministic, so a replay is
the match's setup and every step's inputs: a couple of KB a match. The newest 50 are kept (15 in a browser), plus any
marked kept. Natively they are files in `replays/` in the save folder; in a browser, the page's storage.

**REPLAYS** on the main menu lists them: START plays one, button 1 keeps it, button 2 (twice) deletes it, button 3
exports it, button 4 imports. While one
plays:

| Key | Pad | |
|---|---|---|
| Space / Enter | START | play / pause (at the end: from the start) |
| ← / → | left / right | 5 seconds back / on |
| Page Up / Page Down | | the previous / next round |
| Home | | the start |
| . | button 1 | one step (pauses) |
| ↑ / ↓ | up / down | faster / slower (0.1× to 4×) |
| C | button 2 | the free camera (drag to orbit, right drag to pan, wheel to zoom) |
| H | button 3 | the HUD on / off |
| I | button 4 | each player's inputs, in numpad notation |
| K | | keep |
| V | | make a video |
| Esc | BACK | back to the list |

**Sharing.** **Export…** saves a replay as an `.okreplay` file (a couple of KB) to send to anyone; **Import…** (or
dropping the file on the game's window) adds one, kept. In a browser, Export downloads the file and Import opens the
browser's file picker. The match is computed the same everywhere, so a replay plays the same on another computer: checked
from macOS to the browser build, to Linux and to Windows, and from Windows to macOS (each plays to the same end state).

**Video.** **Video…** (in the list or while one plays; native builds) makes an MP4 of a replay: 720p, 1080p, 1440p or
4K, with or without the HUD, from the game's camera or the free camera where it is. It is rendered off-screen at 60
frames a second whatever the computer, its sounds and music mixed from the game's own, and encoded inside the game
(on macOS HEVC video by the Mac's hardware encoder, elsewhere AV1 by rav1e; Opus audio): nothing to install, and the
game plays on meanwhile (the export runs in a process of its own, its progress and Cancel at the top left). On an M1
Pro the hardware encoder takes a quarter of the replay's length at 1080p, two thirds at 4K; AV1 takes its time: about
1.5x the length at 720p and 3x at 1080p, longer at 4K (`OPENKUMITE_VIDEO=software` chooses it on macOS). AV1 plays in current browsers, VLC and recent phones and TVs; some
older devices do not play it. From the command line: `openkumite --export-video ID|latest --out FILE.mp4 [--size
1920x1080] [--no-hud] [--free-cam YAW,PITCH,DIST,X,Y,Z]`.
`sh packaging/demo_videos.sh` records AI-vs-AI matches headless and makes each an MP4 (`--help` for its options).

A replay plays only on the game data it was recorded on. Before it plays it is run through once (copies kept for
seeking), and its end is checked against the state recorded: the viewer says **verified**, or that it differs; played
to its end, it is checked again. Online, each computer saves its own replay; the last frames, past the match's end,
may hold the other player's predicted inputs (the result is the same on both). `--replay ID|latest` plays one from the
command line.

## Releases

A release's tag is `vX.Y.Z`, equal to the workspace version in `Cargo.toml` (`vX.Y.Z-suffix` for a pre-release).
Either push the tag (`git tag v1.2.3 && git push origin v1.2.3`: the release is made with GitHub's list of changes),
or publish a release on GitHub's Releases page (its title and text are kept). Either way
`.github/workflows/release.yml` builds every platform with the `dist` profile and, only when all of them built,
attaches the files below and adds a Downloads section to the release's text. Run by hand
(Actions → release → Run workflow) on any branch, it builds the same files without publishing them: they are the
run's artifacts, named `vX.Y.Z-dev.<commit>`.

| Download | Built as |
|---|---|
| `linux-amd64`, `linux-arm64` | glibc 2.28 baseline (any distribution since 2018); only libc, ALSA and libudev needed, the GPU and display libraries are loaded at start. `serve` is static (musl) |
| `.deb`, `.rpm`, `.pkg.tar.zst` (Arch) | that build as a package (`packaging/linux/nfpm.yaml`, made with nfpm): `/usr/bin/openkumite`, a desktop entry and icon, the docs and licenses |
| `.apk` (Alpine) | Alpine's own build, linked dynamically to musl (`packaging/linux/alpine.sh`), packaged the same way |
| `windows-amd64`, `windows-arm64` | C runtime linked in (`.cargo/config.toml`): no VC++ redistributable; the icon is in the `.exe` |
| `macos-universal` | `OpenKumite.app` (`packaging/macos/bundle.sh`): one arm64 + x86_64 binary with its icon, macOS 11+ |
| `web` | the browser build (`web/build.sh`) |
| `android-arm64.apk` | the Android build (`packaging/android/build.sh`), Android 8+, signed with the project's release key so each release updates the last |

The game is not a static musl binary on Linux: a static program cannot load libraries, and the game needs the
system's Vulkan / OpenGL and X11 / Wayland libraries to draw.

The icon is `packaging/openkumite.svg` (Linux and the browser use it as it is); `sh packaging/icons.sh` makes the
Windows `.ico` and the macOS `.icns` from it again after a change.

## Development

```sh
sh check.sh commit   # rustfmt, clippy (native, wasm, fuzz) with -D warnings, tests
sh check.sh push     # the above and cargo deny (default)
sh check.sh all 60   # the above and every fuzz target for 60 s each
```

The git hooks (`.cargo-husky/hooks`) run `commit` and `push`. Tests read the converted assets from
`OPENKUMITE_ASSETS` (default `assets/`); the ones that need them skip when they are missing or an older conversion
(CI has none). The integration tests in `app/tests/` run the game's binary headless and need a GPU too. The wasm lint needs `rustup target add wasm32-unknown-unknown` (`nix develop` on Linux has it all);
fuzzing needs nightly and `cargo-fuzz`.

Code rules (workspace lints): no `unsafe` (except the macOS haptics FFI), and no `unwrap` / `expect` / `panic!` on
anything that comes from outside: the game must never crash on bad files or network packets.

## License

OpenKumite's code is licensed under the [MIT licence](LICENSE-MIT).
See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the vendored code.
