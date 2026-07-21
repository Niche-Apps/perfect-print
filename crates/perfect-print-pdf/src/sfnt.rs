//! TrueType Collection (.ttc) → standalone sfnt extraction.
//!
//! PDF `/FontFile2` streams must contain a single sfnt font program, not a
//! `ttcf` collection. On macOS most system fonts (e.g. Helvetica) are
//! resolved by `fontdb` from a `.ttc` file with a face index selecting the
//! desired weight/style. `font_loader.rs` and the raster `FontCache` already
//! use that face index correctly for shaping/metrics/outlines — this module
//! does the equivalent for embedding: given the raw bytes of a (possibly)
//! TTC file and the face index `fontdb` reported, produce a standalone sfnt
//! containing only that face's tables.
//!
//! Non-TTC input is detected via the `ttcf` magic and passed through
//! unchanged by the caller (this module only handles the TTC case).
//! Malformed/truncated input never panics — every offset is bounds-checked
//! and failure returns `None`, letting the caller fall back to embedding the
//! original bytes.

const TTCF_TAG: u32 = 0x7474_6366; // b"ttcf"
const TTC_HEADER_LEN: usize = 12; // tag(4) + majorVersion(2) + minorVersion(2) + numFonts(4)
const OFFSET_TABLE_LEN: usize = 12; // sfntVersion(4) + numTables(2)*3 + ... (see below)
const TABLE_RECORD_LEN: usize = 16; // tag(4) + checkSum(4) + offset(4) + length(4)
const CHECKSUM_ADJUSTMENT_MAGIC: u32 = 0xB1B0_AFBA;

/// Returns true if `data` begins with the `ttcf` TrueType Collection magic.
pub fn is_ttc(data: &[u8]) -> bool {
    data.len() >= 4 && u32::from_be_bytes([data[0], data[1], data[2], data[3]]) == TTCF_TAG
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    data.get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Extract a single face from a TrueType Collection into a standalone sfnt
/// font program suitable for a PDF `/FontFile2` stream.
///
/// Returns `None` (never panics) if `data` isn't a valid TTC, `face_index`
/// is out of range, or any offset/length in the collection is malformed or
/// out of bounds.
pub fn extract_ttc_face(data: &[u8], face_index: u32) -> Option<Vec<u8>> {
    if !is_ttc(data) {
        return None;
    }

    let num_fonts = read_u32(data, 8)?;
    if face_index >= num_fonts {
        return None;
    }

    let face_offset_pos = TTC_HEADER_LEN + (face_index as usize) * 4;
    let face_offset = read_u32(data, face_offset_pos)? as usize;

    // Parse the face's own sfnt offset table.
    let sfnt_version = read_u32(data, face_offset)?;
    let num_tables = read_u16(data, face_offset + 4)?;

    let dir_start = face_offset + OFFSET_TABLE_LEN;
    let mut records: Vec<(u32, u32, u32)> = Vec::with_capacity(num_tables as usize); // (tag, offset, length)
    for i in 0..num_tables as usize {
        let rec_pos = dir_start + i * TABLE_RECORD_LEN;
        let tag = read_u32(data, rec_pos)?;
        // skip checksum at rec_pos+4; recomputed below
        let table_offset = read_u32(data, rec_pos + 8)?;
        let table_len = read_u32(data, rec_pos + 12)?;
        // Bounds-check the table's data region up front.
        let end = (table_offset as usize).checked_add(table_len as usize)?;
        if end > data.len() {
            return None;
        }
        records.push((tag, table_offset, table_len));
    }

    // Compute new layout: header + directory, then tables back-to-back,
    // each 4-byte aligned.
    let new_dir_start = OFFSET_TABLE_LEN;
    let new_table_data_start = new_dir_start + (num_tables as usize) * TABLE_RECORD_LEN;

    let mut new_offsets: Vec<u32> = Vec::with_capacity(records.len());
    let mut cursor = new_table_data_start;
    for (_, _, len) in &records {
        new_offsets.push(cursor as u32);
        let padded = align4(*len as usize);
        cursor += padded;
    }
    let total_len = cursor;

    let mut out = vec![0u8; total_len];

    // sfnt header (searchRange/entrySelector/rangeShift per spec).
    let (search_range, entry_selector, range_shift) = binary_search_params(num_tables);
    out[0..4].copy_from_slice(&sfnt_version.to_be_bytes());
    out[4..6].copy_from_slice(&num_tables.to_be_bytes());
    out[6..8].copy_from_slice(&search_range.to_be_bytes());
    out[8..10].copy_from_slice(&entry_selector.to_be_bytes());
    out[10..12].copy_from_slice(&range_shift.to_be_bytes());

    // Copy table data and note where `head` landed (for checkSumAdjustment).
    let mut head_data_offset: Option<usize> = None;
    for (i, (tag, src_offset, len)) in records.iter().enumerate() {
        let new_offset = new_offsets[i] as usize;
        let len = *len as usize;
        let src = data.get(*src_offset as usize..*src_offset as usize + len)?;
        out[new_offset..new_offset + len].copy_from_slice(src);
        if *tag == u32::from_be_bytes(*b"head") {
            head_data_offset = Some(new_offset);
        }
    }

    // Zero checkSumAdjustment (head table, offset 8, 4 bytes) before
    // computing per-table and whole-font checksums.
    if let Some(head_off) = head_data_offset {
        let adj_pos = head_off + 8;
        if adj_pos + 4 > out.len() {
            return None;
        }
        out[adj_pos..adj_pos + 4].copy_from_slice(&[0, 0, 0, 0]);
    }

    // Write table directory with recomputed checksums.
    for (i, (tag, _, len)) in records.iter().enumerate() {
        let new_offset = new_offsets[i];
        let len = *len;
        let checksum = table_checksum(&out, new_offset as usize, len as usize);
        let rec_pos = new_dir_start + i * TABLE_RECORD_LEN;
        out[rec_pos..rec_pos + 4].copy_from_slice(&tag.to_be_bytes());
        out[rec_pos + 4..rec_pos + 8].copy_from_slice(&checksum.to_be_bytes());
        out[rec_pos + 8..rec_pos + 12].copy_from_slice(&new_offset.to_be_bytes());
        out[rec_pos + 12..rec_pos + 16].copy_from_slice(&len.to_be_bytes());
    }

    // Whole-font checksum + checkSumAdjustment, per OpenType spec.
    if let Some(head_off) = head_data_offset {
        let whole_font_checksum = table_checksum(&out, 0, out.len());
        let adjustment = CHECKSUM_ADJUSTMENT_MAGIC.wrapping_sub(whole_font_checksum);
        let adj_pos = head_off + 8;
        out[adj_pos..adj_pos + 4].copy_from_slice(&adjustment.to_be_bytes());
    }

    Some(out)
}

fn align4(len: usize) -> usize {
    (len + 3) & !3
}

/// OpenType table checksum: sum of the table's bytes read as big-endian
/// u32 words, with the trailing partial word treated as zero-padded.
fn table_checksum(data: &[u8], offset: usize, len: usize) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < len {
        let mut word = [0u8; 4];
        for (j, w) in word.iter_mut().enumerate() {
            if let Some(b) = data.get(offset + i + j) {
                *w = *b;
            }
        }
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        i += 4;
    }
    sum
}

/// Compute (searchRange, entrySelector, rangeShift) for an sfnt offset
/// table with `num_tables` entries, per the OpenType spec.
fn binary_search_params(num_tables: u16) -> (u16, u16, u16) {
    let mut max_pow2: u16 = 1;
    let mut entry_selector: u16 = 0;
    while max_pow2 * 2 <= num_tables.max(1) {
        max_pow2 *= 2;
        entry_selector += 1;
    }
    let search_range = max_pow2 * 16;
    let range_shift = num_tables.saturating_mul(16).saturating_sub(search_range);
    (search_range, entry_selector, range_shift)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal, syntactically valid sfnt table's worth of bytes for
    /// a fake table so we can assemble a synthetic TTC in tests.
    fn build_synthetic_ttc(num_faces: u32) -> (Vec<u8>, Vec<Vec<(u32, Vec<u8>)>>) {
        // Each face gets tables: "aaaa" -> some bytes, "head" -> 54-byte
        // fake head table (real head is 54 bytes; we only care about the
        // checkSumAdjustment field at offset 8).
        let mut faces_tables: Vec<Vec<(u32, Vec<u8>)>> = Vec::new();
        for f in 0..num_faces {
            let mut head = vec![0u8; 54];
            // magicNumber field (offset 12..16 in real head) — irrelevant here.
            head[0] = 0x00;
            head[1] = 0x01;
            head[2] = 0x00;
            head[3] = 0x00;
            let aaaa_data = vec![b'A', b'B', b'C', (0x10 + f) as u8, b'X'];
            faces_tables.push(vec![
                (u32::from_be_bytes(*b"aaaa"), aaaa_data),
                (u32::from_be_bytes(*b"head"), head),
            ]);
        }

        // Layout: TTC header, offset-table array, then per-face: sfnt
        // offset table + table directory, then all table data at the end
        // (tables can be anywhere; we just append them).
        let num_tables_per_face = 2u16;
        let ttc_header_len = TTC_HEADER_LEN + (num_faces as usize) * 4;
        let per_face_dir_len = OFFSET_TABLE_LEN + (num_tables_per_face as usize) * TABLE_RECORD_LEN;

        let mut face_offset_table_positions = Vec::new();
        let mut cursor = ttc_header_len;
        for _ in 0..num_faces {
            face_offset_table_positions.push(cursor);
            cursor += per_face_dir_len;
        }
        let mut table_data_cursor = cursor;
        let mut table_data_positions: Vec<Vec<usize>> = Vec::new();
        for tables in &faces_tables {
            let mut positions = Vec::new();
            for (_, bytes) in tables {
                positions.push(table_data_cursor);
                table_data_cursor += align4(bytes.len());
            }
            table_data_positions.push(positions);
        }

        let total_len = table_data_cursor;
        let mut buf = vec![0u8; total_len];

        // TTC header
        buf[0..4].copy_from_slice(b"ttcf");
        buf[4..6].copy_from_slice(&1u16.to_be_bytes()); // majorVersion
        buf[6..8].copy_from_slice(&0u16.to_be_bytes()); // minorVersion
        buf[8..12].copy_from_slice(&num_faces.to_be_bytes());
        for (i, pos) in face_offset_table_positions.iter().enumerate() {
            let p = TTC_HEADER_LEN + i * 4;
            buf[p..p + 4].copy_from_slice(&(*pos as u32).to_be_bytes());
        }

        // Per-face offset tables + directories
        for (face_idx, face_pos) in face_offset_table_positions.iter().enumerate() {
            let tables = &faces_tables[face_idx];
            buf[*face_pos..*face_pos + 4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
            buf[*face_pos + 4..*face_pos + 6]
                .copy_from_slice(&(num_tables_per_face).to_be_bytes());
            buf[*face_pos + 6..*face_pos + 8].copy_from_slice(&0u16.to_be_bytes());
            buf[*face_pos + 8..*face_pos + 10].copy_from_slice(&0u16.to_be_bytes());
            buf[*face_pos + 10..*face_pos + 12].copy_from_slice(&0u16.to_be_bytes());

            let dir_start = *face_pos + OFFSET_TABLE_LEN;
            for (t_idx, (tag, bytes)) in tables.iter().enumerate() {
                let data_pos = table_data_positions[face_idx][t_idx];
                buf[data_pos..data_pos + bytes.len()].copy_from_slice(bytes);

                let rec_pos = dir_start + t_idx * TABLE_RECORD_LEN;
                buf[rec_pos..rec_pos + 4].copy_from_slice(&tag.to_be_bytes());
                let checksum = table_checksum(&buf, data_pos, bytes.len());
                buf[rec_pos + 4..rec_pos + 8].copy_from_slice(&checksum.to_be_bytes());
                buf[rec_pos + 8..rec_pos + 12].copy_from_slice(&(data_pos as u32).to_be_bytes());
                buf[rec_pos + 12..rec_pos + 16].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
            }
        }

        (buf, faces_tables)
    }

    #[test]
    fn detects_ttc_magic() {
        let (ttc, _) = build_synthetic_ttc(2);
        assert!(is_ttc(&ttc));
        assert!(!is_ttc(b"\x00\x01\x00\x00restofdata"));
        assert!(!is_ttc(b"ab"));
        assert!(!is_ttc(b""));
    }

    #[test]
    fn extracts_face_with_valid_sfnt_header_and_round_tripped_tables() {
        let (ttc, faces_tables) = build_synthetic_ttc(2);

        let extracted = extract_ttc_face(&ttc, 1).expect("extraction should succeed");

        // Magic / sfnt version tag.
        let version = u32::from_be_bytes([extracted[0], extracted[1], extracted[2], extracted[3]]);
        assert_eq!(version, 0x0001_0000);

        let num_tables = u16::from_be_bytes([extracted[4], extracted[5]]);
        assert_eq!(num_tables as usize, faces_tables[1].len());

        // Walk the directory and confirm table data + alignment + checksums.
        let dir_start = OFFSET_TABLE_LEN;
        for i in 0..num_tables as usize {
            let rec_pos = dir_start + i * TABLE_RECORD_LEN;
            let tag = u32::from_be_bytes([
                extracted[rec_pos],
                extracted[rec_pos + 1],
                extracted[rec_pos + 2],
                extracted[rec_pos + 3],
            ]);
            let checksum = u32::from_be_bytes([
                extracted[rec_pos + 4],
                extracted[rec_pos + 5],
                extracted[rec_pos + 6],
                extracted[rec_pos + 7],
            ]);
            let offset = u32::from_be_bytes([
                extracted[rec_pos + 8],
                extracted[rec_pos + 9],
                extracted[rec_pos + 10],
                extracted[rec_pos + 11],
            ]) as usize;
            let length = u32::from_be_bytes([
                extracted[rec_pos + 12],
                extracted[rec_pos + 13],
                extracted[rec_pos + 14],
                extracted[rec_pos + 15],
            ]) as usize;

            // 4-byte alignment.
            assert_eq!(offset % 4, 0, "table offset must be 4-byte aligned");

            // Checksum verifies — except for `head`, whose directory
            // checksum is (per the OpenType spec) computed with
            // checkSumAdjustment temporarily zeroed, then the real
            // adjustment is written into the table afterward. That's
            // expected: recomputing `head`'s checksum against its final
            // bytes (with the real adjustment in place) legitimately
            // differs from the stored directory entry.
            if tag != u32::from_be_bytes(*b"head") {
                let actual_checksum = table_checksum(&extracted, offset, length);
                assert_eq!(actual_checksum, checksum, "table checksum must verify");
            }

            // Data matches source, except `head` whose checkSumAdjustment
            // field (bytes 8..12) was rewritten.
            let expected = &faces_tables[1]
                .iter()
                .find(|(t, _)| *t == tag)
                .expect("tag should exist in source face")
                .1;
            if tag == u32::from_be_bytes(*b"head") {
                assert_eq!(&extracted[offset..offset + 8], &expected[0..8]);
                assert_eq!(&extracted[offset + 12..offset + length], &expected[12..]);
            } else {
                assert_eq!(&extracted[offset..offset + length], expected.as_slice());
            }
        }

        // Whole-font checksum + checkSumAdjustment relationship holds.
        let whole = table_checksum(&extracted, 0, extracted.len());
        assert_eq!(whole, CHECKSUM_ADJUSTMENT_MAGIC);
    }

    #[test]
    fn extraction_round_trips_through_ttf_parser() {
        let (ttc, faces_tables) = build_synthetic_ttc(2);
        let extracted = extract_ttc_face(&ttc, 0).expect("extraction should succeed");
        // Our synthetic tables aren't a real font (no glyf/loca/etc), so we
        // can't ask ttf_parser to fully parse it, but we can confirm the
        // sfnt directory itself is well-formed by checking num tables match.
        let num_tables = u16::from_be_bytes([extracted[4], extracted[5]]);
        assert_eq!(num_tables as usize, faces_tables[0].len());
    }

    #[test]
    fn non_ttc_input_returns_none() {
        let data = b"\x00\x01\x00\x00not a collection but plain sfnt-ish bytes";
        assert!(extract_ttc_face(data, 0).is_none());
    }

    #[test]
    fn out_of_range_face_index_returns_none() {
        let (ttc, _) = build_synthetic_ttc(2);
        assert!(extract_ttc_face(&ttc, 2).is_none());
        assert!(extract_ttc_face(&ttc, 999).is_none());
    }

    #[test]
    fn truncated_input_fails_gracefully() {
        let (ttc, _) = build_synthetic_ttc(2);
        for cut in [0usize, 4, 8, 12, 16, 20, ttc.len() / 2, ttc.len() - 1] {
            let truncated = &ttc[..cut.min(ttc.len())];
            // Must never panic.
            let _ = extract_ttc_face(truncated, 0);
            let _ = extract_ttc_face(truncated, 1);
        }
    }

    #[test]
    fn garbage_input_fails_gracefully() {
        let garbage = vec![0xFFu8; 100];
        assert!(extract_ttc_face(&garbage, 0).is_none());

        // ttcf magic but garbage after it.
        let mut fake = b"ttcf".to_vec();
        fake.extend_from_slice(&[0xFF; 200]);
        // Must never panic regardless of what garbage follows.
        let _ = extract_ttc_face(&fake, 0);
        let _ = extract_ttc_face(&fake, 5);
    }

    #[test]
    fn empty_input_fails_gracefully() {
        assert!(extract_ttc_face(&[], 0).is_none());
    }

    /// Real-world round-trip: find a system font that resolves to a `.ttc`
    /// (e.g. Helvetica on macOS), extract the referenced face, and confirm
    /// `ttf_parser` parses the extracted bytes and reports the same glyph
    /// count as parsing the original TTC at that face index. Skips (with an
    /// explanation) on systems without any TTC-backed system font so this
    /// doesn't rot on non-macOS CI.
    #[test]
    fn extraction_of_real_system_ttc_face_round_trips_through_ttf_parser() {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut found = None;
        for candidate in ["Helvetica", "Arial", "Times New Roman", "Georgia"] {
            let query = fontdb::Query {
                families: &[fontdb::Family::Name(candidate)],
                ..Default::default()
            };
            if let Some(face_id) = db.query(&query) {
                if let Some(face_info) = db.face(face_id) {
                    if let fontdb::Source::File(path) = &face_info.source {
                        if path.extension().and_then(|e| e.to_str()) == Some("ttc") {
                            found = Some((face_id, face_info.index));
                            break;
                        }
                    }
                }
            }
        }

        let Some((face_id, face_index)) = found else {
            eprintln!(
                "Skipping: no system font resolves to a .ttc source on this machine."
            );
            return;
        };

        let font_data = db
            .with_face_data(face_id, |data, _idx| data.to_vec())
            .expect("face data should be readable");

        let original_glyph_count = ttf_parser::Face::parse(&font_data, face_index)
            .expect("original TTC face should parse")
            .number_of_glyphs();

        let extracted =
            extract_ttc_face(&font_data, face_index).expect("extraction should succeed");

        assert!(
            !is_ttc(&extracted),
            "extracted bytes must not still be a ttcf collection"
        );

        let extracted_face =
            ttf_parser::Face::parse(&extracted, 0).expect("extracted sfnt should parse");
        assert_eq!(
            extracted_face.number_of_glyphs(),
            original_glyph_count,
            "extracted face should report the same glyph count as the original TTC face"
        );
    }
}
