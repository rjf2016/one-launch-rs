use std::path::PathBuf;

const MOST: usize = 20_000;

#[derive(Debug, PartialEq)]
pub struct Args {
    pub data_dir: PathBuf,
    pub check: Option<Check>,
    pub doctor: Option<DoctorVault>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DoctorVault {
    OfThisUser,
    #[cfg(feature = "fake")]
    OfTheFixtures,
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
        let (mut doctor, mut fake_vault) = (false, false);
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
            if argument == "--doctor" {
                doctor = true;
                continue;
            }
            if cfg!(feature = "fake") && argument == "--fake-vault" {
                fake_vault = true;
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
                    .ok()
                    .filter(|number| *number <= MOST)
                    .ok_or_else(|| format!("{name}={value} is not a number up to {MOST}"))
            };
            // A folder written as "D:\x\" takes its closing quote, and every word after it, into the value.
            let path = || {
                if value.contains('"') {
                    return Err(format!(
                        "{name}= holds a quote; drop the last \\ of the path"
                    ));
                }
                Ok(PathBuf::from(value))
            };
            match name {
                "--data-dir" => data_dir = Some(path()?),
                "--capture" => file = Some(path()?),
                "--capture-size" => {
                    let side = |text: &str| text.parse::<u16>().ok().filter(|side| *side > 0);
                    let (width, height) = value
                        .split_once('x')
                        .and_then(|(width, height)| Some((side(width)?, side(height)?)))
                        .ok_or_else(|| format!("{name}={value} is not <width>x<height>"))?;
                    plan.size = Some((i32::from(width), i32::from(height)));
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
        if fake_vault && !doctor {
            return Err("--fake-vault needs --doctor".to_owned());
        }
        if doctor && (file.is_some() || probe || steps) {
            return Err("--doctor does not go with --capture or --probe".to_owned());
        }
        let doctor = doctor.then_some(vault(fake_vault));
        let check = match (file, probe) {
            (Some(_), true) => return Err("--capture and --probe do not go together".to_owned()),
            (Some(file), false) => Some(Check::Capture(CapturePlan { file, ..plan })),
            (None, _) if steps => {
                return Err("the --capture-... steps need --capture=<file.png>".to_owned());
            }
            (None, true) => Some(Check::Probe),
            (None, false) => None,
        };
        Ok(Self {
            data_dir,
            check,
            doctor,
        })
    }
}

#[cfg(feature = "fake")]
fn vault(of_the_fixtures: bool) -> DoctorVault {
    if of_the_fixtures {
        DoctorVault::OfTheFixtures
    } else {
        DoctorVault::OfThisUser
    }
}

#[cfg(not(feature = "fake"))]
fn vault(_: bool) -> DoctorVault {
    DoctorVault::OfThisUser
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
            &[r#"--data-dir=D:\scratch" --probe"#],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?}");
        }
    }

    #[test]
    fn a_number_out_of_range_is_refused() {
        for step in [
            "--capture-sweep=20001",
            "--capture-sweep=-1",
            "--capture-row=99999999999999999999",
            "--capture-size=0x600",
            "--capture-size=-960x600",
            "--capture-size=70000x600",
            "--capture-size=960",
        ] {
            let arguments = [r"--data-dir=D:\scratch", r"--capture=D:\a.png", step];
            assert!(parse(&arguments).is_err(), "{step}");
        }
        let quoted = [
            r"--data-dir=D:\scratch",
            r#"--capture=D:\shots" --capture-row=3"#,
        ];
        assert!(parse(&quoted).is_err());
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
    fn the_doctor_runs_alone() {
        let args = parse(&[r"--data-dir=D:\scratch", "--doctor"]).unwrap();
        assert_eq!(args.doctor, Some(DoctorVault::OfThisUser));
        assert_eq!(args.check, None);
        assert_eq!(parse(&[r"--data-dir=D:\scratch"]).unwrap().doctor, None);
        for extra in [
            &["--probe"][..],
            &[r"--capture=D:.png"],
            &["--capture-focus"],
            &[r"--capture=D:.png", "--capture-row=3"],
        ] {
            let mut arguments = vec![r"--data-dir=D:\scratch", "--doctor"];
            arguments.extend(extra);
            assert!(parse(&arguments).is_err(), "{extra:?}");
        }
        assert!(parse(&["--doctor"]).is_err());
        assert!(parse(&[r"--data-dir=D:\scratch", "--doctor=1"]).is_err());
    }

    #[cfg(not(feature = "fake"))]
    #[test]
    fn the_stand_in_vault_cannot_be_asked_for() {
        assert!(parse(&[r"--data-dir=D:\scratch", "--doctor", "--fake-vault"]).is_err());
    }

    #[cfg(feature = "fake")]
    #[test]
    fn the_stand_in_vault_is_for_the_doctor_only() {
        let args = parse(&[r"--data-dir=D:\scratch", "--doctor", "--fake-vault"]).unwrap();
        assert_eq!(args.doctor, Some(DoctorVault::OfTheFixtures));
        assert!(parse(&[r"--data-dir=D:\scratch", "--fake-vault"]).is_err());
        assert!(parse(&[r"--data-dir=D:\scratch", "--probe", "--fake-vault"]).is_err());
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
