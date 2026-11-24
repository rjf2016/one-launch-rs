use std::thread;
use std::time::Duration;

use ol_platform_api::{Clipboard, ClipboardError, Zeroizing};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::{PCWSTR, w};
use zeroize::Zeroize as _;

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

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsClipboard;

// Windows documents that SetClipboardData fails after EmptyClipboard on a clipboard opened with no owner; the copy outlives this window because nothing is rendered later on request.
struct Opened {
    owner: HWND,
}

impl Opened {
    fn open() -> Result<Self, ClipboardError> {
        let owner = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|_| {
            ClipboardError::Refused(
                "Windows would not make the window that holds the clipboard".to_owned(),
            )
        })?;
        for attempt in 1..=TRIES {
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Ok(Self { owner });
            }
            if attempt < TRIES {
                thread::sleep(PAUSE);
            }
        }
        let _ = unsafe { DestroyWindow(owner) };
        Err(ClipboardError::Busy)
    }

    fn empty(&self) -> Result<(), ClipboardError> {
        unsafe { EmptyClipboard() }.map_err(|_| {
            ClipboardError::Refused("Windows would not empty the clipboard".to_owned())
        })
    }

    fn set(
        &self,
        format: u32,
        length: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<(), ClipboardError> {
        let refused = || ClipboardError::Refused("Windows would not take the copy".to_owned());
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
            let at = unsafe { GlobalLock(block) }.cast::<u8>();
            if !at.is_null() {
                unsafe { std::slice::from_raw_parts_mut(at, length) }.zeroize();
                let _ = unsafe { GlobalUnlock(block) };
            }
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
        self.with_units(read_utf16)
    }

    // Hands the clipboard's text, as Windows keeps it, to `read` without a copy.
    fn with_units<T>(&self, read: impl FnOnce(&[u16]) -> T) -> Result<Option<T>, ClipboardError> {
        if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT) }.is_err() {
            return Ok(None);
        }
        let unreadable = || {
            ClipboardError::Refused("Windows would not hand over the clipboard's text".to_owned())
        };
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) }.map_err(|_| unreadable())?;
        let block = HGLOBAL(handle.0);
        let length = unsafe { GlobalSize(block) };
        let at = unsafe { GlobalLock(block) }.cast::<u16>().cast_const();
        if at.is_null() {
            return Err(unreadable());
        }
        let read = read(unsafe { std::slice::from_raw_parts(at, length / 2) });
        let _ = unsafe { GlobalUnlock(block) };
        Ok(Some(read))
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
        let _ = unsafe { DestroyWindow(self.owner) };
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

// The exact room at once, so a growing string leaves no partial copy behind in freed memory.
pub fn read_utf16(units: &[u16]) -> Zeroizing<String> {
    let end = units
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(units.len());
    let letters = || {
        char::decode_utf16(units[..end].iter().copied())
            .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
    };
    let mut text = Zeroizing::new(String::with_capacity(letters().map(char::len_utf8).sum()));
    text.extend(letters());
    text
}

// The text only when it is `length` units long: no more of the user's own text is read than the copy it is compared with.
pub fn read_utf16_of_length(units: &[u16], length: usize) -> Option<Zeroizing<String>> {
    let ends = units.get(length).is_none_or(|unit| *unit == 0);
    let text = units.get(..length)?;
    (ends && !text.contains(&0)).then(|| read_utf16(text))
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
            ClipboardError::Unmarked(format!("Windows does not know the clipboard format {name}"))
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
            .map_err(|_| {
                ClipboardError::Unmarked(
                    "Windows would not take the marks that keep the copy out of the clipboard history"
                        .to_owned(),
                )
            })
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

    fn clear_if(
        &self,
        length: usize,
        holds: &dyn Fn(&str) -> bool,
    ) -> Result<bool, ClipboardError> {
        let clipboard = Opened::open()?;
        let Some(text) = clipboard
            .with_units(|units| read_utf16_of_length(units, length))?
            .flatten()
        else {
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

    #[test]
    fn a_text_of_another_length_is_not_read() {
        let units: Vec<u16> = "abcd".encode_utf16().chain([0, 0x41]).collect();
        assert_eq!(
            read_utf16_of_length(&units, 4)
                .as_deref()
                .map(String::as_str),
            Some("abcd")
        );
        assert!(read_utf16_of_length(&units, 3).is_none());
        assert!(read_utf16_of_length(&units, 5).is_none());
        assert!(read_utf16_of_length(&units, 50).is_none());
        assert!(read_utf16_of_length(&units[..4], 4).is_some());
        let longer: Vec<u16> = "abcdef".encode_utf16().collect();
        assert!(read_utf16_of_length(&longer, 4).is_none());
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

    const MADE_UP: &str = "one-launch-clipboard-check-example";
    // Text and what Windows makes of it by itself: anything else could not be put back.
    const PLAIN: [u32; 4] = [1, 7, CF_UNICODETEXT, 16];

    // What the clipboard held, put back however the test ends. It is never printed or compared.
    struct PutBack(Option<Zeroizing<String>>);

    impl Drop for PutBack {
        fn drop(&mut self) {
            let put_back = match &self.0 {
                Some(earlier) => WindowsClipboard.write_text(earlier),
                None => Opened::open().and_then(|clipboard| clipboard.empty()),
            };
            assert!(put_back.is_ok(), "the clipboard could not be put back");
        }
    }

    // None when the clipboard holds more than plain text.
    fn plain_text_or_nothing() -> Option<Option<Zeroizing<String>>> {
        let clipboard = Opened::open().expect("the clipboard opens");
        let mut format = 0;
        loop {
            format = unsafe { windows::Win32::System::DataExchange::EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if !PLAIN.contains(&format) {
                return None;
            }
        }
        Some(clipboard.text().expect("the clipboard's text reads"))
    }

    fn holds_made_up(board: &WindowsClipboard) -> bool {
        board
            .read_text()
            .is_ok_and(|held| held.is_some_and(|held| held.as_str() == MADE_UP))
    }

    fn private_marks() -> [bool; 3] {
        let formats = private_formats().unwrap();
        let _clipboard = Opened::open().unwrap();
        formats.map(|format| unsafe { IsClipboardFormatAvailable(format) }.is_ok())
    }

    #[test]
    #[ignore = "writes the clipboard of this PC: run once by hand with --ignored"]
    fn the_real_clipboard_takes_a_private_and_a_plain_copy_and_clears_one() {
        let Some(earlier) = plain_text_or_nothing() else {
            println!("skipped: the clipboard holds more than plain text, and was left alone");
            return;
        };
        let _put_back = PutBack(earlier);
        let board = WindowsClipboard;
        let length = MADE_UP.encode_utf16().count();

        board.write_private(MADE_UP).unwrap();
        assert!(holds_made_up(&board), "the private write did not read back");
        assert_eq!(private_marks(), [true; 3]);

        board.write_text(MADE_UP).unwrap();
        assert!(holds_made_up(&board), "the plain write did not read back");
        assert_eq!(private_marks(), [false; 3]);

        assert!(!board.clear_if(length, &|held| held == "another").unwrap());
        assert!(!board.clear_if(length + 1, &|_| true).unwrap());
        assert!(board.read_text().unwrap().is_some());
        assert!(board.clear_if(length, &|held| held == MADE_UP).unwrap());
        assert!(board.read_text().unwrap().is_none());
        println!("passed: private and plain writes, their marks, and a clear");
    }
}
