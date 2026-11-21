use std::thread;
use std::time::Duration;

use ol_platform_api::{Clipboard, ClipboardError, Zeroizing};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::core::PCWSTR;

use crate::wide;

const CF_UNICODETEXT: u32 = 13;
// Each set to a DWORD of zero keeps a copy out of the clipboard history (Win+V) and the cloud clipboard.
pub const PRIVATE_FORMATS: [&str; 3] = [
    "ExcludeClipboardContentFromMonitorProcessing",
    "CanIncludeInClipboardHistory",
    "CanUploadToCloudClipboard",
];
// A clipboard manager holds the clipboard open for a moment after each copy.
const TRIES: u32 = 10;
const PAUSE: Duration = Duration::from_millis(15);

// Opened with no window as its owner: nothing is rendered later on request.
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsClipboard;

struct Opened(());

impl Opened {
    fn open() -> Result<Self, ClipboardError> {
        for attempt in 1..=TRIES {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Self(()));
            }
            if attempt < TRIES {
                thread::sleep(PAUSE);
            }
        }
        Err(ClipboardError(
            "another program holds the clipboard open".to_owned(),
        ))
    }

    fn empty(&self) -> Result<(), ClipboardError> {
        unsafe { EmptyClipboard() }
            .map_err(|_| ClipboardError("Windows would not empty the clipboard".to_owned()))
    }

    fn set(
        &self,
        format: u32,
        length: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<(), ClipboardError> {
        let refused = || ClipboardError("Windows would not take the copy".to_owned());
        let block =
            unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, length) }.map_err(|_| refused())?;
        let at = unsafe { GlobalLock(block) }.cast::<u8>();
        if at.is_null() {
            let _ = unsafe { GlobalFree(Some(block)) };
            return Err(refused());
        }
        fill(unsafe { std::slice::from_raw_parts_mut(at, length) });
        let _ = unsafe { GlobalUnlock(block) };
        if unsafe { SetClipboardData(format, Some(HANDLE(block.0))) }.is_err() {
            let _ = unsafe { GlobalFree(Some(block)) };
            return Err(refused());
        }
        Ok(())
    }

    fn set_text(&self, text: &str) -> Result<(), ClipboardError> {
        let units = text.encode_utf16().count() + 1;
        self.set(CF_UNICODETEXT, units * 2, |bytes| {
            write_utf16(text, bytes);
        })
    }

    fn text(&self) -> Result<Option<Zeroizing<String>>, ClipboardError> {
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
            return Ok(None);
        }
        let unreadable =
            || ClipboardError("Windows would not hand over the clipboard's text".to_owned());
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.map_err(|_| unreadable())?;
        let block = HGLOBAL(handle.0);
        let length = unsafe { GlobalSize(block) };
        let at = unsafe { GlobalLock(block) }.cast::<u16>().cast_const();
        if at.is_null() {
            return Err(unreadable());
        }
        let text = read_utf16(unsafe { std::slice::from_raw_parts(at, length / 2) });
        let _ = unsafe { GlobalUnlock(block) };
        Ok(Some(text))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

pub fn write_utf16(text: &str, bytes: &mut [u8]) {
    for (unit, at) in text
        .encode_utf16()
        .chain(std::iter::once(0))
        .zip(bytes.as_chunks_mut::<2>().0)
    {
        *at = unit.to_le_bytes();
    }
}

// Full room at once, so a growing string leaves no partial copy behind in freed memory.
pub fn read_utf16(units: &[u16]) -> Zeroizing<String> {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let mut text = Zeroizing::new(String::with_capacity(end * 3));
    text.extend(
        char::decode_utf16(units[..end].iter().copied())
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)),
    );
    text
}

pub fn private_format(name: &str) -> Option<u32> {
    let name = wide(name);
    let format = unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) };
    (format != 0).then_some(format)
}

fn private_formats() -> Result<[u32; 3], ClipboardError> {
    let mut formats = [0; 3];
    for (format, name) in formats.iter_mut().zip(PRIVATE_FORMATS) {
        *format = private_format(name).ok_or_else(|| {
            ClipboardError(format!("Windows does not know the clipboard format {name}"))
        })?;
    }
    Ok(formats)
}

impl Clipboard for WindowsClipboard {
    fn write_private(&self, text: &str) -> Result<(), ClipboardError> {
        let formats = private_formats()?;
        let clipboard = Opened::open()?;
        clipboard.empty()?;
        // The history hears of the clipboard when it is closed, so the marks go on first and a failed write takes the text off again.
        let written = formats
            .iter()
            .try_for_each(|format| clipboard.set(*format, 4, |_| {}))
            .and_then(|()| clipboard.set_text(text));
        if written.is_err() {
            let _ = clipboard.empty();
        }
        written
    }

    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        let clipboard = Opened::open()?;
        clipboard.empty()?;
        clipboard.set_text(text)
    }

    fn read_text(&self) -> Result<Option<Zeroizing<String>>, ClipboardError> {
        Opened::open()?.text()
    }

    fn clear_if(&self, holds: &dyn Fn(&str) -> bool) -> Result<bool, ClipboardError> {
        let clipboard = Opened::open()?;
        let Some(text) = clipboard.text()? else {
            return Ok(false);
        };
        if !holds(&text) {
            return Ok(false);
        }
        clipboard.empty()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_goes_out_as_utf16_with_its_nul_and_comes_back_up_to_the_first_nul() {
        let text = "pässwörd-😀";
        let units = text.encode_utf16().count() + 1;
        let mut bytes = vec![0xee; units * 2];
        write_utf16(text, &mut bytes);
        assert_eq!(&bytes[bytes.len() - 2..], [0, 0]);
        let back: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .chain([0x41, 0])
            .collect();
        assert_eq!(read_utf16(&back).as_str(), text);
        assert_eq!(read_utf16(&[0x41, 0x42]).as_str(), "AB");
        assert_eq!(read_utf16(&[0xd800, 0x41, 0]).as_str(), "\u{fffd}A");
    }

    // Registering a format name leaves the clipboard as it is.
    #[test]
    fn the_three_formats_that_keep_a_copy_private_are_known_to_windows() {
        let first: Vec<Option<u32>> = PRIVATE_FORMATS
            .iter()
            .map(|name| private_format(name))
            .collect();
        assert!(
            first
                .iter()
                .all(|format| format.is_some_and(|format| format >= 0xc000))
        );
        let again: Vec<Option<u32>> = PRIVATE_FORMATS
            .iter()
            .map(|name| private_format(name))
            .collect();
        assert_eq!(first, again);
        assert_ne!(first[0], first[1]);
        assert_ne!(first[1], first[2]);
    }
}
