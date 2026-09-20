//! `sshm list` — the human table, and the two machine-readable forms.

use crate::filter::filter_hosts;
use crate::models::tags_to_string;
use crate::models::Host;
use prettytable::{row, Table};
use std::collections::HashMap;

/// How `sshm list` prints what it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListFormat {
    /// The aligned table a person reads.
    #[default]
    Table,
    /// The matching hosts as a JSON array, for scripts.
    Json,
    /// One host name per line. What shell completion consumes: no quoting to
    /// undo, no JSON parser needed in a completion function.
    Names,
}

/// Options parsed off `sshm list`'s argv.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListOptions {
    pub filter: Option<String>,
    pub format: ListFormat,
}

/// Parse the arguments that follow `sshm list`.
///
/// Order-independent, unlike the positional check this replaced — which only
/// saw `--filter` when it was the very first argument, so
/// `sshm list --json --filter tag:prod` silently ignored the filter.
///
/// An unknown flag is an error rather than something to skip: silently
/// ignoring a typo'd flag in a script is worse than refusing it.
pub fn parse_args(args: &[String]) -> Result<ListOptions, String> {
    let mut opts = ListOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--filter" | "-f" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--filter needs an expression, e.g. --filter tag:prod".into());
                };
                opts.filter = Some(value.clone());
                i += 2;
            }
            "--json" => {
                opts.format = ListFormat::Json;
                i += 1;
            }
            "--names" => {
                opts.format = ListFormat::Names;
                i += 1;
            }
            other => {
                return Err(format!(
                    "unknown option for `sshm list`: {other}\n\
                     Usage: sshm list [--filter \"expr\"] [--json | --names]"
                ));
            }
        }
    }
    Ok(opts)
}

/// The hosts a given filter selects, name-sorted.
fn selected<'a>(hosts: &'a HashMap<String, Host>, filter: Option<&str>) -> Vec<&'a Host> {
    let mut rows: Vec<&Host> = match filter {
        Some(f) => filter_hosts(hosts, f),
        None => hosts.values().collect(),
    };
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

pub fn list_hosts(hosts: &HashMap<String, Host>, opts: &ListOptions) {
    let rows = selected(hosts, opts.filter.as_deref());

    match opts.format {
        // Both machine forms stay silent and well-formed when nothing matches:
        // an empty array and no lines. A "no hosts match" sentence on stdout
        // is exactly what a caller cannot parse.
        ListFormat::Json => {
            let json = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string());
            println!("{json}");
        }
        ListFormat::Names => {
            for h in rows {
                println!("{}", h.name);
            }
        }
        ListFormat::Table => {
            if rows.is_empty() {
                println!("No hosts match your filter.");
                return;
            }
            let mut table = Table::new();
            table.add_row(row!["Name", "Username", "Host", "Port", "Tags"]);
            for h in rows {
                table.add_row(row![
                    h.name,
                    h.username,
                    h.host,
                    h.port.to_string(),
                    tags_to_string(&h.tags)
                ]);
            }
            table.printstd();
        }
    }
}

/// Back-compat shim for callers that only ever wanted the table.
pub fn list_hosts_with_filter(hosts: &HashMap<String, Host>, filter: Option<String>) {
    list_hosts(
        hosts,
        &ListOptions {
            filter,
            format: ListFormat::Table,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_arguments_is_the_table() {
        assert_eq!(parse_args(&[]).unwrap(), ListOptions::default());
    }

    #[test]
    fn a_filter_is_picked_up_wherever_it_sits() {
        // The positional version this replaced only saw `--filter` at index 0.
        let expected = ListOptions {
            filter: Some("tag:prod".into()),
            format: ListFormat::Json,
        };
        assert_eq!(
            parse_args(&args(&["--filter", "tag:prod", "--json"])).unwrap(),
            expected
        );
        assert_eq!(
            parse_args(&args(&["--json", "--filter", "tag:prod"])).unwrap(),
            expected
        );
    }

    #[test]
    fn dash_f_is_accepted_for_filter() {
        assert_eq!(
            parse_args(&args(&["-f", "web"])).unwrap().filter.as_deref(),
            Some("web")
        );
    }

    #[test]
    fn a_filter_without_a_value_is_an_error() {
        assert!(parse_args(&args(&["--filter"])).is_err());
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_ignored() {
        let err = parse_args(&args(&["--jsonn"])).unwrap_err();
        assert!(err.contains("--jsonn"), "{err}");
        assert!(
            err.contains("Usage"),
            "the error should say what is accepted: {err}"
        );
    }

    #[test]
    fn the_last_format_flag_wins() {
        assert_eq!(
            parse_args(&args(&["--json", "--names"])).unwrap().format,
            ListFormat::Names
        );
        assert_eq!(
            parse_args(&args(&["--names", "--json"])).unwrap().format,
            ListFormat::Json
        );
    }

    #[test]
    fn a_filter_expression_that_looks_like_a_flag_is_still_its_value() {
        // `--filter --json` means "filter on the literal string --json", odd
        // but unambiguous: the value is whatever follows.
        let o = parse_args(&args(&["--filter", "--json"])).unwrap();
        assert_eq!(o.filter.as_deref(), Some("--json"));
        assert_eq!(o.format, ListFormat::Table);
    }
}
