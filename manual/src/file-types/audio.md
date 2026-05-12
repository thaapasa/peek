# Audio

Metadata-only view — no playback, no waveform. Container + codec params come from a
[symphonia](https://crates.io/crates/symphonia) probe; tag fields from ID3v1/v2, Vorbis
comments, MP4 atoms, and APE.

| Format         | Extensions                  | Status                                        |
|----------------|-----------------------------|-----------------------------------------------|
| MP3            | `.mp3`                      |                                               |
| FLAC           | `.flac`                     |                                               |
| Ogg Vorbis     | `.ogg`, `.oga`              |                                               |
| Opus           | `.opus`                     |                                               |
| WAV            | `.wav`, `.wave`             |                                               |
| MPEG-4 audio   | `.m4a`, `.m4b`, `.m4p`      |                                               |
| AAC (ADTS)     | `.aac`                      |                                               |
| AIFF           | `.aiff`, `.aif`, `.aifc`    |                                               |
| Apple CAF      | `.caf`                      |                                               |
| Matroska audio | `.mka`                      |                                               |
| WMA            | `.wma`                      | Container-only — symphonia doesn't decode WMA |

## Views

Tab cycles between:

- **Info** — duration, codec, sample rate, channels, channel layout, bit depth, bitrate, and
  the **Tags** section (title, artist, album, album-artist, track / disc number, date, genre,
  composer, comment).
- **Cover** — embedded album art rendered as ASCII through the image pipeline. Prefers the
  FrontCover-tagged picture, falls back to the first available. Hidden when no art is embedded.
- **Lyrics** — embedded `USLT` / `SYLT` / `LYRICS=` text. Hidden when none present.
- **Embeds** — listing of every embedded blob: `pictures/<usage>.<ext>` per visual (front /
  back / artist / leaflet / …) plus `lyrics/lyrics.txt`. `e` / Enter extracts; extracted
  picture bytes re-enter peek and render as ASCII, lyrics re-enter as plain text.
  `--extract pictures/front_cover.jpg` dumps the cover.
