// Windows' own message boxes for what the app tells before or beside its window: that it could not start, and that it hit an error it cannot carry on from. Task dialogs, as the Electron app's are.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOG_COMMON_BUTTON_FLAGS, TASKDIALOGCONFIG, TD_ERROR_ICON,
    TDCBF_OK_BUTTON, TDF_ALLOW_DIALOG_CANCELLATION, TaskDialogIndirect,
};
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;
use windows::core::PCWSTR;

use crate::wide;

const FIRST_BUTTON: i32 = 100;

pub struct MessageBox<'a> {
    pub title: &'a str,
    pub instruction: &'a str,
    pub content: &'a str,
    // None: Windows' own OK. Otherwise the buttons in order; the last answers Escape and the window's close.
    pub buttons: Option<&'a [&'a str]>,
}

// Shows the box on this thread, with no owner window, and waits for it. Answers the index of the button pressed, Escape and the close answering the last; None when Windows could not show it.
pub fn show(message: &MessageBox<'_>) -> Option<usize> {
    let title = wide(message.title);
    let instruction = wide(message.instruction);
    let content = wide(message.content);
    let labels: Vec<Vec<u16>> = message
        .buttons
        .unwrap_or_default()
        .iter()
        .map(|label| wide(label))
        .collect();
    let last = labels.len().saturating_sub(1);
    let ids: Vec<i32> = (0..labels.len())
        .map(|at| {
            if at == last {
                IDCANCEL.0
            } else {
                FIRST_BUTTON + at as i32
            }
        })
        .collect();
    let buttons: Vec<TASKDIALOG_BUTTON> = labels
        .iter()
        .zip(&ids)
        .map(|(label, id)| TASKDIALOG_BUTTON {
            nButtonID: *id,
            pszButtonText: PCWSTR(label.as_ptr()),
        })
        .collect();
    let mut config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: HWND::default(),
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        dwCommonButtons: if buttons.is_empty() {
            TDCBF_OK_BUTTON
        } else {
            TASKDIALOG_COMMON_BUTTON_FLAGS(0)
        },
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: ids.first().copied().unwrap_or_default(),
        ..Default::default()
    };
    config.Anonymous1.pszMainIcon = TD_ERROR_ICON;
    let mut pressed = 0_i32;
    unsafe { TaskDialogIndirect(&config, Some(&raw mut pressed), None, None) }.ok()?;
    if buttons.is_empty() {
        return Some(0);
    }
    ids.iter().position(|id| *id == pressed).or(Some(last))
}
