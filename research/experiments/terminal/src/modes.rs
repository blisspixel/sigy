//! Console mode and font reads.
//!
//! Win32 exposes these only through handles. The calls stay in this module.

#![allow(unsafe_code)]

use std::io;

use windows_sys::Win32::System::Console::{
    CONSOLE_FONT_INFOEX, GetConsoleCP, GetConsoleMode, GetConsoleOutputCP, GetCurrentConsoleFontEx,
    GetStdHandle, INPUT_RECORD, KEY_EVENT, KEY_EVENT_RECORD, KEY_EVENT_RECORD_0, STD_INPUT_HANDLE,
    STD_OUTPUT_HANDLE, WriteConsoleInputW,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleModes {
    pub input: u32,
    pub output: u32,
    pub input_cp: u32,
    pub output_cp: u32,
}

pub fn current() -> io::Result<ConsoleModes> {
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        if input.is_null() || output.is_null() {
            return Err(io::Error::other("console handle is unavailable"));
        }
        let mut input_mode = 0;
        let mut output_mode = 0;
        if GetConsoleMode(input, &mut input_mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        if GetConsoleMode(output, &mut output_mode) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ConsoleModes {
            input: input_mode,
            output: output_mode,
            input_cp: GetConsoleCP(),
            output_cp: GetConsoleOutputCP(),
        })
    }
}

pub fn font_face() -> Option<String> {
    unsafe {
        let output = GetStdHandle(STD_OUTPUT_HANDLE);
        if output.is_null() {
            return None;
        }
        let mut info: CONSOLE_FONT_INFOEX = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32;
        if GetCurrentConsoleFontEx(output, 0, &mut info) == 0 {
            return None;
        }
        let length = info
            .FaceName
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(info.FaceName.len());
        let face = String::from_utf16_lossy(&info.FaceName[..length]);
        if face.is_empty() { None } else { Some(face) }
    }
}

pub fn inject_chars(text: &str) -> io::Result<()> {
    for character in text.chars() {
        let value = u16::try_from(u32::from(character)).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "character does not fit in UTF-16",
            )
        })?;
        inject_utf16(value)?;
    }
    Ok(())
}

fn ascii_virtual_key(character: u16) -> u16 {
    const LOWER_A: u16 = b'a' as u16;
    const LOWER_Z: u16 = b'z' as u16;
    const UPPER_A: u16 = b'A' as u16;
    const UPPER_Z: u16 = b'Z' as u16;
    match character {
        value @ LOWER_A..=LOWER_Z => value - (LOWER_A - UPPER_A),
        value @ UPPER_A..=UPPER_Z => value,
        _ => 0,
    }
}

fn inject_utf16(character: u16) -> io::Result<()> {
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE);
        if input.is_null() {
            return Err(io::Error::other("console input is unavailable"));
        }
        let mut record: INPUT_RECORD = std::mem::zeroed();
        record.EventType = u16::try_from(KEY_EVENT).unwrap_or(1);
        record.Event.KeyEvent = KEY_EVENT_RECORD {
            bKeyDown: 1,
            wRepeatCount: 1,
            wVirtualKeyCode: ascii_virtual_key(character),
            wVirtualScanCode: 0,
            uChar: KEY_EVENT_RECORD_0 {
                UnicodeChar: character,
            },
            dwControlKeyState: 0,
        };
        let mut written = 0;
        if WriteConsoleInputW(input, &record, 1, &mut written) == 0 || written != 1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
