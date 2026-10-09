//! An NVENC H.264 encode session: the driver library, the Direct3D 11 device it is opened on, the
//! encoder configuration, and the input / output buffers (every call into the driver).
//!
//! FFI module (docs/adr/0001-platform-ffi.md): every `unsafe` block has a `// SAFETY:` comment, the
//! public items are safe and return `Result<_, String>`, no driver pointer leaves this module.

use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory1};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{Interface, PCSTR, w};

use super::ffi::*;

/// NVIDIA's PCI vendor id.
const NVIDIA: u32 = 0x10DE;

/// The driver's entry points (loaded once).
struct Api {
    list: NV_ENCODE_API_FUNCTION_LIST,
}

// SAFETY: the function table is plain function pointers, immutable after loading; NVENC's API is
// callable from any thread (calls on one encoder session are serialised by `&mut Session`).
unsafe impl Send for Api {}
// SAFETY: see above.
unsafe impl Sync for Api {}

fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(load).as_ref().map_err(Clone::clone)
}

fn load() -> Result<Api, String> {
    // SAFETY: loads the driver's library from the system directory only.
    let module: HMODULE =
        unsafe { LoadLibraryExW(w!("nvEncodeAPI64.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(|_| "no NVIDIA encoder driver".to_string())?;
    // SAFETY: both symbols are looked up with the signatures of nvEncodeAPI.h; the table is
    // zero-initialised (all-zero is valid: null pointers) with its version set, as the API asks.
    unsafe {
        let version: MaxSupportedVersionFn = std::mem::transmute(
            GetProcAddress(module, PCSTR(c"NvEncodeAPIGetMaxSupportedVersion".as_ptr().cast())).ok_or("driver lacks NvEncodeAPIGetMaxSupportedVersion")?,
        );
        let create: CreateInstanceFn =
            std::mem::transmute(GetProcAddress(module, PCSTR(c"NvEncodeAPICreateInstance".as_ptr().cast())).ok_or("driver lacks NvEncodeAPICreateInstance")?);
        let mut v = 0u32;
        if version(&mut v) != NV_ENC_SUCCESS {
            return Err("cannot query the NVENC version".into());
        }
        let ours = (NVENCAPI_VERSION & 0xf) << 4 | ((NVENCAPI_VERSION >> 24) & 0xf);
        if v < ours {
            return Err(format!("the NVIDIA driver's NVENC ({}.{}) is older than {}.{}", v >> 4, v & 0xf, ours >> 4, ours & 0xf));
        }
        let mut list: NV_ENCODE_API_FUNCTION_LIST = std::mem::zeroed();
        list.version = NV_ENCODE_API_FUNCTION_LIST_VER;
        if create(&mut list) != NV_ENC_SUCCESS {
            return Err("NvEncodeAPICreateInstance failed".into());
        }
        Ok(Api { list })
    }
}

/// A Direct3D 11 device on the first NVIDIA adapter (what an NVENC session is opened on).
fn nvidia_device() -> Result<ID3D11Device, String> {
    // SAFETY: plain COM / D3D calls with valid out-pointers; the adapter outlives the call.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| format!("no DXGI: {e}"))?;
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            if desc.VendorId != NVIDIA {
                continue;
            }
            let mut device = None;
            let adapter: IDXGIAdapter = adapter.cast().map_err(|e| e.to_string())?;
            if D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
            .is_ok()
                && let Some(d) = device
            {
                return Ok(d);
            }
        }
    }
    Err("no NVIDIA adapter".into())
}

/// What the encoder says it can do for H.264.
#[derive(Clone, Copy, Debug)]
pub struct Caps {
    pub max_bframes: u32,
    pub min_size: (u32, u32),
    pub max_size: (u32, u32),
}

/// Encoder settings (already validated by the caller).
#[derive(Clone, Debug)]
pub struct Params {
    pub width: u32,
    pub height: u32,
    pub fps: (u32, u32),
    pub bitrate: u32,
    pub max_bitrate: u32,
    pub cbr: bool,
    pub gop: u32,
    /// 0 baseline, 1 main, 2 high.
    pub profile: u8,
    pub level: Option<u8>,
    pub sar: Option<(u32, u32)>,
    /// B-frames between references (0 or 1).
    pub bframes: u32,
}

/// An input buffer locked for writing: NV12 rows go to `data` at `pitch` bytes per row, the
/// chroma plane after `pitch * height` bytes.
pub struct Locked<'a> {
    pub data: &'a mut [u8],
    pub pitch: usize,
}

/// A finished picture's bitstream.
pub struct Output {
    /// Annex B bytes.
    pub data: Vec<u8>,
    pub pts: u64,
    /// `NV_ENC_PIC_TYPE_*`.
    pub pic_type: u32,
}

/// The result of submitting a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submitted {
    /// The pictures submitted so far have their bitstreams ready.
    Ready,
    /// The encoder wants more input (B-frame reordering) before it has output.
    NeedMoreInput,
}

struct Slot {
    input: *mut c_void,
    output: *mut c_void,
}

/// An initialised encoder with its buffers.
pub struct Session {
    api: &'static Api,
    enc: Handle,
    // keeps the Direct3D device (the session's device) alive
    _device: ID3D11Device,
    slots: Vec<Slot>,
    size: (u32, u32),
    out_size: u32,
}

// SAFETY: an encoder session and its buffers are driver objects used from one thread at a time (the
// session is `&mut`-driven); the Direct3D device is a free-threaded COM object.
unsafe impl Send for Session {}

/// A readable form of a driver status.
fn status(what: &str, s: NvStatus, enc: Handle, api: &Api) -> String {
    let detail = (!enc.is_null())
        .then(|| {
            // SAFETY: a valid session handle; the driver returns a NUL-terminated string it owns.
            api.list.nvEncGetLastErrorString.and_then(|f| unsafe {
                let p = f(enc);
                (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
            })
        })
        .flatten()
        .filter(|d| !d.is_empty());
    match detail {
        Some(d) => format!("{what} failed ({s}): {d}"),
        None => format!("{what} failed ({s})"),
    }
}

impl Session {
    /// Open a session on the system's NVIDIA GPU, or say why not.
    pub fn open() -> Result<Session, String> {
        let api = api()?;
        let device = nvidia_device()?;
        // SAFETY: a zeroed parameter block with its version and the device's COM pointer; the
        // device is kept in the session for as long as the encoder lives. On failure a non-null
        // handle is the driver's and is destroyed exactly once, here, before returning.
        let enc = unsafe {
            let mut p: NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS = std::mem::zeroed();
            p.version = NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER;
            p.deviceType = NV_ENC_DEVICE_TYPE_DIRECTX;
            p.device = device.as_raw();
            p.apiVersion = NVENCAPI_VERSION;
            let open = api.list.nvEncOpenEncodeSessionEx.ok_or("the driver has no nvEncOpenEncodeSessionEx")?;
            let mut enc: Handle = std::ptr::null_mut();
            let s = open(&mut p, &mut enc);
            if s != NV_ENC_SUCCESS || enc.is_null() {
                let e = status("opening the NVENC session", s, enc, api);
                // a failed open may still hand back a session; destroy it so the GPU's encoder
                // session limit is not used up by sessions nobody owns
                if !enc.is_null()
                    && let Some(d) = api.list.nvEncDestroyEncoder
                {
                    d(enc);
                }
                return Err(e);
            }
            enc
        };
        Ok(Session { api, enc, _device: device, slots: Vec::new(), size: (0, 0), out_size: 0 })
    }

    /// The encoder's limits for H.264.
    pub fn caps(&self) -> Result<Caps, String> {
        let get = |cap: u32| -> Result<u32, String> {
            let f = self.api.list.nvEncGetEncodeCaps.ok_or("no nvEncGetEncodeCaps")?;
            // SAFETY: a valid session, a zeroed parameter block with its version, one int out.
            unsafe {
                let mut p: NV_ENC_CAPS_PARAM = std::mem::zeroed();
                p.version = NV_ENC_CAPS_PARAM_VER;
                p.capsToQuery = cap;
                let mut v = 0i32;
                let s = f(self.enc, NV_ENC_CODEC_H264_GUID, &mut p, &mut v);
                if s != NV_ENC_SUCCESS {
                    return Err(status("querying the encoder", s, self.enc, self.api));
                }
                Ok(v.max(0) as u32)
            }
        };
        Ok(Caps {
            max_bframes: get(NV_ENC_CAPS_NUM_MAX_BFRAMES)?,
            min_size: (get(NV_ENC_CAPS_WIDTH_MIN)?, get(NV_ENC_CAPS_HEIGHT_MIN)?),
            max_size: (get(NV_ENC_CAPS_WIDTH_MAX)?, get(NV_ENC_CAPS_HEIGHT_MAX)?),
        })
    }

    /// Configure the encoder (preset P5, high quality tuning, the settings' rate control) and
    /// create `ring` pairs of input / output buffers.
    pub fn initialize(&mut self, p: &Params, ring: usize) -> Result<(), String> {
        let preset_fn = self.api.list.nvEncGetEncodePresetConfigEx.ok_or("no nvEncGetEncodePresetConfigEx")?;
        let init_fn = self.api.list.nvEncInitializeEncoder.ok_or("no nvEncInitializeEncoder")?;
        // SAFETY: zeroed parameter blocks with their versions; the preset call fills `preset`; the
        // configuration lives in a box that outlives `nvEncInitializeEncoder`, which copies it.
        unsafe {
            let mut preset: Box<NV_ENC_PRESET_CONFIG> = Box::new(std::mem::zeroed());
            preset.version = NV_ENC_PRESET_CONFIG_VER;
            preset.presetCfg.version = NV_ENC_CONFIG_VER;
            let s = preset_fn(self.enc, NV_ENC_CODEC_H264_GUID, NV_ENC_PRESET_P5_GUID, NV_ENC_TUNING_INFO_HIGH_QUALITY, &mut *preset);
            if s != NV_ENC_SUCCESS {
                return Err(status("reading the encoder preset", s, self.enc, self.api));
            }
            let mut cfg: Box<NV_ENC_CONFIG> = Box::new(preset.presetCfg);
            cfg.version = NV_ENC_CONFIG_VER;
            cfg.profileGUID = match p.profile {
                0 => NV_ENC_H264_PROFILE_BASELINE_GUID,
                1 => NV_ENC_H264_PROFILE_MAIN_GUID,
                _ => NV_ENC_H264_PROFILE_HIGH_GUID,
            };
            cfg.gopLength = p.gop.max(1);
            cfg.frameIntervalP = 1 + p.bframes as i32;
            let rc = &mut cfg.rcParams;
            rc.version = NV_ENC_RC_PARAMS_VER;
            rc.rateControlMode = if p.cbr { NV_ENC_PARAMS_RC_CBR } else { NV_ENC_PARAMS_RC_VBR };
            rc.averageBitRate = p.bitrate.saturating_mul(1000);
            rc.maxBitRate = if p.cbr { rc.averageBitRate } else { p.max_bitrate.max(p.bitrate).saturating_mul(1000) };
            // a one-second buffer, the usual for streaming-style rate control
            rc.vbvBufferSize = rc.maxBitRate;
            rc.vbvInitialDelay = rc.vbvBufferSize;
            let h = &mut cfg.encodeCodecConfig.h264Config;
            h.idrPeriod = p.gop.max(1);
            h.level = p.level.map_or(NV_ENC_LEVEL_AUTOSELECT, u32::from);
            h.flags &= !(H264_OUTPUT_AUD | H264_REPEAT_SPSPPS);
            h.entropyCodingMode = if p.profile == 0 { 2 } else { NV_ENC_H264_ENTROPY_CODING_MODE_CABAC };
            h.maxNumRefFrames = 0;
            // BT.709 limited range, like our own encoder's default signalling
            let vui = &mut h.h264VUIParameters;
            vui.videoSignalTypePresentFlag = 1;
            vui.videoFormat = NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED;
            vui.videoFullRangeFlag = 0;
            vui.colourDescriptionPresentFlag = 1;
            vui.colourPrimaries = NV_ENC_VUI_COLOR_PRIMARIES_BT709;
            vui.transferCharacteristics = NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709;
            vui.colourMatrix = NV_ENC_VUI_MATRIX_COEFFS_BT709;
            vui.timingInfoPresentFlag = 1;
            vui.numUnitInTicks = p.fps.1;
            vui.timeScale = p.fps.0.saturating_mul(2);

            let mut init: Box<NV_ENC_INITIALIZE_PARAMS> = Box::new(std::mem::zeroed());
            init.version = NV_ENC_INITIALIZE_PARAMS_VER;
            init.encodeGUID = NV_ENC_CODEC_H264_GUID;
            init.presetGUID = NV_ENC_PRESET_P5_GUID;
            init.encodeWidth = p.width;
            init.encodeHeight = p.height;
            let (sn, sd) = p.sar.unwrap_or((1, 1));
            init.darWidth = p.width.saturating_mul(sn.max(1));
            init.darHeight = p.height.saturating_mul(sd.max(1));
            init.frameRateNum = p.fps.0;
            init.frameRateDen = p.fps.1;
            init.enablePTD = 1;
            init.encodeConfig = &mut *cfg;
            init.tuningInfo = NV_ENC_TUNING_INFO_HIGH_QUALITY;
            init.bufferFormat = NV_ENC_BUFFER_FORMAT_NV12;
            let s = init_fn(self.enc, &mut *init);
            if s != NV_ENC_SUCCESS {
                return Err(status("initialising the encoder", s, self.enc, self.api));
            }
        }
        self.size = (p.width, p.height);
        self.out_size = (p.width.saturating_mul(p.height).saturating_mul(2)).max(1 << 20);
        for _ in 0..ring.max(1) {
            self.add_slot()?;
        }
        Ok(())
    }

    fn add_slot(&mut self) -> Result<(), String> {
        let (create_in, create_out) = (
            self.api.list.nvEncCreateInputBuffer.ok_or("no nvEncCreateInputBuffer")?,
            self.api.list.nvEncCreateBitstreamBuffer.ok_or("no nvEncCreateBitstreamBuffer")?,
        );
        // SAFETY: zeroed parameter blocks with their versions; the driver fills in the buffer
        // handles, which `Drop` destroys.
        unsafe {
            let mut i: NV_ENC_CREATE_INPUT_BUFFER = std::mem::zeroed();
            i.version = NV_ENC_CREATE_INPUT_BUFFER_VER;
            i.width = self.size.0;
            i.height = self.size.1;
            i.memoryHeap = NV_ENC_MEMORY_HEAP_SYSMEM_CACHED;
            i.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;
            let s = create_in(self.enc, &mut i);
            if s != NV_ENC_SUCCESS {
                return Err(status("creating an input buffer", s, self.enc, self.api));
            }
            let mut o: NV_ENC_CREATE_BITSTREAM_BUFFER = std::mem::zeroed();
            o.version = NV_ENC_CREATE_BITSTREAM_BUFFER_VER;
            o.size = self.out_size;
            o.memoryHeap = NV_ENC_MEMORY_HEAP_SYSMEM_CACHED;
            let s = create_out(self.enc, &mut o);
            if s != NV_ENC_SUCCESS {
                if let Some(d) = self.api.list.nvEncDestroyInputBuffer {
                    d(self.enc, i.inputBuffer);
                }
                return Err(status("creating an output buffer", s, self.enc, self.api));
            }
            self.slots.push(Slot { input: i.inputBuffer, output: o.bitstreamBuffer });
        }
        Ok(())
    }

    /// Number of buffer slots.
    pub fn slots(&self) -> usize {
        self.slots.len()
    }

    /// The SPS and PPS as one Annex B byte string.
    pub fn sequence_params(&self) -> Result<Vec<u8>, String> {
        let f = self.api.list.nvEncGetSequenceParams.ok_or("no nvEncGetSequenceParams")?;
        let mut buf = vec![0u8; 1024];
        let mut len = 0u32;
        // SAFETY: a zeroed parameter block with its version; `buf` and `len` outlive the call and
        // the driver writes at most `inBufferSize` bytes into `buf`.
        unsafe {
            let mut p: NV_ENC_SEQUENCE_PARAM_PAYLOAD = std::mem::zeroed();
            p.version = NV_ENC_SEQUENCE_PARAM_PAYLOAD_VER;
            p.inBufferSize = buf.len() as u32;
            p.spsppsBuffer = buf.as_mut_ptr().cast();
            p.outSPSPPSPayloadSize = &mut len;
            let s = f(self.enc, &mut p);
            if s != NV_ENC_SUCCESS {
                return Err(status("reading the parameter sets", s, self.enc, self.api));
            }
        }
        buf.truncate(len as usize);
        Ok(buf)
    }

    /// Write a picture into slot `slot`'s input buffer through `fill` (which gets the locked
    /// buffer), then submit it with timestamp `pts`; `output_slot` receives the bitstream.
    pub fn submit(&mut self, slot: usize, pts: u64, fill: impl FnOnce(Locked<'_>)) -> Result<Submitted, String> {
        let s = self.slots.get(slot).ok_or("no such buffer slot")?;
        let (input, output) = (s.input, s.output);
        let (lock, unlock, encode) = (
            self.api.list.nvEncLockInputBuffer.ok_or("no nvEncLockInputBuffer")?,
            self.api.list.nvEncUnlockInputBuffer.ok_or("no nvEncUnlockInputBuffer")?,
            self.api.list.nvEncEncodePicture.ok_or("no nvEncEncodePicture")?,
        );
        let (w, h) = self.size;
        // SAFETY: the buffer handle is one this session created. `LockInputBuffer` returns a
        // writable block of `pitch * h * 3 / 2` bytes (NV12: luma rows then chroma rows) that stays
        // valid until `UnlockInputBuffer`; the slice borrows it only inside `fill`.
        let pitch = unsafe {
            let mut l: NV_ENC_LOCK_INPUT_BUFFER = std::mem::zeroed();
            l.version = NV_ENC_LOCK_INPUT_BUFFER_VER;
            l.inputBuffer = input;
            let st = lock(self.enc, &mut l);
            if st != NV_ENC_SUCCESS || l.bufferDataPtr.is_null() || l.pitch == 0 {
                return Err(status("locking an input buffer", st, self.enc, self.api));
            }
            // a driver pitch narrower than a row would make `fill` write past each row
            if (l.pitch as usize) < w as usize {
                // the lock succeeded, so release it before giving up (its status no longer matters)
                let _ = unlock(self.enc, input);
                return Err(format!("the driver returned an input pitch of {} bytes for {w}-pixel rows", l.pitch));
            }
            let len = (l.pitch as usize).saturating_mul(h as usize).saturating_mul(3) / 2;
            fill(Locked { data: std::slice::from_raw_parts_mut(l.bufferDataPtr.cast::<u8>(), len), pitch: l.pitch as usize });
            let st = unlock(self.enc, input);
            if st != NV_ENC_SUCCESS {
                return Err(status("unlocking an input buffer", st, self.enc, self.api));
            }
            l.pitch
        };
        // SAFETY: a zeroed parameter block with its version; the buffers are this session's.
        let st = unsafe {
            let mut e: NV_ENC_PIC_PARAMS = std::mem::zeroed();
            e.version = NV_ENC_PIC_PARAMS_VER;
            e.inputWidth = w;
            e.inputHeight = h;
            e.inputPitch = pitch;
            e.inputBuffer = input;
            e.outputBitstream = output;
            e.bufferFmt = NV_ENC_BUFFER_FORMAT_NV12;
            e.pictureStruct = NV_ENC_PIC_STRUCT_FRAME;
            e.inputTimeStamp = pts;
            e.inputDuration = 1;
            encode(self.enc, &mut e)
        };
        match st {
            NV_ENC_SUCCESS => Ok(Submitted::Ready),
            NV_ENC_ERR_NEED_MORE_INPUT => Ok(Submitted::NeedMoreInput),
            s => Err(status("encoding a picture", s, self.enc, self.api)),
        }
    }

    /// Tell the encoder the stream ends: every picture it still holds becomes ready.
    pub fn end_of_stream(&mut self) -> Result<(), String> {
        let encode = self.api.list.nvEncEncodePicture.ok_or("no nvEncEncodePicture")?;
        // SAFETY: a zeroed parameter block with its version and the EOS flag (no input buffer).
        let st = unsafe {
            let mut e: NV_ENC_PIC_PARAMS = std::mem::zeroed();
            e.version = NV_ENC_PIC_PARAMS_VER;
            e.encodePicFlags = NV_ENC_PIC_FLAG_EOS;
            encode(self.enc, &mut e)
        };
        if st != NV_ENC_SUCCESS {
            return Err(status("ending the stream", st, self.enc, self.api));
        }
        Ok(())
    }

    /// Read the finished bitstream of slot `slot` (blocks until the encoder has it).
    pub fn read(&mut self, slot: usize) -> Result<Output, String> {
        let output = self.slots.get(slot).ok_or("no such buffer slot")?.output;
        let (lock, unlock) =
            (self.api.list.nvEncLockBitstream.ok_or("no nvEncLockBitstream")?, self.api.list.nvEncUnlockBitstream.ok_or("no nvEncUnlockBitstream")?);
        // SAFETY: a zeroed parameter block with its version and a buffer of this session; after a
        // successful lock `bitstreamBufferPtr` addresses `bitstreamSizeInBytes` readable bytes until
        // `UnlockBitstream`, and they are copied before it.
        unsafe {
            let mut l: NV_ENC_LOCK_BITSTREAM = std::mem::zeroed();
            l.version = NV_ENC_LOCK_BITSTREAM_VER;
            l.outputBitstream = output;
            let st = lock(self.enc, &mut l);
            if st != NV_ENC_SUCCESS {
                return Err(status("reading a bitstream", st, self.enc, self.api));
            }
            let n = l.bitstreamSizeInBytes as usize;
            let data =
                if l.bitstreamBufferPtr.is_null() || n == 0 { Vec::new() } else { std::slice::from_raw_parts(l.bitstreamBufferPtr.cast::<u8>(), n).to_vec() };
            let out = Output { data, pts: l.outputTimeStamp, pic_type: l.pictureType };
            let st = unlock(self.enc, output);
            if st != NV_ENC_SUCCESS {
                return Err(status("releasing a bitstream", st, self.enc, self.api));
            }
            Ok(out)
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: every handle was created by this session and is destroyed once, buffers first.
        unsafe {
            for s in self.slots.drain(..) {
                if let Some(d) = self.api.list.nvEncDestroyInputBuffer {
                    d(self.enc, s.input);
                }
                if let Some(d) = self.api.list.nvEncDestroyBitstreamBuffer {
                    d(self.enc, s.output);
                }
            }
            if let Some(d) = self.api.list.nvEncDestroyEncoder {
                d(self.enc);
            }
        }
    }
}
