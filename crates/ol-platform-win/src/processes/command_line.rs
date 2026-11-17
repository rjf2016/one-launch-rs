use ol_platform_api::ProcessArg;
use zeroize::Zeroizing;

const QUOTE: u16 = b'"' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SPACE: u16 = b' ' as u16;

// Why a command line could not be made. Neither names an argument: one can be a password.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unwritable {
    QuotedProgram,
    Nul,
}

// The program in quotes, then each argument as the C runtime splits them again. A password is copied into this buffer and nowhere else, and the buffer is wiped when it is dropped.
pub(crate) fn command_line(
    exe: &str,
    args: &[ProcessArg<'_>],
) -> Result<Zeroizing<Vec<u16>>, Unwritable> {
    if exe.contains('"') {
        return Err(Unwritable::QuotedProgram);
    }
    // Room for the worst case up front: a buffer that grew would leave a copy of the password behind, unwiped.
    let room = exe.encode_utf16().count()
        + 3
        + args
            .iter()
            .map(|(ProcessArg::Plain(text) | ProcessArg::Secret(text))| {
                3 * text.encode_utf16().count() + 4
            })
            .sum::<usize>();
    let mut line = Zeroizing::new(Vec::with_capacity(room));
    line.push(QUOTE);
    line.extend(exe.encode_utf16());
    line.push(QUOTE);
    for arg in args {
        let (ProcessArg::Plain(text) | ProcessArg::Secret(text)) = arg;
        if text.contains('\0') {
            return Err(Unwritable::Nul);
        }
        line.push(SPACE);
        push_argument(&mut line, text);
    }
    line.push(0);
    debug_assert!(line.capacity() == room, "the command line grew");
    Ok(line)
}

fn push_argument(line: &mut Vec<u16>, text: &str) {
    let quoted = text.is_empty() || text.contains([' ', '\t']);
    if quoted {
        line.push(QUOTE);
    }
    let mut backslashes = 0;
    for unit in text.encode_utf16() {
        if unit == BACKSLASH {
            backslashes += 1;
        } else {
            if unit == QUOTE {
                line.extend(std::iter::repeat_n(BACKSLASH, backslashes + 1));
            }
            backslashes = 0;
        }
        line.push(unit);
    }
    if quoted {
        line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
        line.push(QUOTE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(exe: &str, args: &[ProcessArg<'_>]) -> String {
        let line = command_line(exe, args).unwrap();
        String::from_utf16(&line[..line.len() - 1]).unwrap()
    }

    #[test]
    fn each_argument_is_quoted_so_that_it_splits_back_as_it_was() {
        assert_eq!(
            shown(r"C:\Program Files\Steam\steam.exe", &[]),
            r#""C:\Program Files\Steam\steam.exe""#
        );
        assert_eq!(
            shown(
                r"D:\x\a.exe",
                &[
                    ProcessArg::Plain("-shutdown"),
                    ProcessArg::Plain(""),
                    ProcessArg::Plain("two words"),
                    ProcessArg::Plain(r#"--exec="launch Pro""#),
                    ProcessArg::Plain(r"C:\folder\"),
                    ProcessArg::Plain(r"C:\folder with space\"),
                    ProcessArg::Plain(r#"a\"b"#),
                    ProcessArg::Secret("pass word"),
                ]
            ),
            r#""D:\x\a.exe" -shutdown "" "two words" "--exec=\"launch Pro\"" C:\folder\ "C:\folder with space\\" a\\\"b "pass word""#
        );
    }

    #[test]
    fn a_program_in_quotes_or_a_nul_is_refused_without_the_argument() {
        assert_eq!(
            command_line(r#"D:\x"\a.exe"#, &[]).unwrap_err(),
            Unwritable::QuotedProgram
        );
        let refused =
            command_line(r"D:\a.exe", &[ProcessArg::Secret("place\0holder")]).unwrap_err();
        assert_eq!(refused, Unwritable::Nul);
        assert!(!format!("{refused:?}").contains("place"));
    }
}
