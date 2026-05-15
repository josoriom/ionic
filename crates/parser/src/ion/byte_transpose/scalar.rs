pub(super) fn shuffle(input: &[u8], output: &mut [u8], stride: usize) {
    match stride {
        2 => shuffle2(input, output),
        4 => shuffle4(input, output),
        8 => shuffle8(input, output),
        _ => shuffle_any(input, output, stride),
    }
}

pub(super) fn unshuffle(input: &[u8], output: &mut [u8], stride: usize) {
    match stride {
        2 => unshuffle2(input, output),
        4 => unshuffle4(input, output),
        8 => unshuffle8(input, output),
        _ => unshuffle_any(input, output, stride),
    }
}

pub(super) fn shuffle_any(input: &[u8], output: &mut [u8], stride: usize) {
    let count = input.len() / stride;
    for byte_pos in 0..stride {
        for i in 0..count {
            output[byte_pos * count + i] = input[i * stride + byte_pos];
        }
    }
}

pub(super) fn unshuffle_any(input: &[u8], output: &mut [u8], stride: usize) {
    let count = input.len() / stride;
    for byte_pos in 0..stride {
        for i in 0..count {
            output[i * stride + byte_pos] = input[byte_pos * count + i];
        }
    }
}

pub(super) fn shuffle2(input: &[u8], output: &mut [u8]) {
    let half = input.len() / 2;
    for i in 0..half {
        output[i] = input[i * 2];
        output[half + i] = input[i * 2 + 1];
    }
}

pub(super) fn unshuffle2(input: &[u8], output: &mut [u8]) {
    let half = input.len() / 2;
    for i in 0..half {
        output[i * 2] = input[i];
        output[i * 2 + 1] = input[half + i];
    }
}

pub(super) fn shuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    for i in 0..quarter {
        output[i] = input[i * 4];
        output[quarter + i] = input[i * 4 + 1];
        output[2 * quarter + i] = input[i * 4 + 2];
        output[3 * quarter + i] = input[i * 4 + 3];
    }
}

pub(super) fn unshuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    for i in 0..quarter {
        output[i * 4] = input[i];
        output[i * 4 + 1] = input[quarter + i];
        output[i * 4 + 2] = input[2 * quarter + i];
        output[i * 4 + 3] = input[3 * quarter + i];
    }
}

pub(super) fn shuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    for i in 0..eighth {
        for b in 0..8usize {
            output[b * eighth + i] = input[i * 8 + b];
        }
    }
}

pub(super) fn unshuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    for i in 0..eighth {
        for b in 0..8usize {
            output[i * 8 + b] = input[b * eighth + i];
        }
    }
}
