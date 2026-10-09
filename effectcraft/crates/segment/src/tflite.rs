//! Running TensorFlow Lite models without TensorFlow: a reader for the `.tflite` FlatBuffer
//! (fields as numbered in TensorFlow Lite's published `schema.fbs`, Apache-2.0) and an
//! interpreter for the float operators small vision models use: `CONV_2D`, `DEPTHWISE_CONV_2D`,
//! `MAX_POOL_2D`, `ADD`, `PAD`, `CONCATENATION`, `RESHAPE`, `RELU`, `RELU6`, `PRELU`, `LOGISTIC`
//! and `DEQUANTIZE` of float16 weights (folded at load). Weights are re-laid out once at load for
//! the [`nn`](crate::nn) kernels. Anything else (other operators, quantised tensors, dilation) is a
//! load error, never a panic: models are pinned by SHA-256, so what loads once always loads.

use crate::nn::{Conv, Depthwise, Linear};
use crate::pt::half_to_f32;

type Result<T> = std::result::Result<T, String>;

// ---------------------------------------------------------------- FlatBuffers

/// A FlatBuffer: bounds-checked little-endian reads.
#[derive(Clone, Copy)]
struct Fb<'a>(&'a [u8]);

/// A table: its position in the buffer.
#[derive(Clone, Copy)]
struct Table(usize);

impl<'a> Fb<'a> {
    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        self.0.get(at..at.checked_add(N).ok_or("bad offset")?).and_then(|s| s.try_into().ok()).ok_or_else(|| "truncated model".to_string())
    }
    fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.bytes::<1>(at)?[0])
    }
    fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(at)?))
    }
    fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(at)?))
    }
    fn i32(&self, at: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(at)?))
    }
    fn u64(&self, at: usize) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes(at)?))
    }
    /// Follow the offset stored at `at`.
    fn deref(&self, at: usize) -> Result<usize> {
        at.checked_add(self.u32(at)? as usize).ok_or_else(|| "bad offset".to_string())
    }
    fn root(&self) -> Result<Table> {
        Ok(Table(self.deref(0)?))
    }
    /// Where field `id` of `t` is stored (absent fields have their default).
    fn field(&self, t: Table, id: usize) -> Result<Option<usize>> {
        let vt = (t.0 as i64 - self.i32(t.0)? as i64).try_into().map_err(|_| "bad vtable")?;
        let vt_len = self.u16(vt)? as usize;
        let slot = 4 + 2 * id;
        if slot + 2 > vt_len {
            return Ok(None);
        }
        let off = self.u16(vt + slot)? as usize;
        Ok((off != 0).then_some(t.0 + off))
    }
    fn int(&self, t: Table, id: usize, default: i32) -> Result<i32> {
        self.field(t, id)?.map_or(Ok(default), |p| self.i32(p))
    }
    fn byte(&self, t: Table, id: usize, default: u8) -> Result<u8> {
        self.field(t, id)?.map_or(Ok(default), |p| self.u8(p))
    }
    fn table(&self, t: Table, id: usize) -> Result<Option<Table>> {
        self.field(t, id)?.map(|p| self.deref(p).map(Table)).transpose()
    }
    /// A vector field: (start of its elements, length).
    fn vector(&self, t: Table, id: usize) -> Result<(usize, usize)> {
        let Some(p) = self.field(t, id)? else { return Ok((0, 0)) };
        let v = self.deref(p)?;
        let n = self.u32(v)? as usize;
        if v.saturating_add(4).saturating_add(n) > self.0.len() {
            return Err("truncated model".into());
        }
        Ok((v + 4, n))
    }
    fn tables(&self, t: Table, id: usize) -> Result<Vec<Table>> {
        let (at, n) = self.vector(t, id)?;
        (0..n).map(|i| self.deref(at + 4 * i).map(Table)).collect()
    }
    fn ints(&self, t: Table, id: usize) -> Result<Vec<i32>> {
        let (at, n) = self.vector(t, id)?;
        (0..n).map(|i| self.i32(at + 4 * i)).collect()
    }
    fn string(&self, t: Table, id: usize) -> Result<String> {
        let (at, n) = self.vector(t, id)?;
        Ok(String::from_utf8_lossy(self.0.get(at..at + n).ok_or("truncated model")?).into_owned())
    }
}

// ---------------------------------------------------------------- the model

/// Operator codes (`BuiltinOperator`).
mod op {
    pub const ADD: i32 = 0;
    pub const CONCATENATION: i32 = 2;
    pub const CONV_2D: i32 = 3;
    pub const DEPTHWISE_CONV_2D: i32 = 4;
    pub const DEQUANTIZE: i32 = 6;
    pub const LOGISTIC: i32 = 14;
    pub const MAX_POOL_2D: i32 = 17;
    pub const RELU: i32 = 19;
    pub const RELU6: i32 = 21;
    pub const RESHAPE: i32 = 22;
    pub const PAD: i32 = 34;
    pub const PRELU: i32 = 54;
}

/// The most values a tensor may hold.
const MAX_TENSOR: usize = 1 << 28;

/// Tensor types (`TensorType`).
const FLOAT32: u8 = 0;
const FLOAT16: u8 = 1;
const INT32: u8 = 2;

/// A fused activation (`ActivationFunctionType`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    None,
    Relu,
    ReluN1To1,
    Relu6,
    Tanh,
}

impl Act {
    fn from(code: u8) -> Result<Act> {
        Ok(match code {
            0 => Act::None,
            1 => Act::Relu,
            2 => Act::ReluN1To1,
            3 => Act::Relu6,
            4 => Act::Tanh,
            c => return Err(format!("unsupported fused activation {c}")),
        })
    }
    fn apply(self, x: &mut [f32]) {
        let f: fn(f32) -> f32 = match self {
            Act::None => return,
            Act::Relu => |v| v.max(0.0),
            Act::ReluN1To1 => |v| v.clamp(-1.0, 1.0),
            Act::Relu6 => |v| v.clamp(0.0, 6.0),
            Act::Tanh => f32::tanh,
        };
        for v in x {
            *v = f(*v);
        }
    }
}

/// One operator, its weights laid out for the kernels.
#[derive(Clone, Debug)]
enum Op {
    /// A 1×1, stride-1 convolution: a dense layer over the pixels.
    Pointwise(Linear, Act),
    Conv(Conv, [usize; 2], Act),
    Depthwise(Depthwise, [usize; 2], Act),
    /// Kernel, stride, padding before (top, left).
    MaxPool {
        k: [usize; 2],
        stride: [usize; 2],
        pad: [usize; 2],
        act: Act,
    },
    Add(Act),
    /// Constant padding (zeros), per dimension (before, after).
    Pad(Vec<[usize; 2]>),
    Concat {
        axis: usize,
        act: Act,
    },
    Reshape,
    Act(Act),
    Prelu(Vec<f32>),
    Logistic,
}

#[derive(Clone, Debug)]
struct Step {
    op: Op,
    /// Runtime inputs (tensor ids).
    inputs: Vec<usize>,
    output: usize,
}

/// A loaded model, ready to run (batch of one, static shapes).
#[derive(Clone, Debug)]
pub struct Model {
    shapes: Vec<Vec<usize>>,
    names: Vec<String>,
    /// Constant float tensors (weights folded into the steps are not kept).
    consts: Vec<Option<Vec<f32>>>,
    steps: Vec<Step>,
    inputs: Vec<usize>,
    outputs: Vec<usize>,
    /// After which step each tensor is last needed (to free memory as the model runs).
    last_use: Vec<usize>,
}

fn index(i: i32, n: usize) -> Result<usize> {
    usize::try_from(i).ok().filter(|&i| i < n).ok_or_else(|| format!("bad tensor index {i}"))
}

/// "Same" padding: output size and padding before.
fn same(input: usize, k: usize, stride: usize) -> (usize, usize) {
    let out = input.div_ceil(stride);
    let total = ((out.saturating_sub(1)) * stride + k).saturating_sub(input);
    (out, total / 2)
}

/// Output size and padding before, for `padding` (0 same, 1 valid).
fn window(padding: u8, input: usize, k: usize, stride: usize) -> Result<(usize, usize)> {
    match padding {
        0 => Ok(same(input, k, stride)),
        1 if input >= k => Ok(((input - k) / stride + 1, 0)),
        _ => Err("bad window".into()),
    }
}

impl Model {
    /// Read a `.tflite` file.
    pub fn read(bytes: &[u8]) -> Result<Model> {
        let fb = Fb(bytes);
        let root = fb.root()?;
        let codes: Vec<i32> = fb.tables(root, 1)?.into_iter().map(|c| Ok(fb.int(c, 3, 0)?.max(fb.byte(c, 0, 0)? as i8 as i32))).collect::<Result<_>>()?;
        let buffers = fb.tables(root, 4)?;
        let graphs = fb.tables(root, 2)?;
        let g = *graphs.first().ok_or("the model has no graph")?;
        let tensors = fb.tables(g, 0)?;
        let n = tensors.len();
        let mut shapes = Vec::with_capacity(n);
        let mut names = Vec::with_capacity(n);
        // Constant data: float tensors as f32, int tensors (pads, shapes) as i32.
        let mut floats: Vec<Option<Vec<f32>>> = vec![None; n];
        let mut ints: Vec<Option<Vec<i32>>> = vec![None; n];
        for (i, t) in tensors.iter().enumerate() {
            let shape: Vec<usize> = fb
                .ints(*t, 0)?
                .into_iter()
                .map(|d| usize::try_from(d).map_err(|_| "dynamic shapes are not supported"))
                .collect::<std::result::Result<_, _>>()?;
            let ty = fb.byte(*t, 1, FLOAT32)?;
            let buf = fb.int(*t, 2, 0)? as u32 as usize;
            names.push(fb.string(*t, 3)?);
            // Every size computed from shapes later stays in range.
            let count = shape.iter().try_fold(1usize, |a, &d| a.checked_mul(d)).filter(|&c| c <= MAX_TENSOR).ok_or("a tensor is too large")?;
            if fb.field(*t, 4)?.is_some() && fb.table(*t, 4)?.is_some_and(|q| fb.vector(q, 2).is_ok_and(|v| v.1 > 0)) {
                return Err(format!("{}: quantised tensors are not supported", names[i]));
            }
            if buf > 0 {
                let b = *buffers.get(buf).ok_or("bad buffer index")?;
                let (mut at, mut len) = fb.vector(b, 0)?;
                if len == 0 {
                    // Large models keep data after the FlatBuffer: offset and size from its start.
                    let (o, s) = (fb.field(b, 1)?.map(|p| fb.u64(p)).transpose()?.unwrap_or(0), fb.field(b, 2)?.map(|p| fb.u64(p)).transpose()?.unwrap_or(0));
                    (at, len) = (o as usize, s as usize);
                }
                if len > 0 {
                    let data = bytes.get(at..at.checked_add(len).ok_or("bad buffer")?).ok_or("truncated model")?;
                    match ty {
                        FLOAT32 if data.len() == 4 * count => {
                            floats[i] = Some(data.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
                        }
                        FLOAT16 if data.len() == 2 * count => {
                            floats[i] = Some(data.as_chunks::<2>().0.iter().map(|c| half_to_f32(u16::from_le_bytes([c[0], c[1]]))).collect())
                        }
                        INT32 if data.len() == 4 * count => {
                            ints[i] = Some(data.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
                        }
                        _ => return Err(format!("{}: unsupported tensor data", names[i])),
                    }
                }
            }
            shapes.push(shape);
        }
        let inputs = fb.ints(g, 1)?.into_iter().map(|i| index(i, n)).collect::<Result<Vec<_>>>()?;
        let outputs = fb.ints(g, 2)?.into_iter().map(|i| index(i, n)).collect::<Result<Vec<_>>>()?;
        let mut steps = vec![];
        for o in fb.tables(g, 3)? {
            let code = *codes.get(fb.int(o, 0, 0)? as u32 as usize).ok_or("bad operator code")?;
            // Optional inputs are -1.
            let ins: Vec<Option<usize>> = fb.ints(o, 1)?.into_iter().map(|i| if i < 0 { Ok(None) } else { index(i, n).map(Some) }).collect::<Result<_>>()?;
            let outs = fb.ints(o, 2)?;
            let output = index(*outs.first().ok_or("an operator has no output")?, n)?;
            // Float16 weights: fold their DEQUANTIZE.
            if code == op::DEQUANTIZE
                && let Some(f) = ins.first().copied().flatten().and_then(|i| floats[i].clone())
            {
                floats[output] = Some(f);
                continue;
            }
            let opts = fb.table(o, 4)?;
            let opt_int = |id: usize, default: i32| opts.map_or(Ok(default), |t| fb.int(t, id, default));
            let opt_byte = |id: usize, default: u8| opts.map_or(Ok(default), |t| fb.byte(t, id, default));
            let input = |k: usize| ins.get(k).copied().flatten().ok_or_else(|| format!("{}: missing input {k}", names[output]));
            let weights = |k: usize| -> Result<Vec<f32>> {
                let id = input(k)?;
                floats[id].clone().ok_or_else(|| format!("{}: input {k} is not constant", names[output]))
            };
            let bias = |count: usize| -> Result<Vec<f32>> {
                match ins.get(2).copied().flatten() {
                    Some(id) => floats[id].clone().filter(|b| b.len() == count).ok_or_else(|| format!("{}: bad bias", names[output])),
                    None => Ok(vec![0.0; count]),
                }
            };
            let in_shape = |k: usize| -> Result<Vec<usize>> { Ok(shapes[input(k)?].clone()) };
            let out_shape = shapes[output].clone();
            let stride = |w: usize, h: usize| -> Result<[usize; 2]> {
                let s = [opt_int(h, 1)?, opt_int(w, 1)?];
                if s.iter().any(|v| *v < 1) {
                    return Err("bad stride".into());
                }
                Ok(s.map(|v| v as usize))
            };
            let mut runtime = vec![input(0)?];
            let op = match code {
                op::DEQUANTIZE => Op::Reshape,
                op::CONV_2D | op::DEPTHWISE_CONV_2D => {
                    let depthwise = code == op::DEPTHWISE_CONV_2D;
                    let (padding, s) = (opt_byte(0, 0)?, stride(1, 2)?);
                    let act_id = if depthwise { 4 } else { 3 };
                    let dil = if depthwise { [opt_int(6, 1)?, opt_int(5, 1)?] } else { [opt_int(5, 1)?, opt_int(4, 1)?] };
                    if dil != [1, 1] {
                        return Err(format!("{}: dilated convolutions are not supported", names[output]));
                    }
                    let act = Act::from(opt_byte(act_id, 0)?)?;
                    let (x, f) = (in_shape(0)?, shapes[input(1)?].clone());
                    let (&[1, h, w, cin], &[fo, kh, kw, fi], &[1, oh, ow, cout]) = (&x[..], &f[..], &out_shape[..]) else {
                        return Err(format!("{}: unexpected convolution shapes", names[output]));
                    };
                    if kh != kw || s[0] != s[1] {
                        return Err(format!("{}: only square kernels and strides are supported", names[output]));
                    }
                    let (ey, py) = window(padding, h, kh, s[0])?;
                    let (ex, px) = window(padding, w, kw, s[1])?;
                    if (ey, ex) != (oh, ow) {
                        return Err(format!("{}: output size disagrees with the padding", names[output]));
                    }
                    let wt = weights(1)?;
                    let b = bias(cout)?;
                    if depthwise {
                        if fo != 1 || fi != cout || cin != cout {
                            return Err(format!("{}: depth multipliers are not supported", names[output]));
                        }
                        Op::Depthwise(Depthwise { w: wt, b, c: cout, ks: kh, stride: s[0] }, [py, px], act)
                    } else {
                        if fo != cout || fi != cin {
                            return Err(format!("{}: unexpected filter shape", names[output]));
                        }
                        // OHWI → [(ky·ks + kx)·inp + ci] × out.
                        let kk = kh * kw * cin;
                        let mut t = vec![0.0f32; kk * cout];
                        for o in 0..cout {
                            for p in 0..kk {
                                t[p * cout + o] = *wt.get(o * kk + p).ok_or("bad filter")?;
                            }
                        }
                        if kh == 1 && s[0] == 1 {
                            Op::Pointwise(Linear { wt: t, b, inp: cin, out: cout }, act)
                        } else {
                            Op::Conv(Conv { wt: t, b, inp: cin, out: cout, ks: kh, stride: s[0], pad: 0 }, [py, px], act)
                        }
                    }
                }
                op::MAX_POOL_2D => {
                    let (padding, s) = (opt_byte(0, 0)?, stride(1, 2)?);
                    let k = [opt_int(4, 1)?, opt_int(3, 1)?];
                    if k.iter().any(|v| *v < 1) {
                        return Err("bad pool size".into());
                    }
                    let k = k.map(|v| v as usize);
                    let x = in_shape(0)?;
                    let (&[1, h, w, _], &[1, oh, ow, _]) = (&x[..], &out_shape[..]) else { return Err("unexpected pool shapes".into()) };
                    let (ey, py) = window(padding, h, k[0], s[0])?;
                    let (ex, px) = window(padding, w, k[1], s[1])?;
                    if (ey, ex) != (oh, ow) {
                        return Err(format!("{}: output size disagrees with the padding", names[output]));
                    }
                    Op::MaxPool { k, stride: s, pad: [py, px], act: Act::from(opt_byte(5, 0)?)? }
                }
                op::ADD => {
                    runtime.push(input(1)?);
                    Op::Add(Act::from(opt_byte(0, 0)?)?)
                }
                op::PAD => {
                    let p = ints[input(1)?].clone().ok_or("PAD: paddings must be constant")?;
                    let pads = p
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| Ok([usize::try_from(c[0]).map_err(|_| "bad pad")?, usize::try_from(c[1]).map_err(|_| "bad pad")?]))
                        .collect::<Result<Vec<_>>>()?;
                    Op::Pad(pads)
                }
                op::CONCATENATION => {
                    runtime = (0..ins.len()).map(input).collect::<Result<_>>()?;
                    let rank = out_shape.len() as i32;
                    let a = opt_int(0, 0)?;
                    let axis = if a < 0 { a + rank } else { a };
                    Op::Concat { axis: index(axis, out_shape.len())?, act: Act::from(opt_byte(1, 0)?)? }
                }
                op::RESHAPE => Op::Reshape,
                op::RELU => Op::Act(Act::Relu),
                op::RELU6 => Op::Act(Act::Relu6),
                op::LOGISTIC => Op::Logistic,
                op::PRELU => {
                    let alpha = weights(1)?;
                    let c = *out_shape.last().ok_or("PRELU: bad shape")?;
                    if alpha.len() != c {
                        return Err(format!("{}: only per-channel PReLU is supported", names[output]));
                    }
                    Op::Prelu(alpha)
                }
                c => return Err(format!("operator {c} is not supported")),
            };
            steps.push(Step { op, inputs: runtime, output });
        }
        let mut last_use = vec![usize::MAX; n];
        for (k, s) in steps.iter().enumerate() {
            for &i in &s.inputs {
                last_use[i] = k;
            }
        }
        for &o in &outputs {
            last_use[o] = usize::MAX;
        }
        Ok(Model { shapes, names, consts: floats, steps, inputs, outputs, last_use })
    }

    /// The shape of input `k`.
    pub fn input_shape(&self, k: usize) -> Option<&[usize]> {
        self.inputs.get(k).and_then(|&i| self.shapes.get(i)).map(Vec::as_slice)
    }

    /// The shape and name of output `k`.
    pub fn output(&self, k: usize) -> Option<(&[usize], &str)> {
        let &i = self.outputs.get(k)?;
        Some((self.shapes.get(i)?.as_slice(), self.names.get(i)?.as_str()))
    }

    /// Run the model on its inputs (row-major, NHWC); returns its outputs.
    pub fn run(&self, inputs: &[&[f32]]) -> Result<Vec<Vec<f32>>> {
        let mut vals: Vec<Option<Vec<f32>>> = vec![None; self.shapes.len()];
        for (k, &id) in self.inputs.iter().enumerate() {
            let x = inputs.get(k).ok_or("missing model input")?;
            if x.len() != self.shapes[id].iter().product::<usize>() {
                return Err(format!("input {k}: expected {:?}", self.shapes[id]));
            }
            vals[id] = Some(x.to_vec());
        }
        for (k, s) in self.steps.iter().enumerate() {
            let y = {
                let get = |i: usize| -> Result<&[f32]> {
                    vals.get(i)
                        .and_then(Option::as_deref)
                        .or_else(|| self.consts.get(i).and_then(Option::as_deref))
                        .ok_or_else(|| format!("{}: input not computed", self.names[s.output]))
                };
                let x = get(s.inputs[0])?;
                let xs = &self.shapes[s.inputs[0]];
                let ys = &self.shapes[s.output];
                let (h, w) = (xs.get(1).copied().unwrap_or(1), xs.get(2).copied().unwrap_or(1));
                let (oh, ow) = (ys.get(1).copied().unwrap_or(1), ys.get(2).copied().unwrap_or(1));
                let y = match &s.op {
                    Op::Pointwise(l, a) => {
                        let mut y = l.forward(x, h * w);
                        a.apply(&mut y);
                        y
                    }
                    Op::Conv(c, pad, a) => {
                        let mut y = c.forward_sized(x, h, w, oh, ow, *pad);
                        a.apply(&mut y);
                        y
                    }
                    Op::Depthwise(d, pad, a) => {
                        let mut y = d.forward_sized(x, h, w, oh, ow, *pad);
                        a.apply(&mut y);
                        y
                    }
                    Op::MaxPool { k, stride, pad, act } => {
                        let mut y = max_pool(x, [h, w], *xs.last().unwrap_or(&1), [oh, ow], *k, *stride, *pad);
                        act.apply(&mut y);
                        y
                    }
                    Op::Add(a) => {
                        let b = get(s.inputs[1])?;
                        if b.len() != x.len() {
                            return Err(format!("{}: broadcasting ADD is not supported", self.names[s.output]));
                        }
                        let mut y: Vec<f32> = x.iter().zip(b).map(|(p, q)| p + q).collect();
                        a.apply(&mut y);
                        y
                    }
                    Op::Pad(p) => pad(x, xs, p)?,
                    Op::Concat { axis, act } => {
                        let parts = s.inputs.iter().map(|&i| Ok((get(i)?, &self.shapes[i]))).collect::<Result<Vec<_>>>()?;
                        let mut y = concat(&parts, *axis)?;
                        act.apply(&mut y);
                        y
                    }
                    Op::Reshape => x.to_vec(),
                    Op::Act(a) => {
                        let mut y = x.to_vec();
                        a.apply(&mut y);
                        y
                    }
                    Op::Prelu(alpha) => {
                        let c = alpha.len().max(1);
                        x.iter().enumerate().map(|(i, &v)| if v >= 0.0 { v } else { v * alpha[i % c] }).collect()
                    }
                    Op::Logistic => x.iter().map(|v| 1.0 / (1.0 + (-v).exp())).collect(),
                };
                if y.len() != ys.iter().product::<usize>() {
                    return Err(format!("{}: computed {} values for shape {ys:?}", self.names[s.output], y.len()));
                }
                y
            };
            vals[s.output] = Some(y);
            for &i in &s.inputs {
                if self.last_use[i] == k {
                    vals[i] = None;
                }
            }
        }
        self.outputs.iter().map(|&o| vals[o].take().or_else(|| self.consts[o].clone()).ok_or_else(|| "an output was not computed".to_string())).collect()
    }
}

/// Max pooling over `k` windows; window cells outside the input are ignored.
fn max_pool(x: &[f32], [h, w]: [usize; 2], c: usize, [oh, ow]: [usize; 2], k: [usize; 2], s: [usize; 2], pad: [usize; 2]) -> Vec<f32> {
    let mut y = vec![f32::NEG_INFINITY; oh * ow * c];
    for oy in 0..oh {
        for ox in 0..ow {
            let out = &mut y[(oy * ow + ox) * c..][..c];
            for ky in 0..k[0] {
                let Some(iy) = (oy * s[0] + ky).checked_sub(pad[0]).filter(|&v| v < h) else { continue };
                for kx in 0..k[1] {
                    let Some(ix) = (ox * s[1] + kx).checked_sub(pad[1]).filter(|&v| v < w) else { continue };
                    let Some(src) = x.get((iy * w + ix) * c..(iy * w + ix + 1) * c) else { continue };
                    for (o, v) in out.iter_mut().zip(src) {
                        *o = o.max(*v);
                    }
                }
            }
        }
    }
    y
}

/// Zero padding of a row-major tensor of shape `shape`.
fn pad(x: &[f32], shape: &[usize], pads: &[[usize; 2]]) -> Result<Vec<f32>> {
    if pads.len() != shape.len() {
        return Err("PAD: rank mismatch".into());
    }
    let out: Vec<usize> = shape.iter().zip(pads).map(|(d, p)| d + p[0] + p[1]).collect();
    let mut y = vec![0.0f32; out.iter().product()];
    let rank = shape.len();
    let inner = *shape.last().unwrap_or(&1);
    let rows = x.len() / inner.max(1);
    // Copy each innermost row to its padded place.
    let mut idx = vec![0usize; rank.saturating_sub(1)];
    for r in 0..rows {
        let mut at = 0usize;
        for d in 0..rank {
            let i = if d + 1 == rank { pads[d][0] } else { idx[d] + pads[d][0] };
            at = at * out[d] + i;
        }
        if let (Some(dst), Some(src)) = (y.get_mut(at..at + inner), x.get(r * inner..(r + 1) * inner)) {
            dst.copy_from_slice(src);
        }
        for d in (0..idx.len()).rev() {
            idx[d] += 1;
            if idx[d] < shape[d] {
                break;
            }
            idx[d] = 0;
        }
    }
    Ok(y)
}

/// Concatenate along `axis`.
fn concat(parts: &[(&[f32], &Vec<usize>)], axis: usize) -> Result<Vec<f32>> {
    let Some((_, s0)) = parts.first() else { return Ok(vec![]) };
    let outer: usize = s0.get(..axis).ok_or("CONCATENATION: bad axis")?.iter().product();
    let mut y = Vec::with_capacity(parts.iter().map(|p| p.0.len()).sum());
    for o in 0..outer {
        for (x, s) in parts {
            let chunk: usize = s.get(axis..).ok_or("CONCATENATION: bad axis")?.iter().product();
            y.extend_from_slice(x.get(o * chunk..(o + 1) * chunk).ok_or("CONCATENATION: bad input")?);
        }
    }
    Ok(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_padding_matches_tensorflow() {
        // TensorFlow pads the extra row after: 128 → 64 with a 3×3 stride-2 window pads 0 before.
        assert_eq!(same(128, 3, 2), (64, 0));
        assert_eq!(same(128, 5, 2), (64, 1));
        assert_eq!(same(16, 3, 1), (16, 1));
        assert_eq!(same(7, 2, 2), (4, 0));
        assert_eq!(window(1, 4, 2, 2), Ok((2, 0)));
        assert!(window(1, 1, 2, 2).is_err());
    }

    #[test]
    fn asymmetric_conv_padding() {
        // A 3×3 stride-2 "same" conv on 4×4: pads nothing before, one after.
        let x: Vec<f32> = (0..16).map(|v| v as f32).collect();
        let ones = vec![1.0f32; 9];
        let c = Conv { wt: ones.clone(), b: vec![0.0], inp: 1, out: 1, ks: 3, stride: 2, pad: 0 };
        let y = c.forward_sized(&x, 4, 4, 2, 2, [0, 0]);
        // Top-left window rows 0–2, cols 0–2; the bottom-right one is cut by the edge.
        assert_eq!(y, vec![45.0, 39.0, 66.0, 50.0]);
        let d = Depthwise { w: ones, b: vec![0.0], c: 1, ks: 3, stride: 2 };
        assert_eq!(d.forward_sized(&x, 4, 4, 2, 2, [0, 0]), y);
    }

    #[test]
    fn pool_pad_and_concat() {
        let x: Vec<f32> = (0..16).map(|v| v as f32).collect();
        assert_eq!(max_pool(&x, [4, 4], 1, [2, 2], [2, 2], [2, 2], [0, 0]), vec![5.0, 7.0, 13.0, 15.0]);
        // Same padding on 3×3 → 2×2: the last window is cut by the edge.
        let x3: Vec<f32> = (0..9).map(|v| v as f32).collect();
        assert_eq!(max_pool(&x3, [3, 3], 1, [2, 2], [2, 2], [2, 2], [0, 0]), vec![4.0, 5.0, 7.0, 8.0]);
        // Channel padding (residual connections widen the channels with zeros).
        let p = pad(&[1.0, 2.0, 3.0, 4.0], &[1, 1, 2, 2], &[[0, 0], [0, 0], [0, 0], [0, 1]]).unwrap();
        assert_eq!(p, vec![1.0, 2.0, 0.0, 3.0, 4.0, 0.0]);
        let p = pad(&[1.0, 2.0], &[1, 2], &[[0, 0], [1, 0]]).unwrap();
        assert_eq!(p, vec![0.0, 1.0, 2.0]);
        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [5.0, 6.0];
        let (sa, sb) = (vec![1, 2, 2], vec![1, 1, 2]);
        assert_eq!(concat(&[(&a, &sa), (&b, &sb)], 1).unwrap(), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let (sa, sb) = (vec![2, 2], vec![2, 1]);
        assert_eq!(concat(&[(&a, &sa), (&b, &sb)], 1).unwrap(), vec![1.0, 2.0, 5.0, 3.0, 4.0, 6.0]);
    }

    #[test]
    fn damaged_files_are_errors_not_panics() {
        assert!(Model::read(b"").is_err());
        assert!(Model::read(&[0xff; 64]).is_err());
        // Every prefix and a few corruptions of a hand-made buffer.
        let mut fb = vec![12, 0, 0, 0, 0, 0, 6, 0, 8, 0, 4, 0, 6, 0, 0, 0, 0, 0, 0, 0];
        for n in 0..fb.len() {
            let _ = Model::read(&fb[..n]);
        }
        for i in 0..fb.len() {
            fb[i] ^= 0x5a;
            let _ = Model::read(&fb);
        }
    }
}
