//! Block decoders for the ADPCM codecs used by the games.
//!
//! * IMA ADPCM: Microsoft "Multimedia Standards Update" IMA/DVI layout with the IMA reference
//!   (shift-add) nibble expansion, identical to the C ripper verified on real GTA III PC files.
//! * Microsoft ADPCM: per the Microsoft ADPCM specification; bit-identical to FFmpeg in tests.
//! * PS-ADPCM (Sony SPU/VAG): the algorithm of vgmstream's `decode_psx`.

pub const IMA_STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107,
    118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
    1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894,
    6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
];
const IMA_INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

#[inline]
fn ima_nibble(pred: &mut i32, index: &mut i32, n: u8) -> i16 {
    let step = IMA_STEP[*index as usize];
    let mut diff = step >> 3;
    if n & 4 != 0 {
        diff += step;
    }
    if n & 2 != 0 {
        diff += step >> 1;
    }
    if n & 1 != 0 {
        diff += step >> 2;
    }
    if n & 8 != 0 {
        *pred -= diff;
    } else {
        *pred += diff;
    }
    *pred = (*pred).clamp(-32768, 32767);
    *index = (*index + IMA_INDEX[n as usize]).clamp(0, 88);
    *pred as i16
}

/// Samples per block when the `fmt ` chunk does not state it.
pub fn ima_samples_per_block(block_align: u16, channels: u16) -> u32 {
    let (ba, ch) = (block_align as u32, channels as u32);
    (ba.saturating_sub(4 * ch)) * 2 / ch + 1
}

/// Decodes one (possibly short, final) IMA ADPCM block into interleaved i16; returns frames.
pub fn ima_block(blk: &[u8], channels: usize, samples_per_block: u32, out: &mut Vec<i16>) -> u32 {
    if blk.len() < 4 * channels {
        return 0;
    }
    let mut pred = [0i32; 8];
    let mut idx = [0i32; 8];
    let groups_avail = ((blk.len() - 4 * channels) / (4 * channels)) as u32;
    let mut frames = 1 + groups_avail * 8;
    let mut groups = groups_avail;
    if frames > samples_per_block {
        frames = samples_per_block;
        groups = (samples_per_block - 1) / 8;
    }
    let base = out.len();
    out.resize(base + frames as usize * channels, 0);
    for c in 0..channels {
        pred[c] = i16::from_le_bytes([blk[c * 4], blk[c * 4 + 1]]) as i32;
        idx[c] = (blk[c * 4 + 2] as i32).min(88);
        out[base + c] = pred[c] as i16;
    }
    // each 4-byte group per channel holds 8 samples, low nibble first
    for g in 0..groups as usize {
        for c in 0..channels {
            let d = &blk[4 * channels + (g * channels + c) * 4..][..4];
            for (f, &byte) in d.iter().enumerate() {
                let fr = 1 + g * 8 + f * 2;
                out[base + fr * channels + c] = ima_nibble(&mut pred[c], &mut idx[c], byte & 15);
                out[base + (fr + 1) * channels + c] = ima_nibble(&mut pred[c], &mut idx[c], byte >> 4);
            }
        }
    }
    frames
}

const MS_ADAPT: [i32; 16] = [230, 230, 230, 230, 307, 409, 512, 614, 768, 614, 512, 409, 307, 230, 230, 230];
pub const MS_DEFAULT_COEFS: [(i16, i16); 7] =
    [(256, 0), (512, -256), (0, 0), (192, 64), (240, 0), (460, -208), (392, -232)];

pub fn ms_samples_per_block(block_align: u16, channels: u16) -> u32 {
    let (ba, ch) = (block_align as u32, channels as u32);
    (ba.saturating_sub(7 * ch)) * 2 / ch + 2
}

/// Decodes one Microsoft ADPCM block (mono or stereo) into interleaved i16; returns frames.
pub fn ms_block(blk: &[u8], channels: usize, samples_per_block: u32, coefs: &[(i16, i16)], out: &mut Vec<i16>) -> u32 {
    if blk.len() < 7 * channels || channels > 2 || coefs.is_empty() {
        return 0;
    }
    let rd = |o: usize| i16::from_le_bytes([blk[o], blk[o + 1]]) as i32;
    let (mut c1, mut c2, mut delta, mut s1, mut s2) = ([0i32; 2], [0i32; 2], [0i32; 2], [0i32; 2], [0i32; 2]);
    let base = out.len();
    let total_nibbles = (blk.len() - 7 * channels) * 2;
    let frames = ((2 + total_nibbles / channels) as u32).min(samples_per_block);
    out.resize(base + frames as usize * channels, 0);
    for c in 0..channels {
        let p = (blk[c] as usize).min(coefs.len() - 1);
        c1[c] = coefs[p].0 as i32;
        c2[c] = coefs[p].1 as i32;
        delta[c] = rd(channels + c * 2);
        s1[c] = rd(3 * channels + c * 2);
        s2[c] = rd(5 * channels + c * 2);
        out[base + c] = s2[c] as i16;
        out[base + channels + c] = s1[c] as i16;
    }
    let mut nib = 0usize;
    for i in 2..frames as usize {
        for c in 0..channels {
            let byte = blk[7 * channels + nib / 2];
            let n = if nib & 1 == 1 { byte & 15 } else { byte >> 4 } as i32; // high nibble first
            nib += 1;
            let sn = if n >= 8 { n - 16 } else { n };
            let pred = ((s1[c] * c1[c] + s2[c] * c2[c]) / 256 + sn * delta[c]).clamp(-32768, 32767);
            out[base + i * channels + c] = pred as i16;
            s2[c] = s1[c];
            s1[c] = pred;
            delta[c] = (MS_ADAPT[n as usize] * delta[c] / 256).max(16);
        }
    }
    frames
}

/// PS-ADPCM coefficients as rational values (vgmstream `ps_adpcm_coefs_f`, first 5 rows).
const PSX_COEFS: [(f32, f32); 5] =
    [(0.0, 0.0), (0.9375, 0.0), (1.796875, -0.8125), (1.53125, -0.859375), (1.90625, -0.9375)];

/// Decoder state for one PS-ADPCM channel.
#[derive(Debug, Default, Clone, Copy)]
pub struct PsxState {
    hist1: i32,
    hist2: i32,
}

impl PsxState {
    /// Decodes one 16-byte frame (28 samples), writing every `stride`-th element of `out`.
    pub fn frame(&mut self, f: &[u8; 16], out: &mut [i16], stride: usize) {
        let mut coef = (f[0] >> 4) as usize;
        let mut shift = (f[0] & 0xF) as i32;
        let flag = f[1];
        if coef > 4 {
            coef = 0;
        }
        if shift > 12 {
            shift = 9; // per vgmstream, citing the Nocash PSX documentation
        }
        let shift = 20 - shift;
        let (c0, c1) = PSX_COEFS[coef];
        for i in 0..28 {
            let mut sample = 0i32;
            if flag < 7 {
                let b = f[2 + i / 2];
                let nib = if i & 1 == 1 { b >> 4 } else { b & 0xF }; // low nibble first
                let signed = ((nib as i32) << 28) >> 28;
                sample = signed << shift;
                sample += ((c0 * self.hist1 as f32 + c1 * self.hist2 as f32) * 256.0) as i32;
                sample >>= 8;
            }
            out[i * stride] = sample.clamp(-32768, 32767) as i16;
            self.hist2 = self.hist1;
            self.hist1 = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ima_reference_first_nibble() {
        // header predictor -4482, index 0; first byte 0x7f -> low nibble 0xF = -(0+7+3+1)
        let mut blk = vec![0x7e, 0xee, 0, 0, 0x7f, 0x7f, 0xf7, 0x97];
        blk.resize(4 + 4, 0);
        let mut out = Vec::new();
        let n = ima_block(&blk, 1, 1017, &mut out);
        assert_eq!(n, 9);
        assert_eq!(out[0], -4482);
        assert_eq!(out[1], -4493);
    }

    #[test]
    fn psx_silent_on_flag7_and_shift_clamp() {
        let mut s = PsxState::default();
        let mut out = [1i16; 28];
        let mut f = [0u8; 16];
        f[1] = 7;
        f[2] = 0x77;
        s.frame(&f, &mut out, 1);
        assert!(out.iter().all(|&v| v == 0));
        // shift 12, filter 0, nibble 1 -> 1 << 8 >> 8 = 1
        let mut f = [0u8; 16];
        f[0] = 0x0C;
        f[2] = 0x01;
        s.frame(&f, &mut out, 1);
        assert_eq!(out[0], 1);
        // nibble 7 at shift 0 -> 7<<20>>8 = 28672
        let mut s = PsxState::default();
        f[0] = 0x00;
        f[2] = 0x07;
        s.frame(&f, &mut out, 1);
        assert_eq!(out[0], 28672);
    }
}
