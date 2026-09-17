#![forbid(unsafe_code)]

use std::io::{Cursor, Write};

use sha2::{Digest, Sha256};

const IDS: &[&str] = &[
    "json_depth_33",
    "pdf_text",
    "pdf_truncated",
    "pdf_encrypted",
    "ooxml_docx",
    "ooxml_macro",
    "zip_basic",
    "zip_traversal",
    "zip_bomb",
    "tar_basic",
    "tar_symlink",
    "png_pixel",
    "png_huge_header",
    "jpeg_pixel",
    "jpeg_truncated",
    "wav_pcm",
    "wav_length_mismatch",
    "y4m_frame",
    "y4m_truncated",
];

fn minimal_pdf(encrypted: bool) -> Result<Vec<u8>, String> {
    use lopdf::{Document, Object, Stream, StringFormat, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let content_id = doc.new_object_id();
    let font_id = doc.new_object_id();
    let resources_id = doc.new_object_id();
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    doc.objects.insert(
        resources_id,
        Object::Dictionary(dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
        }),
    );
    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => Object::Reference(resources_id),
            "Contents" => Object::Reference(content_id),
        }),
    );
    doc.objects.insert(
        font_id,
        Object::Dictionary(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        }),
    );
    doc.objects.insert(
        content_id,
        Object::Stream(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 100 700 Td (USTE synthetic page) Tj ET".to_vec(),
        )),
    );
    if encrypted {
        doc.trailer.set(
            "ID",
            Object::Array(vec![
                Object::String((1_u8..=16).collect(), StringFormat::Literal),
                Object::String((1_u8..=16).rev().collect(), StringFormat::Literal),
            ]),
        );
        // V1 is intentionally used only for a byte-stable password-required parser fixture.
        // It is not the USTE storage cryptographic profile.
        let profile = lopdf::EncryptionVersion::V1 {
            document: &doc,
            owner_password: "owner-secret",
            user_password: "user-secret",
            permissions: lopdf::Permissions::all(),
        };
        let state = lopdf::EncryptionState::try_from(profile).map_err(|error| error.to_string())?;
        doc.encrypt(&state).map_err(|error| error.to_string())?;
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn zip_with(entries: &[(&str, &[u8])], deflated: bool) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let method = if deflated {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    };
    let options = zip::write::SimpleFileOptions::default().compression_method(method);
    for (name, bytes) in entries {
        writer
            .start_file(*name, options)
            .map_err(|error| error.to_string())?;
        writer.write_all(bytes).map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| error.to_string())
}

fn tar_fixture(symlink: bool) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut builder = tar::Builder::new(&mut bytes);
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o644);
    header.set_mtime(0);
    if symlink {
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header
            .set_link_name("../outside")
            .map_err(|error| error.to_string())?;
        header.set_cksum();
        builder
            .append_data(&mut header, "link", std::io::empty())
            .map_err(|error| error.to_string())?;
    } else {
        let payload = b"USTE tar member\n";
        header.set_size(payload.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "member.txt", payload.as_slice())
            .map_err(|error| error.to_string())?;
    }
    builder.finish().map_err(|error| error.to_string())?;
    drop(builder);
    Ok(bytes)
}

fn png_fixture(width: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, 1);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(&vec![0x7f; width as usize * 3])
        .map_err(|error| error.to_string())?;
    drop(writer);
    Ok(bytes)
}

fn jpeg_fixture() -> Result<Vec<u8>, String> {
    // Synthetic 1x1 red baseline JPEG, generated once by jpeg-encoder 0.7.1 and pinned as
    // fixture data so its compound IJG-licensed encoder is not a build/runtime dependency.
    decode_hex(concat!(
        "ffd8ffe000104a46494600010200000100010000ffc00011080001000103001100011101021101",
        "ffdb0043000302020302020303030304030304050805050404050a070706080c0a0c0c0b0a0b0b",
        "0d0e12100d0e110e0b0b1016101113141515150c0f171816141812141514ffdb0043010304040504",
        "0509050509140d0b0d14141414141414141414141414141414141414141414141414141414141414",
        "14141414141414141414141414141414ffc4001f0000010501010101010100000000000000000102",
        "030405060708090a0bffc400b5100002010303020403050504040000017d0102030004110512213141",
        "0613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435",
        "363738393a434445464748494a535455565758595a636465666768696a737475767778797a83848586",
        "8788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9",
        "cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffc4001f01000301",
        "01010101010101010000000000000102030405060708090a0bffc400b5110002010204040304070504",
        "0400010277000102031104052131061241510761711322328108144291a1b1c109233352f0156272d1",
        "0a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a636465",
        "666768696a737475767778797a82838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aa",
        "b2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3",
        "f4f5f6f7f8f9faffda000c03000001110211003f00f9d2bf0c3fd533ffd9"
    ))
}

fn wav_fixture() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let cursor = Cursor::new(&mut bytes);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(cursor, spec).map_err(|error| error.to_string())?;
    for _ in 0..8_000 {
        writer
            .write_sample(0_i16)
            .map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn y4m_fixture() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = y4m::encode(2, 2, y4m::Ratio::new(1, 1))
        .write_header(&mut bytes)
        .map_err(|error| error.to_string())?;
    let y = [16_u8; 4];
    let u = [128_u8; 1];
    let v = [128_u8; 1];
    encoder
        .write_frame(&y4m::Frame::new([&y, &u, &v], None))
        .map_err(|error| error.to_string())?;
    drop(encoder);
    Ok(bytes)
}

fn materialize(id: &str) -> Result<Vec<u8>, String> {
    match id {
        "json_depth_33" => Ok(format!("{}0{}", "[".repeat(33), "]".repeat(33)).into_bytes()),
        "pdf_text" => minimal_pdf(false),
        "pdf_truncated" => minimal_pdf(false).map(|mut bytes| {
            bytes.truncate(bytes.len() - 8);
            bytes
        }),
        "pdf_encrypted" => minimal_pdf(true),
        "ooxml_docx" => zip_with(
            &[
                ("[Content_Types].xml", b"<Types/>"),
                ("word/document.xml", b"<document><p>USTE</p></document>"),
            ],
            true,
        ),
        "ooxml_macro" => zip_with(
            &[
                ("[Content_Types].xml", b"<Types/>"),
                ("word/document.xml", b"<document><p>USTE</p></document>"),
                ("word/vbaProject.bin", b"INERT-SYNTHETIC-MACRO"),
            ],
            true,
        ),
        "zip_basic" => zip_with(&[("member.txt", b"USTE zip member\n")], true),
        "zip_traversal" => zip_with(&[("../escape.txt", b"must not escape")], false),
        "zip_bomb" => zip_with(&[("zeros.bin", &vec![0_u8; 1024 * 1024])], true),
        "tar_basic" => tar_fixture(false),
        "tar_symlink" => tar_fixture(true),
        "png_pixel" => png_fixture(1),
        "png_huge_header" => png_fixture(32_769),
        "jpeg_pixel" => jpeg_fixture(),
        "jpeg_truncated" => jpeg_fixture().map(|mut bytes| {
            bytes.truncate(bytes.len() / 2);
            bytes
        }),
        "wav_pcm" => wav_fixture(),
        "wav_length_mismatch" => wav_fixture().map(|mut bytes| {
            bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes
        }),
        "y4m_frame" => y4m_fixture(),
        "y4m_truncated" => y4m_fixture().map(|mut bytes| {
            bytes.pop();
            bytes
        }),
        _ => Err(format!("unknown fixture ID: {id}")),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("odd hexadecimal fixture length".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(text, 16).map_err(|error| error.to_string())
        })
        .collect()
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("list") if args.next().is_none() => {
            println!("id\tbytes\tsha256");
            for id in IDS {
                let bytes = materialize(id)?;
                println!("{id}\t{}\t{}", bytes.len(), hex(&Sha256::digest(&bytes)));
            }
        }
        Some("emit") => {
            let id = args.next().ok_or("missing fixture ID")?;
            if args.next().is_some() {
                return Err("unexpected argument".into());
            }
            std::io::stdout()
                .lock()
                .write_all(&materialize(&id)?)
                .map_err(|error| error.to_string())?;
        }
        _ => return Err("usage: uste-content-fixtures list | emit ID".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("content-fixtures: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const PINNED: &str = include_str!("../../../acceptance/r0/content-generated.tsv");

    #[test]
    fn every_id_materializes_deterministically() {
        for id in IDS {
            let first = materialize(id).unwrap();
            assert!(!first.is_empty(), "{id}");
            assert_eq!(first, materialize(id).unwrap(), "{id}");
        }
    }

    #[test]
    fn generated_positive_formats_are_readable() {
        assert_eq!(
            lopdf::Document::load_mem(&materialize("pdf_text").unwrap())
                .unwrap()
                .get_pages()
                .len(),
            1
        );
        let encrypted = lopdf::Document::load_mem(&materialize("pdf_encrypted").unwrap()).unwrap();
        assert!(encrypted.is_encrypted());

        let zip = materialize("zip_basic").unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut text = String::new();
        archive
            .by_name("member.txt")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "USTE zip member\n");

        let png = materialize("png_pixel").unwrap();
        assert_eq!(
            png::Decoder::new(Cursor::new(png))
                .read_info()
                .unwrap()
                .info()
                .width,
            1
        );

        let jpeg = materialize("jpeg_pixel").unwrap();
        let mut decoder = zune_jpeg::JpegDecoder::new(Cursor::new(jpeg.as_slice()));
        decoder.decode_headers().unwrap();
        assert_eq!(decoder.dimensions(), Some((1, 1)));

        assert_eq!(
            hound::WavReader::new(Cursor::new(materialize("wav_pcm").unwrap()))
                .unwrap()
                .duration(),
            8_000
        );
        let mut y4m = y4m::decode(Cursor::new(materialize("y4m_frame").unwrap())).unwrap();
        assert!(y4m.read_frame().is_ok());
    }

    #[test]
    fn hostile_archive_entries_are_observable_without_extraction() {
        let zip = materialize("zip_traversal").unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        assert!(archive.by_index(0).unwrap().enclosed_name().is_none());

        let tar = materialize("tar_symlink").unwrap();
        let mut archive = tar::Archive::new(Cursor::new(tar));
        assert!(
            archive
                .entries()
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .header()
                .entry_type()
                .is_symlink()
        );
    }

    #[test]
    fn pinned_lengths_and_sha256_match_materialized_bytes() {
        let mut rows = PINNED.lines();
        assert_eq!(rows.next(), Some("id\tbytes\tsha256"));
        let mut seen = Vec::new();
        for row in rows {
            let fields: Vec<_> = row.split('\t').collect();
            assert_eq!(fields.len(), 3);
            let bytes = materialize(fields[0]).unwrap();
            assert_eq!(
                bytes.len(),
                fields[1].parse::<usize>().unwrap(),
                "{}",
                fields[0]
            );
            assert_eq!(hex(&Sha256::digest(&bytes)), fields[2], "{}", fields[0]);
            seen.push(fields[0]);
        }
        assert_eq!(seen, IDS);
    }
}
