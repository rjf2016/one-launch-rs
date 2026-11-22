use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use gpui::{App, PathPromptOptions, Task};

#[derive(Clone)]
pub enum FilePicker {
    Windows,
    // Agents run captures and probes, which must never put a dialog on the owner's desktop.
    Scripted(Rc<RefCell<Script>>),
}

impl gpui::Global for FilePicker {}

#[derive(Default)]
pub struct Script {
    answers: VecDeque<Option<String>>,
    asked: usize,
}

impl Script {
    pub fn answer(&mut self, answer: Option<String>) {
        self.answers.push_back(answer);
    }

    pub fn asked(&self) -> usize {
        self.asked
    }

    fn next(&mut self) -> Picked {
        self.asked += 1;
        match self.answers.pop_front().flatten() {
            Some(path) => Picked::Path(path),
            None => Picked::Cancelled,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Picked {
    Path(String),
    Cancelled,
    Failed(String),
}

enum Ask {
    Dialog,
    Answered(Picked),
}

fn ask(picker: Option<&FilePicker>) -> Ask {
    match picker {
        Some(FilePicker::Windows) => Ask::Dialog,
        Some(FilePicker::Scripted(script)) => Ask::Answered(script.borrow_mut().next()),
        None => Ask::Answered(Picked::Cancelled),
    }
}

pub(crate) fn pick_program(cx: &App) -> Task<Picked> {
    pick_file(cx)
}

// The picture for "Choose portrait image…". GPUI's dialog takes no filter, so it lists every file; what is chosen is checked as a picture before it is kept.
pub(crate) fn pick_image(cx: &App) -> Task<Picked> {
    pick_file(cx)
}

fn pick_file(cx: &App) -> Task<Picked> {
    match ask(cx.try_global::<FilePicker>()) {
        Ask::Answered(picked) => Task::ready(picked),
        Ask::Dialog => {
            let chosen = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: None,
            });
            cx.foreground_executor().spawn(async move {
                match chosen.await {
                    Ok(Ok(Some(paths))) => {
                        paths.into_iter().next().map_or(Picked::Cancelled, |path| {
                            Picked::Path(path.to_string_lossy().into_owned())
                        })
                    }
                    Ok(Ok(None)) | Err(_) => Picked::Cancelled,
                    Ok(Err(error)) => Picked::Failed(error.to_string()),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered(picker: Option<&FilePicker>) -> Picked {
        match ask(picker) {
            Ask::Answered(picked) => picked,
            Ask::Dialog => panic!("the dialog was asked for"),
        }
    }

    #[test]
    fn a_scripted_picker_answers_in_order_and_cancels_once_it_has_no_answer_left() {
        let script = Rc::new(RefCell::new(Script::default()));
        script
            .borrow_mut()
            .answer(Some(r"D:\x\steam.exe".to_owned()));
        script.borrow_mut().answer(None);
        let picker = FilePicker::Scripted(Rc::clone(&script));
        assert_eq!(
            answered(Some(&picker)),
            Picked::Path(r"D:\x\steam.exe".to_owned())
        );
        assert_eq!(answered(Some(&picker)), Picked::Cancelled);
        assert_eq!(answered(Some(&picker)), Picked::Cancelled);
        assert_eq!(script.borrow().asked(), 3);
    }

    #[test]
    fn only_the_windows_picker_asks_for_the_dialog() {
        assert!(matches!(ask(Some(&FilePicker::Windows)), Ask::Dialog));
        assert_eq!(answered(None), Picked::Cancelled);
    }
}
