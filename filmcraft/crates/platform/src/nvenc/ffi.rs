// The declarations in this file are transcribed from NVIDIA's `nvEncodeAPI.h` (Video Codec SDK,
// API 12.1), which carries this notice:
//
// Copyright (c) 2010-2023 NVIDIA Corporation
//
// Permission is hereby granted, free of charge, to any person
// obtaining a copy of this software and associated documentation
// files (the "Software"), to deal in the Software without
// restriction, including without limitation the rights to use,
// copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the software, and to permit persons to whom the
// software is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice shall be
// included in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
// OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
// HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
// WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
// OTHER DEALINGS IN THE SOFTWARE.

//! The NVIDIA Video Codec SDK's encode API (`nvEncodeAPI.h`, API 12.1, MIT-licensed header by
//! NVIDIA) as the driver's `nvEncodeAPI64.dll` expects it: the structures this backend fills in,
//! the function table, and the constants it uses. Written from the public header; sizes and field
//! offsets are checked against a C compiler's by the generated `abi_tests.rs`.
//!
//! API 12.1 is targeted on purpose: it is understood by every driver since 531.x, and the encoder
//! features used here (H.264, presets, rate control) are the same in later versions.
//!
//! FFI module (docs/adr/0001-platform-ffi.md): plain `repr(C)` data, no logic.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_int, c_void};

use windows::core::GUID;

pub const NVENCAPI_VERSION: u32 = 12 | (1 << 24);

/// `NVENCAPI_STRUCT_VERSION(ver)`.
pub const fn struct_version(ver: u32) -> u32 {
    NVENCAPI_VERSION | (ver << 16) | (0x7 << 28)
}

pub const NV_ENCODE_API_FUNCTION_LIST_VER: u32 = struct_version(2);
pub const NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS_VER: u32 = struct_version(1);
pub const NV_ENC_CAPS_PARAM_VER: u32 = struct_version(1);
pub const NV_ENC_CONFIG_VER: u32 = struct_version(8) | (1 << 31);
pub const NV_ENC_INITIALIZE_PARAMS_VER: u32 = struct_version(6) | (1 << 31);
pub const NV_ENC_PRESET_CONFIG_VER: u32 = struct_version(4) | (1 << 31);
pub const NV_ENC_RC_PARAMS_VER: u32 = struct_version(1);
pub const NV_ENC_CREATE_INPUT_BUFFER_VER: u32 = struct_version(1);
pub const NV_ENC_CREATE_BITSTREAM_BUFFER_VER: u32 = struct_version(1);
pub const NV_ENC_PIC_PARAMS_VER: u32 = struct_version(6) | (1 << 31);
pub const NV_ENC_LOCK_BITSTREAM_VER: u32 = struct_version(1) | (1 << 31);
pub const NV_ENC_LOCK_INPUT_BUFFER_VER: u32 = struct_version(1);
pub const NV_ENC_SEQUENCE_PARAM_PAYLOAD_VER: u32 = struct_version(1);

pub type NvStatus = c_int;
pub const NV_ENC_SUCCESS: NvStatus = 0;
pub const NV_ENC_ERR_NEED_MORE_INPUT: NvStatus = 17;
pub const NV_ENC_ERR_INVALID_VERSION: NvStatus = 15;
pub const NV_ENC_ERR_UNSUPPORTED_DEVICE: NvStatus = 2;
pub const NV_ENC_ERR_ENCODER_BUSY: NvStatus = 18;
pub const NV_ENC_ERR_LOCK_BUSY: NvStatus = 13;

pub const NV_ENC_DEVICE_TYPE_DIRECTX: u32 = 0;
pub const NV_ENC_MEMORY_HEAP_SYSMEM_CACHED: u32 = 2;
pub const NV_ENC_BUFFER_FORMAT_NV12: u32 = 0x1;
pub const NV_ENC_PARAMS_RC_VBR: u32 = 1;
pub const NV_ENC_PARAMS_RC_CBR: u32 = 2;
pub const NV_ENC_TUNING_INFO_HIGH_QUALITY: u32 = 1;
pub const NV_ENC_PIC_STRUCT_FRAME: u32 = 1;
pub const NV_ENC_PIC_TYPE_P: u32 = 0;
pub const NV_ENC_PIC_TYPE_B: u32 = 1;
pub const NV_ENC_PIC_TYPE_I: u32 = 2;
pub const NV_ENC_PIC_TYPE_IDR: u32 = 3;
pub const NV_ENC_PIC_FLAG_EOS: u32 = 0x8;
pub const NV_ENC_LEVEL_AUTOSELECT: u32 = 0;
pub const NV_ENC_H264_ENTROPY_CODING_MODE_CABAC: u32 = 1;
pub const NV_ENC_VUI_VIDEO_FORMAT_UNSPECIFIED: u32 = 5;
pub const NV_ENC_VUI_COLOR_PRIMARIES_BT709: u32 = 1;
pub const NV_ENC_VUI_TRANSFER_CHARACTERISTIC_BT709: u32 = 1;
pub const NV_ENC_VUI_MATRIX_COEFFS_BT709: u32 = 1;

/// `NV_ENC_CAPS` values (the enum counts from zero in the header's order).
pub const NV_ENC_CAPS_NUM_MAX_BFRAMES: u32 = 0;
pub const NV_ENC_CAPS_SUPPORTED_RATECONTROL_MODES: u32 = 1;
pub const NV_ENC_CAPS_WIDTH_MAX: u32 = 16;
pub const NV_ENC_CAPS_HEIGHT_MAX: u32 = 17;
pub const NV_ENC_CAPS_WIDTH_MIN: u32 = 45;
pub const NV_ENC_CAPS_HEIGHT_MIN: u32 = 46;

pub const NV_ENC_CODEC_H264_GUID: GUID = GUID::from_values(0x6bc82762, 0x4e63, 0x4ca4, [0xaa, 0x85, 0x1e, 0x50, 0xf3, 0x21, 0xf6, 0xbf]);
pub const NV_ENC_H264_PROFILE_BASELINE_GUID: GUID = GUID::from_values(0x0727bcaa, 0x78c4, 0x4c83, [0x8c, 0x2f, 0xef, 0x3d, 0xff, 0x26, 0x7c, 0x6a]);
pub const NV_ENC_H264_PROFILE_MAIN_GUID: GUID = GUID::from_values(0x60b5c1d4, 0x67fe, 0x4790, [0x94, 0xd5, 0xc4, 0x72, 0x6d, 0x7b, 0x6e, 0x6d]);
pub const NV_ENC_H264_PROFILE_HIGH_GUID: GUID = GUID::from_values(0xe7cbc309, 0x4f7a, 0x4b89, [0xaf, 0x2a, 0xd5, 0x37, 0xc9, 0x2b, 0xe3, 0x10]);
pub const NV_ENC_PRESET_P4_GUID: GUID = GUID::from_values(0x90a7b826, 0xdf06, 0x4862, [0xb9, 0xd2, 0xcd, 0x6d, 0x73, 0xa0, 0x86, 0x81]);
pub const NV_ENC_PRESET_P5_GUID: GUID = GUID::from_values(0x21c6e6b4, 0x297a, 0x4cba, [0x99, 0x8f, 0xb6, 0xcb, 0xde, 0x72, 0xad, 0xe3]);

pub type Handle = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS {
    pub version: u32,
    pub deviceType: u32,
    pub device: *mut c_void,
    pub reserved: *mut c_void,
    pub apiVersion: u32,
    pub reserved1: [u32; 253],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CAPS_PARAM {
    pub version: u32,
    pub capsToQuery: u32,
    pub reserved: [u32; 62],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_QP {
    pub qpInterP: u32,
    pub qpInterB: u32,
    pub qpIntra: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_RC_PARAMS {
    pub version: u32,
    pub rateControlMode: u32,
    pub constQP: NV_ENC_QP,
    pub averageBitRate: u32,
    pub maxBitRate: u32,
    pub vbvBufferSize: u32,
    pub vbvInitialDelay: u32,
    /// enableMinQP:1, enableMaxQP:1, enableInitialRCQP:1, enableAQ:1, reservedBitField1:1,
    /// enableLookahead:1, disableIadapt:1, disableBadapt:1, enableTemporalAQ:1, zeroReorderDelay:1,
    /// enableNonRefP:1, strictGOPTarget:1, aqStrength:4, enableExtLookahead:1, reserved:15.
    pub flags: u32,
    pub minQP: NV_ENC_QP,
    pub maxQP: NV_ENC_QP,
    pub initialRCQP: NV_ENC_QP,
    pub temporallayerIdxMask: u32,
    pub temporalLayerQP: [u8; 8],
    pub targetQuality: u8,
    pub targetQualityLSB: u8,
    pub lookaheadDepth: u16,
    pub lowDelayKeyFrameScale: u8,
    pub yDcQPIndexOffset: i8,
    pub uDcQPIndexOffset: i8,
    pub vDcQPIndexOffset: i8,
    pub qpMapMode: u32,
    pub multiPass: u32,
    pub alphaLayerBitrateRatio: u32,
    pub cbQPIndexOffset: i8,
    pub crQPIndexOffset: i8,
    pub reserved2: u16,
    pub reserved: [u32; 4],
}

pub const RC_ENABLE_AQ: u32 = 1 << 3;
pub const RC_ENABLE_LOOKAHEAD: u32 = 1 << 5;
pub const RC_ZERO_REORDER_DELAY: u32 = 1 << 9;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CONFIG_H264_VUI_PARAMETERS {
    pub overscanInfoPresentFlag: u32,
    pub overscanInfo: u32,
    pub videoSignalTypePresentFlag: u32,
    pub videoFormat: u32,
    pub videoFullRangeFlag: u32,
    pub colourDescriptionPresentFlag: u32,
    pub colourPrimaries: u32,
    pub transferCharacteristics: u32,
    pub colourMatrix: u32,
    pub chromaSampleLocationFlag: u32,
    pub chromaSampleLocationTop: u32,
    pub chromaSampleLocationBot: u32,
    pub bitstreamRestrictionFlag: u32,
    pub timingInfoPresentFlag: u32,
    pub numUnitInTicks: u32,
    pub timeScale: u32,
    pub reserved: [u32; 12],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CONFIG_H264 {
    /// enableTemporalSVC:1, enableStereoMVC:1, hierarchicalPFrames:1, hierarchicalBFrames:1,
    /// outputBufferingPeriodSEI:1, outputPictureTimingSEI:1, outputAUD:1, disableSPSPPS:1,
    /// outputFramePackingSEI:1, outputRecoveryPointSEI:1, enableIntraRefresh:1,
    /// enableConstrainedEncoding:1, repeatSPSPPS:1, enableVFR:1, enableLTR:1, ...
    pub flags: u32,
    pub level: u32,
    pub idrPeriod: u32,
    pub separateColourPlaneFlag: u32,
    pub disableDeblockingFilterIDC: u32,
    pub numTemporalLayers: u32,
    pub spsId: u32,
    pub ppsId: u32,
    pub adaptiveTransformMode: u32,
    pub fmoMode: u32,
    pub bdirectMode: u32,
    pub entropyCodingMode: u32,
    pub stereoMode: u32,
    pub intraRefreshPeriod: u32,
    pub intraRefreshCnt: u32,
    pub maxNumRefFrames: u32,
    pub sliceMode: u32,
    pub sliceModeData: u32,
    pub h264VUIParameters: NV_ENC_CONFIG_H264_VUI_PARAMETERS,
    pub ltrNumFrames: u32,
    pub ltrTrustMode: u32,
    pub chromaFormatIDC: u32,
    pub maxTemporalLayers: u32,
    pub useBFramesAsRef: u32,
    pub numRefL0: u32,
    pub numRefL1: u32,
    pub reserved1: [u32; 267],
    pub reserved2: [*mut c_void; 64],
}

pub const H264_OUTPUT_AUD: u32 = 1 << 6;
pub const H264_REPEAT_SPSPPS: u32 = 1 << 12;
pub const H264_OUTPUT_RECOVERY_POINT_SEI: u32 = 1 << 9;

#[repr(C)]
#[derive(Clone, Copy)]
pub union NV_ENC_CODEC_CONFIG {
    pub h264Config: NV_ENC_CONFIG_H264,
    pub reserved: [u32; 320],
    align: [*mut c_void; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CONFIG {
    pub version: u32,
    pub profileGUID: GUID,
    pub gopLength: u32,
    pub frameIntervalP: i32,
    pub monoChromeEncoding: u32,
    pub frameFieldMode: u32,
    pub mvPrecision: u32,
    pub rcParams: NV_ENC_RC_PARAMS,
    pub encodeCodecConfig: NV_ENC_CODEC_CONFIG,
    pub reserved: [u32; 278],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_PRESET_CONFIG {
    pub version: u32,
    pub presetCfg: NV_ENC_CONFIG,
    pub reserved1: [u32; 255],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE {
    pub bits: u32,
    pub reserved1: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_INITIALIZE_PARAMS {
    pub version: u32,
    pub encodeGUID: GUID,
    pub presetGUID: GUID,
    pub encodeWidth: u32,
    pub encodeHeight: u32,
    pub darWidth: u32,
    pub darHeight: u32,
    pub frameRateNum: u32,
    pub frameRateDen: u32,
    pub enableEncodeAsync: u32,
    pub enablePTD: u32,
    /// reportSliceOffsets:1, enableSubFrameWrite:1, enableExternalMEHints:1, enableMEOnlyMode:1,
    /// enableWeightedPrediction:1, splitEncodeMode:4, enableOutputInVidmem:1,
    /// enableReconFrameOutput:1, enableOutputStats:1, reserved:20.
    pub flags: u32,
    pub privDataSize: u32,
    pub privData: *mut c_void,
    pub encodeConfig: *mut NV_ENC_CONFIG,
    pub maxEncodeWidth: u32,
    pub maxEncodeHeight: u32,
    pub maxMEHintCountsPerBlock: [NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE; 2],
    pub tuningInfo: u32,
    pub bufferFormat: u32,
    pub numStateBuffers: u32,
    pub outputStatsLevel: u32,
    pub reserved: [u32; 285],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CREATE_INPUT_BUFFER {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub memoryHeap: u32,
    pub bufferFmt: u32,
    pub reserved: u32,
    pub inputBuffer: *mut c_void,
    pub pSysMemBuffer: *mut c_void,
    pub reserved1: [u32; 57],
    pub reserved2: [*mut c_void; 63],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CREATE_BITSTREAM_BUFFER {
    pub version: u32,
    pub size: u32,
    pub memoryHeap: u32,
    pub reserved: u32,
    pub bitstreamBuffer: *mut c_void,
    pub bitstreamBufferPtr: *mut c_void,
    pub reserved1: [u32; 58],
    pub reserved2: [*mut c_void; 64],
}

/// `NV_ENC_CODEC_PIC_PARAMS`: a union of codec structures, all zero here (defaults).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_CODEC_PIC_PARAMS {
    pub reserved: [u32; 388],
    align: [*mut c_void; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_PIC_PARAMS {
    pub version: u32,
    pub inputWidth: u32,
    pub inputHeight: u32,
    pub inputPitch: u32,
    pub encodePicFlags: u32,
    pub frameIdx: u32,
    pub inputTimeStamp: u64,
    pub inputDuration: u64,
    pub inputBuffer: *mut c_void,
    pub outputBitstream: *mut c_void,
    pub completionEvent: *mut c_void,
    pub bufferFmt: u32,
    pub pictureStruct: u32,
    pub pictureType: u32,
    pub codecPicParams: NV_ENC_CODEC_PIC_PARAMS,
    pub meHintCountsPerBlock: [NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE; 2],
    pub meExternalHints: *mut c_void,
    pub reserved1: [u32; 6],
    pub reserved2: [*mut c_void; 2],
    pub qpDeltaMap: *mut i8,
    pub qpDeltaMapSize: u32,
    pub reservedBitFields: u32,
    pub meHintRefPicDist: [u16; 2],
    pub alphaBuffer: *mut c_void,
    pub meExternalSbHints: *mut c_void,
    pub meSbHintsCount: u32,
    pub stateBufferIdx: u32,
    pub outputReconBuffer: *mut c_void,
    pub reserved3: [u32; 284],
    pub reserved4: [*mut c_void; 57],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_LOCK_BITSTREAM {
    pub version: u32,
    /// doNotWait:1, ltrFrame:1, getRCStats:1, reserved:29.
    pub flags: u32,
    pub outputBitstream: *mut c_void,
    pub sliceOffsets: *mut u32,
    pub frameIdx: u32,
    pub hwEncodeStatus: u32,
    pub numSlices: u32,
    pub bitstreamSizeInBytes: u32,
    pub outputTimeStamp: u64,
    pub outputDuration: u64,
    pub bitstreamBufferPtr: *mut c_void,
    pub pictureType: u32,
    pub pictureStruct: u32,
    pub frameAvgQP: u32,
    pub frameSatd: u32,
    pub ltrFrameIdx: u32,
    pub ltrFrameBitmap: u32,
    pub temporalId: u32,
    pub intraMBCount: u32,
    pub interMBCount: u32,
    pub averageMVX: i32,
    pub averageMVY: i32,
    pub alphaLayerSizeInBytes: u32,
    pub outputStatsPtrSize: u32,
    pub outputStatsPtr: *mut c_void,
    pub frameIdxDisplay: u32,
    pub reserved1: [u32; 220],
    pub reserved2: [*mut c_void; 63],
    pub reservedInternal: [u32; 8],
}

pub const LOCK_DO_NOT_WAIT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_LOCK_INPUT_BUFFER {
    pub version: u32,
    /// doNotWait:1, reserved:31.
    pub flags: u32,
    pub inputBuffer: *mut c_void,
    pub bufferDataPtr: *mut c_void,
    pub pitch: u32,
    pub reserved1: [u32; 251],
    pub reserved2: [*mut c_void; 64],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NV_ENC_SEQUENCE_PARAM_PAYLOAD {
    pub version: u32,
    pub inBufferSize: u32,
    pub spsId: u32,
    pub ppsId: u32,
    pub spsppsBuffer: *mut c_void,
    pub outSPSPPSPayloadSize: *mut u32,
    pub reserved: [u32; 250],
    pub reserved2: [*mut c_void; 64],
}

type Unused = *const c_void;

/// The driver's function table (`NV_ENCODE_API_FUNCTION_LIST`); the entries this backend does not
/// call are plain pointers.
#[repr(C)]
pub struct NV_ENCODE_API_FUNCTION_LIST {
    pub version: u32,
    pub reserved: u32,
    pub nvEncOpenEncodeSession: Unused,
    pub nvEncGetEncodeGUIDCount: Unused,
    pub nvEncGetEncodeProfileGUIDCount: Unused,
    pub nvEncGetEncodeProfileGUIDs: Unused,
    pub nvEncGetEncodeGUIDs: Unused,
    pub nvEncGetInputFormatCount: Unused,
    pub nvEncGetInputFormats: Unused,
    pub nvEncGetEncodeCaps: Option<unsafe extern "system" fn(Handle, GUID, *mut NV_ENC_CAPS_PARAM, *mut c_int) -> NvStatus>,
    pub nvEncGetEncodePresetCount: Unused,
    pub nvEncGetEncodePresetGUIDs: Unused,
    pub nvEncGetEncodePresetConfig: Unused,
    pub nvEncInitializeEncoder: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_INITIALIZE_PARAMS) -> NvStatus>,
    pub nvEncCreateInputBuffer: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_CREATE_INPUT_BUFFER) -> NvStatus>,
    pub nvEncDestroyInputBuffer: Option<unsafe extern "system" fn(Handle, *mut c_void) -> NvStatus>,
    pub nvEncCreateBitstreamBuffer: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_CREATE_BITSTREAM_BUFFER) -> NvStatus>,
    pub nvEncDestroyBitstreamBuffer: Option<unsafe extern "system" fn(Handle, *mut c_void) -> NvStatus>,
    pub nvEncEncodePicture: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_PIC_PARAMS) -> NvStatus>,
    pub nvEncLockBitstream: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_LOCK_BITSTREAM) -> NvStatus>,
    pub nvEncUnlockBitstream: Option<unsafe extern "system" fn(Handle, *mut c_void) -> NvStatus>,
    pub nvEncLockInputBuffer: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_LOCK_INPUT_BUFFER) -> NvStatus>,
    pub nvEncUnlockInputBuffer: Option<unsafe extern "system" fn(Handle, *mut c_void) -> NvStatus>,
    pub nvEncGetEncodeStats: Unused,
    pub nvEncGetSequenceParams: Option<unsafe extern "system" fn(Handle, *mut NV_ENC_SEQUENCE_PARAM_PAYLOAD) -> NvStatus>,
    pub nvEncRegisterAsyncEvent: Unused,
    pub nvEncUnregisterAsyncEvent: Unused,
    pub nvEncMapInputResource: Unused,
    pub nvEncUnmapInputResource: Unused,
    pub nvEncDestroyEncoder: Option<unsafe extern "system" fn(Handle) -> NvStatus>,
    pub nvEncInvalidateRefFrames: Unused,
    pub nvEncOpenEncodeSessionEx: Option<unsafe extern "system" fn(*mut NV_ENC_OPEN_ENCODE_SESSION_EX_PARAMS, *mut Handle) -> NvStatus>,
    pub nvEncRegisterResource: Unused,
    pub nvEncUnregisterResource: Unused,
    pub nvEncReconfigureEncoder: Unused,
    pub reserved1: Unused,
    pub nvEncCreateMVBuffer: Unused,
    pub nvEncDestroyMVBuffer: Unused,
    pub nvEncRunMotionEstimationOnly: Unused,
    pub nvEncGetLastErrorString: Option<unsafe extern "system" fn(Handle) -> *const c_char>,
    pub nvEncSetIOCudaStreams: Unused,
    pub nvEncGetEncodePresetConfigEx: Option<unsafe extern "system" fn(Handle, GUID, GUID, u32, *mut NV_ENC_PRESET_CONFIG) -> NvStatus>,
    pub nvEncGetSequenceParamEx: Unused,
    pub nvEncRestoreEncoderState: Unused,
    pub nvEncLookaheadPicture: Unused,
    pub reserved2: [Unused; 275],
}

pub type CreateInstanceFn = unsafe extern "system" fn(*mut NV_ENCODE_API_FUNCTION_LIST) -> NvStatus;
pub type MaxSupportedVersionFn = unsafe extern "system" fn(*mut u32) -> NvStatus;
