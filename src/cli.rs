//! Command-line options, parsed before touching session state.

pub const USAGE: &str =
    "gmessages-tui [--manual | --qr] | --unpair | --uninstall | --version | --help";

pub const HELP: &str = "\
gmessages-tui: Google Messages (RCS and SMS) in the terminal, paired with your Android phone.

Options:
  --manual       pair from a page in your browser (cookies pasted from Google)
  --qr           pair by QR code, scanned from Google Messages, for phones that still offer it
  --unpair       unpair: your phone forgets this device, the session here is deleted (the
                 messages stored stay); the next start pairs again. Also --reset-login
  --uninstall    unpair, then delete the store key from the keyring, the state and the cache
  --version      print the version (with the commit, beyond the release)
  --help         show this help";

#[derive(Debug, PartialEq)]
pub enum Command {
    Run {
        qr: bool,
        manual: bool,
    },
    Unpair,
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
    // --reset-login: its name up to 1.3.1.
    if args == ["--unpair"] || args == ["--reset-login"] {
        return Ok(Command::Unpair);
    }
    #[cfg(feature = "wry-signin")]
    if args == ["--internal-google-signin"] {
        return Ok(Command::Signin);
    }
    let mode = match args[..] {
        [] => None,
        [mode @ ("--manual" | "--qr")] => Some(mode),
        _ => return Err(USAGE),
    };
    Ok(Command::Run {
        qr: mode == Some("--qr"),
        manual: mode == Some("--manual") || (mode.is_none() && !cfg!(feature = "wry-signin")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpair_and_its_old_name() {
        assert_eq!(parse(["--unpair"]), Ok(Command::Unpair));
        assert_eq!(parse(["--reset-login"]), Ok(Command::Unpair));
    }

    #[test]
    fn invalid_combinations_are_rejected() {
        for args in [
            vec!["--qr", "--manual"],
            vec!["--unpair", "--manual"],
            vec!["--reset-login", "--qr"],
            vec!["--unpair", "--unpair"],
            vec!["--unpair", "--version"],
            vec!["--unknown"],
            vec!["--unpair", "--internal-google-signin"],
            vec!["--uninstall", "--unpair"],
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
        assert_eq!(
            parse(std::iter::empty::<&str>()),
            Ok(Command::Run {
                qr: false,
                manual: !cfg!(feature = "wry-signin"),
            })
        );
        assert_eq!(
            parse(["--qr"]),
            Ok(Command::Run {
                qr: true,
                manual: false,
            })
        );
        assert_eq!(
            parse(["--manual"]),
            Ok(Command::Run {
                qr: false,
                manual: true,
            })
        );
    }
}
