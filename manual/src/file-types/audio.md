# Audio

peek shows audio metadata only. It doesn't play audio or show a waveform. The container and codec
parameters come from a [symphonia](https://crates.io/crates/symphonia) probe. The tag fields come
from ID3v1 and ID3v2 tags, Vorbis comments, MP4 atoms, and APE tags.

| Format         | Extensions               | Status                                               |
| -------------- | ------------------------ | ---------------------------------------------------- |
| MP3            | `.mp3`                   |                                                      |
| FLAC           | `.flac`                  |                                                      |
| Ogg Vorbis     | `.ogg`, `.oga`           |                                                      |
| Opus           | `.opus`                  |                                                      |
| WAV            | `.wav`, `.wave`          |                                                      |
| MPEG-4 audio   | `.m4a`, `.m4b`, `.m4p`   |                                                      |
| AAC (ADTS)     | `.aac`                   |                                                      |
| AIFF           | `.aiff`, `.aif`, `.aifc` |                                                      |
| Apple CAF      | `.caf`                   |                                                      |
| Matroska audio | `.mka`                   |                                                      |
| WMA            | `.wma`                   | Container only, because symphonia doesn't decode WMA |

## Views

To cycle through the views, press Tab:

- **Info** shows the duration, codec, sample rate, channels, channel layout, bit depth, and bitrate.
  It also has a **Tags** section with the title, artist, album, album artist, track number, disc
  number, date, genre, composer, and comment.
- **Cover** renders the embedded album art as ASCII through the image pipeline. peek prefers the
  picture tagged as the front cover. If there is none, it uses the first picture. peek hides this
  view when the file has no embedded art.
- **Lyrics** shows the embedded `USLT`, `SYLT`, or `LYRICS=` text. peek hides this view when the
  file has no lyrics.
- **Embeds** lists every embedded item. Each picture appears as `pictures/<usage>.<ext>`, where the
  usage is front, back, artist, leaflet, or another picture type. Lyrics appear as
  `lyrics/lyrics.txt`.

To extract the selected item in Embeds, press `e` or `Enter`. An extracted picture opens in peek and
renders as ASCII. Extracted lyrics open as plain text. To write the cover to a file from the command
line, use `--extract pictures/front_cover.jpg`.
