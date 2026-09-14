use std::io::Write;
use std::path::Path;

use crate::error::{Result, ShotError};

pub fn write_avc_mp4(
    path: &Path,
    width: u32,
    height: u32,
    fps: u32,
    sps: &[u8],
    pps: &[u8],
    samples: &[Vec<u8>],
    keyframe: &[bool],
) -> Result<()> {
    if samples.is_empty() {
        return Err(ShotError::Other("no encoded samples".into()));
    }
    let timescale = fps.max(1);
    let mut mdat_payload = Vec::new();
    let mut sample_sizes = Vec::with_capacity(samples.len());
    for sample in samples {
        mdat_payload.extend_from_slice(sample);
        sample_sizes.push(sample.len() as u32);
    }
    let mut stss = Vec::new();
    for (i, is_key) in keyframe.iter().enumerate() {
        if *is_key {
            stss.push((i as u32) + 1);
        }
    }
    if stss.is_empty() {
        stss.push(1);
    }

    let mut moov = Vec::new();
    {
        let mut mvhd = vec![0u8; 4];
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&timescale.to_be_bytes());
        mvhd.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        mvhd.extend_from_slice(&0x00010000u32.to_be_bytes());
        mvhd.extend_from_slice(&0x0100u16.to_be_bytes());
        mvhd.extend_from_slice(&0u16.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        for v in [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            mvhd.extend_from_slice(&v.to_be_bytes());
        }
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&2u32.to_be_bytes());
        write_box(&mut moov, *b"mvhd", &mvhd);
    }

    let mut trak = Vec::new();
    {
        let mut tkhd = vec![0u8; 4];
        tkhd.extend_from_slice(&0x00000003u32.to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&1u32.to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&0u32.to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        tkhd.extend_from_slice(&0u16.to_be_bytes());
        for v in [0x00010000u32, 0, 0, 0, 0x00010000, 0, 0, 0, 0x40000000] {
            tkhd.extend_from_slice(&v.to_be_bytes());
        }
        tkhd.extend_from_slice(&(width << 16).to_be_bytes());
        tkhd.extend_from_slice(&(height << 16).to_be_bytes());
        write_box(&mut trak, *b"tkhd", &tkhd);
    }

    let mut mdia = Vec::new();
    {
        let mut mdhd = vec![0u8; 4];
        mdhd.extend_from_slice(&0u32.to_be_bytes());
        mdhd.extend_from_slice(&0u32.to_be_bytes());
        mdhd.extend_from_slice(&0u32.to_be_bytes());
        mdhd.extend_from_slice(&timescale.to_be_bytes());
        mdhd.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        mdhd.extend_from_slice(&0x55c40000u32.to_be_bytes());
        write_box(&mut mdia, *b"mdhd", &mdhd);

        let mut hdlr = vec![0u8; 4];
        hdlr.extend_from_slice(&0u32.to_be_bytes());
        hdlr.extend_from_slice(&0u32.to_be_bytes());
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&0u32.to_be_bytes());
        hdlr.extend_from_slice(&0u32.to_be_bytes());
        hdlr.extend_from_slice(&0u32.to_be_bytes());
        hdlr.extend_from_slice(b"VideoHandler\0");
        write_box(&mut mdia, *b"hdlr", &hdlr);
    }

    let mut minf = Vec::new();
    {
        let mut vmhd = vec![0u8; 4];
        vmhd.extend_from_slice(&0u32.to_be_bytes());
        vmhd.extend_from_slice(&1u32.to_be_bytes());
        vmhd.extend_from_slice(&0u32.to_be_bytes());
        vmhd.extend_from_slice(&0u32.to_be_bytes());
        write_box(&mut minf, *b"vmhd", &vmhd);

        let mut dinf = Vec::new();
        let mut dref = vec![0u8; 4];
        dref.extend_from_slice(&0u32.to_be_bytes());
        dref.extend_from_slice(&1u32.to_be_bytes());
        let mut url = vec![0u8; 4];
        url.extend_from_slice(&0x00000001u32.to_be_bytes());
        write_box(&mut dref, *b"url ", &url);
        write_box(&mut dinf, *b"dref", &dref);
        write_box(&mut minf, *b"dinf", &dinf);
    }

    let mut stbl = Vec::new();
    {
        let mut avcc = vec![1u8, sps[1], sps[2], sps[3], 0xff, 0xe1];
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(sps);
        avcc.push(1);
        avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(pps);

        let mut avc1 = vec![0u8; 6];
        avc1.extend_from_slice(&1u16.to_be_bytes());
        avc1.extend_from_slice(&0u16.to_be_bytes());
        avc1.extend_from_slice(&0u16.to_be_bytes());
        avc1.extend_from_slice(&0u32.to_be_bytes());
        avc1.extend_from_slice(&0u32.to_be_bytes());
        avc1.extend_from_slice(&0u32.to_be_bytes());
        avc1.extend_from_slice(&(width as u16).to_be_bytes());
        avc1.extend_from_slice(&(height as u16).to_be_bytes());
        avc1.extend_from_slice(&0x00480000u32.to_be_bytes());
        avc1.extend_from_slice(&0x00480000u32.to_be_bytes());
        avc1.extend_from_slice(&0u32.to_be_bytes());
        avc1.extend_from_slice(&1u16.to_be_bytes());
        avc1.push(0);
        avc1.extend_from_slice(&[0u8; 31]);
        avc1.extend_from_slice(&0x0018u16.to_be_bytes());
        avc1.extend_from_slice(&(-1i16).to_be_bytes());
        write_box(&mut avc1, *b"avcC", &avcc);

        let mut stsd = vec![0u8; 4];
        stsd.extend_from_slice(&0u32.to_be_bytes());
        stsd.extend_from_slice(&1u32.to_be_bytes());
        write_box(&mut stsd, *b"avc1", &avc1);
        write_box(&mut stbl, *b"stsd", &stsd);

        let mut stts = vec![0u8; 4];
        stts.extend_from_slice(&0u32.to_be_bytes());
        stts.extend_from_slice(&1u32.to_be_bytes());
        stts.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        stts.extend_from_slice(&1u32.to_be_bytes());
        write_box(&mut stbl, *b"stts", &stts);

        let mut stss_box = vec![0u8; 4];
        stss_box.extend_from_slice(&0u32.to_be_bytes());
        stss_box.extend_from_slice(&(stss.len() as u32).to_be_bytes());
        for idx in stss {
            stss_box.extend_from_slice(&idx.to_be_bytes());
        }
        write_box(&mut stbl, *b"stss", &stss_box);

        let mut stsc = vec![0u8; 4];
        stsc.extend_from_slice(&0u32.to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
        stsc.extend_from_slice(&(samples.len() as u32).to_be_bytes());
        stsc.extend_from_slice(&1u32.to_be_bytes());
        write_box(&mut stbl, *b"stsc", &stsc);

        let mut stsz = vec![0u8; 4];
        stsz.extend_from_slice(&0u32.to_be_bytes());
        stsz.extend_from_slice(&0u32.to_be_bytes());
        stsz.extend_from_slice(&(sample_sizes.len() as u32).to_be_bytes());
        for size in sample_sizes {
            stsz.extend_from_slice(&size.to_be_bytes());
        }
        write_box(&mut stbl, *b"stsz", &stsz);

        let ftyp_size = 24u32;
        let mut stco = vec![0u8; 4];
        stco.extend_from_slice(&0u32.to_be_bytes());
        stco.extend_from_slice(&1u32.to_be_bytes());
        stco.extend_from_slice(&(ftyp_size + 8).to_be_bytes());
        write_box(&mut stbl, *b"stco", &stco);
    }

    write_box(&mut minf, *b"stbl", &stbl);
    write_box(&mut mdia, *b"minf", &minf);
    write_box(&mut trak, *b"mdia", &mdia);
    write_box(&mut moov, *b"trak", &trak);

    let mut file = Vec::new();
    let mut ftyp = Vec::new();
    ftyp.extend_from_slice(b"isom");
    ftyp.extend_from_slice(&0x200u32.to_be_bytes());
    ftyp.extend_from_slice(b"isom");
    ftyp.extend_from_slice(b"avc1");
    write_box(&mut file, *b"ftyp", &ftyp);

    let mut mdat = Vec::new();
    mdat.extend_from_slice(&mdat_payload);
    write_box(&mut file, *b"mdat", &mdat);

    let moov_start = file.len();
    write_box(&mut file, *b"moov", &moov);

    let ftyp_size = 24u32;
    let mdat_size = 8 + mdat_payload.len() as u32;
    let chunk_offset = ftyp_size + mdat_size;
    let _ = (moov_start, chunk_offset);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(path)?;
    out.write_all(&file)?;
    Ok(())
}

fn write_box(out: &mut Vec<u8>, fourcc: [u8; 4], payload: &[u8]) {
    let size = (8 + payload.len()) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&fourcc);
    out.extend_from_slice(payload);
}

pub fn annexb_to_avcc(data: &[u8]) -> (Vec<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, bool) {
    let nals = split_annexb(data);
    let mut sample = Vec::new();
    let mut sps = None;
    let mut pps = None;
    let mut key = false;
    for nal in nals {
        if nal.is_empty() {
            continue;
        }
        let nal_type = nal[0] & 0x1f;
        match nal_type {
            7 => sps = Some(nal.clone()),
            8 => pps = Some(nal.clone()),
            5 => key = true,
            _ => {}
        }
        sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        sample.extend_from_slice(&nal);
    }
    (vec![sample], sps, pps, key)
}

fn split_annexb(data: &[u8]) -> Vec<Vec<u8>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                starts.push(i + 3);
                i += 3;
                continue;
            }
            if i + 4 <= data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                starts.push(i + 4);
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    let mut nals = Vec::new();
    for (idx, start) in starts.iter().enumerate() {
        let end = starts.get(idx + 1).copied().unwrap_or(data.len());
        let mut end = end;
        if idx + 1 < starts.len() {
            let next_start = starts[idx + 1];
            let prefix = if next_start >= 4 && data[next_start - 4..next_start] == [0, 0, 0, 1] {
                4
            } else {
                3
            };
            end = next_start - prefix;
        }
        if *start < end {
            nals.push(data[*start..end].to_vec());
        }
    }
    nals
}
