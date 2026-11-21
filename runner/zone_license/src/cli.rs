use clap::Parser;

use crate::command::Command;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "zone-license",
    version,
    about = "Zone Enterprise license signer"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_argument_takes_a_key_value() {
        let cli = Cli::command();
        for subcommand in cli.get_subcommands() {
            for argument in subcommand.get_arguments() {
                let name = argument.get_id().as_str();
                assert!(
                    !name.contains("private_key") || name == "private_key_file",
                    "{} accepts --{name}",
                    subcommand.get_name()
                );
            }
        }
        let refused = Cli::try_parse_from([
            "zone-license",
            "sign",
            "--customer",
            "Acme",
            "--seats",
            "1",
            "--expires-at",
            "2027-01-01",
            "--private-key",
            "AAAA",
        ]);
        assert!(refused.is_err());
    }

    #[test]
    fn parses_a_sign_command() {
        let cli = Cli::try_parse_from([
            "zone-license",
            "sign",
            "--customer",
            "Acme Ltd",
            "--seats",
            "25",
            "--expires-at",
            "2027-11-01",
            "--key-id",
            "2026-1",
            "--private-key-file",
            "/run/secrets/zone-license",
            "--issued-at",
            "2026-11-01T00:00:00Z",
            "--id",
            "0193a8e2-7c4b-7d10-8f00-4d2c5a9b1e01",
            "--subscription",
            "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c",
        ])
        .unwrap();
        let Command::Sign(arguments) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(arguments.customer, "Acme Ltd");
        assert_eq!(arguments.seats, 25);
        assert_eq!(
            arguments.expires_at.to_rfc3339(),
            "2027-11-01T00:00:00+00:00"
        );
        assert_eq!(arguments.key_id.as_deref(), Some("2026-1"));
        assert_eq!(
            arguments
                .issued_at
                .map(|instant| instant.to_rfc3339())
                .as_deref(),
            Some("2026-11-01T00:00:00+00:00")
        );
        assert!(arguments.id.is_some());
        assert_eq!(
            arguments.subscription.as_deref(),
            Some("b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c")
        );
    }

    #[test]
    fn a_sign_command_needs_no_subscription() {
        let cli = Cli::try_parse_from([
            "zone-license",
            "sign",
            "--customer",
            "Acme Ltd",
            "--seats",
            "25",
            "--expires-at",
            "2027-11-01",
        ])
        .unwrap();
        let Command::Sign(arguments) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(arguments.subscription, None);
    }

    #[test]
    fn refuses_a_malformed_expiry() {
        let refused = Cli::try_parse_from([
            "zone-license",
            "sign",
            "--customer",
            "Acme",
            "--seats",
            "1",
            "--expires-at",
            "next year",
        ]);
        assert!(refused.is_err());
    }

    fn collapsed(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn every_argument_help_renders_whole() {
        let mut cli = Cli::command();
        for subcommand in cli.get_subcommands_mut() {
            let help = collapsed(&subcommand.render_help().to_string());
            for argument in subcommand.get_arguments() {
                let Some(declared) = argument.get_help() else {
                    continue;
                };
                let declared = collapsed(&declared.to_string());
                assert!(
                    help.contains(&declared),
                    "{} --{} renders as:\n{help}",
                    subcommand.get_name(),
                    argument.get_id()
                );
            }
        }
    }

    #[test]
    fn parses_a_keygen_command() {
        let cli = Cli::try_parse_from([
            "zone-license",
            "keygen",
            "--output",
            "signing.key",
            "--key-id",
            "2026-2",
        ])
        .unwrap();
        let Command::Keygen(arguments) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(
            arguments.output.as_deref(),
            Some(std::path::Path::new("signing.key"))
        );
        assert_eq!(arguments.key_id.as_deref(), Some("2026-2"));
    }
}
