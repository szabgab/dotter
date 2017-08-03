use clap;

pub fn get_args() -> clap::ArgMatches<'static> {
    clap::App::new("Dotter")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .version(crate_version!())
        .author(crate_authors!())
        .about("A small dotfile manager.")
        .arg(
            clap::Arg::with_name("directory")
                .short("d")
                .long("directory")
                .value_name("DIRECTORY")
                .takes_value(true)
                .default_value(".")
                .help("Do all operations relative to this directory."),
        )
        .arg(
            clap::Arg::with_name("files")
                .short("f")
                .long("files")
                .value_name("FILES")
                .takes_value(true)
                .default_value("dotter_settings/files.toml")
                .help("Config for dotter's files."),
        )
        .arg(
            clap::Arg::with_name("variables")
                .short("V")
                .long("variables")
                .value_name("VARIABLES")
                .takes_value(true)
                .default_value("dotter_settings/variables.toml")
                .help("Config for dotter's variables."),
        )
        .arg(
            clap::Arg::with_name("secrets")
                .short("s")
                .long("secrets")
                .value_name("SECRETS")
                .takes_value(true)
                .default_value("dotter_settings/secrets.toml")
                .help("Secrets file for dotter, doesn't have to exist."),
        )
        .arg(
            clap::Arg::with_name("verbose")
                .short("v")
                .long("verbose")
                .multiple(true)
                .help(
                    "Print information about what's being done. Repeat for \
                   more information.",
                ),
        )
        .arg(clap::Arg::with_name("dry_run").long("dry-run").help(
            "Dry run - don't do anything, only print information. \
                   Implies -v at least once.",
        ))
        .subcommand(
            clap::SubCommand::with_name("deploy")
                .about("Copy all files to their configured locations.")
                .arg(
                    clap::Arg::with_name("nocache")
                        .short("c")
                        .long("nocache")
                        .help(
                            "Don't use a cache \
                                       (used to not touch files that didn't change)",
                        ),
                )
                .arg(
                    clap::Arg::with_name("cache_directory")
                        .short("d")
                        .long("cache-directory")
                        .value_name("DIRECTORY")
                        .takes_value(true)
                        .default_value("dotter_cache")
                        .help("Directory to cache in."),
                ),
        )
        .subcommand(
            clap::SubCommand::with_name("config")
                .about("Configure files/variables.")
                .arg(clap::Arg::with_name("file").short("f").long("file").help(
                    "Operate on files.",
                ))
                .arg(
                    clap::Arg::with_name("variable")
                        .short("v")
                        .long("variable")
                        .help("Operate on variables."),
                )
                .arg(
                    clap::Arg::with_name("secret")
                        .short("s")
                        .long("secret")
                        .help("Operate on secrets."),
                )
                .group(clap::ArgGroup::with_name("target").required(true).args(
                    &[
                        "file",
                        "variable",
                        "secret",
                    ],
                ))
                .arg(
                    clap::Arg::with_name("add")
                        .short("a")
                        .long("add")
                        .value_names(&["from", "to"])
                        .help(
                            "In case of file, add file -> target entry, \
                               in case of variable/secret, \
                               add key -> value entry.",
                        ),
                )
                .arg(
                    clap::Arg::with_name("remove")
                        .short("r")
                        .long("remove")
                        .value_name("object")
                        .takes_value(true)
                        .help("Remove a file or variable from configuration."),
                )
                .arg(
                    clap::Arg::with_name("display")
                        .short("d")
                        .long("display")
                        .help("Display the configuration."),
                )
                .group(clap::ArgGroup::with_name("action").required(true).args(
                    &[
                        "add",
                        "remove",
                        "display",
                    ],
                )),
        )
        .get_matches()
}
