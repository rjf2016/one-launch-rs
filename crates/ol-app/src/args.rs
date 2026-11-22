use std::path::PathBuf;

const MOST: usize = 20_000;

#[derive(Debug, PartialEq)]
pub struct Args {
    pub data_dir: PathBuf,
    pub check: Option<Check>,
    pub doctor: Option<DoctorVault>,
    #[cfg(feature = "fake")]
    pub fake: Option<FakeMachine>,
    // The registry, the processes, the files and the shell of this PC behind the launchers. Without it the app reaches no launcher.
    #[cfg(not(feature = "fake"))]
    pub real_launchers: bool,
}

// The folder a fake machine lives in, and the scenario and fixtures to fill it from when it holds none yet.
#[cfg(feature = "fake")]
#[derive(Debug, PartialEq)]
pub struct FakeMachine {
    pub folder: PathBuf,
    pub lay_down: Option<(PathBuf, PathBuf)>,
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
        let mut real_launchers = false;
        #[cfg(feature = "fake")]
        let (mut fake_folder, mut fake_scenario, mut fake_fixtures) = (None, None, None);
        for argument in arguments {
            if argument == "--real-launchers" {
                if cfg!(feature = "fake") {
                    return Err(
                        "--real-launchers is for the build without the feature fake".to_owned()
                    );
                }
                real_launchers = true;
                continue;
            }
            if argument == "--probe" {
                probe = true;
                continue;
            }
            // Read where the window is made, by the rule below the UI that also wants the tray.
            if argument == ol_api::settings::START_HIDDEN_FLAG {
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
                #[cfg(feature = "fake")]
                "--fake-launchers" => fake_folder = Some(path()?),
                #[cfg(feature = "fake")]
                "--fake-scenario" => fake_scenario = Some(path()?),
                #[cfg(feature = "fake")]
                "--fake-fixtures" => fake_fixtures = Some(path()?),
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
        // A capture or a probe is run by agents and kept as a file: neither may ever show a real launcher's accounts.
        if real_launchers && (file.is_some() || probe || steps || doctor) {
            return Err(
                "--real-launchers does not go with --capture, --capture-..., --probe or --doctor: a capture or a probe must never show a real launcher's accounts"
                    .to_owned(),
            );
        }
        #[cfg(feature = "fake")]
        let fake = fake_machine(fake_folder, fake_scenario, fake_fixtures)?;
        #[cfg(feature = "fake")]
        if fake.is_some() && doctor {
            return Err("--doctor does not go with --fake-launchers".to_owned());
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
            #[cfg(feature = "fake")]
            fake,
            #[cfg(not(feature = "fake"))]
            real_launchers,
        })
    }
}

#[cfg(feature = "fake")]
fn fake_machine(
    folder: Option<PathBuf>,
    scenario: Option<PathBuf>,
    fixtures: Option<PathBuf>,
) -> Result<Option<FakeMachine>, String> {
    let lay_down = match (scenario, fixtures) {
        (Some(scenario), Some(fixtures)) => Some((scenario, fixtures)),
        (None, None) => None,
        _ => return Err("--fake-scenario and --fake-fixtures go together".to_owned()),
    };
    let Some(folder) = folder else {
        return match lay_down {
            Some(_) => Err("--fake-scenario needs --fake-launchers=<folder>".to_owned()),
            None => Ok(None),
        };
    };
    let named = [("--fake-launchers", Some(&folder))].into_iter().chain([
        ("--fake-scenario", lay_down.as_ref().map(|from| &from.0)),
        ("--fake-fixtures", lay_down.as_ref().map(|from| &from.1)),
    ]);
    for (name, path) in named {
        if path.is_some_and(|path| !path.is_absolute()) {
            return Err(format!("{name}=<folder> must be an absolute path"));
        }
    }
    Ok(Some(FakeMachine { folder, lay_down }))
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
        for extra in [
            "--start-hidden=1",
            "--Start-Hidden",
            "--capture-speed=3",
            "proof",
            "--probe=1",
        ] {
            assert!(
                parse(&[r"--data-dir=D:\scratch", extra]).is_err(),
                "{extra}"
            );
        }
    }

    #[test]
    fn a_start_in_the_tray_is_asked_for_by_its_flag() {
        let args = parse(&[r"--data-dir=D:\scratch", "--start-hidden"]).unwrap();
        assert_eq!(args.check, None);
        assert!(
            parse(&[
                r"--data-dir=D:\scratch",
                r"--capture=D:\a.png",
                "--start-hidden"
            ])
            .is_ok()
        );
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

    #[cfg(not(feature = "fake"))]
    #[test]
    fn the_fake_launchers_cannot_be_asked_for() {
        for extra in [
            r"--fake-launchers=D:\scratch\launchers",
            r"--fake-scenario=D:\scenarios\all-four",
            r"--fake-fixtures=D:\fixtures",
        ] {
            assert!(
                parse(&[r"--data-dir=D:\scratch", extra]).is_err(),
                "{extra}"
            );
        }
    }

    #[cfg(feature = "fake")]
    #[test]
    fn a_fake_machine_is_a_folder_and_what_to_fill_it_from() {
        let data = r"--data-dir=D:\scratch\data";
        let folder = r"--fake-launchers=D:\scratch\launchers";
        let scenario = r"--fake-scenario=D:\scenarios\all-four";
        let fixtures = r"--fake-fixtures=D:\fixtures";
        assert_eq!(parse(&[data]).unwrap().fake, None);
        assert_eq!(
            parse(&[data, folder]).unwrap().fake,
            Some(FakeMachine {
                folder: PathBuf::from(r"D:\scratch\launchers"),
                lay_down: None,
            })
        );
        assert_eq!(
            parse(&[data, folder, scenario, fixtures]).unwrap().fake,
            Some(FakeMachine {
                folder: PathBuf::from(r"D:\scratch\launchers"),
                lay_down: Some((
                    PathBuf::from(r"D:\scenarios\all-four"),
                    PathBuf::from(r"D:\fixtures")
                )),
            })
        );
        for refused in [
            &[data, scenario, fixtures][..],
            &[data, folder, scenario],
            &[data, folder, fixtures],
            &[data, "--fake-launchers=launchers"],
            &[data, folder, "--fake-scenario=all-four", fixtures],
            &[data, folder, scenario, "--fake-fixtures=fixtures"],
            &[data, folder, "--doctor"],
            &[data, "--fake-launchers"],
        ] {
            assert!(parse(refused).is_err(), "{refused:?}");
        }
    }

    #[cfg(not(feature = "fake"))]
    #[test]
    fn the_real_launchers_are_reached_only_when_asked_and_never_in_a_capture_or_a_probe() {
        let data = r"--data-dir=D:\scratch";
        assert!(!parse(&[data]).unwrap().real_launchers);
        assert!(parse(&[data, "--real-launchers"]).unwrap().real_launchers);
        for refused in [
            &[data, "--real-launchers", r"--capture=D:\a.png"][..],
            &[
                data,
                r"--capture=D:\a.png",
                "--capture-row=3",
                "--real-launchers",
            ],
            &[
                data,
                r"--capture=D:\a.png",
                "--capture-focus",
                "--real-launchers",
            ],
            &[data, "--real-launchers", "--capture-keys=a"],
            &[data, "--probe", "--real-launchers"],
            &[data, "--real-launchers", "--doctor"],
            &[data, "--real-launchers=1"],
            &[data, "--Real-Launchers"],
            &[
                data,
                "--real-launchers",
                r"--fake-launchers=D:\scratch\launchers",
            ],
        ] {
            assert!(parse(refused).is_err(), "{refused:?}");
        }
        let said = parse(&[data, "--real-launchers", "--probe"]).unwrap_err();
        assert!(
            said.contains("must never show a real launcher's accounts"),
            "{said}"
        );
    }

    #[cfg(feature = "fake")]
    #[test]
    fn a_build_with_the_fake_launchers_never_reaches_the_real_ones() {
        let data = r"--data-dir=D:\scratch";
        for refused in [
            &[data, "--real-launchers"][..],
            &[
                data,
                r"--fake-launchers=D:\scratch\launchers",
                "--real-launchers",
            ],
        ] {
            let said = parse(refused).unwrap_err();
            assert!(said.contains("without the feature fake"), "{said}");
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
