pub mod data_folder;
pub mod dpapi;
pub mod own_process;
pub mod probe;
pub mod processes;
pub mod registry;
pub mod tray;
pub mod window;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
