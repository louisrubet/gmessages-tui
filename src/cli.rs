//! Command-line options, parsed before touching session state.

pub const USAGE: &str =
    "gmessages-tui [--reset-login] [--manual | --qr] | --uninstall | --version | --help";

pub const HELP: &str = "\
gmessages-tui: Google Messages (RCS and SMS) in the terminal, paired with your Android phone.

Options:
  --reset-login  forget the pairing, then pair again
  --manual       pair from a page in your browser (cookies pasted from Google)
  --qr           pair by QR code, for phones that still offer it
  --uninstall    delete the store key from the keyring, the state and the cache
  --version      print the release version
  --help         show this help";

#[derive(Debug, PartialEq)]
pub enum Command {
    Run {
        qr: bool,
        manual: bool,
        reset_login: bool,
    },
    Version,
    Uninstall,
    Help,
    #[cfg(feature = "wry-signin")]
    Signin,
}

pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<Command, &'static str> {
    let args: Vec<_> = args.into_iter().collect();
    if args == ["--version"] {
        return Ok(Command::Version);
    }
    if args == ["--help"] || args == ["-h"] {
        return Ok(Command::Help);
    }
    if args == ["--uninstall"] {
        return Ok(Command::Uninstall);
    }
    #[cfg(feature = "wry-signin")]
    if args == ["--internal-google-signin"] {
        return Ok(Command::Signin);
    }
    let mut mode = None;
    let mut reset_login = false;
    for arg in args {
        match arg {
            "--reset-login" if !reset_login => reset_login = true,
            "--manual" | "--qr" if mode.is_none() => mode = Some(arg),
            _ => return Err(USAGE),
        }
    }
    Ok(Command::Run {
        qr: mode == Some("--qr"),
        manual: mode == Some("--manual") || (mode.is_none() && !cfg!(feature = "wry-signin")),
        reset_login,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_login_uses_default_signin() {
        assert_eq!(
            parse(["--reset-login"]),
            Ok(Command::Run {
                qr: false,
                manual: !cfg!(feature = "wry-signin"),
                reset_login: true,
            })
        );
    }

    #[test]
    fn reset_combines_with_pairing_modes_in_either_order() {
        for args in [["--reset-login", "--manual"], ["--manual", "--reset-login"]] {
            assert_eq!(
                parse(args),
                Ok(Command::Run {
                    qr: false,
                    manual: true,
                    reset_login: true,
                })
            );
        }
        for args in [["--reset-login", "--qr"], ["--qr", "--reset-login"]] {
            assert_eq!(
                parse(args),
                Ok(Command::Run {
                    qr: true,
                    manual: false,
                    reset_login: true,
                })
            );
        }
    }

    #[test]
    fn invalid_combinations_are_rejected_before_reset() {
        for args in [
            vec!["--qr", "--manual"],
            vec!["--reset-login", "--version"],
            vec!["--reset-login", "--unknown"],
            vec!["--reset-login", "--internal-google-signin"],
            vec!["--reset-login", "--reset-login"],
            vec!["--uninstall", "--reset-login"],
            vec!["--qr", "--uninstall"],
            vec!["--help", "--qr"],
        ] {
            assert!(parse(args).is_err());
        }
    }

    #[test]
    fn existing_commands_still_work() {
        assert_eq!(parse(["--version"]), Ok(Command::Version));
        assert_eq!(parse(["--uninstall"]), Ok(Command::Uninstall));
        assert_eq!(parse(["--help"]), Ok(Command::Help));
        assert_eq!(parse(["-h"]), Ok(Command::Help));
        assert!(matches!(
            parse(std::iter::empty::<&str>()),
            Ok(Command::Run {
                reset_login: false,
                ..
            })
        ));
        assert!(matches!(
            parse(["--qr"]),
            Ok(Command::Run {
                qr: true,
                reset_login: false,
                ..
            })
        ));
        assert!(matches!(
            parse(["--manual"]),
            Ok(Command::Run {
                manual: true,
                reset_login: false,
                ..
            })
        ));
    }
}
