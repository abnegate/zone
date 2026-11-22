use clap::Parser;

use crate::command::Command;

#[derive(Debug, Clone, Parser)]
#[command(name = "zone-license", version, about = "Zone license signer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::path::Path;

    use clap::CommandFactory;
    use clap::error::ErrorKind;
    use zone_enterprise::Plan;

    use super::*;
    use crate::registry_command::RegistryCommand;

    fn subcommands(command: &clap::Command) -> Vec<clap::Command> {
        command
            .get_subcommands()
            .flat_map(|subcommand| {
                let mut found = vec![subcommand.clone()];
                found.extend(subcommands(subcommand));
                found
            })
            .collect()
    }

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_argument_takes_a_key_value() {
        for subcommand in subcommands(&Cli::command()) {
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
        let refused =
            Cli::try_parse_from(["zone-license", "registry", "jwks", "--private-key", "AAAA"]);
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
            "--plan",
            "team",
        ])
        .unwrap();
        let Command::Sign(arguments) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(arguments.plan, Plan::Team);
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
        assert_eq!(arguments.plan, Plan::Enterprise);
    }

    fn sign_with_plan(plan: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from([
            "zone-license",
            "sign",
            "--customer",
            "Acme Ltd",
            "--seats",
            "25",
            "--expires-at",
            "2027-11-01",
            "--plan",
            plan,
        ])
    }

    #[test]
    fn parses_either_plan() {
        for plan in Plan::ALL {
            let cli = sign_with_plan(plan.as_str()).unwrap();
            let Command::Sign(arguments) = cli.command else {
                panic!("parsed {:?}", cli.command);
            };
            assert_eq!(arguments.plan, plan);
        }
    }

    #[test]
    fn refuses_an_unknown_plan() {
        for plan in ["business", "Team", "ENTERPRISE", ""] {
            let error = sign_with_plan(plan).unwrap_err();
            assert_eq!(
                error.kind(),
                ErrorKind::ValueValidation,
                "{plan:?}: {error}"
            );
        }
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
        for mut subcommand in subcommands(&Cli::command()) {
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

    fn registry_jwks(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(
            ["zone-license", "registry", "jwks"]
                .into_iter()
                .chain(arguments.iter().copied()),
        )
    }

    #[test]
    fn parses_a_registry_jwks_command() {
        let cli = registry_jwks(&[
            "--generation",
            "1",
            "--generation",
            "2",
            "--key-id",
            "2026-2",
            "--private-key-file",
            "/run/secrets/zone-license",
        ])
        .unwrap();
        let Command::Registry(RegistryCommand::Jwks(arguments)) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(
            arguments.generations,
            [NonZeroU32::new(1).unwrap(), NonZeroU32::new(2).unwrap()]
        );
        assert_eq!(arguments.key_id.as_deref(), Some("2026-2"));
        assert_eq!(
            arguments.private_key_file.as_deref(),
            Some(Path::new("/run/secrets/zone-license"))
        );
    }

    #[test]
    fn a_registry_jwks_command_defaults_to_generation_one() {
        let cli = registry_jwks(&[]).unwrap();
        let Command::Registry(RegistryCommand::Jwks(arguments)) = cli.command else {
            panic!("parsed {:?}", cli.command);
        };
        assert_eq!(arguments.generations, [NonZeroU32::MIN]);
        assert_eq!(arguments.key_id, None);
        assert_eq!(arguments.private_key_file, None);
    }

    #[test]
    fn refuses_a_registry_key_generation_below_one() {
        for generation in ["0", "-1", "one", ""] {
            let error = registry_jwks(&[&format!("--generation={generation}")]).unwrap_err();
            assert_eq!(
                error.kind(),
                ErrorKind::ValueValidation,
                "{generation:?}: {error}"
            );
        }
    }

    #[test]
    fn the_registry_command_needs_a_subcommand() {
        assert!(Cli::try_parse_from(["zone-license", "registry"]).is_err());
    }
}
