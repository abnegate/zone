use std::collections::BTreeSet;

use super::table::Table;

const COMMENT: char = ';';
const DATABASE_HEADER: &str = "dbname:";
const FORMAT_HEADER: &str = "Format:";
const TABLE: &str = "TABLE";
const DATA: &str = "DATA";
const EXTENSION: &str = "EXTENSION";
const UNQUALIFIED: &str = "-";

/// Where an entry's description starts, after its dump id, catalog id and object id.
const DESCRIPTION: usize = 3;

/// The format `pg_restore --list` names a `pg_dump -Fc` archive by.
pub const CUSTOM_FORMAT: &str = "CUSTOM";

/// What `pg_restore --list` says an archive holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listing {
    database: String,
    format: String,
    defined: BTreeSet<Table>,
    filled: BTreeSet<Table>,
    extensions: BTreeSet<String>,
}

impl Listing {
    /// Parse the table of contents `pg_restore --list` printed.
    pub fn parse(listing: &str) -> Self {
        let mut database = String::new();
        let mut format = String::new();
        let mut defined = BTreeSet::new();
        let mut filled = BTreeSet::new();
        let mut extensions = BTreeSet::new();
        for line in listing.lines() {
            if let Some(comment) = line.strip_prefix(COMMENT) {
                let comment = comment.trim();
                if let Some(name) = comment.strip_prefix(DATABASE_HEADER) {
                    database = name.trim().to_string();
                } else if let Some(name) = comment.strip_prefix(FORMAT_HEADER) {
                    format = name.trim().to_string();
                }
                continue;
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            match tokens.get(DESCRIPTION..) {
                Some([TABLE, DATA, schema, name, ..]) => {
                    filled.insert(Table::new(*schema, *name));
                }
                Some([TABLE, schema, name, ..]) => {
                    defined.insert(Table::new(*schema, *name));
                }
                Some([EXTENSION, UNQUALIFIED, name, ..]) => {
                    extensions.insert(name.to_string());
                }
                _ => {}
            }
        }
        Self {
            database,
            format,
            defined,
            filled,
            extensions,
        }
    }

    /// The database the archive was dumped from.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// The archive format `pg_dump` wrote, such as `CUSTOM` for `-Fc` or `TAR` for `-Ft`.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Whether the archive holds both the definition and the rows of `table`, so restoring it
    /// recreates the table with its data.
    pub fn holds(&self, table: &Table) -> bool {
        self.defined.contains(table) && self.filled.contains(table)
    }

    /// Whether the archive creates the extension `name`.
    pub fn holds_extension(&self, name: &str) -> bool {
        self.extensions.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::CUSTOM_FORMAT;
    use super::Listing;
    use super::Table;

    fn public(name: &str) -> Table {
        Table::new("public", name)
    }

    const LISTING: &str = ";
; Archive created at 2026-11-06 20:25:50 NZDT
;     dbname: zone_test
;     TOC Entries: 529
;     Compression: gzip
;     Dump Version: 1.16-0
;     Format: CUSTOM
;     Integer: 4 bytes
;     Offset: 8 bytes
;     Dumped from database version: 17.11 (Debian 17.11-1.pgdg12+2)
;     Dumped by pg_dump version: 18.6 (Homebrew)
;
;
; Selected TOC Entries:
;
2; 3079 16393 EXTENSION - vector 
4739; 0 0 COMMENT - EXTENSION vector 
219; 1259 16740 TABLE public agent_logins postgres
248; 1259 16992 TABLE public projects postgres
257; 1259 17070 TABLE public sources postgres
259; 1259 17089 TABLE public sync_configs postgres
4664; 0 16385 TABLE DATA public _sqlx_migrations postgres
4693; 0 16992 TABLE DATA public projects postgres
4702; 0 17070 TABLE DATA public sources postgres
4704; 0 17089 TABLE DATA public sync_configs postgres
4740; 0 0 SEQUENCE SET public chat_entries_position_seq postgres
4053; 2606 17322 CONSTRAINT public agent_logins agent_logins_pkey postgres
4518; 2620 17653 TRIGGER public sync_configs sync_configs_updated_at postgres
4469; 2606 17984 FK CONSTRAINT public sync_configs sync_configs_project_id_fkey postgres
";

    #[test]
    fn the_header_names_the_database() {
        assert_eq!(Listing::parse(LISTING).database(), "zone_test");
    }

    #[test]
    fn the_header_names_the_format() {
        assert_eq!(Listing::parse(LISTING).format(), CUSTOM_FORMAT);
        let tar = LISTING.replace(";     Format: CUSTOM", ";     Format: TAR");
        assert_eq!(Listing::parse(&tar).format(), "TAR");
        assert_eq!(Listing::parse(&tar).database(), "zone_test");
    }

    #[test]
    fn a_table_is_held_only_with_its_definition_and_its_rows() {
        let listing = Listing::parse(LISTING);
        for table in ["projects", "sources", "sync_configs"] {
            assert!(listing.holds(&public(table)), "{table}");
        }
        assert!(
            !listing.holds(&public("agent_logins")),
            "a table whose data was excluded still has schema entries"
        );
        assert!(
            !listing.holds(&public("_sqlx_migrations")),
            "a table whose definition is missing, as in a data-only dump, is not held"
        );
        assert!(!listing.holds(&public("project")));
        assert!(!listing.holds(&Table::new("archive", "projects")));
    }

    #[test]
    fn an_extension_is_held_only_by_its_own_entry() {
        let listing = Listing::parse(LISTING);
        assert!(listing.holds_extension("vector"));
        assert!(!listing.holds_extension("pgcrypto"));
        let without = Listing::parse(&LISTING.replace("2; 3079 16393 EXTENSION - vector", ""));
        assert!(
            !without.holds_extension("vector"),
            "the extension's comment is not the extension"
        );
    }

    #[test]
    fn a_commented_out_entry_holds_nothing() {
        let listing = Listing::parse(
            ";248; 1259 16992 TABLE public projects postgres\n\
             ;4693; 0 16992 TABLE DATA public projects postgres\n",
        );
        assert!(!listing.holds(&public("projects")));
        assert_eq!(listing.database(), "");
        assert_eq!(listing.format(), "");
    }
}
