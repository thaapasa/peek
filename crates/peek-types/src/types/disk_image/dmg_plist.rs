//! DMG embedded-plist blkx extractor.
//!
//! A flat DMG carries an XML property list (pointed at by the koly
//! trailer's `plist_offset` / `plist_length`) whose `resource-fork`
//! dict holds a `blkx` array — one entry per partition. Each entry is a
//! dict with a human name (`CFName`, e.g. `"disk image (Apple_HFS : 4)"`,
//! preferred over the legacy `Name` which builders may double-encode) and
//! a base64 `Data` blob that decodes to a "mish" block table (see
//! [`super::mish`]).
//!
//! This is *not* a general plist parser. It hand-walks the XML with
//! `quick-xml` (already a crate dep, used the same way for DOCX / ODT)
//! and pulls only the blkx array's name + `Data` pairs — the minimum
//! the partition view needs. Anything else in the plist (`plst`, `nsiz`,
//! size resources) is ignored.

use quick_xml::events::Event;
use quick_xml::name::QName;
use quick_xml::reader::Reader;

/// One blkx array entry: its descriptive name and the decoded mish bytes.
pub struct BlkxEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Scalar element currently being read; its text accumulates until the
/// matching end tag.
enum Scalar {
    Key,
    Str,
    Data,
}

#[derive(Default)]
struct EntryAcc {
    name: Option<String>,
    data: Option<Vec<u8>>,
}

/// Walk the plist XML and return every blkx entry whose `Data` decoded.
/// Malformed entries are skipped, not fatal — one bad block table
/// shouldn't hide the rest of the partition map.
pub fn extract_blkx(xml: &str) -> Vec<BlkxEntry> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut out: Vec<BlkxEntry> = Vec::new();

    // Container nesting depth (dict / array only). `blkx_depth` is the
    // depth of the blkx <array>; its direct-child dicts (depth+1) are the
    // partition entries.
    let mut depth: i32 = 0;
    let mut in_blkx = false;
    let mut blkx_depth: i32 = -1;
    let mut expect_blkx_array = false;

    let mut cur_key = String::new();
    let mut scalar: Option<Scalar> = None;
    let mut text = String::new();
    let mut entry: Option<EntryAcc> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local(e.name()).as_slice() {
                b"key" => {
                    scalar = Some(Scalar::Key);
                    text.clear();
                }
                b"string" => {
                    scalar = Some(Scalar::Str);
                    text.clear();
                }
                b"data" => {
                    scalar = Some(Scalar::Data);
                    text.clear();
                }
                b"array" => {
                    if expect_blkx_array {
                        in_blkx = true;
                        blkx_depth = depth;
                        expect_blkx_array = false;
                    }
                    depth += 1;
                }
                b"dict" => {
                    if in_blkx && depth == blkx_depth + 1 {
                        entry = Some(EntryAcc::default());
                    }
                    depth += 1;
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if scalar.is_some()
                    && let Ok(s) = t.xml10_content()
                {
                    text.push_str(&s);
                }
            }
            Ok(Event::End(e)) => match local(e.name()).as_slice() {
                b"key" => {
                    cur_key = text.trim().to_string();
                    if cur_key == "blkx" {
                        expect_blkx_array = true;
                    }
                    scalar = None;
                }
                b"string" => {
                    if let Some(acc) = entry.as_mut() {
                        match cur_key.as_str() {
                            // `CFName` wins over `Name`. CFName is the
                            // CoreFoundation canonical string — always proper
                            // UTF-8; some builders write `Name` double-encoded
                            // (UTF-8→Mac Roman→UTF-8), mojibaking non-ASCII
                            // names. CFName is emitted first, so assign it
                            // unconditionally and only fall back to Name when
                            // no CFName is present.
                            "CFName" => acc.name = Some(text.trim().to_string()),
                            "Name" if acc.name.is_none() => {
                                acc.name = Some(text.trim().to_string());
                            }
                            _ => {}
                        }
                    }
                    scalar = None;
                }
                b"data" => {
                    if let Some(acc) = entry.as_mut()
                        && cur_key == "Data"
                    {
                        acc.data = crate::base64::decode(&text);
                    }
                    scalar = None;
                }
                b"array" => {
                    depth -= 1;
                    if in_blkx && depth == blkx_depth {
                        in_blkx = false;
                        blkx_depth = -1;
                    }
                }
                b"dict" => {
                    depth -= 1;
                    if in_blkx
                        && depth == blkx_depth + 1
                        && let Some(acc) = entry.take()
                        && let (Some(name), Some(data)) = (acc.name, acc.data)
                    {
                        out.push(BlkxEntry { name, data });
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    out
}

fn local(name: QName<'_>) -> Vec<u8> {
    name.local_name().as_ref().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal plist mirroring `hdiutil`'s output shape: a resource-fork
    /// dict, a blkx array, two entries. `Data` values are base64 of a
    /// short marker string here — real ones are mish blocks, but the
    /// extractor only decodes, it doesn't validate.
    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>resource-fork</key>
  <dict>
    <key>blkx</key>
    <array>
      <dict>
        <key>Attributes</key><string>0x0050</string>
        <key>CFName</key><string>Protective Master Boot Record (MBR : 0)</string>
        <key>Data</key><data>bWlzaA==</data>
        <key>ID</key><string>-1</string>
        <key>Name</key><string>Protective Master Boot Record (MBR : 0)</string>
      </dict>
      <dict>
        <key>Attributes</key><string>0x0050</string>
        <key>CFName</key><string>disk image（Apple_HFS：4）</string>
        <key>Data</key><data>aGVsbG8=</data>
        <key>ID</key><string>4</string>
        <key>Name</key><string>disk imageÔºàApple_HFSÔºö4Ôºâ</string>
      </dict>
    </array>
    <key>plst</key>
    <array>
      <dict><key>Name</key><string>not a blkx entry</string></dict>
    </array>
  </dict>
</dict>
</plist>"#;

    #[test]
    fn extracts_named_blkx_entries() {
        let entries = extract_blkx(SAMPLE);
        assert_eq!(entries.len(), 2, "only blkx array entries, not plst");
        assert_eq!(entries[0].name, "Protective Master Boot Record (MBR : 0)");
        assert_eq!(entries[0].data, b"mish");
        // CFName wins over the double-encoded Name (mojibake guard).
        assert_eq!(entries[1].name, "disk image（Apple_HFS：4）");
        assert_eq!(entries[1].data, b"hello");
    }

    #[test]
    fn ignores_unrelated_arrays() {
        // The plst array dict has a Name but no Data and lives outside
        // blkx — it must not surface.
        let entries = extract_blkx(SAMPLE);
        assert!(entries.iter().all(|e| e.name != "not a blkx entry"));
    }

    #[test]
    fn empty_on_garbage() {
        assert!(extract_blkx("not xml at all").is_empty());
        assert!(extract_blkx("<plist><dict></dict></plist>").is_empty());
    }
}
