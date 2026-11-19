// SPDX-License-Identifier: Apache-2.0
//! Windows backend: DirectInput 8 constant-force effect.
//!
//! This is the only module in the workspace that contains Windows-specific
//! code, and the only one in this crate that uses `unsafe` (DirectInput is a
//! COM API). It is compiled on Windows alone.
//!
//! The device is acquired in exclusive background mode, which DirectInput
//! requires for force feedback. A custom one-axis data format is registered
//! so the steering position can be read and so the effect can name its
//! axis; the stock joystick format lives in an import library this crate
//! deliberately does not link.
//!
//! STATUS: written against the DirectInput documentation and compiled, but
//! not exercised on hardware by its author. Treat the output sign as
//! unknown until checked on a wheel; the pipeline's `invert` setting exists
//! for that.
#![allow(unsafe_code)]

use crate::device::{
    DeviceInfo, FfbDevice, FfbError, InputSource, RawInputState, INPUT_AXES, INPUT_BUTTONS,
};
use std::ffi::c_void;
use windows::core::w;
use windows::core::GUID;
use windows::Win32::Devices::HumanInterfaceDevice::{
    DirectInput8Create, IDirectInput8W, IDirectInputDevice8W, IDirectInputEffect,
    DI8DEVCLASS_GAMECTRL, DICONSTANTFORCE, DIDATAFORMAT, DIDEVICEINSTANCEW,
    DIDEVICEOBJECTINSTANCEW, DIDFT_ANYINSTANCE, DIDFT_AXIS, DIDFT_BUTTON, DIDF_ABSAXIS,
    DIEB_NOTRIGGER, DIEDFL_ATTACHEDONLY, DIEDFL_FORCEFEEDBACK, DIEFFECT, DIEFF_CARTESIAN,
    DIEFF_OBJECTOFFSETS, DIENUM_CONTINUE, DIEP_START, DIEP_TYPESPECIFICPARAMS, DIERR_INPUTLOST,
    DIERR_NOTACQUIRED, DIOBJECTDATAFORMAT, DIPH_BYOFFSET, DIPH_DEVICE, DIPROPAUTOCENTER_OFF,
    DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DIRECTINPUT_VERSION, DISCL_BACKGROUND, DISCL_EXCLUSIVE,
    DISCL_NONEXCLUSIVE, DISFFC_RESET, DISFFC_SETACTUATORSON, DI_FFNOMINALMAX,
};
use windows::Win32::Foundation::{BOOL, HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPED,
};

/// Name of this backend in [`DeviceInfo`].
pub const BACKEND: &str = "directinput";

// Object and effect GUIDs from dinput.h. They are defined in a static
// library there; the values are fixed parts of the DirectInput ABI.
const GUID_X_AXIS: GUID = GUID::from_u128(0xa36d02e0_c9f3_11cf_bfc7_444553540000);
const GUID_Y_AXIS: GUID = GUID::from_u128(0xa36d02e1_c9f3_11cf_bfc7_444553540000);
const GUID_Z_AXIS: GUID = GUID::from_u128(0xa36d02e2_c9f3_11cf_bfc7_444553540000);
const GUID_RX_AXIS: GUID = GUID::from_u128(0xa36d02f4_c9f3_11cf_bfc7_444553540000);
const GUID_RY_AXIS: GUID = GUID::from_u128(0xa36d02f5_c9f3_11cf_bfc7_444553540000);
const GUID_RZ_AXIS: GUID = GUID::from_u128(0xa36d02e3_c9f3_11cf_bfc7_444553540000);
const GUID_SLIDER: GUID = GUID::from_u128(0xa36d02e4_c9f3_11cf_bfc7_444553540000);
const GUID_CONSTANT_FORCE: GUID = GUID::from_u128(0x13541c20_8e33_11d0_9ad0_00a0c9a06e35);

// DirectInput property identifiers are small integers cast to GUID
// pointers (MAKEDIPROP in dinput.h), not real GUIDs.
const DIPROP_RANGE: *const GUID = std::ptr::without_provenance(4);
const DIPROP_AUTOCENTER: *const GUID = std::ptr::without_provenance(9);

/// Object flag: the object may be missing from the device (dinput.h).
const DIDFT_OPTIONAL: u32 = 0x8000_0000;
/// Effect duration meaning "until stopped".
const INFINITE: u32 = u32::MAX;
/// Axis range requested from the device.
const AXIS_MAX: i32 = 32_767;

fn device_error(context: &str, e: windows::core::Error) -> FfbError {
    FfbError::Device(format!("{context}: {e}"))
}

fn guid_to_id(g: &GUID) -> String {
    format!("{:032x}", g.to_u128())
}

fn id_to_guid(id: &str) -> Option<GUID> {
    u128::from_str_radix(id, 16).ok().map(GUID::from_u128)
}

fn wide_to_string(text: &[u16]) -> String {
    let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
    String::from_utf16_lossy(&text[..end])
}

fn create_direct_input() -> Result<IDirectInput8W, FfbError> {
    // SAFETY: plain FFI. `DirectInput8Create` writes an interface pointer
    // of the requested type into `raw` on success, which `from_raw` then
    // takes ownership of.
    unsafe {
        let module = GetModuleHandleW(None).map_err(|e| device_error("GetModuleHandle", e))?;
        let mut raw: *mut c_void = std::ptr::null_mut();
        DirectInput8Create(
            HINSTANCE(module.0),
            DIRECTINPUT_VERSION,
            &<IDirectInput8W as windows::core::Interface>::IID,
            &mut raw,
            None,
        )
        .map_err(|e| device_error("DirectInput8Create", e))?;
        if raw.is_null() {
            return Err(FfbError::Device(
                "DirectInput8Create returned no interface".into(),
            ));
        }
        Ok(<IDirectInput8W as windows::core::Interface>::from_raw(raw))
    }
}

unsafe extern "system" fn collect_device(
    instance: *mut DIDEVICEINSTANCEW,
    context: *mut c_void,
) -> BOOL {
    // SAFETY: DirectInput passes a valid instance for the duration of the
    // call, and `context` is the `Vec` handed to `EnumDevices` below, which
    // outlives the enumeration.
    unsafe {
        if let (Some(instance), Some(list)) = (
            instance.as_ref(),
            (context as *mut Vec<(GUID, String)>).as_mut(),
        ) {
            list.push((
                instance.guidInstance,
                wide_to_string(&instance.tszProductName),
            ));
        }
    }
    BOOL(DIENUM_CONTINUE as i32)
}

fn enumerate_with(di: &IDirectInput8W, flags: u32) -> Result<Vec<(GUID, String)>, FfbError> {
    let mut found: Vec<(GUID, String)> = Vec::new();
    // SAFETY: the callback only touches `found`, which lives until
    // `EnumDevices` returns; enumeration is synchronous.
    unsafe {
        di.EnumDevices(
            DI8DEVCLASS_GAMECTRL,
            Some(collect_device),
            &mut found as *mut Vec<(GUID, String)> as *mut c_void,
            flags,
        )
        .map_err(|e| device_error("EnumDevices", e))?;
    }
    Ok(found)
}

/// Lists attached game controllers, marking those with force feedback.
pub fn enumerate() -> Result<Vec<DeviceInfo>, FfbError> {
    let di = create_direct_input()?;
    let all = enumerate_with(&di, DIEDFL_ATTACHEDONLY)?;
    let with_ffb = enumerate_with(&di, DIEDFL_ATTACHEDONLY | DIEDFL_FORCEFEEDBACK)?;
    Ok(all
        .into_iter()
        .map(|(guid, name)| DeviceInfo {
            id: guid_to_id(&guid),
            name,
            has_force_feedback: with_ffb.iter().any(|(g, _)| *g == guid),
            backend: BACKEND,
        })
        .collect())
}

/// An acquired DirectInput device with a running constant-force effect.
pub struct DInputDevice {
    info: DeviceInfo,
    device: IDirectInputDevice8W,
    effect: Option<IDirectInputEffect>,
    // Kept alive because the device refers to it for as long as it exists.
    _direct_input: IDirectInput8W,
    last_magnitude: i32,
    // Hidden window created when the caller had none to offer.
    owned_window: Option<HWND>,
}

// SAFETY: DirectInput device and effect objects may be called from any
// thread. This type is only ever used through `&mut self`, so calls are
// never concurrent.
unsafe impl Send for DInputDevice {}

impl DInputDevice {
    /// Opens a device by the identifier [`enumerate`] returned.
    ///
    /// `window` is the native handle of the application's top-level window.
    /// DirectInput needs one for exclusive access; when `None`, a hidden
    /// window is created and owned by the device.
    pub fn open(id: &str, window: Option<isize>) -> Result<Self, FfbError> {
        let guid = id_to_guid(id).ok_or_else(|| FfbError::NotFound(id.to_string()))?;
        let di = create_direct_input()?;
        let listed = enumerate_with(&di, DIEDFL_ATTACHEDONLY)?;
        let name = listed
            .iter()
            .find(|(g, _)| *g == guid)
            .map(|(_, n)| n.clone())
            .ok_or_else(|| FfbError::NotFound(id.to_string()))?;
        let has_ffb = enumerate_with(&di, DIEDFL_ATTACHEDONLY | DIEDFL_FORCEFEEDBACK)?
            .iter()
            .any(|(g, _)| *g == guid);

        // SAFETY: every pointer passed below refers to a local that outlives
        // the call it is passed to. Struct sizes are set as the API requires.
        unsafe {
            let mut created: Option<IDirectInputDevice8W> = None;
            di.CreateDevice(&guid, &mut created, None)
                .map_err(|e| device_error("CreateDevice", e))?;
            let device = created
                .ok_or_else(|| FfbError::Device("CreateDevice returned no device".into()))?;

            // One absolute axis (steering) at offset 0 of a 4-byte state.
            let mut object = DIOBJECTDATAFORMAT {
                pguid: &GUID_X_AXIS,
                dwOfs: 0,
                dwType: DIDFT_AXIS | DIDFT_ANYINSTANCE,
                dwFlags: 0,
            };
            let mut format = DIDATAFORMAT {
                dwSize: std::mem::size_of::<DIDATAFORMAT>() as u32,
                dwObjSize: std::mem::size_of::<DIOBJECTDATAFORMAT>() as u32,
                dwFlags: DIDF_ABSAXIS,
                dwDataSize: 4,
                dwNumObjs: 1,
                rgodf: &mut object,
            };
            device
                .SetDataFormat(&mut format)
                .map_err(|e| device_error("SetDataFormat", e))?;

            let mut owned_window: Option<HWND> = None;
            let hwnd = match window {
                Some(handle) if handle != 0 => HWND(handle as *mut c_void),
                _ => {
                    // DirectInput only grants exclusive access through a
                    // top-level window of the calling process. A program
                    // without one (the command-line tool) gets a hidden
                    // window created for the purpose.
                    let module =
                        GetModuleHandleW(None).map_err(|e| device_error("GetModuleHandle", e))?;
                    let hidden = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("STATIC"),
                        w!("force feedback"),
                        WS_OVERLAPPED,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        HINSTANCE(module.0),
                        None,
                    )
                    .map_err(|e| device_error("CreateWindow (hidden)", e))?;
                    owned_window = Some(hidden);
                    hidden
                }
            };
            device
                .SetCooperativeLevel(hwnd, DISCL_EXCLUSIVE | DISCL_BACKGROUND)
                .map_err(|e| device_error("SetCooperativeLevel (exclusive, background)", e))?;

            let mut range = DIPROPRANGE {
                diph: DIPROPHEADER {
                    dwSize: std::mem::size_of::<DIPROPRANGE>() as u32,
                    dwHeaderSize: std::mem::size_of::<DIPROPHEADER>() as u32,
                    dwObj: 0,
                    dwHow: DIPH_BYOFFSET,
                },
                lMin: -AXIS_MAX,
                lMax: AXIS_MAX,
            };
            device
                .SetProperty(DIPROP_RANGE, &mut range.diph)
                .map_err(|e| device_error("SetProperty(range)", e))?;

            if has_ffb {
                // The driver's own centring spring would fight the sim.
                let mut autocenter = DIPROPDWORD {
                    diph: DIPROPHEADER {
                        dwSize: std::mem::size_of::<DIPROPDWORD>() as u32,
                        dwHeaderSize: std::mem::size_of::<DIPROPHEADER>() as u32,
                        dwObj: 0,
                        dwHow: DIPH_DEVICE,
                    },
                    dwData: DIPROPAUTOCENTER_OFF,
                };
                // Some drivers do not support this property; that is fine.
                let _ = device.SetProperty(DIPROP_AUTOCENTER, &mut autocenter.diph);
            }

            device.Acquire().map_err(|e| device_error("Acquire", e))?;

            let mut this = Self {
                info: DeviceInfo {
                    id: id.to_string(),
                    name,
                    has_force_feedback: has_ffb,
                    backend: BACKEND,
                },
                device,
                effect: None,
                _direct_input: di,
                last_magnitude: 0,
                owned_window,
            };
            if has_ffb {
                let _ = this.device.SendForceFeedbackCommand(DISFFC_RESET);
                let _ = this.device.SendForceFeedbackCommand(DISFFC_SETACTUATORSON);
                this.effect = Some(this.create_effect()?);
            }
            Ok(this)
        }
    }

    fn create_effect(&self) -> Result<IDirectInputEffect, FfbError> {
        // SAFETY: all pointers refer to locals that outlive `CreateEffect`.
        unsafe {
            let mut axes = [0u32]; // offset of the steering axis in our data format
            let mut direction = [0i32];
            let mut force = DICONSTANTFORCE { lMagnitude: 0 };
            let mut params = DIEFFECT {
                dwSize: std::mem::size_of::<DIEFFECT>() as u32,
                dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
                dwDuration: INFINITE,
                dwSamplePeriod: 0,
                dwGain: DI_FFNOMINALMAX,
                dwTriggerButton: DIEB_NOTRIGGER,
                dwTriggerRepeatInterval: 0,
                cAxes: 1,
                rgdwAxes: axes.as_mut_ptr(),
                rglDirection: direction.as_mut_ptr(),
                lpEnvelope: std::ptr::null_mut(),
                cbTypeSpecificParams: std::mem::size_of::<DICONSTANTFORCE>() as u32,
                lpvTypeSpecificParams: &mut force as *mut DICONSTANTFORCE as *mut c_void,
                dwStartDelay: 0,
            };
            let mut effect: Option<IDirectInputEffect> = None;
            self.device
                .CreateEffect(&GUID_CONSTANT_FORCE, &mut params, &mut effect, None)
                .map_err(|e| device_error("CreateEffect(constant force)", e))?;
            let effect =
                effect.ok_or_else(|| FfbError::Device("CreateEffect returned no effect".into()))?;
            effect
                .Start(1, 0)
                .map_err(|e| device_error("Effect::Start", e))?;
            Ok(effect)
        }
    }

    fn send_magnitude(&mut self, magnitude: i32) -> Result<(), FfbError> {
        let Some(effect) = self.effect.as_ref() else {
            return Ok(());
        };
        // SAFETY: only the type-specific block is read by `SetParameters`
        // with this flag, and it points at a local that outlives the call.
        unsafe {
            let mut force = DICONSTANTFORCE {
                lMagnitude: magnitude,
            };
            let mut params = DIEFFECT {
                dwSize: std::mem::size_of::<DIEFFECT>() as u32,
                cbTypeSpecificParams: std::mem::size_of::<DICONSTANTFORCE>() as u32,
                lpvTypeSpecificParams: &mut force as *mut DICONSTANTFORCE as *mut c_void,
                ..DIEFFECT::default()
            };
            let flags = DIEP_TYPESPECIFICPARAMS | DIEP_START;
            match effect.SetParameters(&mut params, flags) {
                Ok(()) => {}
                Err(e) if e.code() == DIERR_INPUTLOST || e.code() == DIERR_NOTACQUIRED => {
                    // Another application took the device; take it back once.
                    self.device
                        .Acquire()
                        .map_err(|e| device_error("Acquire", e))?;
                    effect
                        .SetParameters(&mut params, flags)
                        .map_err(|e| device_error("Effect::SetParameters", e))?;
                }
                Err(e) => return Err(device_error("Effect::SetParameters", e)),
            }
        }
        self.last_magnitude = magnitude;
        Ok(())
    }
}

/// Creates the hidden top-level window DirectInput needs for its
/// cooperative level when the caller has none to offer.
fn hidden_window() -> Result<HWND, FfbError> {
    // SAFETY: plain window creation with static class and title strings.
    unsafe {
        let module = GetModuleHandleW(None).map_err(|e| device_error("GetModuleHandle", e))?;
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("device input"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            HINSTANCE(module.0),
            None,
        )
        .map_err(|e| device_error("CreateWindow (hidden)", e))
    }
}

/// Size of the state block of [`DInputReader`]: eight 4-byte axes, then
/// one byte per button.
const READER_STATE_BYTES: usize = INPUT_AXES * 4 + INPUT_BUTTONS;

/// A device opened for reading every axis and button, without exclusive
/// access. Several readers and one force-feedback device can share a
/// physical device.
pub struct DInputReader {
    info: DeviceInfo,
    axes_present: [bool; INPUT_AXES],
    buttons: usize,
    device: IDirectInputDevice8W,
    _direct_input: IDirectInput8W,
    window: Option<HWND>,
}

// SAFETY: as for `DInputDevice`: DirectInput device objects may be called
// from any thread, and this type is only used through `&mut self`.
unsafe impl Send for DInputReader {}

impl DInputReader {
    /// Opens a device by the identifier [`enumerate`] returned.
    pub fn open(id: &str) -> Result<Self, FfbError> {
        let guid = id_to_guid(id).ok_or_else(|| FfbError::NotFound(id.to_string()))?;
        let di = create_direct_input()?;
        let (_, name) = enumerate_with(&di, DIEDFL_ATTACHEDONLY)?
            .into_iter()
            .find(|(g, _)| *g == guid)
            .ok_or_else(|| FfbError::NotFound(id.to_string()))?;
        // SAFETY: every pointer passed below refers to a local or a static
        // that outlives the call it is passed to.
        unsafe {
            let mut created: Option<IDirectInputDevice8W> = None;
            di.CreateDevice(&guid, &mut created, None)
                .map_err(|e| device_error("CreateDevice", e))?;
            let device = created
                .ok_or_else(|| FfbError::Device("CreateDevice returned no device".into()))?;

            // Eight optional axes, then optional buttons, one byte each.
            let axis_guids: [&GUID; INPUT_AXES] = [
                &GUID_X_AXIS,
                &GUID_Y_AXIS,
                &GUID_Z_AXIS,
                &GUID_RX_AXIS,
                &GUID_RY_AXIS,
                &GUID_RZ_AXIS,
                &GUID_SLIDER,
                &GUID_SLIDER,
            ];
            let mut objects: Vec<DIOBJECTDATAFORMAT> =
                Vec::with_capacity(INPUT_AXES + INPUT_BUTTONS);
            for (i, g) in axis_guids.iter().enumerate() {
                objects.push(DIOBJECTDATAFORMAT {
                    pguid: *g,
                    dwOfs: (i * 4) as u32,
                    dwType: DIDFT_AXIS | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                    dwFlags: 0,
                });
            }
            for b in 0..INPUT_BUTTONS {
                objects.push(DIOBJECTDATAFORMAT {
                    pguid: std::ptr::null(),
                    dwOfs: (INPUT_AXES * 4 + b) as u32,
                    dwType: DIDFT_BUTTON | DIDFT_ANYINSTANCE | DIDFT_OPTIONAL,
                    dwFlags: 0,
                });
            }
            let mut format = DIDATAFORMAT {
                dwSize: std::mem::size_of::<DIDATAFORMAT>() as u32,
                dwObjSize: std::mem::size_of::<DIOBJECTDATAFORMAT>() as u32,
                dwFlags: DIDF_ABSAXIS,
                dwDataSize: READER_STATE_BYTES as u32,
                dwNumObjs: objects.len() as u32,
                rgodf: objects.as_mut_ptr(),
            };
            device
                .SetDataFormat(&mut format)
                .map_err(|e| device_error("SetDataFormat", e))?;
            let window = hidden_window()?;
            let result = (|| {
                device
                    .SetCooperativeLevel(window, DISCL_NONEXCLUSIVE | DISCL_BACKGROUND)
                    .map_err(|e| device_error("SetCooperativeLevel (shared, background)", e))?;
                // One range for every axis on the device.
                let mut range = DIPROPRANGE {
                    diph: DIPROPHEADER {
                        dwSize: std::mem::size_of::<DIPROPRANGE>() as u32,
                        dwHeaderSize: std::mem::size_of::<DIPROPHEADER>() as u32,
                        dwObj: 0,
                        dwHow: DIPH_DEVICE,
                    },
                    lMin: -AXIS_MAX,
                    lMax: AXIS_MAX,
                };
                device
                    .SetProperty(DIPROP_RANGE, &mut range.diph)
                    .map_err(|e| device_error("SetProperty(range)", e))?;
                device.Acquire().map_err(|e| device_error("Acquire", e))
            })();
            if let Err(e) = result {
                let _ = DestroyWindow(window);
                return Err(e);
            }
            // Which slots of the format the device filled.
            let exists = |offset: usize| {
                let mut object = DIDEVICEOBJECTINSTANCEW {
                    dwSize: std::mem::size_of::<DIDEVICEOBJECTINSTANCEW>() as u32,
                    ..DIDEVICEOBJECTINSTANCEW::default()
                };
                device
                    .GetObjectInfo(&mut object, offset as u32, DIPH_BYOFFSET)
                    .is_ok()
            };
            let mut axes_present = [false; INPUT_AXES];
            for (i, present) in axes_present.iter_mut().enumerate() {
                *present = exists(i * 4);
            }
            let buttons = (0..INPUT_BUTTONS)
                .filter(|b| exists(INPUT_AXES * 4 + b))
                .count();
            Ok(Self {
                info: DeviceInfo {
                    id: id.to_string(),
                    name,
                    has_force_feedback: false,
                    backend: BACKEND,
                },
                axes_present,
                buttons,
                device,
                _direct_input: di,
                window: Some(window),
            })
        }
    }
}

impl InputSource for DInputReader {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn axes_present(&self) -> [bool; INPUT_AXES] {
        self.axes_present
    }

    fn button_count(&self) -> usize {
        self.buttons
    }

    fn read_state(&mut self) -> Result<RawInputState, FfbError> {
        let mut raw = [0u8; READER_STATE_BYTES];
        // SAFETY: the buffer is exactly the size of the registered format.
        unsafe {
            let _ = self.device.Poll();
            let ptr = raw.as_mut_ptr() as *mut c_void;
            let mut result = self.device.GetDeviceState(READER_STATE_BYTES as u32, ptr);
            if let Err(e) = &result {
                if e.code() == DIERR_INPUTLOST || e.code() == DIERR_NOTACQUIRED {
                    self.device
                        .Acquire()
                        .map_err(|e| device_error("Acquire", e))?;
                    result = self.device.GetDeviceState(READER_STATE_BYTES as u32, ptr);
                }
            }
            result.map_err(|e| device_error("GetDeviceState", e))?;
        }
        let mut state = RawInputState::default();
        for (i, axis) in state.axes.iter_mut().enumerate() {
            let v =
                i32::from_le_bytes([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]]);
            *axis = (v as f32 / AXIS_MAX as f32).clamp(-1.0, 1.0);
        }
        for b in 0..INPUT_BUTTONS {
            if raw[INPUT_AXES * 4 + b] & 0x80 != 0 {
                state.buttons |= 1u128 << b;
            }
        }
        Ok(state)
    }
}

impl Drop for DInputReader {
    fn drop(&mut self) {
        // SAFETY: plain COM and window calls on objects this struct owns.
        unsafe {
            let _ = self.device.Unacquire();
            if let Some(window) = self.window.take() {
                let _ = DestroyWindow(window);
            }
        }
    }
}

impl FfbDevice for DInputDevice {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn set_torque(&mut self, normalized: f32) -> Result<(), FfbError> {
        let n = if normalized.is_finite() {
            normalized.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let magnitude = (n * DI_FFNOMINALMAX as f32).round() as i32;
        if magnitude == self.last_magnitude {
            return Ok(());
        }
        self.send_magnitude(magnitude)
    }

    fn read_steering(&mut self) -> Result<Option<f32>, FfbError> {
        // SAFETY: the state buffer matches the 4-byte data format set in
        // `open`.
        unsafe {
            let _ = self.device.Poll();
            let mut axis: i32 = 0;
            let mut result = self
                .device
                .GetDeviceState(4, &mut axis as *mut i32 as *mut c_void);
            if let Err(e) = &result {
                if e.code() == DIERR_INPUTLOST || e.code() == DIERR_NOTACQUIRED {
                    self.device
                        .Acquire()
                        .map_err(|e| device_error("Acquire", e))?;
                    result = self
                        .device
                        .GetDeviceState(4, &mut axis as *mut i32 as *mut c_void);
                }
            }
            result.map_err(|e| device_error("GetDeviceState", e))?;
            Ok(Some((axis as f32 / AXIS_MAX as f32).clamp(-1.0, 1.0)))
        }
    }

    fn stop(&mut self) -> Result<(), FfbError> {
        self.send_magnitude(0)
    }
}

impl Drop for DInputDevice {
    fn drop(&mut self) {
        // SAFETY: plain COM calls on objects this struct owns.
        unsafe {
            if let Some(effect) = self.effect.take() {
                let _ = effect.Stop();
                let _ = effect.Unload();
            }
            let _ = self.device.Unacquire();
            if let Some(window) = self.owned_window.take() {
                let _ = DestroyWindow(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_round_trip() {
        let g = GUID::from_u128(0x0123_4567_89ab_cdef_0011_2233_4455_6677);
        assert_eq!(id_to_guid(&guid_to_id(&g)), Some(g));
        assert_eq!(id_to_guid("not hex"), None);
    }

    #[test]
    fn struct_sizes_match_the_directinput_abi() {
        // Sizes from dinput.h on 64-bit Windows. A mismatch would make
        // DirectInput reject the structures.
        if cfg!(target_pointer_width = "64") {
            assert_eq!(std::mem::size_of::<DIDATAFORMAT>(), 32);
            assert_eq!(std::mem::size_of::<DIOBJECTDATAFORMAT>(), 24);
            assert_eq!(std::mem::size_of::<DIEFFECT>(), 80);
        }
        assert_eq!(std::mem::size_of::<DIPROPHEADER>(), 16);
        assert_eq!(std::mem::size_of::<DIPROPDWORD>(), 20);
        assert_eq!(std::mem::size_of::<DIPROPRANGE>(), 24);
        assert_eq!(std::mem::size_of::<DICONSTANTFORCE>(), 4);
    }

    #[test]
    fn enumeration_runs_without_a_wheel() {
        // With no controller attached this returns an empty list; with one
        // attached it must not fail either.
        let devices = enumerate().expect("DirectInput should be available on Windows");
        for d in devices {
            assert_eq!(d.backend, BACKEND);
            assert_eq!(d.id.len(), 32);
        }
    }

    #[test]
    fn every_attached_device_can_be_read_without_exclusive_access() {
        for d in enumerate().unwrap() {
            let mut reader = DInputReader::open(&d.id).expect("open for reading");
            let state = reader.read_state().expect("read");
            assert!(state.axes.iter().all(|a| a.is_finite() && a.abs() <= 1.0));
            // A second reader of the same device works alongside the first.
            let mut second = DInputReader::open(&d.id).expect("second reader");
            second.read_state().expect("read again");
        }
    }

    #[test]
    fn opening_an_unknown_device_reports_not_found() {
        assert!(matches!(
            DInputDevice::open("00000000000000000000000000000001", None),
            Err(FfbError::NotFound(_))
        ));
        assert!(matches!(
            DInputDevice::open("zz", None),
            Err(FfbError::NotFound(_))
        ));
    }
}
