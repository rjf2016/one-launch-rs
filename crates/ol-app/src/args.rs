use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub struct Args {
    pub data_dir: PathBuf,
    pub check: Option<Check>,
}

#[derive(Debug, PartialEq)]
pub enum Check {
    Capture(CapturePlan),
    Probe,
}

#[derive(Debug, Default, PartialEq)]
pub struct CapturePlan {
    pub file: PathBuf,
    pub size: Option<(i32, i32)>,
    pub sweep: Option<usize>,
    pub row: Option<usize>,
    pub text: Option<String>,
    pub keys: Vec<String>,
    pub focus: bool,
}

impl Args {
    pub fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let (mut data_dir, mut file, mut probe) = (None, None, false);
        let mut plan = CapturePlan::default();
        let mut steps = false;
        for argument in arguments {
            if argument == "--probe" {
                probe = true;
                continue;
            }
            if argument == "--capture-focus" {
                (plan.focus, steps) = (true, true);
                continue;
            }
            let Some((name, value)) = argument.split_once('=') else {
                return Err(if argument == "--data-dir" {
                    "write the data folder as --data-dir=<folder>, in one word".to_owned()
                } else {
                    format!("unknown argument {argument}")
                });
            };
            if value.is_empty() {
                return Err(format!("{name}= needs a value"));
            }
            let number = || {
                value
                    .parse::<usize>()
                    .map_err(|_| format!("{name}={value} is not a number"))
            };
            match name {
                "--data-dir" => data_dir = Some(PathBuf::from(value)),
                "--capture" => file = Some(PathBuf::from(value)),
                "--capture-size" => {
                    let size = value
                        .split_once('x')
                        .and_then(|(width, height)| {
                            Some((width.parse().ok()?, height.parse().ok()?))
                        })
                        .ok_or_else(|| format!("{name}={value} is not <width>x<height>"))?;
                    plan.size = Some(size);
                }
                "--capture-sweep" => plan.sweep = Some(number()?),
                "--capture-row" => plan.row = Some(number()?),
                "--capture-type" => plan.text = Some(value.to_owned()),
                "--capture-keys" => plan.keys = value.split(',').map(str::to_owned).collect(),
                _ => return Err(format!("unknown argument {name}")),
            }
            steps |= name.starts_with("--capture-");
        }
        let data_dir = data_dir.ok_or("refusing to start without --data-dir=<folder>")?;
        if !data_dir.is_absolute() {
            return Err("--data-dir=<folder> must be an absolute path".to_owned());
        }
        let check = match (file, probe) {
            (Some(_), true) => return Err("--capture and --probe do not go together".to_owned()),
            (Some(file), false) => Some(Check::Capture(CapturePlan { file, ..plan })),
            (None, _) if steps => {
                return Err("the --capture-... steps need --capture=<file.png>".to_owned());
            }
            (None, true) => Some(Check::Probe),
            (None, false) => None,
        };
        Ok(Self { data_dir, check })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Args, String> {
        Args::parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    #[test]
    fn a_data_folder_starts_the_app() {
        let args = parse(&[r"--data-dir=D:\scratch\proof"]).unwrap();
        assert_eq!(args.data_dir, PathBuf::from(r"D:\scratch\proof"));
        assert_eq!(args.check, None);
        let args = parse(&[r"--data-dir=D:\scratch\proof", "--probe"]).unwrap();
        assert_eq!(args.check, Some(Check::Probe));
    }

    #[test]
    fn no_data_folder_is_refused() {
        for arguments in [
            &[][..],
            &["--probe"],
            &[r"--capture=D:\shot.png"],
            &["--data-dir", r"D:\scratch\proof"],
            &["--data-dir="],
            &[r"--Data-Dir=D:\scratch\proof"],
            &[r"-data-dir=D:\scratch\proof"],
            &[r"/data-dir=D:\scratch\proof"],
            &["--data-dir=scratch"],
            &[r"--data-dir=\scratch"],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?}");
        }
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        for extra in ["--start-hidden", "--capture-speed=3", "proof", "--probe=1"] {
            assert!(
                parse(&[r"--data-dir=D:\scratch", extra]).is_err(),
                "{extra}"
            );
        }
    }

    #[test]
    fn capture_steps_need_a_file() {
        assert!(parse(&[r"--data-dir=D:\scratch", "--capture-row=3"]).is_err());
        assert!(parse(&[r"--data-dir=D:\scratch", "--capture-focus"]).is_err());
        assert!(parse(&[r"--data-dir=D:\scratch", r"--capture=D:\a.png", "--probe"]).is_err());
        let args = parse(&[
            r"--data-dir=D:\scratch",
            r"--capture=D:\a.png",
            "--capture-size=960x600",
            "--capture-row=3",
            "--capture-type=game 12",
            "--capture-keys=shift-home,ctrl-a",
        ])
        .unwrap();
        assert_eq!(
            args.check,
            Some(Check::Capture(CapturePlan {
                file: PathBuf::from(r"D:\a.png"),
                size: Some((960, 600)),
                sweep: None,
                row: Some(3),
                text: Some("game 12".to_owned()),
                keys: vec!["shift-home".to_owned(), "ctrl-a".to_owned()],
                focus: false,
            }))
        );
    }
}
